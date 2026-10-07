//! composer (v57 输入组件化)：真输入框 = gpui-component InputState 多行
//! 模式（ui/composer_input.rs 门面）——点击定位光标、左右键移动、多行上下
//! 键、选区、IME、undo、超限纵向滚动条全为组件内置，不再手搓 String+绘制
//! 光标。单容器 16px 圆角 1px 边框，宽 = 消息列宽（外层同 px(15) 内边距 +
//! max_w 920），窗口缩放时始终贴合，不再用百分比近似
//! 居中，悬浮胶囊上浮叠在聊天区上（0 高 wrapper 不吞点击/滚轮）。控件行：
//! 左 = 图片 + 工具预设「默认∨」；右 = 上下文用量环 + 模型∨ + 思考∨ +
//! 圆形发送 ↑ / 主题色停止块（运行中）。
//!
//! 按键路由（详见 composer_input.rs 头注）：
//! - Enter → 组件 PressEnter{secondary:false} → on_submit（菜单开=接受补全；
//!   运行中=引导；空闲=发送）
//! - Shift+Enter → wrapper on_key_down（运行中=排队 follow-up，空闲=换行）
//! - ↑/↓/Tab → "Input" 上下文覆盖绑定截获为 ComposerUp/Down/Tab（菜单导航
//!   /历史回溯/补全），非空多行重新派发组件 MoveUp/MoveDown
//! - Escape → wrapper on_key_down（菜单开=取消补全，运行中=中止）

use std::rc::Rc;

use gpui::{AnimationExt, Context, KeyDownEvent, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::ComposerInput;
use crate::PillMenu;
use crate::MenuKind;
use crate::{ComposerDown, ComposerTab, ComposerUp, ComposerPaste};
use crate::i18n::tr;
use crate::services::format::fmt_thousand;
use crate::theme::theme as T;
use crate::ui::{icon, icon_hover};

/// 操作栏字体大小（工具预设/模型/思考统一；单点改这里）。
/// inputpanel 操作栏文字 = 面板设置值（字体大小设置.md §1）
const BAR_FONT: f32 = 12.;

/// 悬停标志更新 + 淡出状态机（乱序免疫）：只按 (was_open, open) 转移，
/// 环→面板交接时两个 hover 事件无论先后都收敛到正确状态。双面全空才
/// 开始淡出（200ms 定时器到期清除，守卫时刻戳防旧定时器误杀新一轮）。
fn ctx_tip_hover(
    chat: &mut Chat,
    ring: bool,
    entered: bool,
    cx: &mut Context<Chat>,
) {
    let was_open = chat.ctx_tip_ring_hover || chat.ctx_tip_panel_hover;
    if ring {
        chat.ctx_tip_ring_hover = entered;
    } else {
        chat.ctx_tip_panel_hover = entered;
    }
    let open = chat.ctx_tip_ring_hover || chat.ctx_tip_panel_hover;
    match (was_open, open) {
        (false, true) => {
            chat.ctx_tip_closing = None;
            cx.notify();
        }
        (true, false) => {
            let t = std::time::Instant::now();
            chat.ctx_tip_closing = Some(t);
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let _ = this.update(cx, |c, cx| {
                    if c.ctx_tip_closing == Some(t) {
                        c.ctx_tip_closing = None;
                        cx.notify();
                    }
                });
            })
            .detach();
            cx.notify();
        }
        _ => {}
    }
}

