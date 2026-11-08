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
use super::picker_shell::PAD;
use crate::TextInput;
use crate::appearance::theme::theme as T;

/// 菜单行高（限高 = `ROW_H × 10`，与 mcp_picker 一致）。
pub(crate) const ROW_H: f32 = 30.;
/// 过滤框显示阈值：模型数少于该值不渲染过滤输入（菜单/设置·下列表同规则）。
pub(crate) const MODEL_FILTER_MIN: usize = 10;

#[cfg(test)]
mod tests;
/// 打开时的键盘选择位 + 滚动位（过滤文本变化时 sel 归零）。
pub(crate) mod view;
pub(crate) use view::{
    ModelPickAction, group_header_row, group_model_row, model_menu_body, model_menu_content_h, view,
};
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
        self.picker_models_with(cx, None)
    }

    /// [`Chat::picker_models`] 的参数化版：`flt_override` = 显式过滤文本
    /// （设置·子代理页模型菜单传表单过滤框值；composer 传 None = 读菜单
    /// 输入框）。阈值规则与显示序完全一致。
    pub(crate) fn picker_models_with(
        &self,
        cx: &gpui::App,
        flt_override: Option<String>,
    ) -> (Vec<pi_link::protocol::ModelInfo>, usize) {
        let cwd = self.rt().read(cx).cwd.clone();
        // composer 路径：显式过滤缺省时读菜单输入框
        let flt = flt_override.or_else(|| match &self.dlg.model_picker {
            Some(p) => Some(p.input.read(cx).value().to_string()),
            None => None,
        });
        self.picker_models_from(Some(&cwd), flt)
    }

    /// [`Chat::picker_models`] 的 cx-free 版：cwd 显式给定（设置·子代理页
    /// 传 None = 不做项目级收窄，模型目录来自 Chat 共享账本）。过滤文本
    /// 仅在总数 ≥ [`MODEL_FILTER_MIN`] 时生效；返回 (可见, 总数)，可见已按
    /// 菜单显示序排好（默认模型置顶）。
    pub(crate) fn picker_models_from(
        &self,
        cwd: Option<&std::path::Path>,
        flt_override: Option<String>,
    ) -> (Vec<pi_link::protocol::ModelInfo>, usize) {
        let picker_enabled = !self.cfg.mc_state.all_enabled;
        // 统一接口（2026-11-01）：与设置页同一模型源 model_listing（账本并入
        // + 已配置内置商静态 + **侧栏成员过滤**——无凭据的商不再出现在
        // 菜单里），cwd 由调用方给定（composer 用当前会话，设置页传 None）
        let scoped: Vec<pi_link::protocol::ModelInfo> = self
            .model_listing(cwd)
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
        // 文本也不参与过滤——两处列表同规则）；显式值与菜单输入框统一在此
        // trim + 小写归一
        let flt = if total < MODEL_FILTER_MIN {
            String::new()
        } else {
            flt_override
                .map(|f| f.trim().to_lowercase())
                .unwrap_or_default()
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
