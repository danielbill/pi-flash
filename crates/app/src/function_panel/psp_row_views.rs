//! psp 行渲染 —— 会话行 / 置顶组头行 / 伪标签占位行。自 mod.rs 内切拆出
//! （check_arch 文件 1500 行上限 + 视图函数 300 行上限），纯结构搬移。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::WithTip;
use crate::ui::tokens::align;
use crate::ui::{icon, spinner};

/// 会话行：15px 状态槽（运行=旋转圈 / 未读=绿点 / 空）+ 标题；右键详情卡。
/// `shift` = 所属项目块的避让位移（020 拖拽：整组随头行一起平移让位）。
/// `in_pinned` = 是否置顶组内的行（只用于元素 id 加 ps-pin- 前缀；渲染
/// 样式与普通行完全一致——pin 钮的图标按会话路径判定置顶态）。
pub(crate) fn session_row_view(
    chat: &Chat,
    p: usize,
    s: usize,
    shift: f32,
    in_pinned: bool,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    window: &gpui::Window,
    cx: &gpui::Context<Chat>,
    badges: &[(gpui::SharedString, String)],
) -> gpui::AnyElement {
    let Some(g) = chat.psp.projects.get(p) else {
        return div().into_any_element();
    };
    let Some(info) = g.sessions.get(s) else {
        return div().into_any_element();
    };
    let is_active = chat
        .psp
        .active_file
        .as_deref()
        .is_some_and(|f| crate::services::workspace::same_path(f, &info.path));
    // 运行态**每帧现算**：v54 把它缓存进 chat.running_files，而那份缓存的唯一
    // 写入点是 SessionEvent::Changed 订阅（只在进程退出 / 手动压缩 / 换工具预设
    // 时发），正常一轮的 AgentStart/AgentSettled/AgentEnd 都不发 Changed →
    // 缓存恒空，状态槽的旋转圈再也不出现。回到渲染期直接扫常驻 runtime。
    // 比较用 same_path（大小写不敏感）：pi 自报的会话路径可能带它自拼的
    // group 大小写形态，Path 的 == 区分大小写会永假（转圈消失的真机回归）
    let running = chat.pool.runtimes.values().any(|rt| {
        let r = rt.read(cx);
        r.file
            .as_deref()
            .is_some_and(|f| crate::services::workspace::same_path(f, &info.path))
            && (r.agent_running || r.state.as_ref().is_some_and(|s| s.is_streaming))
    });
    let title: SharedString = info
        .name
        .clone()
        .filter(|n| !n.trim().is_empty())
        .map(SharedString::from)
        .unwrap_or_else(|| {
            if info.preview.is_empty() {
                "(empty)".into()
            } else {
                info.preview.clone().into()
            }
        });
    let path = info.path.clone();
    let path_click = path.clone();
    let path_card = path.clone();
    let path_hover = path.clone();
    let w_click = weak.clone();
    let w_card = weak.clone();
    let w_hover = weak.clone();
    // 置顶态（会话 pin，020 会话置顶）与悬停态（pin 槽位替换依据）
    let pinned = chat.session_pinned(&info.path);
    let hovered = chat
        .psp
        .hovered_session
        .as_deref()
        .is_some_and(|h| crate::services::workspace::same_path(h, &info.path));
    // 行元素 id：置顶组副本行换前缀（同会话在原组与置顶组各渲染一行，
    // 同 id 会撞 gpui 元素状态槽）
    let row_id: SharedString = if in_pinned {
        format!("ps-pin-{}", info.id).into()
    } else {
        format!("ps-{}", info.id).into()
    };

    // ---- ZCode TaskListItem 样式 parity（源码实测值）----
    // 列表 p-3 留白 + 行 pl-2.5/pr-1/py-1 + space-y-0.5 行距 + rounded-lg；
    // 选中/悬停 = 中性前景薄纱（10% / 5%，非 accent——accent 底会与毛玻璃
    // overlay 互相染色）；标题溢出 = 尾部 24px 渐隐，**量宽确认溢出才挂**
    // （短标题不糊尾）。overlay 尾色 = 行底合成视觉色（不透明）：透明端必
    // 须同色 a=0，透明黑会在 HSL 插值中段产生暗带。
    let row_sel = gpui::rgba((t.text << 8) | 0x1a); // 10%（悬停与选中同色）
    let row_base = if is_active {
        gpui::rgb(crate::appearance::theme::mix_rgb(t.text, t.nav, 0.1))
    } else {
        gpui::rgb(t.nav)
    };
    let fade_to = gpui::Hsla::from(row_base);
    let fade = gpui::linear_gradient(
        90.,
        gpui::linear_color_stop(gpui::Hsla { a: 0., ..fade_to }, 0.),
        gpui::linear_color_stop(fade_to, 1.),
    );
    // 悬停态行底变成 10% 白纱（over nav），渐隐尾色必须跟着换白纱的合成
    // 视觉色——否则纯 nav 的 overlay 盖在白纱底上就是一块深色（黑块回归）
    let fade_hover_to = gpui::Hsla::from(gpui::rgb(crate::appearance::theme::mix_rgb(
        t.text, t.nav, 0.1,
    )));
    let fade_hover = gpui::linear_gradient(
        90.,
        gpui::linear_color_stop(
            gpui::Hsla {
                a: 0.,
                ..fade_hover_to
            },
            0.,
        ),
        gpui::linear_color_stop(fade_hover_to, 1.),
    );

    // 溢出测量：标题真实排版宽 vs 行内可用宽。chrome 全部走 align 令牌
    // （容器 LIST_PX×2 + 行 ROW_PL/LIST_PX），其余 = slot 15 + 两个 gap 16
    // + 时间列 38；对齐线改动时此处自动跟随
    let chrome_w = align::LIST_PX * 2. + align::ROW_PL + align::LIST_PX;
    let panel_family = crate::appearance::panel_font().family;
    let measure_font = gpui::Font {
        family: panel_family.clone().into(),
        features: gpui::FontFeatures::default(),
        fallbacks: None,
        weight: gpui::FontWeight::NORMAL,
        style: gpui::FontStyle::Normal,
    };
    let measure_run = gpui::TextRun {
        len: title.len(),
        font: measure_font,
        color: rgb(t.text).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    // 量宽字号必须 = 渲染字号（ui_size(12.)，随界面字号设置缩放）。
    // 曾经写死 px(13.)：设置 ≥14 时低估标题宽 → 溢出不挂渐隐、生硬截断。
    let title_overflows = {
        let measure_size = crate::appearance::ui_size(12.);
        let title_w = window
            .text_system()
            .layout_line(&title, measure_size, &[measure_run], None)
            .width;
        let available = px(chat.layout.slp_w - chrome_w - 15. - 16. - 38.);
        title_w > available
    };

    div()
        // 与项目行同理：滚动容器 overflow 压缩时会连内部行一起压（内层
        // h(32) 被 wrapper 的收缩顶掉），列表行一律禁止收缩
        .flex_shrink_0()
        // 所属项目块的避让平移（相对偏移，不动布局）
        .relative()
        .top(px(shift))
        .child(
            div()
                .id(row_id)
                .relative()
                .group("psrow")
                .w_full()
                // §5.8：mb 补间距写法废除，真行高 32
                .h(px(crate::ui::tokens::row::MAIN))
                .flex()
                .items_center()
                .gap(px(8.))
                .pl(px(align::ROW_PL))
                .pr(px(align::LIST_PX))
                .rounded(px(8.))
                .cursor_pointer()
                .when(is_active, |d| d.bg(row_sel).hover(|s| s.bg(row_sel)))
                .when(!is_active, |d| d.hover(|s| s.bg(row_sel)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_click.update(cx, |c, cx| {
                        c.open_session(path_click.clone(), false, cx);
                    });
                })
                .on_hover(move |h, _, cx| {
                    // pin 槽位替换依据：只在翻转时 notify（行序重建后 path 判等
                    // 自动失效，不需清旧）
                    let _ = w_hover.update(cx, |c, cx| {
                        let next = h.then(|| path_hover.clone());
                        if c.psp.hovered_session.as_ref() != next.as_ref() {
                            c.psp.hovered_session = next;
                            cx.notify();
                        }
                    });
                })
                .on_mouse_down(
                    MouseButton::Right,
                    move |ev: &gpui::MouseDownEvent, _, cx| {
                        // 右键详情卡（2026-10-15 定夺：悬停自动弹窗整体撤销——穿行
                        // 滞留/换卡修补不平交互，改上下文卡语义：定位右键点、上/下方
                        // 12px 弹出、点外即收）。锚点 = 鼠标视口坐标
                        let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
                        let _ = w_card.update(cx, |c, cx| {
                            c.dlg.proj_card = None;
                            c.dlg.hover_card = Some(crate::function_panel::HoverCard {
                                path: path_card.clone(),
                                x,
                                y,
                                confirming: false,
                                renaming: false,
                                rename_input: None,
                            });
                            cx.notify();
                        });
                    },
                )
                .child(session_status_slot(
                    chat,
                    &info.path,
                    running,
                    hovered,
                    pinned,
                    weak.clone(),
                    t,
                ))
                .child(
                    // 标题：flex_1 到时间区左缘为止（不进入时间区）；仅确认溢出时
                    // 挂尾部 24px 渐隐（ZCode TaskTitleOverflowText parity）
                    div()
                        .relative()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .font_family(panel_family)
                        // 会话标签 = 面板设置值（字体大小设置.md §1）
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(rgb(t.text))
                        .child(title)
                        .when(title_overflows, |d| {
                            d.child(
                                div()
                                    .absolute()
                                    .right_0()
                                    .top_0()
                                    .bottom_0()
                                    .w(px(24.))
                                    .bg(fade)
                                    .group_hover("psrow", |s| s.bg(fade_hover)),
                            )
                        }),
                )
                // 时间区（v55 fmtAgo）：固定宽右对齐成一列（宽度与左侧图标区+
                // padding 视觉平衡，标题可用空间最大化）
                // 时间区：插件徽标（契约 v3 sessions.setBadge，002「时间位置放微信
                // icon」）优先占位——全部画出、从右到左、间隔 10px（多插件不裁不并，
                // 用户定夺 2026-10-28）；没徽标才回落相对时间
                .child({
                    if badges.is_empty() {
                        div()
                            .flex_shrink_0()
                            .w(px(38.))
                            .flex()
                            .items_center()
                            .justify_end()
                            // 时间 = 面板字号−2（2026-10-10 定夺，原 −1）
                            .text_size(crate::appearance::ui_size(10.))
                            .text_color(rgb(t.text_faint))
                            .child(SharedString::from(crate::services::format::fmt_ago(
                                info.modified,
                            )))
                            .into_any_element()
                    } else {
                        div()
                            .flex_shrink_0()
                            .min_w(px(38.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_end()
                            .gap(px(10.))
                            .children(badges.into_iter().rev().map(|(icon, tone)| {
                                let color = if tone == "accent" {
                                    crate::appearance::theme::UNREAD
                                } else {
                                    t.text_faint
                                };
                                gpui::svg()
                                    .path(icon)
                                    .text_color(rgb(color))
                                    .size(px(crate::ui::tokens::icon::MD))
                            }))
                            .into_any_element()
                    }
                }),
        )
        .into_any_element()
}

/// 会话行状态槽（15px）：常态 = 转圈/彩点/空；悬停换成置顶钮（020 会话
/// 置顶 ①；MD 档中性色）。**任何行都不常显 pin**——置顶组行也不例外
/// （2026-10-18 二改：常显 pin 白占状态槽，转圈/彩点看不到）。悬停态图标
/// 随置顶态翻转：未置顶 = pin（置顶）、已置顶 = pin-off（取消置顶）。
/// pin 钮只在可见时存在（tracked hover 态门控），不可见的钮不拦截点击
/// ——状态槽那 15px 未悬停时要留给以后可能的槽位交互。
fn session_status_slot(
    chat: &Chat,
    path: &std::path::Path,
    running: bool,
    hovered: bool,
    pinned: bool,
    weak: gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    div()
        .relative()
        .size(px(15.))
        // 状态与 pin 互斥渲染（pin 是描边 icon，叠加会从缝隙露出
        // 底下的转圈/彩点）：未悬停才画状态
        .when(!hovered, |d| {
            d.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(if running {
                        spinner(15., t.accent)
                    } else if chat.psp.turn_errors.contains(path) {
                        // 红 = 外部错误/崩溃（t.danger，§6 danger 系唯一表达）
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(rgb(t.danger))
                            .into_any_element()
                    } else if chat.psp.turn_warnings.contains(path) {
                        // 黄 = 内部中断（上限截断/中止）
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(rgb(crate::appearance::theme::WARN))
                            .into_any_element()
                    } else if chat.psp.unread.contains(path) {
                        // 青蓝 = 完成通知（绿色退出通知语义，仅表达正确性）
                        div()
                            .size(px(7.))
                            .rounded_full()
                            .bg(rgb(crate::appearance::theme::NOTICE))
                            .into_any_element()
                    } else {
                        div().into_any_element()
                    }),
            )
        })
        .when(hovered, |d| {
            // 闭包要求 'static：先转 owned 再 move 进闭包（move 捕获 &Path
            // 引用本身，to_path_buf 写在闭包体内救不了）
            let path_pin = path.to_path_buf();
            d.child(
                div()
                    .id("pin-toggle")
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .with_tip(if pinned { "psp.unpin" } else { "psp.pin" })
                    // 拦住冒泡：点 pin 不是点行（不开会话）
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = weak.update(cx, |c, cx| {
                            c.session_pin_toggle(path_pin.clone(), cx);
                        });
                    })
                    .child(icon(
                        if pinned { "pin-off" } else { "pin" },
                        crate::ui::tokens::icon::MD,
                        t.text_dim,
                    )),
            )
        })
        .into_any_element()
}