/// 悬浮详情浮层（挂在 ctx-ring 容器内，absolute 越界浮在聊天区上）：
/// Open 常显；Closing 包一层 160ms 透明度淡出，定时器随后卸载。wrapper
/// 下方 12px 隐形尾迹压住环顶 2px，环→面板悬停交接无死区。
fn ctx_tip_element(
    chat: &mut Chat,
    t: &'static crate::theme::Theme,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let visible =
        chat.ctx_tip_ring_hover || chat.ctx_tip_panel_hover || chat.ctx_tip_closing.is_some();
    if !visible {
        return None;
    }
    let (stats, compacting, can_compact) = {
        let r = chat.rt().read(cx);
        (r.stats.clone(), r.compacting, r.can_compact())
    };
    let closing = chat.ctx_tip_closing.is_some();
    let wrap = div()
        .id("ctx-tip")
        .absolute()
        .bottom(px(24.)) // 环命中区 26px，压顶 2px 防接缝死区
        // 相对圆环居中：向左/右对称扩张 + flex 居中。手算 left 偏移要跟着
        // 环的 padding(4)+负 margin(-4) 一起走，对称扩张则天然以环为轴，
        // 环宽变了也不用改常数。±200 > 320/2 保证面板不被压缩。
        .left(px(-200.))
        .right(px(-200.))
        .pb(px(12.))
        .flex()
        .flex_row()
        .justify_center()
        // hover 挂整层：面板与环之间的空档也属于浮层，鼠标横穿不闪断
        .on_hover(cx.listener(|this, h: &bool, _w, cx| ctx_tip_hover(this, false, *h, cx)))
        // mousedown 拦截只挂面板本体（点面板不穿透到消息列表）；空档区域
        // 无 mousedown handler，点击照常落到下层消息
        .child(
            div()
                .w(px(320.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(ctx_usage_panel(stats, compacting, can_compact, t, cx)),
        );
    let el = if closing {
        wrap.with_animation(
            "ctx-tip-out",
            gpui::Animation::new(std::time::Duration::from_millis(160)),
            |el, delta| el.opacity(1. - delta),
        )
        .into_any_element()
    } else {
        wrap.into_any_element()
    };
    // vendored gpui 的 Style::paint 把边框画在子元素之后——面板挂在胶囊
    // 内部会被胶囊聚焦边框压线（实测横穿面板）；deferred 推迟到整帧末尾
    // 绘制（布局仍在原位，锚定/hover 关系不变，弹层同机制）
    Some(gpui::deferred(el).into_any_element())
}

/// 用量详情面板本体（pi-web session-info-popover 简化版——上下文比例 /
/// token 分项 / 费用 / 缓存命中率）。
fn ctx_usage_panel(
    stats: Option<pi_link::protocol::SessionStats>,
    compacting: bool,
    can_compact: bool,
    t: &'static crate::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    // 上下文用量弹窗：全部文字 = 面板设置值，title 与正文同号
    // （字体大小设置.md §1）
    let row = |label: &str, value: String| -> gpui::AnyElement {
        div()
            .flex()
            .items_center()
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(label.to_string())),
            )
            .child(
                div()
                    .ml_auto()
                    .text_size(crate::appearance::ui_size(12.))
                    .font_family(crate::markdown::MONO_FAMILY)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(value)),
            )
            .into_any_element()
    };
    let section = |title: &str, rows: Vec<gpui::AnyElement>| -> gpui::AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(title.to_string())),
            )
            .children(rows)
            .into_any_element()
    };

    let body: Vec<gpui::AnyElement> = match stats {
        None => vec![
            div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .child(tr("会话尚未产生用量。"))
                .into_any_element(),
        ],
        Some(s) => {
            let fmt_k = |n: u64| -> String { format!("{}K", (n as f64 / 1000.).round() as u64) };
            let ctx_rows = vec![
                row(
                    tr("使用比例"),
                    s.context_percent
                        .map(|p| format!("{p:.1}%"))
                        .unwrap_or_else(|| "\u{2014}".into()),
                ),
                row(
                    tr("上下文 token"),
                    match (s.context_tokens, s.context_window) {
                        (Some(t), Some(w)) => format!("{} / {}", fmt_k(t), fmt_k(w)),
                        (Some(t), None) => fmt_k(t),
                        _ => "\u{2014}".into(),
                    },
                ),
            ];
            let token_rows = vec![
                row(tr("输入"), fmt_thousand(s.input)),
                row(tr("输出"), fmt_thousand(s.output)),
                row(tr("缓存读"), fmt_thousand(s.cache_read)),
                row(tr("缓存写"), fmt_thousand(s.cache_write)),
                row(tr("总计"), fmt_thousand(s.tokens_total)),
            ];
            let mut cost_rows = vec![row(tr("累计费用"), format!("${:.4}", s.cost))];
            if let Some(r) = s.cache_hit_rate() {
                cost_rows.push(row(tr("缓存命中率"), format!("{:.1}%", r * 100.)));
            }
            vec![
                section(tr("上下文"), ctx_rows),
                section(tr("Token 累计"), token_rows),
                section(tr("费用"), cost_rows),
            ]
        }
    };

    // 「压 缩」字间插窄空格（gpui 无 letter_spacing，字符串层处理；
    // 「压缩中…」多字，保持原样不插）
    let compact_label = {
        let raw = if compacting { tr("压缩中…") } else { tr("压缩") };
        let mut cs = raw.chars();
        match (cs.next(), cs.next(), cs.next()) {
            (Some(a), Some(b), None) => format!("{a}\u{2009}{b}"),
            _ => raw.to_string(),
        }
    };

    div()
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg))
        .shadow_lg()
        .p(px(12.))
        .flex()
        .flex_col()
        .gap(px(12.))
        .children(body)
        .child(
            // 底部手动压缩按钮：无门槛（点不点由用户决定），描边按钮
            // 无常态底色（hover 才出 bg_hover）；固定高 34 + items_center
            // 保证文字在框内垂直居中。compacting 仅防连点。
            div()
                .id("ctx-compact")
                .h(px(34.))
                .w_full()
                .rounded(px(6.))
                .border_1()
                .border_color(rgb(t.border))
                .flex()
                .items_center()
                .justify_center()
                .text_size(crate::appearance::ui_size(12.))
                .when(can_compact, |s| s.cursor_pointer())
                .text_color(rgb(if compacting {
                    t.text_muted
                } else if can_compact {
                    t.accent
                } else {
                    t.text_faint
                }))
                .hover(move |s| if can_compact { s.bg(rgb(t.bg_hover)) } else { s })
                .on_mouse_down(MouseButton::Left, cx.listener(
                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                        // 双重守卫：按钮禁用态之外再查一次 runtime（agent
                        // 跑动中 compact 会中止当前 turn）
                        if this.rt().read(cx).can_compact() {
                            this.rt().update(cx, |r, cx| r.compact(cx));
                        }
                    },
                ))
                .child(SharedString::from(compact_label)),
        )
}

