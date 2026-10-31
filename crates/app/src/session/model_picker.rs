//! 模型菜单（042 定稿，对齐 模型菜单.bmp）+ 分组模型列表共享组件。
//!
//! 菜单 = 锚定模型钮的 pill 卡片（弃用原居中 ModelSelect 弹窗）：过滤输入
//! 与 ↑/↓/Enter 键盘导航保留，行 = 左✓（当前会话模型）+ 显示名 + 右侧
//! 五角星（app-settings 默认，与设置页下列表同一份、双向同步）。
//!
//! 菜单版式（2026-10-19 定稿）：
//! - 卡片宽 380px（+40：模型名 + 右缘星标不再贴边）。
//! - **标星默认模型置顶**：默认模型所在 Provider 组置顶、组内置首
//!   （`order_model_indices` 纯函数，单测看护）；菜单行序 = 键盘选择序 =
//!   Enter 落点序（同一 vec 下标，三者不漂移）。
//! - 内容超高时列表滚动 + 右缘常显滚动条（`menu_scrollbar`）；列表高同时
//!   受 10 行上限与视口可用高约束（卡片不顶出窗口上沿）。
//!
//! 组件面（`group_entries` / `group_header_row` / `group_model_row` /
//! `MODEL_FILTER_MIN`）为菜单与设置·模型页下列表共用：分组结构、组头样式、
//! 行缩进、五角星、过滤阈值都在这一份实现，两处不许再各自漂移。

use gpui::{AnyElement, MouseButton, SharedString, div, point, prelude::*, px, rgb};

use super::Chat;
use crate::TextInput;
use crate::appearance::theme::theme as T;

/// 菜单行高（限高 = `ROW_H × 10`，与 mcp_picker 一致）。
pub(crate) const ROW_H: f32 = 30.;
/// 过滤框显示阈值：模型数少于该值不渲染过滤输入（菜单/设置·下列表同规则）。
pub(crate) const MODEL_FILTER_MIN: usize = 10;

/// 打开时的键盘选择位 + 滚动位（过滤文本变化时 sel 归零）。
pub(crate) struct ModelPicker {
    pub(crate) input: gpui::Entity<TextInput>,
    pub(crate) sel: usize,
    pub(crate) scroll: gpui::ScrollHandle,
}

/// 全量 provider 注册表（新增 Provider 弹窗数据源）的拉取状态。
#[derive(Default, Clone, Debug)]
pub(crate) enum RegistryState {
    #[default]
    /// 未拉取（可拉）
    Idle,
    /// 后台 dump 进行中
    Loading,
    /// 上次 dump 失败（附原因；可重试）
    Failed(String),
}

