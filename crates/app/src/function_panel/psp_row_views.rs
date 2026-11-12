//! psp 行渲染 —— 会话行。自 mod.rs 内切拆出（check_arch 文件 1500 行上限
//! + 视图函数 300 行上限），纯结构搬移。itlq.12 超规拆分后状态槽/置顶头/
//! 伪标签行在 `psp_row_slot`（下方 re-export 维持 mod.rs 既有导入路径）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::ui::tokens::align;

use super::psp_row_slot::session_status_slot;
pub(crate) use super::psp_row_slot::{pending_row_view, pinned_header_row};

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
    // 后台会话押着未兑现的阻塞扩展请求（#问题卡不弹徽标；与 pool PARKED
    // 队列一一对应）。键用 path 判等：绑卡前 key 即会话路径。只数阻塞件
    // （is_blocking）——fire-and-forget（notify/setStatus/…）只是状态重放，
    // 不代表「在等你回答」：旧口径整个 ext_queue 非空就点亮，微信桥/监控
    // 类服务会话 park 期间猛发 setStatus，橙点常亮且切回也不消（2026-11-05）
    let awaiting = chat.pool.runtimes.values().any(|rt| {
        let r = rt.read(cx);
        r.ext_queue.iter().any(|q| q.req.method.is_blocking())
            && crate::services::workspace::same_path(std::path::Path::new(&r.key), &info.path)
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

    // 行内标题字体族（测量在 session_title_overflows，装配仍要用）
    let panel_family = crate::appearance::panel_font().family;
    let title_overflows = session_title_overflows(&title, chat.layout.slp_w, window, t);

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
                // 色条与行体分离：行保持 32 命中区，可见色条上下各缩 1px
                // （条高 30，相邻色条间留 2px 空隙——色条贴合无美感，
                // 2026-11-05 用户定夺）。bg 不再挂行体：active 恒显，
                // 非 active 走 group("psrow") + group_hover（pin 槽位同款）
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(1.))
                        .bottom(px(1.))
                        .rounded(px(8.))
                        .when(is_active, |d| d.bg(row_sel))
                        .when(!is_active, |d| d.group_hover("psrow", |s| s.bg(row_sel))),
                )
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_click.update(cx, |c, cx| {
                        c.open_session(path_click.clone(), false, cx);
                    });
                })
                .on_hover(move |h, _, cx| {
                    // pin 槽位替换依据（行序重建后 path 判等自动失效，不需清旧）。
                    // **离行只清自己的**：一次 MouseMove 跨过行边界时，「离行
                    // false」与「入行 true」在同一事件里成对触发，且 hover 回调
                    // 走 bubble 序（= paint 序的逆 → 列表**从下往上**派发）——
                    // 向下滑时先收到下行 enter(true)、后收到上行 leave(false)，
                    // 无脑覆盖会把 enter 刚写入的值抹成 None（pin 时隐时现的
                    // 真机回归：慢速移动每行独立事件所以时好时坏，快速滑动
                    // 必触）。enter 无条件写；leave 仅当当前悬停仍是自己时才
                    // 清（project_row 同款护栏）。
                    let _ = w_hover.update(cx, |c, cx| {
                        if let Some(next) =
                            hover_session_transition(&c.psp.hovered_session, &path_hover, *h)
                        {
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
                    awaiting,
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
                                    // 与色条同内缩 1px：渐隐尾色=色条色，超出
                                    // 色条会在条外 1px 留下淡痕
                                    .top(px(1.))
                                    .bottom(px(1.))
                                    .w(px(24.))
                                    .bg(fade)
                                    .group_hover("psrow", |s| s.bg(fade_hover)),
                            )
                        }),
                )
                // 时间区（v55 fmtAgo）：固定宽右对齐成一列（宽度与左侧图标区+
                // padding 视觉平衡，标题可用空间最大化）
                // 时间区：徽标（PF 主体 pin 优先最右 + 插件按注册序，契约
                // v3 sessions.setBadge / session_list_render，落时间位置）——
                // 从右到左、间隔 10px、上限 5 保右弃左（用户定夺 2026-12，
                // 替代 2026-10-28 的"不裁不并"）；没徽标才回落相对时间
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
                                // tone：accent = 完成通知色（插件亮灯）；
                                // dim = PF 主体 pin（对齐状态槽 pin 钮用色）；
                                // 其余 = 常态灰
                                let color = if tone == "accent" {
                                    crate::appearance::theme::UNREAD
                                } else if tone == "dim" {
                                    t.text_dim
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

/// 会话行悬停状态迁移（纯函数，测试锚点）：`current` = 全局悬停态，
/// `own` = 本行会话路径，`entered` = 本行 hover 翻转方向（true = 鼠标进入）。
/// 返回 Some(next) 表示状态已变化需写回 + notify；None = 无变化。
/// enter 无条件写入（除非已是自己）；leave 仅当当前悬停仍是自己才清——
/// 详见调用点注释（bubble 序成对回调的覆盖竞态）。
/// 溢出测量：标题真实排版宽 vs 行内可用宽（vh67 自 session_row_view 抽出，
/// 逐字未动）。chrome 全部走 align 令牌（容器 LIST_PX×2 + 行 ROW_PL/LIST_PX），
/// 其余 = slot 15 + 两个 gap 16 + 时间列 38；对齐线改动时此处自动跟随
fn session_title_overflows(
    title: &str,
    slp_w: f32,
    window: &gpui::Window,
    t: &'static Theme,
) -> bool {
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
    let measure_size = crate::appearance::ui_size(12.);
    let title_w = window
        .text_system()
        .layout_line(&title, measure_size, &[measure_run], None)
        .width;
    let available = px(slp_w - chrome_w - 15. - 16. - 38.);
    title_w > available
}

fn hover_session_transition(
    current: &Option<std::path::PathBuf>,
    own: &std::path::Path,
    entered: bool,
) -> Option<Option<std::path::PathBuf>> {
    if entered {
        if current.as_deref() != Some(own) {
            return Some(Some(own.to_path_buf()));
        }
    } else if current
        .as_deref()
        .is_some_and(|cur| crate::services::workspace::same_path(cur, own))
    {
        return Some(None);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::hover_session_transition;
    use std::path::{Path, PathBuf};

    /// 复现一次 MouseMove 内的成对回调：gpui bubble 序 = paint 序的逆，
    /// 列表行自上而下 paint → 回调**自下而上**派发。`seq` 按派发序喂入
    /// (行路径, entered)，返回最终状态（None = 无行悬停）。
    fn replay(current: &mut Option<PathBuf>, seq: &[(&Path, bool)]) {
        for (own, entered) in seq {
            if let Some(next) = hover_session_transition(current, own, *entered) {
                *current = next;
            }
        }
    }

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn hover_down_top_to_bottom_keeps_entered_row() {
        // 回归：向下滑（A 上 → B 下），bubble 序 = B(true) 先、A(false) 后。
        // 旧实现无脑覆盖 → 最终 None，pin 不显示；新实现必须留在 B。
        let mut cur = Some(p("C:/p/A"));
        replay(
            &mut cur,
            &[(Path::new("C:/p/B"), true), (Path::new("C:/p/A"), false)],
        );
        assert_eq!(cur, Some(p("C:/p/B")));
    }

    #[test]
    fn hover_up_bottom_to_top_keeps_entered_row() {
        // 向上滑（B 下 → A 上），bubble 序 = B(false) 先、A(true) 后 → A。
        let mut cur = Some(p("C:/p/B"));
        replay(
            &mut cur,
            &[(Path::new("C:/p/B"), false), (Path::new("C:/p/A"), true)],
        );
        assert_eq!(cur, Some(p("C:/p/A")));
    }

    #[test]
    fn hover_cross_multiple_rows_lands_on_entered() {
        // 快速滑动一次跨多行：下行 C(true)、中行 B(true)、上行 A(false)。
        let mut cur = Some(p("C:/p/A"));
        replay(
            &mut cur,
            &[
                (Path::new("C:/p/C"), true),
                (Path::new("C:/p/B"), true),
                (Path::new("C:/p/A"), false),
            ],
        );
        assert_eq!(cur, Some(p("C:/p/B")));
    }

    #[test]
    fn hover_leave_leaves_list_clears_only_self() {
        // 鼠标离开列表：最后一行 leave 且无 enter → 清空。
        let mut cur = Some(p("C:/p/A"));
        replay(&mut cur, &[(Path::new("C:/p/A"), false)]);
        assert_eq!(cur, None);
        // 离行的不是当前悬停行（已被别行顶掉）→ 不动。
        let mut cur = Some(p("C:/p/B"));
        replay(&mut cur, &[(Path::new("C:/p/A"), false)]);
        assert_eq!(cur, Some(p("C:/p/B")));
    }

    #[test]
    fn hover_enter_same_path_is_noop_and_leave_is_case_insensitive() {
        // 重复 enter 同一行不抖动 notify。
        let cur = Some(p("C:/p/A"));
        assert_eq!(
            hover_session_transition(&cur, Path::new("C:/p/A"), true),
            None
        );
        // leave 判等走 same_path（Windows 大小写不敏感；pi 自报路径大小写漂移）。
        assert_eq!(
            hover_session_transition(&cur, Path::new("c:/P/a"), false),
            Some(None)
        );
    }
}