/// 插件按钮（034 定稿）：工具胶囊右边，**仅「自定义」档激活**。
/// 点击开合三层勾选菜单（已选中 / 项目未选中 / 全局未选中），勾选后「选择并切换」
/// 才重绑进程；非自定义档置灰不可点。
fn plugin_button(
    chat: &mut Chat,
    t: &'static crate::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let ui = crate::appearance::ui_size;
    // 激活条件只有一个：**当前档是「自定义」**（用户定稿）。运行中也能点开看，
    // 真换绑由 `mc_set_tools_preset`/confirm 的「运行中不能更换工具预设」拦。
    let (is_custom, n, open) = {
        let r = chat.rt().read(cx);
        (
            r.tool_preset_key() == "custom",
            r.ext_sources.len(),
            chat.plugin_picker.is_some(),
        )
    };
    let active = is_custom;
    let label = if n > 0 {
        format!("{}({n})", crate::i18n::tr("插件"))
    } else {
        crate::i18n::tr("插件").to_string()
    };
    let color = if !active {
        t.text_faint
    } else if open {
        t.accent
    } else {
        t.text_muted
    };
    let mut el = div()
        .id("plugins-menu")
        .h(px(28.))
        .flex()
        .items_center()
        .gap(px(5.))
        .rounded(px(8.))
        .text_size(ui(BAR_FONT))
        .text_color(rgb(color))
        .child(icon_hover("plug", 13., color))
        .child(SharedString::from(label));
    if active {
        el = el
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &gpui::MouseDownEvent, _w, cx| {
                    this.toggle_plugin_menu_at(event.position, cx);
                }),
            );
    }
    el.into_any_element()
}

