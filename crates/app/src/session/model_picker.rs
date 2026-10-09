//! 模型菜单（042 定稿，对齐 模型菜单.bmp）+ 分组模型列表共享组件。
//!
//! 菜单 = 锚定模型钮的 pill 卡片（弃用原居中 ModelSelect 弹窗）：过滤输入
//! 与 ↑/↓/Enter 键盘导航保留，行 = 左✓（当前会话模型）+ 显示名 + 右侧
//! 五角星（app-settings 默认，与设置页下列表同一份、双向同步）。
//!
//! 组件面（`group_entries` / `group_header_row` / `group_model_row` /
//! `MODEL_FILTER_MIN`）为菜单与设置·模型页下列表共用：分组结构、组头样式、
//! 行缩进、五角星、过滤阈值都在这一份实现，两处不许再各自漂移。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb};

use super::Chat;
use crate::theme::theme as T;
use crate::TextInput;

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

/// 分组列表的一个可见行：组头或模型（Model(ix) 索引进模型 vec——键盘
/// ↑/↓ 只在模型行间移动，组头自动跳过）。
pub(crate) enum ModelGroupEntry {
    Header(String),
    Model(usize),
}

impl Chat {
    /// 自动化入口（无点击坐标）：锚点缺省走面板右下兜底定位（open_mcp_picker 同款）。
    pub(crate) fn open_model_picker(&mut self, cx: &mut gpui::Context<Self>) {
        self.pill_anchor = None;
        self.picker_open(cx);
    }

    /// 模型钮点击：已开则收，未开则开（锚点 = 模型钮，统一定位）。
    pub(crate) fn toggle_model_menu_at(
        &mut self,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut gpui::Context<Self>,
    ) {
        if self.model_picker.is_some() {
            self.model_picker = None;
            cx.notify();
            return;
        }
        self.pill_anchor = Some(crate::PillBtns::anchor(&self.pill_btn.model, at));
        self.picker_open(cx);
    }

    /// 建菜单实体（两条入口共用；调用方已定锚点）。
    fn picker_open(&mut self, cx: &mut gpui::Context<Self>) {
        // 目录缺失（该项目还没有任何带进程的 runtime 答过）→ 借同 cwd 的
        // 活进程补拉一次；草稿无进程也照常弹出，列表来自 Chat 共享目录
        self.ensure_models_requested(cx);
        let weak = cx.entity().downgrade();
        let weak_change = weak.clone();
        let input = cx.new(|cx| TextInput::new(cx).placeholder(crate::i18n::tr("过滤模型...")));
        input.update(cx, |ti, _| {
            ti.set_on_change(Box::new(move |_, cx| {
                let _ = weak_change.update(cx, |c, cx| {
                    // list contents changed with the filter text — restart
                    // keyboard selection from the top
                    if let Some(p) = c.model_picker.as_mut() {
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
                        c.model_picker = None;
                        cx.notify();
                    });
                });
            }));
        });
        self.model_picker = Some(ModelPicker {
            input,
            sel: 0,
            scroll: gpui::ScrollHandle::new(),
        });
        cx.notify();
    }

    /// 菜单可见模型：当前项目目录（`catalog_for`）经 enabledModels 白名单
    /// 收窄；过滤文本仅在总数 ≥ [`MODEL_FILTER_MIN`] 时生效（阈值以下不渲染
    /// 过滤框，文本也不参与过滤——两处列表同规则）。返回 (可见, 总数)。
    pub(crate) fn picker_models(&self, cx: &gpui::App) -> (Vec<pi_link::protocol::ModelInfo>, usize) {
        let picker_enabled = !self.mc_state.all_enabled;
        let scoped: Vec<pi_link::protocol::ModelInfo> = self
            .catalog_for(&self.rt().read(cx).cwd)
            .iter()
            .filter(|m| {
                !picker_enabled || {
                    let r = format!("{}/{}", m.provider, m.id);
                    self.mc_state.enabled.iter().any(|e| e == &r)
                }
            })
            .cloned()
            .collect();
        let total = scoped.len();
        if total < MODEL_FILTER_MIN {
            return (scoped, total);
        }
        let flt = match &self.model_picker {
            Some(p) => p.input.read(cx).value().trim().to_lowercase(),
            None => String::new(),
        };
        let visible = scoped
            .into_iter()
            .filter(|m| {
                flt.is_empty()
                    || m.id.to_lowercase().contains(&flt)
                    || m.name.to_lowercase().contains(&flt)
                    || m.provider.to_lowercase().contains(&flt)
            })
            .collect();
        (visible, total)
    }

    /// 过滤框当前是否渲染（main.rs 焦点保持链用：框不渲染就不抢焦点）。
    pub(crate) fn picker_filter_shown(&self, cx: &gpui::App) -> bool {
        self.picker_models(cx).1 >= MODEL_FILTER_MIN
    }

    /// 分组行展开（组头 + 模型行交错；provider 按显示名序，组内保持入参序）。
    pub(crate) fn group_entries(
        models: &[pi_link::protocol::ModelInfo],
    ) -> Vec<ModelGroupEntry> {
        let mut providers: Vec<String> = Vec::new();
        for m in models {
            if !providers.contains(&m.provider) {
                providers.push(m.provider.clone());
            }
        }
        providers.sort_by_key(|p| crate::ui::provider_display_name(p).to_lowercase());
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
        if let Some(p) = self.model_picker.as_mut() {
            p.sel = ((p.sel as i32) + delta).clamp(0, n as i32 - 1) as usize;
            cx.notify();
        }
    }

    /// Enter on the picker: switch the session to the highlighted model and
    /// close（会话级切换，不落盘——落盘的是五角星）。
    pub(crate) fn apply_model_sel(&mut self, cx: &mut Context<Self>) {
        let sel = match &self.model_picker {
            Some(p) => p.sel,
            None => return,
        };
        let pick = self
            .picker_models(cx)
            .0
            .get(sel)
            .map(|m| (m.provider.clone(), m.id.clone()));
        if let Some((provider, id)) = pick {
            self.rt().update(cx, |r, cx| r.select_model(provider, id, cx));
        }
        self.model_picker = None;
        cx.notify();
    }
}