impl Chat {
    /// 确保全量注册表可用：盘上缓存（vendor VERSION 校验）命中即用；
    /// 否则后台起一次性 node 走 SDK dump 并落缓存。幂等——Loading 中
    /// 重复调用直接返回。失败置 [`RegistryState::Failed`]，下次调用重试。
    pub(crate) fn ensure_registry(&mut self, cx: &mut Context<Self>) {
        if self.cfg.provider_registry.is_some()
            || matches!(self.cfg.registry_state, RegistryState::Loading)
        {
            return;
        }
        if let Some(d) = pi_link::registry::read_cache() {
            self.cfg.provider_registry = Some(std::sync::Arc::new(d));
            cx.notify();
            return;
        }
        self.cfg.registry_state = RegistryState::Loading;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { pi_link::registry::dump_and_cache() })
                .await;
            let _ = this.update(cx, |c, cx| {
                match outcome {
                    Ok(d) => {
                        c.cfg.provider_registry = Some(std::sync::Arc::new(d));
                        c.cfg.registry_state = RegistryState::Idle;
                    }
                    Err(e) => c.cfg.registry_state = RegistryState::Failed(e),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// 分组列表的一个可见行：组头或模型（Model(ix) 索引进模型 vec——键盘
/// ↑/↓ 只在模型行间移动，组头自动跳过）。
pub(crate) enum ModelGroupEntry {
    Header(String),
    Model(usize),
}

/// 选中模型行在滚动内容里的 y 偏移（组头 0.8×行高 + 模型行 1×行高 累加）。
/// 纯函数（不碰 cx），供键盘选中行滚进可视区用。
fn sel_row_y(entries: &[ModelGroupEntry], sel: usize) -> f32 {
    let mut y = 0.;
    let mut seen = 0usize;
    for e in entries {
        match e {
            ModelGroupEntry::Header(_) => y += ROW_H * 0.8,
            ModelGroupEntry::Model(_) => {
                if seen == sel {
                    return y;
                }
                seen += 1;
                y += ROW_H;
            }
        }
    }
    y
}

/// 菜单行序（**标星默认模型置顶**）：默认模型所在 Provider 组置顶、组内
/// 默认模型置首；其余 Provider 按显示名字典序、组内保持目录原序。
///
/// 输入 = 候选模型下标（已过白名单/文本过滤），输出 = 显示序下标。纯函数
/// （不碰 cx），单测看护。默认模型 = app-settings 的五角星默认（全局唯一）。
pub(crate) fn order_model_indices(
    models: &[pi_link::protocol::ModelInfo],
    cand: &[usize],
    default_ref: Option<&str>,
) -> Vec<usize> {
    if cand.len() < 2 {
        return cand.to_vec();
    }
    // 默认模型键 = provider/id（切分一次，避免逐模型拼串）
    let default_key = default_ref
        .and_then(|r| r.split_once('/'))
        .map(|(p, id)| (p.to_string(), id.to_string()));
    let default_prov = default_key.as_ref().map(|(p, _)| p.as_str());

    // Provider 出现序（去重）
    let mut providers: Vec<&str> = Vec::new();
    for &ix in cand {
        let p = models[ix].provider.as_str();
        if !providers.contains(&p) {
            providers.push(p);
        }
    }
    // 默认商置顶（不在候选里就不置顶）；其余按显示名字典序
    let mut rest: Vec<&str> = providers
        .iter()
        .copied()
        .filter(|p| Some(*p) != default_prov)
        .collect();
    rest.sort_by_key(|p| crate::ui::provider_display_name(p).to_lowercase());
    let mut ordered: Vec<&str> = Vec::with_capacity(providers.len());
    if let Some(dp) = default_prov
        && providers.contains(&dp)
    {
        ordered.push(dp);
    }
    ordered.extend(rest);

    // 组内：默认模型置首，其余保持候选序
    let mut out = Vec::with_capacity(cand.len());
    for p in ordered {
        let mut group: Vec<usize> = cand
            .iter()
            .copied()
            .filter(|ix| models[*ix].provider == p)
            .collect();
        if let Some((dp, did)) = default_key.as_ref()
            && p == dp.as_str()
            && let Some(pos) = group.iter().position(|ix| models[*ix].id == *did)
        {
            let dm = group.remove(pos);
            group.insert(0, dm);
        }
        out.extend(group);
    }
    out
}

impl Chat {
    /// 自动化入口（无点击坐标）：锚点缺省走面板右下兜底定位（open_mcp_picker 同款）。
    pub(crate) fn open_model_picker(&mut self, cx: &mut gpui::Context<Self>) {
        self.dlg.pill_anchor = None;
        self.picker_open(cx);
    }

    /// 模型钮点击：已开则收，未开则开（锚点 = 模型钮，统一定位）。
    pub(crate) fn toggle_model_menu_at(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.dlg.model_picker.is_some() {
            self.dlg.model_picker = None;
            cx.notify();
            return;
        }
        self.dlg.pill_anchor = Some(crate::PillBtns::anchor(&self.dlg.pill_btn.model, at));
        self.picker_open(cx);
    }

    /// 建菜单实体（两条入口共用；调用方已定锚点）。
    fn picker_open(&mut self, cx: &mut gpui::Context<Self>) {
        // 目录缺失（该项目还没有任何带进程的 runtime 答过）→ 借同 cwd 的
        // 活进程补拉一次；草稿无进程也照常弹出，列表来自 Chat 共享目录
        self.ensure_models_requested(cx);
        let weak = cx.entity().downgrade();
        let weak_change = weak.clone();
        let input =
            cx.new(|cx| TextInput::new(cx).placeholder(crate::i18n::tr("action.filter_models")));
        input.update(cx, |ti, _| {
            ti.set_on_change(Box::new(move |_, cx| {
                let _ = weak_change.update(cx, |c, cx| {
                    // list contents changed with the filter text — restart
                    // keyboard selection from the top
                    if let Some(p) = c.dlg.model_picker.as_mut() {
                        p.sel = 0;
                    }
                    cx.notify();
                });
            }));
            ti.set_on_escape(Box::new(move |cx| {
                // dispatch-lease hazard（原 ModelSelect 同款）：关菜单会丢弃
                // 正在派发的输入实体——defer 到派发周期收尾
                let weak = weak.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.dlg.model_picker = None;
                        cx.notify();
                    });
                });
            }));
        });
        self.dlg.model_picker = Some(ModelPicker {
            input,
            sel: 0,
            scroll: gpui::ScrollHandle::new(),
        });
        cx.notify();
    }