/// `hero` = 012 新会话页模式：胶囊走正常流（由新会话页内容簇摆位），
/// 不再 0 高 + 绝对定位贴聊天区底。
pub(crate) fn input_area(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    streaming: bool,
    input_focused: bool,
    hero: bool,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let can_queue = !chat.input.is_empty() || !chat.pending_images.is_empty();
    // 新会话默认值（pi selectInitialModelScope parity：settings 默认模型
    // 在 scope 内 → 用它，否则 scope 首个；思考档 pin > per-model > 全局）
    // ——草稿态（无 state）的 pill 显示就落在它身上，进程起来后由 pi 的
    // get_state 接管
    let (draft_model, draft_thinking) = chat.new_session_default();
    let (model_label, thinking_label, tools_label, ctx_pct, compacting) = {
        let r = chat.rt().read(cx);
        let model_label = r
            .state
            .as_ref()
            .and_then(|s| s.model_label())
            .or_else(|| {
                r.pending_model
                    .as_ref()
                    .map(|(p, id)| chat.model_display_name(p, id))
            })
            .or_else(|| {
                draft_model
                    .as_ref()
                    .map(|(p, id)| chat.model_display_name(p, id))
            })
            .unwrap_or_else(|| tr("选择模型").to_string());
        let thinking_label = r
            .state
            .as_ref()
            .and_then(|s| s.thinking_level.clone())
            .or_else(|| r.thinking_override.clone())
            .or_else(|| draft_thinking.clone())
            .unwrap_or_else(|| "auto".to_string());
        (
            model_label,
            thinking_label,
            r.tool_preset_label(),
            r.stats.as_ref().and_then(|s| s.context_percent),
            r.compacting,
        )
    };

    // 输入组件：惰性创建 + 每帧同步占位/值（set_value 同值跳过）
    let composer = ensure_composer(chat, weak, cx);
    // `!` shell 模式（031 pi-web bashMode parity）：去前导空白后以 ! 开头且
    // 无图片附着——边框着色 + 提示行（!! = 输出仅本地）
    let bash_mode = chat.pending_images.is_empty() && chat.input.trim_start().starts_with('!');
    // @ 菜单可能打开：预热/续期文件索引（TTL 内幂等，后台构建）
    if chat.active_menu(cx) == Some(MenuKind::At) {
        chat.ensure_at_index(cx);
    }
    let ph: SharedString = if compacting {
        // 压缩期间输入锁死（下方 on_change/on_submit 丢弃变更），用
        // placeholder 文案告知用户系统在做什么
        tr("上下文压缩中，请等待……").into()
    } else if streaming {
        tr("ESC打断模型").into()
    } else {
        tr("/skill，!shell命令，Shift回车换行").into()
    };
    composer.update(cx, |f, _| f.set_placeholder(Some(ph)));
    // 压缩锁：编辑器组件整块不挂载（无焦点/IME/粘贴），显示 placeholder
    // 样式的压缩提示；草稿保留在 chat.input，解锁即恢复
    composer.update(cx, |f, fcx| f.set_read_only(compacting, fcx));
    let cur = chat.input.clone();
    composer.update(cx, |f, fcx| f.set_value(cur, fcx));
    // 命令名同步给组件：会话进程答案优先，否则启动装载的（项目 skill + 扩展缓存）
    let names: Vec<String> = chat.slash_commands(cx).iter().map(|c| c.name.clone()).collect();
    composer.update(cx, |f, fcx| f.set_command_names(names, fcx));

    // ---- 胶囊 ----
    let mut capsule = div()
        .id("composer")
        .w_full() // 尺寸由外层锚点统一定（消息列宽 / max 920），/ 菜单与胶囊同宽
        .rounded(px(16.))
        .border_1()
        // 边框不随 agent 运行变色（pi-web 的 streaming 琥珀色已去掉）：
        // 焦点态用 accent；`!` shell 模式用工具绿（pi-web tool-bg parity）
        .border_color(if bash_mode {
            gpui::rgba(crate::session::messages::rgba_a(0x22c55e, 0.4))
        } else if input_focused {
            rgb(t.accent)
        } else {
            rgb(t.border)
        })
        .bg(rgb(t.bg))
        .shadow_lg()
        // 鼠标透传修复（psp_overlays 同款方案）：胶囊浮在聊天区上，指针处
        // 双层 hitbox 都命中，事件漏进底下的消息列表（悬停/滚轮/点击穿
        // 透）。occlude 让胶囊自身 bounds 遮挡先绘制的 hitbox；控件行子
        // 元素后绘制不受影响（occlusion 只作用于更早的 hitbox）。
        .occlude()
        .flex()
        .flex_col()
        .pt(px(10.))
        .pb(px(12.)); // 操作栏到下边框的距离
    // 附加图片缩略图（v58 图片粘贴）：56×56 圆角方块，panel 左上角并列、
    // 间隔 5px；右上角悬浮 16px 圆形 X 点击删除（pi-web ImagePreview 布局
    // parity：X 偏移 -4,-4 半嵌在缩略图角上）
    if !chat.pending_images.is_empty() {
        let thumbs: Vec<gpui::AnyElement> = chat
            .pending_images
            .iter()
            .enumerate()
            .map(|(i, img)| {
                let weak_i = weak.clone();
                let weak_open = weak.clone();
                let thumb = img.thumb.clone();
                let thumb_for_open = thumb.clone();
                div()
                    .id(SharedString::from(format!("img-thumb-{i}")))
                    .relative()
                    .size(px(56.))
                    .flex_shrink_0()
                    .child(
                        // 点击看大图（X 是 sibling 不在祖先链，两 handler 互不误触）
                        div()
                            .id(SharedString::from(format!("img-view-{i}")))
                            .size_full()
                            .rounded(px(6.))
                            .border_1()
                            .border_color(rgb(t.border))
                            .bg(rgb(t.bg_panel))
                            .overflow_hidden()
                            .cursor_pointer()
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let Some(image) = thumb_for_open.clone() else { return; };
                                let _ = weak_open.update(cx, |c, cx| {
                                    c.dialog = Some(crate::Dialog::ImagePreview { image });
                                    cx.notify();
                                });
                            })
                            .child(match thumb {
                                Some(image) => gpui::img(image)
                                    .size_full()
                                    .object_fit(gpui::ObjectFit::Cover)
                                    .into_any_element(),
                                None => div().into_any_element(),
                            }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("img-del-{i}")))
                            .absolute()
                            .top(px(-4.))
                            .right(px(-4.))
                            .size(px(16.))
                            .rounded_full()
                            .bg(rgb(t.bg_panel))
                            .border_1()
                            .border_color(rgb(t.border))
                            .flex()
                            .items_center()
                            .justify_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                // X 与图片盒的 hitbox 都含点击点（X 叠在其角
                                // 上），且二者非祖先——不拦截会继续派发到图
                                // 片盒的"开大图"handler（实测删除前弹预览）
                                cx.stop_propagation();
                                let _ = weak_i.update(cx, |c, cx| {
                                    if i < c.pending_images.len() {
                                        c.pending_images.remove(i);
                                    }
                                    cx.notify();
                                });
                            })
                            .child(icon("x", 8., t.text_muted)),
                    )
                    .into_any_element()
            })
            .collect();
        capsule = capsule.child(
            div()
                .w_full()
                .pt(px(4.))
                .px(px(12.))
                .flex()
                .flex_wrap()
                .gap(px(5.))
                .children(thumbs),
        );
    }
    // 编辑区：真输入组件（多行 AutoGrow，超出 max 出纵向滚动条）
    capsule = capsule.child(
        div()
            .w_full()
            .min_h(px(60.))
            .pt(px(2.))
            .pb(px(2.))
            .px(px(6.))
            .child(composer),
    );
    // 按键路由：组件处理的键（Enter/方向键/粘贴/undo…）到不了这里；
    // 这里只接组件无绑定的 Shift+Enter、已传播的 Escape，以及覆盖绑定
    // 截获的 ↑/↓/Tab 动作
    capsule = capsule
        .on_key_down(cx.listener(
            |this, ev: &KeyDownEvent, window, cx| {
                let key = ev.keystroke.key.as_str();                if key == "enter" && ev.keystroke.modifiers.shift {
                    // IME 组合期平台不派发按键（gpui windows events.rs），
                    // 无需组合守卫
                    let streaming = this.rt().read(cx).agent_running;
                    let can_queue =
                        !this.input.is_empty() || !this.pending_images.is_empty();
                    if streaming {
                        if can_queue {
                            this.follow_up_input(cx);
                        }
                    } else if let Some(c) = &this.composer {
                        c.update(cx, |f, fcx| f.insert_newline(window, fcx));
                    }
                    cx.stop_propagation();
                    return;
                }
                if key == "escape" {
                    // 走到这说明组件的 escape 已处理（解除 IME 标记且未清
                    // 草稿）并传播；这里做 app 层语义：菜单取消 / 中止
                    let menu = this.active_menu(cx);
                    if let Some(kind) = menu {
                        if kind == MenuKind::Slash && !this.input.is_empty() {
                            let v = format!("{} ", this.input);
                            this.set_input(v, cx);
                        }
                        // @ 菜单 Esc = 只收起（pi-web parity：token 原样保留，
                        // 下一次输入变化重新打开）
                        this.menu_dismissed = true;
                        this.menu_ix = 0;
                        cx.notify();
                    } else {
                        // `!` 执行中优先中止 bash（rpc abort_bash），其次模型流
                        let bash_running = this.rt().read(cx).bash_running;
                        let streaming = this.rt().read(cx).agent_running;
                        if bash_running {
                            this.abort_bash(cx);
                        } else if streaming {
                            this.abort_stream(cx);
                        }
                    }
                    cx.stop_propagation();
                }
            },
        ))
        .on_action(cx.listener(|this, _: &ComposerUp, window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu(cx).is_some() && !items.is_empty() {
                this.menu_ix = this.menu_ix.saturating_sub(1);
                this.menu_scroll.scroll_to_item(this.menu_ix);
                cx.notify();
            } else if this.input.is_empty() && !this.history.is_empty() {
                let ix = match this.history_ix {
                    None => this.history.len() - 1,
                    Some(i) => i.saturating_sub(1),
                };
                this.history_ix = Some(ix);
                let v = this.history[ix].clone();
                this.set_input(v, cx);
            } else {
                // 非空多行：上移一行交还组件
                window.dispatch_action(Box::new(gpui_component::input::MoveUp), cx);
            }
        }))
        .on_action(cx.listener(|this, _: &ComposerDown, window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu(cx).is_some() && !items.is_empty() {
                this.menu_ix = (this.menu_ix + 1).min(items.len() - 1);
                this.menu_scroll.scroll_to_item(this.menu_ix);
                cx.notify();
            } else if this.input.is_empty() {
                if let Some(i) = this.history_ix {
                    if i + 1 < this.history.len() {
                        this.history_ix = Some(i + 1);
                        let v = this.history[i + 1].clone();
                        this.set_input(v, cx);
                    } else {
                        this.history_ix = None;
                        this.set_input(String::new(), cx);
                    }
                }
            } else {
                window.dispatch_action(Box::new(gpui_component::input::MoveDown), cx);
            }
        }))
        .on_action(cx.listener(|this, _: &ComposerTab, _window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu(cx).is_some() && !items.is_empty() {
                let ix = this.menu_ix.min(items.len() - 1);
                let insert = items[ix].insert.clone();
                this.accept_menu(insert, cx);
            }
        }))
        // Ctrl+V 截获（bind_keys 覆盖组件粘贴绑定）：剪贴板含图 → 附件化；
        // 否则重派发组件 Paste 走原文本粘贴
        .on_action(cx.listener(|this, _: &ComposerPaste, window, cx| {
            if !this.attach_clipboard_image(cx) {
                window.dispatch_action(Box::new(gpui_component::input::Paste), cx);
            }
        }));
    // 控件行
    // 锁定期间强制收起胶囊菜单（工具预设/思考），否则菜单会浮在
    // 锁定的控件上还能点
    if compacting && chat.pill_menu.is_some() {
        chat.pill_menu = None;
    }
    capsule = capsule.child(composer_bar(
        chat,
        streaming,
        can_queue,
        compacting,
        &model_label,
        &thinking_label,
        &tools_label,
        ctx_pct,
        t,
        cx,
    ));

    // 012 新会话页（hero）胶囊走正常流，由新会话页的内容簇摆位；会话界面
    // 则 0 高 wrapper + 胶囊绝对定位悬浮（聊天消息从胶囊后滚过）。/ 菜单两种
    // 模式同构：挂在测量元素正上方（pi-web：bottom 100% + 8px 间隙）
    // 斜杠与 @ 菜单共用 slash_menu_view（视图内部按 kind 分支渲染）
    let slash_open = chat.active_menu(cx).is_some();
    // 「回到最新」按钮：不贴底且有消息时，悬浮在输入面板顶部上方 20px
    let show_scroll_btn = !chat.rt().read(cx).pager.is_at_bottom()
        && !chat.rt().read(cx).messages.is_empty();
    // 胶囊高度测量槽（导航刻度条 033 居中用）；先克隆引用再拼元素，
    // 避免 &chat.composer_h 与下方 slash_menu_view(&mut chat) 借用冲突
    let composer_h_slot = chat.composer_h.clone();
    let inner = div()
        .relative()
        // 必须撑满父宽：为 `w_full`+`max_w(920)` 的测量元素提供宽度基准，
        // 否则 shrink-to-fit 会把胶囊挤成窄条（会话界面实测踩坑）
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .when(slash_open, |d| {
            d.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(gpui::relative(1.))
                    .pb(px(8.))
                    .child(crate::slash_menu_view(chat, weak, t, cx)),
            )
        })
        .child(
            // 测量元素只包胶囊本体（不含回底按钮，按钮显隐会
            // 引入 ±52px 抖动）；/ 菜单是 absolute 不占布局
            crate::ui::measure_height(
                "composer-h",
                &composer_h_slot,
                div()
                    .relative()
                    // 外层（会话界面 px(15) / 新会话页内容簇）已扣除内边距，
                    // 此处宽度 == 消息列宽，再以 920 封顶（pi-web 单一内容列宽）
                    .w_full()
                    .max_w(px(920.))
                    .child(capsule),
            ),
        );
    composer_wrap(inner, hero, show_scroll_btn, weak, t)
}