/// 组头行：provider id 大写（截图同款 DEEPSEEK / ZAI-CODING-CN / FREEFLOW）。
pub(crate) fn group_header_row(
    provider: &str,
    pad_x: f32,
    h: f32,
    t: &'static crate::theme::Theme,
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
    t: &'static crate::theme::Theme,
) -> AnyElement {
    let ui = crate::appearance::ui_size;
    let row_weak = weak.clone();
    let star_weak = weak.clone();
    let (dp, di) = (m.provider.clone(), m.id.clone());
    let (sp, si) = (dp.clone(), di.clone());
    let label = if m.name.is_empty() { m.id.clone() } else { m.name.clone() };
    let mut row = div()
        .id(SharedString::from(format!("mp-{}-{}", m.provider, m.id)))
        .h(px(row_h))
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
                        .text_color(rgb(if is_default { t.text } else { t.text_muted }))
                        .font_weight(if is_default {
                            gpui::FontWeight::SEMIBOLD
                        } else {
                            gpui::FontWeight::NORMAL
                        })
                        .child(SharedString::from(label)),
                )
                .children(pin.map(|p| {
                    div()
                        .px(px(4.))
                        .py(px(1.))
                        .rounded(px(3.))
                        .bg(crate::settings::widgets::indigo_bg())
                        .text_size(ui(9.))
                        .text_color(crate::settings::widgets::indigo_fg())
                        .child(SharedString::from(p))
                        .into_any_element()
                })),
        )
        .child(
            // 五角星：实心 = 当前默认（全局唯一）；再点取消
            div()
                .id(SharedString::from(format!("mp-star-{}-{}", m.provider, m.id)))
                .flex_shrink_0()
                .cursor_pointer()
                .hover(|s| s.opacity(0.8))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    let _ = star_weak.update(cx, |c, cx| {
                        c.mc_set_default(sp.clone(), si.clone(), cx)
                    });
                })
                .child(crate::ui::icon(
                    if is_default { "star-filled" } else { "star" },
                    12.,
                    if is_default { 0xf5a623 } else { t.text_faint },
                )),
        );
    if clickable {
        row = row.cursor_pointer().hover(|s| s.bg(rgb(t.bg_hover))).on_mouse_down(
            MouseButton::Left,
            move |_, _, cx| {
                cx.stop_propagation();
                if let Some(on_row) = &on_row {
                    let _ = row_weak.update(cx, |c, cx| on_row(c, cx));
                }
            },
        );
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
    let picker = chat.model_picker.as_ref()?;
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
                        c.rt().update(cx, |r, cx| r.select_model(dp.clone(), di.clone(), cx));
                        // picking is also the dismissal gesture
                        c.model_picker = None;
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
                .child(SharedString::from(crate::i18n::tr("no models match"))),
        );
    }
    let n_rows = entries.len().max(1);
    let list_h = (n_rows as f32 * ROW_H).min(ROW_H * 10.);

    // 锚点算法与 MCP 菜单一致（统一定位：按钮居中 + 5px）
    let vp = window.viewport_size();
    let gap = px(5.);
    let panel_w = px(340.);
    let (bottom, left) = match chat.pill_anchor {
        Some(a) => {
            let bottom = (vp.height - a.top + gap).max(px(8.));
            let mut left = a.center_x - panel_w / 2.;
            if left + panel_w > vp.width - px(8.) {
                left = vp.width - panel_w - px(8.);
            }
            (bottom, left.max(px(8.)))
        }
        None => (px(64.), vp.width - panel_w - px(24.)),
    };

    let dismiss_weak = weak.clone();
    let layer = crate::ui::overlay::layer(false, None, move |_w, cx| {
        let _ = dismiss_weak.update(cx, |c, _cx| c.model_picker = None);
    });

    let weak_apply = weak.clone();
    let weak_up = weak.clone();
    let weak_down = weak.clone();
    let card = crate::ui::overlay::stop_click(
        div()
            .absolute()
            .bottom(bottom)
            .left(left)
            .w(panel_w)
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.bg))
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
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
                div()
                    .id("model-picker-list")
                    .pb(px(6.))
                    .h(px(list_h))
                    .overflow_y_scroll()
                    .track_scroll(&picker.scroll)
                    .flex()
                    .flex_col()
                    .child(body),
            ),
    );
    Some(layer.child(card).into_any_element())
}