    /// 菜单可见模型：当前项目目录（`catalog_for`）经 enabledModels 白名单
    /// 收窄；过滤文本仅在总数 ≥ [`MODEL_FILTER_MIN`] 时生效（阈值以下不渲染
    /// 过滤框，文本也不参与过滤——两处列表同规则）。返回 (可见, 总数)。
    ///
    /// 返回的 vec 已按**菜单显示序**排好（默认模型置顶，见
    /// [`order_model_indices`]）：键盘选择的行号 = 该 vec 下标 = 屏幕行序，
    /// 三者同源（否则 ↑/↓ 高亮行与 Enter 落到的模型会错位）。
    pub(crate) fn picker_models(
        &self,
        cx: &gpui::App,
    ) -> (Vec<pi_link::protocol::ModelInfo>, usize) {
        let picker_enabled = !self.cfg.mc_state.all_enabled;
        let scoped: Vec<pi_link::protocol::ModelInfo> = self
            .catalog_for(Some(self.rt().read(cx).cwd.as_path()))
            .iter()
            .filter(|m| {
                !picker_enabled || {
                    let r = format!("{}/{}", m.provider, m.id);
                    self.cfg.mc_state.enabled.iter().any(|e| e == &r)
                }
            })
            .cloned()
            .collect();
        let total = scoped.len();
        // 过滤文本仅在总数 ≥ MODEL_FILTER_MIN 时生效（阈值以下不渲染过滤框，
        // 文本也不参与过滤——两处列表同规则）
        let flt = if total < MODEL_FILTER_MIN {
            String::new()
        } else {
            match &self.dlg.model_picker {
                Some(p) => p.input.read(cx).value().trim().to_lowercase(),
                None => String::new(),
            }
        };
        // 候选下标（过文本过滤）→ 显示序下标（默认模型置顶）
        let cand: Vec<usize> = scoped
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                flt.is_empty()
                    || m.id.to_lowercase().contains(&flt)
                    || m.name.to_lowercase().contains(&flt)
                    || m.provider.to_lowercase().contains(&flt)
            })
            .map(|(ix, _)| ix)
            .collect();
        let default_ref = self
            .cfg
            .mc_default_model
            .as_ref()
            .map(|(p, id)| format!("{p}/{id}"));
        let visible = order_model_indices(&scoped, &cand, default_ref.as_deref())
            .into_iter()
            .map(|ix| scoped[ix].clone())
            .collect();
        (visible, total)
    }

    /// 过滤框当前是否渲染（main.rs 焦点保持链用：框不渲染就不抢焦点）。
    pub(crate) fn picker_filter_shown(&self, cx: &gpui::App) -> bool {
        self.picker_models(cx).1 >= MODEL_FILTER_MIN
    }

    /// 分组行展开（组头 + 模型行交错）：Provider 按**入参出现序**分组
    /// （入参已由 [`Chat::picker_models`] 排成显示序——默认模型所在商在前），
    /// 组内保持入参序。组序不再二次排序，避免把置顶的默认商打回原位。
    pub(crate) fn group_entries(models: &[pi_link::protocol::ModelInfo]) -> Vec<ModelGroupEntry> {
        let mut providers: Vec<&str> = Vec::new();
        for m in models {
            if !providers.contains(&m.provider.as_str()) {
                providers.push(m.provider.as_str());
            }
        }
        let mut out = Vec::new();
        for p in providers {
            out.push(ModelGroupEntry::Header(p.to_uppercase()));
            for (ix, m) in models.iter().enumerate() {
                if m.provider == p {
                    out.push(ModelGroupEntry::Model(ix));
                }
            }
        }
        out
    }

    /// ↑/↓ on the picker: move the keyboard selection (clamped to model rows).
    pub(crate) fn move_model_sel(&mut self, delta: i32, cx: &mut Context<Self>) {
        let (models, _) = self.picker_models(cx);
        let n = models.len().min(crate::MODEL_PICKER_ROWS);
        if n == 0 {
            return;
        }
        let sel = match self.dlg.model_picker.as_mut() {
            Some(p) => {
                p.sel = ((p.sel as i32) + delta).clamp(0, n as i32 - 1) as usize;
                p.sel
            }
            None => return,
        };
        self.reveal_model_sel(sel, cx);
        cx.notify();
    }

    /// 选中行滚进可视区（列表可滚动后必需：↓ 走出可视区时高亮会消失）。
    /// 只在出界时动偏移（已可见则不碰视口），gpui 布局阶段会夹住越界偏移。
    fn reveal_model_sel(&self, sel: usize, cx: &mut Context<Self>) {
        let Some(p) = self.dlg.model_picker.as_ref() else {
            return;
        };
        let view_h = p.scroll.bounds().size.height.to_f64() as f32;
        if view_h <= 0. {
            return; // 还没布局（首帧）→ 无视口尺寸可算
        }
        let entries = Chat::group_entries(&self.picker_models(cx).0);
        let y = sel_row_y(&entries, sel);
        let cur = (-p.scroll.offset().y).to_f64() as f32; // 当前已滚下的距离（向下为正）
        let new_off = if y < cur {
            y
        } else if y + ROW_H > cur + view_h {
            y + ROW_H - view_h
        } else {
            cur
        };
        if (new_off - cur).abs() > 0.5 {
            p.scroll.set_offset(point(px(0.), px(-new_off)));
        }
    }

    /// Enter on the picker: switch the session to the highlighted model and
    /// close（会话级切换，不落盘——落盘的是五角星）。
    pub(crate) fn apply_model_sel(&mut self, cx: &mut Context<Self>) {
        let sel = match &self.dlg.model_picker {
            Some(p) => p.sel,
            None => return,
        };
        let pick = self
            .picker_models(cx)
            .0
            .get(sel)
            .map(|m| (m.provider.clone(), m.id.clone()));
        if let Some((provider, id)) = pick {
            self.rt()
                .update(cx, |r, cx| r.select_model(provider, id, cx));
        }
        self.dlg.model_picker = None;
        cx.notify();
    }
}