/// composer 定位包装：会话界面（`hero=false`）= 0 高 wrapper + 胶囊绝对定位
/// 悬浮在聊天区底部之上（聊天消息从胶囊后滚过）；012 新会话页（`hero=true`）
/// = 正常流，由新会话页的内容簇摆位（012 的「下移 10%」）。
fn composer_wrap(
    inner: gpui::Div,
    hero: bool,
    show_scroll_btn: bool,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::theme::Theme,
) -> gpui::AnyElement {
    if hero {
        return div()
            .id("composer-wrap")
            .relative()
            .w_full()
            .flex_shrink_0()
            .child(inner)
            .into_any_element();
    }
    div()
        .id("composer-wrap")
        .relative()
        .h(px(0.))
        .flex_shrink_0()
        .child(
            div()
                .absolute()
                .bottom(px(20.))
                .left_0()
                .right_0()
                // 与消息列表同一水平内边距（session_list 的 px(15)）→ 胶囊列宽
                // 恒等于消息列宽，窗口缩放时同步收缩，不再靠百分比猜
                .px(px(15.))
                .flex()
                .flex_col()
                .items_center()
                // 按钮与胶囊间距 20px：随输入框增高自动上移，永不叠进面板
                .gap(px(20.))
                .when(show_scroll_btn, |d| {
                    d.child(crate::session::scroll_to_bottom_button(weak, t))
                })
                .child(inner),
        )
        .into_any_element()
}