/// 置顶组头行（020 会话置顶 ③）：folder-bookmark icon（MD 档；中性
/// text_dim——pin/置顶不用主题色，不抢注意力，2026-10-18 用户定夺）
/// +「置顶」；样式对齐项目行（同高同字号同对齐线）。零交互：不收折/
/// 不拖拽/无右键卡——置顶是全局单组，组序恒在最顶，头行没有可操作面。
pub(crate) fn pinned_header_row(t: &'static Theme) -> gpui::AnyElement {
    div()
        .id("psp-pinned-header")
        .h(px(crate::ui::tokens::row::MAIN))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(align::ROW_PL))
        .pr(px(align::LIST_PX))
        .rounded(px(8.))
        .child(icon(
            "folder-bookmark",
            crate::ui::tokens::icon::MD,
            t.text_dim,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(gpui::FontWeight::MEDIUM)
                .text_color(rgb(t.text_dim))
                .child(tr("psp.pinned_group")),
        )
        .into_any_element()
}

/// 伪标签行（draft 已发送、pi 未落盘的占位）：行高/字号/选中与悬停底色
/// 同 [`session_row_view`]（ZCode TaskListItem 样式），左侧状态槽 =
/// spinner（此刻进程正在 boot/跑，与真标签的 running 态同视觉），标题 =
/// 首条消息首行（真标签落地后的 preview 与它一致，衔接无跳变）。没有文
/// 件就没有 hover 卡 / 改名 / 删除——点击只做 switch_to。FileBound 落地
/// 后本行被真标签顶替。`shift` = 所属项目块的避让位移（020 拖拽）。
pub(crate) fn pending_row_view(
    chat: &Chat,
    key: &str,
    shift: f32,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    let Some(entry) = chat.psp.pending_tabs.iter().find(|e| e.key == key) else {
        return div().into_any_element();
    };
    let is_active = chat.pool.active_key == entry.key;
    let title: SharedString = if entry.preview.is_empty() {
        tr("session.new").to_string().into()
    } else {
        entry.preview.clone().into()
    };
    let row_sel = gpui::rgba((t.text << 8) | 0x1a); // 10%（与 session 行同色）
    let panel_family = crate::appearance::panel_font().family;
    let key_click = entry.key.clone();
    let w_click = weak.clone();
    div()
        // 与 session 行同理：滚动容器 overflow 压缩时行不许收缩
        .flex_shrink_0()
        // 所属项目块的避让平移（相对偏移，不动布局）
        .relative()
        .top(px(shift))
        .child(
            div()
                .id(SharedString::from(format!("ps-draft-{key}")))
                .w_full()
                .h(px(crate::ui::tokens::row::MAIN))
                .flex()
                .items_center()
                .gap(px(8.))
                .pl(px(align::ROW_PL))
                .pr(px(align::LIST_PX))
                .rounded(px(8.))
                .cursor_pointer()
                .when(is_active, |d| d.bg(row_sel).hover(|s| s.bg(row_sel)))
                .when(!is_active, |d| d.hover(|s| s.bg(row_sel)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let key = key_click.clone();
                    let _ = w_click.update(cx, |c, cx| {
                        if let Some(rt) = c.pool.runtimes.get(&key).cloned() {
                            c.switch_pool_rt(rt, cx);
                        }
                    });
                })
                .child(
                    // 状态槽（15px，与会话行同位）：伪标签期间轮次必在飞
                    div()
                        .size(px(15.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(spinner(15., t.accent)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .font_family(panel_family)
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(rgb(t.text))
                        .child(title),
                )
                // 时间区占位：与 session 行同宽，标题截断位对齐
                .child(div().flex_shrink_0().w(px(38.))),
        )
        .into_any_element()
}