/// 组头行：provider id 大写（截图同款 DEEPSEEK / ZAI-CODING-CN / FREEFLOW）。
pub(crate) fn group_header_row(
    provider: &str,
    pad_x: f32,
    h: f32,
    t: &'static crate::appearance::theme::Theme,
) -> AnyElement {
    div()
        .h(px(h))
        .px(px(pad_x))
        .flex()
        .items_center()
        .child(
            div()
                .text_size(crate::appearance::ui_size(9.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(provider.to_string())),
        )
        .into_any_element()
}

/// 整行点击动作（菜单 = 切换会话模型并收起；设置·下列表无行点击）。
pub(crate) type ModelRowAction = Box<dyn Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static>;

/// 模型行（两处共用）：✓槽位（当前会话模型）+ 文本 + pin 徽章 + 五角星
/// （app-settings 默认）。槽位恒占位——组头 20px 的层级缩进由它保证。
#[allow(clippy::too_many_arguments)]
pub(crate) fn group_model_row(
    weak: &gpui::WeakEntity<Chat>,
    m: &pi_link::protocol::ModelInfo,
    is_default: bool,
    is_current: bool,
    selected: bool,
    clickable: bool,
    row_h: f32,
    pad_x: f32,
    pin: Option<String>,
    on_row: Option<ModelRowAction>,
    t: &'static crate::appearance::theme::Theme,
) -> AnyElement {
    let ui = crate::appearance::ui_size;
    let row_weak = weak.clone();
    let star_weak = weak.clone();
    let (dp, di) = (m.provider.clone(), m.id.clone());
    let (sp, si) = (dp.clone(), di.clone());
    let label = if m.name.is_empty() {
        m.id.clone()
    } else {
        m.name.clone()
    };
    let mut row = div()
        .id(SharedString::from(format!("mp-{}-{}", m.provider, m.id)))
        .h(px(row_h))
        // uniform_list/vlist 的行是 fit-content：不写 w_full 右侧星标会贴
        // 着文字（flex_1 无处可撑）；菜单 flex-col 里天然满宽，写了也无害
        .w_full()
        .px(px(pad_x))
        .flex()
        .items_center()
        .gap(px(8.))
        .when(selected, |d| d.bg(rgb(t.bg_selected)))
        .child(if is_current {
            crate::ui::icon("check", 12., t.accent)
        } else {
            div().w(px(12.)).flex_shrink_0().into_any_element()
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap(px(6.))
                .child(
                    div()
                        .text_size(ui(11.))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        // 默认行只用品色区分（2026-10-10 定稿：不加粗）
                        .text_color(rgb(if is_default { t.text } else { t.text_muted }))
                        .child(SharedString::from(label)),
                )
                .children(pin.map(|p| {
                    crate::ui::kit::tag(
                        &p,
                        crate::ui::kit::indigo_bg(),
                        crate::ui::kit::indigo_fg(),
                    )
                })),
        )
        .child(
            // 五角星：实心 = 当前默认（全局唯一）；再点取消
            div()
                .id(SharedString::from(format!(
                    "mp-star-{}-{}",
                    m.provider, m.id
                )))
                .flex_shrink_0()
                .cursor_pointer()
                .hover(|s| s.opacity(0.8))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ =
                        star_weak.update(cx, |c, cx| c.mc_set_default(sp.clone(), si.clone(), cx));
                })
                .child(crate::ui::icon(
                    if is_default { "star-filled" } else { "star" },
                    12.,
                    if is_default { 0xf5a623 } else { t.text_faint },
                )),
        );
    if clickable {
        row = row
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                if let Some(on_row) = &on_row {
                    let _ = row_weak.update(cx, |c, cx| on_row(c, cx));
                }
            });
    }
    row.into_any_element()
}