/// 惰性创建 composer 输入组件（InputState 构造需要 &mut Window，挂在渲染
/// 期；回调经 weak 回写 Chat，不改持有结构）。
fn ensure_composer(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut Context<Chat>,
) -> gpui::Entity<ComposerInput> {
    if let Some(c) = &chat.composer {
        return c.clone();
    }
    let weak_change = weak.clone();
    let weak_submit = weak.clone();
    let weak_chip = weak.clone();
    let c = cx.new(|icx| {
        let mut f = ComposerInput::new(icx);
        f.set_on_change(Rc::new(move |v, cx| {
            let _ = weak_change.update(cx, |chat, cx| {
                if chat.input != v {
                    chat.input = v.to_string();
                    chat.menu_ix = 0;
                    // 过滤集变了回顶部，避免新列表停留在旧滚动深处
                    chat.menu_scroll
                        .set_offset(gpui::point(px(0.), px(0.)));
                    cx.notify();
                }
            });
        }));
        f.set_on_submit(Rc::new(move |v, cx| {
            let _ = weak_submit.update(cx, |chat, cx| {
                if chat.rt().read(cx).compacting {
                    return;
                }
                // 先落到剥离换行后的 v 再判菜单：组件 enter() 会先插一个
                // 换行并经 Change 污染 chat.input（含空白使菜单判定失败，
                // 回车被误当发送）
                chat.input = v.to_string();
                // chip 激活（命令已确认成胶囊）时 Enter=发送——裸 "/命令"
                // 的输入形态与菜单过滤态相同，必须以 chip 态区分，否则
                // Enter 会走"接受补全"永远发不出去
                let chip = chat
                    .composer
                    .as_ref()
                    .map(|c| c.read(cx).chip_active())
                    .unwrap_or(false);
                let items = chat.menu_items(cx);
                if !chip && chat.active_menu(cx).is_some() && !items.is_empty() {
                    let ix = chat.menu_ix.min(items.len() - 1);
                    let insert = items[ix].insert.clone();
                    chat.accept_menu(insert, cx);
                    return;
                }
                let can_queue = !chat.input.is_empty() || !chat.pending_images.is_empty();
                let streaming = chat.rt().read(cx).agent_running;
                if streaming {
                    if can_queue {
                        chat.steer_input(cx);
                    }
                } else if can_queue {
                    chat.send_input(cx);
                } else {
                    // 空输入回车：不发送，顺手清掉组件自插的换行
                    chat.set_input(String::new(), cx);
                }
            });
        }));
        f.set_on_chip_backspace(Rc::new(move |cx| {
            let _ = weak_chip.update(cx, |chat, cx| {
                chat.set_input(String::new(), cx);
            });
        }));
        f
    });
    chat.composer = Some(c.clone());
    c
}