/// 面板元素（`None` = 没开）。挂在 app root 上，锚点同 MCP 菜单。
pub(crate) fn view(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &gpui::Window,
    cx: &gpui::App,
) -> Option<AnyElement> {
    let picker = chat.dlg.model_picker.as_ref()?;
    let t = T();
    let ui = crate::appearance::ui_size;

    let (models, total) = chat.picker_models(cx);
    let show_filter = total >= MODEL_FILTER_MIN;
    let entries = Chat::group_entries(&models);
    let current = chat
        .rt()
        .read(cx)
        .state
        .as_ref()
        .and_then(|s| s.model.as_ref())
        .map(|m| format!("{}/{}", m.provider, m.id));
    let default_ref = chat
        .cfg
        .mc_default_model
        .as_ref()
        .map(|(p, id)| format!("{p}/{id}"));

    // sel（模型行序）→ 展开行的绝对下标（entries 里第 sel 个 Model 行）
    let mut sel_abs = None;
    {
        let mut seen = 0usize;
        for (abs, e) in entries.iter().enumerate() {
            if matches!(e, ModelGroupEntry::Model(_)) {
                if seen == picker.sel {
                    sel_abs = Some(abs);
                    break;
                }
                seen += 1;
            }
        }
    }

    let mut body = div().flex().flex_col();
    for (abs, e) in entries.iter().enumerate() {
        match e {
            ModelGroupEntry::Header(name) => {
                body = body.child(group_header_row(name, 10., ROW_H * 0.8, t));
            }
            ModelGroupEntry::Model(ix) => {
                let m = &models[*ix];
                let selected = sel_abs == Some(abs);
                let r = format!("{}/{}", m.provider, m.id);
                let (dp, di) = (m.provider.clone(), m.id.clone());
                body = body.child(group_model_row(
                    weak,
                    m,
                    default_ref.as_deref() == Some(r.as_str()),
                    current.as_deref() == Some(r.as_str()),
                    selected,
                    true,
                    ROW_H,
                    10.,
                    None,
                    Some(Box::new(move |c, cx| {
                        c.rt()
                            .update(cx, |r, cx| r.select_model(dp.clone(), di.clone(), cx));
                        // picking is also the dismissal gesture
                        c.dlg.model_picker = None;
                        cx.notify();
                    })),
                    t,
                ));
            }
        }
    }
    if models.is_empty() {
        body = body.child(
            div()
                .px(px(10.))
                .py(px(12.))
                .text_size(ui(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(crate::i18n::tr(
                    "action.no_models_match",
                ))),
        );
    }
    // 列表内容高：组头 0.8×行高 + 模型行 1×行高（精确值，不用行数估）；
    // 空列表 = 空态提示行（上下 12px 留白 + 一行文字）
    let content_h: f32 = if entries.is_empty() {
        46.
    } else {
        entries
            .iter()
            .map(|e| match e {
                ModelGroupEntry::Header(_) => ROW_H * 0.8,
                ModelGroupEntry::Model(_) => ROW_H,
            })
            .sum()
    };

    // 锚点算法与 MCP 菜单一致（统一定位：按钮居中 + 5px）
    let vp = window.viewport_size();
    let panel_w = px(380.);
    let (bottom, left) =
        crate::session::picker_shell::pill_anchor_edges(chat.dlg.pill_anchor, vp, panel_w);

    // 列表高 = min(内容高, 10 行上限, 视口内可用高)：内容超高时列表滚动
    // （滚动条常显），卡片也不会顶出窗口上沿。
    // 可用高 = 视口高 - 底距 - 过滤块(10+30+6) - 列表底部留白(6)
    let filter_h = if show_filter { 46. } else { 0. };
    let avail_h = (vp.height - bottom - px(filter_h + 6.)).max(px(80.));
    let list_h = content_h.min(ROW_H * 10.).min(avail_h.to_f64() as f32);
    // 内容超出可视区 = 可滚动：右缘多留 4px 给滚动条命中带（star 不被抢点击）
    let scrollable = content_h > list_h - 6. + 0.5;
    let row_pr = if scrollable { 14. } else { 10. };

    let layer = crate::ui::menu::menu_layer(None, weak, |c, _cx| c.dlg.model_picker = None);

    let weak_apply = weak.clone();
    let weak_up = weak.clone();
    let weak_down = weak.clone();
    let card = crate::ui::overlay::stop_click(
        crate::session::picker_shell::picker_card(bottom, left, panel_w, t)
            // Enter：与原 ModelSelect 同路——过滤输入单行模式的 Enter 会向上
            // 冒泡，在卡片上拦下；apply 会丢弃派发中的实体，defer 出派发周期
            .on_key_down(move |ev: &gpui::KeyDownEvent, _w, cx| {
                if ev.keystroke.key != "enter" {
                    return;
                }
                cx.stop_propagation();
                let weak = weak_apply.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| c.apply_model_sel(cx));
                });
            })
            .on_action(move |_: &crate::ComposerUp, _w, cx| {
                let _ = weak_up.update(cx, |c, cx| c.move_model_sel(-1, cx));
            })
            .on_action(move |_: &crate::ComposerDown, _w, cx| {
                let _ = weak_down.update(cx, |c, cx| c.move_model_sel(1, cx));
            })
            // 无标题；过滤框按 MODEL_FILTER_MIN 阈值出现（占位壳保持节奏）
            .child(
                div()
                    .px(px(10.))
                    .pt(px(10.))
                    .pb(px(6.))
                    .children(show_filter.then(|| picker.input.clone())),
            )
            .child(
                // 列表 + 滚动条：relative 壳让滚动条贴列表右缘（不随内容滚走），
                // 可滚动即常显（menu_scrollbar 变体：可滚动才画）
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id("model-picker-list")
                            .pb(px(6.))
                            .h(px(list_h))
                            .overflow_y_scroll()
                            .track_scroll(&picker.scroll)
                            .flex()
                            .flex_col()
                            // 右侧让出滚动条位（可滚动时多留 4px：滚动条命中带
                            // 不压住模型名/星标）
                            .pr(px(row_pr))
                            .child(body),
                    )
                    .child(
                        div()
                            .absolute()
                            .top(px(2.))
                            .bottom(px(2.))
                            .right(px(0.))
                            .w(px(10.))
                            .child(crate::ui::psp_scrollbar::menu_scrollbar(&picker.scroll)),
                    ),
            ),
    );
    Some(layer.child(card).into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(provider: &str, id: &str) -> pi_link::protocol::ModelInfo {
        pi_link::protocol::ModelInfo {
            id: id.to_string(),
            name: id.to_string(),
            provider: provider.to_string(),
            context_window: None,
            max_tokens: None,
            reasoning: None,
            input: None,
        }
    }

    /// 候选下标 → 显示序下标（0 起全序）。
    fn order(models: &[pi_link::protocol::ModelInfo], default_ref: Option<&str>) -> Vec<usize> {
        let cand: Vec<usize> = (0..models.len()).collect();
        order_model_indices(models, &cand, default_ref)
    }

    #[test]
    fn default_model_group_goes_first() {
        let models = vec![
            m("openai", "gpt-5"),
            m("deepseek", "ds-v4"),
            m("openai", "o3"),
        ];
        // 默认 = deepseek/ds-v4 → 该商组置顶、组内置首
        let ord = order(&models, Some("deepseek/ds-v4"));
        assert_eq!(ord, vec![1, 0, 2]);
    }

    #[test]
    fn default_model_first_inside_group() {
        let models = vec![m("openai", "a"), m("openai", "b"), m("openai", "c")];
        let ord = order(&models, Some("openai/b"));
        assert_eq!(ord, vec![1, 0, 2]);
    }

    #[test]
    fn other_providers_sorted_by_display_name() {
        let models = vec![
            m("zai-coding-cn", "glm"),
            m("deepseek", "ds"),
            m("openai", "gpt"),
        ];
        // 无默认模型 → 按 Provider 显示名：DeepSeek / OpenAI / Z.AI Coding CN
        let ord = order(&models, None);
        assert_eq!(ord, vec![1, 2, 0]);
    }

    #[test]
    fn default_not_in_candidates_is_ignored() {
        let models = vec![m("openai", "gpt"), m("deepseek", "ds")];
        // 默认模型已被过滤掉 → 退回纯显示名序
        let ord = order(&models, Some("anthropic/claude"));
        assert_eq!(ord, vec![1, 0]);
    }

    #[test]
    fn empty_and_single_are_untouched() {
        let models: Vec<pi_link::protocol::ModelInfo> = vec![];
        assert!(order(&models, Some("openai/gpt")).is_empty());
        let models = vec![m("openai", "gpt")];
        assert_eq!(order(&models, Some("openai/gpt")), vec![0]);
    }
}