/// 控件行：左 = 图片 + 工具预设；右 = 上下文环 + 模型 + 思考 + 发送/停止。
#[allow(clippy::too_many_arguments)]
fn composer_bar(
    chat: &mut Chat,
    streaming: bool,
    can_queue: bool,
    // 压缩锁：整行控件禁用（图片/工具预设/模型/思考/发送）
    locked: bool,
    model_label: &str,
    thinking_label: &str,
    tools_label: &str,
    ctx_pct: Option<f64>,
    t: &'static crate::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    // `!` bash 执行中与模型流共用停止按钮形态（031）
    let bash_running = chat.rt().read(cx).bash_running;
    let running = streaming || bash_running;
    // `!` shell 模式（与 input_area 边框同判定）：左组控件整组换成提示
    let bash_mode = chat.pending_images.is_empty() && chat.input.trim_start().starts_with('!');
    let bash_excluded = chat.input.trim_start().starts_with("!!");
    let thinking_open = chat.pill_menu == Some(PillMenu::Thinking);
    let tools_open = chat.pill_menu == Some(PillMenu::Tools);
    // 工具预设 = spawn 参数（--tools/--no-tools），换它要重绑会话进程 +
    // 整表重读：运行中换会把当前这一轮掐掉、并让「等待模型响应」掉回屏底。
    // 所以 agent 跑动期间置灰禁止（思考强度/模型都是实时 RPC，不动消息表，照旧可用）。
    let tools_locked = locked || streaming;
    let mut bar = div()
        .flex()
        .items_center()
        .px(px(10.))
        .child(if bash_mode {
            // `!` shell 模式：左组换成模式提示（覆盖图片/工具/插件位，
            // 不额外占一行；右侧环/模型/思考/发送照旧）
            div()
                .flex()
                .items_center()
                .gap(px(6.))
                .pl(px(8.))
                .h(px(28.))
                .child(icon("terminal", 13., t.text_dim))
                .child(
                    div()
                        .text_size(crate::appearance::ui_size(BAR_FONT))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(if bash_excluded {
                            tr("Shell · 输出仅本地（!!）")
                        } else {
                            tr("Shell · 输出发给模型")
                        })),
                )
                .into_any_element()
        } else {
        // 左侧：图片 + 工具预设（内容裸宽，组内间距 10px，与右侧一致）
        div().flex().items_center().gap(px(10.)).child(
            // 图片
            div()
                .id("attach-image")
                .pl(px(8.)) // 图片与正文首行左对齐：正文=编辑行6+组件12=18，图片=栏10+8=18
                .flex()
                .items_center()
                .text_color(rgb(if locked { t.text_faint } else { t.text_muted }))
                .when(!locked, |d| d.cursor_pointer())
                .hover(move |s| {
                    if locked { s } else { s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)) }
                })
                .on_mouse_down(MouseButton::Left, cx.listener(
                    |this, _: &gpui::MouseDownEvent, _w, cx| {
                        if this.rt().read(cx).compacting {
                            return;
                        }
                        this.attach_images(cx);
                    },
                ))
                .child(icon_hover("image", 15., if locked { t.text_faint } else { t.text_muted })),
            )
            .child(
            // 工具预设「默认∨」
            div()
                .id("tools-menu")
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(5.))
                .rounded(px(8.))
                .text_size(crate::appearance::ui_size(BAR_FONT))
                .text_color(rgb(if tools_locked {
                    t.text_faint
                } else if tools_open {
                    t.accent
                } else {
                    t.text_muted
                }))
                .when(!tools_locked, |d| d.cursor_pointer())
                .hover(move |s| {
                    if tools_locked {
                        s
                    } else {
                        s.bg(rgb(t.bg_hover)).text_color(rgb(t.text))
                    }
                })
                .on_mouse_down(MouseButton::Left, cx.listener(
                    |this, event: &gpui::MouseDownEvent, _w, cx| {
                        if this.rt().read(cx).compacting {
                            return;
                        }
                        if this.rt().read(cx).agent_running {
                            // 置灰之外再给一句说明：工具预设换不了是因为要重绑进程
                            this.set_status(
                                crate::i18n::tr("运行中不能更换工具预设").to_string(),
                                cx,
                            );
                            return;
                        }
                        this.pill_anchor = Some(event.position);
                        this.pill_menu = match this.pill_menu {
                            Some(PillMenu::Tools) => None,
                            _ => Some(PillMenu::Tools),
                        };
                        cx.notify();
                    },
                ))
                .child(icon_hover("wrench", 13., if tools_open { t.accent } else { t.text_muted }))
                .child(SharedString::from(tools_label.to_string()))
                .child(icon("chevron-down", 10., t.text_dim)),
            )
            // 插件按钮（034 定稿）：**工具胶囊的兄弟节点**，不是子节点——
            // 塞进胶囊里点它会冒泡触发工具菜单（用户踩过）。仅「自定义」档
            // 激活，点开三层勾选菜单（已选中 / 项目未选中 / 全局未选中）。
            .child(plugin_button(chat, t, cx))
            .into_any_element()
        });
    // 右侧：环 + 模型 + 思考 + 发送，按钮间距统一 10px；操作栏左右
    // padding 10px = 发送钮距胶囊边框 10px
    let ctx_tip = ctx_tip_element(chat, t, cx);
    let mut right = div().ml_auto().flex().items_center().gap(px(10.)).child(
        // 上下文用量环：track 在下、实际比例的主题色弧在上（弧 SVG 按百分比
        // 运行时生成，icons/ring-p{1-99}；100% 走静态 ring-100）。悬浮显示
        // 用量详情，挪开淡出（ctx_tip_* 状态机）
        div()
            .id("ctx-ring")
            .relative()
            .p(px(4.))
            .m(px(-4.)) // 命中区扩到 26px，视觉位置不变
            .cursor_pointer()
            .rounded_full()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_hover(cx.listener(|this, h: &bool, _w, cx| ctx_tip_hover(this, true, *h, cx)))
            .child(
                // 同心双环：track 在下、进度弧在上（svg 是 flex 行内子元素
                // 会并排——必须各自绝对定位铺满后居中才叠成同心）
                div()
                    .relative()
                    .size(px(18.))
                    .child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(crate::ui::icon("ring-track", 18., t.bg_selected)),
                    )
                    .children(ctx_pct.map(|p| {
                        let path = if p >= 100. {
                            "icons/ring-100.svg".to_string()
                        } else {
                            format!("icons/ring-p{}.svg", p.round().clamp(1., 99.) as u8)
                        };
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(
                                gpui::svg()
                                    .path(SharedString::from(path))
                                    .text_color(rgb(t.accent))
                                    .size(px(18.)),
                            )
                    }))
                    // 悬浮详情浮层（absolute 越界，浮层随 hover/淡出态挂载）
                    .children(ctx_tip),
            ),
    );
    // 模型 ∨
    right = right.child(
        div()
            .id("open-model-select")
            .h(px(28.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(8.))
            .text_size(crate::appearance::ui_size(BAR_FONT))
            .text_color(rgb(if locked { t.text_faint } else { t.text_muted }))
            .when(!locked, |d| d.cursor_pointer())
            .hover(move |s| {
                if locked { s } else { s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)) }
            })
            .on_mouse_down(MouseButton::Left, cx.listener(
                |this, _: &gpui::MouseDownEvent, _w, cx| {
                    if this.rt().read(cx).compacting {
                        return;
                    }
                    // 目录缺失（该项目还没有任何带进程的 runtime 答过）→
                    // 借同 cwd 的活进程补拉一次；草稿无进程也照常弹出，
                    // 列表来自 Chat 共享目录，不依赖本会话进程
                    this.ensure_models_requested(cx);
                    this.dialog = Some(Chat::model_select_dialog(cx));
                    cx.notify();
                },
            ))
            .child(SharedString::from(model_label.to_string()))
            .child(icon("chevron-down", 10., if locked { t.text_faint } else { t.text_dim })),
    );
    // 思考强度 ∨
    right = right.child(
        div()
            .id("thinking-menu")
            .h(px(28.))
            .flex()
            .items_center()
            .gap(px(5.))
            .rounded(px(8.))
            .text_size(crate::appearance::ui_size(BAR_FONT))
            .text_color(rgb(if locked {
                t.text_faint
            } else if thinking_open {
                t.accent
            } else {
                t.text_muted
            }))
            .when(!locked, |d| d.cursor_pointer())
            .hover(move |s| {
                if locked { s } else { s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)) }
            })
            .on_mouse_down(MouseButton::Left, cx.listener(
                |this, event: &gpui::MouseDownEvent, _w, cx| {
                    if this.rt().read(cx).compacting {
                        return;
                    }
                    this.pill_anchor = Some(event.position);
                    this.pill_menu = match this.pill_menu {
                        Some(PillMenu::Thinking) => None,
                        _ => Some(PillMenu::Thinking),
                    };
                    cx.notify();
                },
            ))
            .child(icon_hover(
                "lightbulb",
                13.,
                if locked {
                    t.text_faint
                } else if thinking_open {
                    t.accent
                } else {
                    t.text_muted
                },
            ))
            .child(SharedString::from(thinking_label.to_string()))
            .child(icon("chevron-down", 10., if locked { t.text_faint } else { t.text_dim })),
    );
    // 圆形发送 ↑（运行中变停止：主题色圆角方块+对比色停止块）；用户定位：
    // 左移 5px、上移 8px
    right = right.child(
        div()
            .id("send")
            .mr(px(2.)) // 右边距=栏padding10+2=12（与下边距对齐）
            .mt(px(-8.))
            .size(px(36.)) // 发送/停止共用直径（用户微调处）
            .rounded(if running { px(9.) } else { px(18.) })
            .flex()
            .items_center()
            .justify_center()
            .when(!locked, |d| d.cursor_pointer())
            .bg(rgb(if locked {
                // 压缩锁：恒灰，不给"可点"的暗示
                t.bg_selected
            } else if running {
                t.accent // 停止态外圈=主题色（用户定稿，非深色）
            } else if can_queue {
                t.accent
            } else {
                t.bg_selected
            }))
            .hover(move |s| if locked { s } else { s.opacity(0.9) })
            .on_mouse_down(MouseButton::Left, cx.listener(
                |this, _: &gpui::MouseDownEvent, _w, cx| {
                    if this.rt().read(cx).compacting {
                        return;
                    }
                    // 与按钮渲染同源：agent_running（事件驱动），快照
                    // is_streaming 恒 false 会把"停止"点成"发送"；
                    // `!` bash 执行中点它 = abort_bash
                    let bash_running = this.rt().read(cx).bash_running;
                    if bash_running {
                        this.abort_bash(cx);
                    } else if this.rt().read(cx).agent_running {
                        this.abort_stream(cx);
                    } else {
                        this.send_input(cx);
                    }
                },
            ))
            .child(if running {
                // 停止方块
                div()
                    .size(px(12.))
                    .rounded(px(2.5))
                    .bg(rgb(t.accent_contrast))
                    .into_any_element()
            } else {
                icon("arrow-up", 15., t.accent_contrast)
            }),
    );
    bar = bar.child(right);
    bar
}
