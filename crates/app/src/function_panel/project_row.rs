//! psp 项目行渲染（015/020）：folder 行 + 收起/展开 + hover 新会话钮 +
//! 拖拽排序三件套（on_drag/on_drag_move/避让平移）。自 mod.rs 拆出（090 重构）。

use super::*;

pub(crate) fn project_row(
    chat: &Chat,
    pi: usize,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> gpui::AnyElement {
    let Some(g) = chat.psp.projects.get(pi) else {
        return div().into_any_element();
    };
    let active = chat
        .ws
        .cwd
        .as_deref()
        .is_some_and(|cur| same_ws(&g.path.to_string_lossy(), &cur.to_string_lossy()));
    let collapsed = chat
        .psp
        .collapsed_keys
        .contains(&same_ws_key(&g.path.to_string_lossy()));
    let hovered = chat.psp.hovered_project == Some(pi);
    let path = g.path.clone();
    let path_toggle = path.clone();
    let path_card = path.clone();
    let name: SharedString = g.name.clone().into();
    let drag_key = same_ws_key(&g.path.to_string_lossy());
    let w_toggle = weak.clone();
    let w_new = weak.clone();
    let w_tip_on = weak.clone();
    let w_card = weak.clone();
    let w_drag_ix = weak.clone();
    let ghost_name = g.name.clone();
    let ghost_w = chat.layout.slp_w - 2. * align::LIST_PX;
    // 拖拽中的源行：隐形但保留 32px 槽位——「被拎起」的空感，落点空槽
    // 由其余块的避让平移让出（ZCode 源行 opacity 0 同款）
    let is_dragged = chat
        .psp
        .proj_drag
        .as_ref()
        .is_some_and(|d| d.key == drag_key);
    let shift = psp_drag::row_shift(chat, pi);

    div()
        .id(SharedString::from(format!("prj-{pi}")))
        .relative()
        .top(px(shift))
        // §5.8 主列表档真行高 32（原 33=32+mb2 补偿，mb 写法废除）
        .h(px(crate::ui::tokens::row::MAIN))
        // 滚动容器 overflow 压缩行时 flex-shrink 会把行压到内容高
        // （min-height:auto），hover 按钮一现（28 高）行高就跳变、整列
        // 下移（「行浮动」）——列表行一律禁止收缩
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(8.))
        .pl(px(align::ROW_PL))
        .pr(px(align::LIST_PX))
        .rounded(px(8.))
        // 抓取手型（020）：Windows 系统没有掌形光标，OpenHand/ClosedHand
        // 由 vendored gpui 自绘 SVG 光标实现（platform/windows/cursor.rs，
        // 用户供图 lucide hand/hand-grab）；拖拽开始后全窗口切握紧手
        .cursor(gpui::CursorStyle::OpenHand)
        .when(is_dragged, |d| d.opacity(0.))
        // 折叠切换走 on_click（按下+抬起无位移才触发）：on_mouse_down 会
        // 在拖拽起手时就翻转折叠态，与拖拽语义打架（gpui 拖拽激活会吞掉
        // click，两者天然互斥）
        .on_click(move |_, _, cx| {
            let _ = w_toggle.update(cx, |c, cx| {
                let key = crate::services::workspace::same_ws_key(&path_toggle.to_string_lossy());
                if !c.psp.collapsed_keys.remove(&key) {
                    c.psp.collapsed_keys.insert(key);
                }
                c.persist_ui();
                cx.notify();
            });
        })
        .on_mouse_down(
            MouseButton::Right,
            move |ev: &gpui::MouseDownEvent, _, cx| {
                // 项目行右键详情卡（020）：样式对齐会话详情卡，锚点 = 鼠标
                // 视口坐标。与会话卡互斥：右键别行 = 收旧开新
                let (x, y) = (f32::from(ev.position.x), f32::from(ev.position.y));
                let _ = w_card.update(cx, |c, cx| {
                    c.dlg.hover_card = None;
                    c.dlg.proj_card = Some(crate::function_panel::ProjCard {
                        path: path_card.clone(),
                        x,
                        y,
                        confirming: false,
                    });
                    cx.notify();
                });
            },
        )
        .on_drag(
            psp_drag::ProjDragTab {
                key: drag_key.clone(),
            },
            {
                // 幽灵定位由 gpui 按 mouse − cursor_offset 完成（天然保留
                // 抓取点偏移），这里只捕获锁 x 补偿要用的起手鼠标位
                move |_, _, window, cx| {
                    let mouse = window.mouse_position();
                    cx.new(|_| psp_drag::ProjDragGhost {
                        name: ghost_name.clone(),
                        width: ghost_w,
                        start_mouse_x: f32::from(mouse.x),
                    })
                }
            },
        )
        .on_drag_move::<psp_drag::ProjDragTab>(move |ev, _, cx| {
            // 每次 mouse move 全体在册行都会收到（capture 相、无悬停过
            // 滤），contains 自筛：只有鼠标所在的行往下走。命中区随避让
            // 平移走 =「所见即所拖」（dnd-kit 手感）
            if !ev.bounds.contains(&ev.event.position) {
                return;
            }
            let key = ev.drag(cx).key.clone();
            // 半行判定：上/下半 = 插到本行前/后；拖回自己或相邻 = no-op
            // → 空槽闭合、drop 也不动（no-op 对比的是**被拖项目**的当前位
            // 置，不是本行——本行位置和 target 同源，比了等于全判原地）
            let target = if ev.event.position.y < ev.bounds.center().y {
                pi
            } else {
                pi + 1
            };
            let _ = w_drag_ix.update(cx, |c, cx| {
                psp_drag::sync_drag(c, key.clone(), true, cx);
                let no_op = c
                    .psp
                    .projects
                    .iter()
                    .position(|g| psp_drag::group_key(g) == key)
                    .is_some_and(|cur| target == cur || target == cur + 1);
                psp_drag::set_drop_target(c, (!no_op).then_some(target), cx);
            });
        })
        .on_hover(move |h, _, cx| {
            let _ = w_tip_on.update(cx, |c, cx| {
                let on = *h;
                if on {
                    if c.psp.hovered_project != Some(pi) {
                        c.psp.hovered_project = Some(pi);
                        cx.notify();
                    }
                } else {
                    if c.psp.hovered_project == Some(pi) {
                        c.psp.hovered_project = None;
                    }
                    if c.dlg.proj_tip.is_some() {
                        c.dlg.proj_tip = None;
                        cx.notify();
                    }
                }
            });
        })
        // 项目路径悬浮条（proj_tip）已关：用户肯定知道自己的目录路径
        //（2026-10-14 定夺）——触发器摘除，机制保留
        .child(icon(
            if collapsed { "folder" } else { "folder-open" },
            15.,
            t.text_dim,
        ))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis()
                // 项目标签 = 面板设置值（字体大小设置.md §1）；
                // 020-1：选中项目 = 主题色 + 粗体（与文件树操作栏项目名一致）
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(if active {
                    gpui::FontWeight::BOLD
                } else {
                    gpui::FontWeight::MEDIUM
                })
                .text_color(if active {
                    rgb(t.accent)
                } else {
                    rgb(t.text_dim)
                })
                .child(name),
        )
        .when(hovered, |d| {
            d.child(
                div()
                    .flex()
                    // 列对齐：行内容右缘 = 容器 px10 + 行 pr10 = 距面板 20，
                    // 而上下两条控制栏钮右缘距面板 10——负右距 10 把钮组拉回
                    // 同线，新会话↔最大化同 x（仍落在行 bounds 右缘之内，
                    // 行 hover 不会丢）。垃圾桶钮已撤（2026-10-16）：删除
                    // 走项目行右键详情卡
                    .mr(px(-10.))
                    .gap(px(2.))
                    // ＋ → 小号新建会话 icon（与控制栏「会话」同图形，小档）
                    .child(
                        div()
                            .id(SharedString::from(format!("prj-new-{pi}")))
                            .w(px(26.))
                            .h(px(26.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .group("prj-act")
                            .text_color(rgb(t.text_dim))
                            .hover(|s| s.bg(rgb(t.bg_hover)))
                            .with_tip("psp.new_session_in_project")
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                cx.stop_propagation();
                                let _ = w_new.update(cx, |c, cx| {
                                    c.new_session_in(path.clone(), cx);
                                });
                            })
                            .child(
                                gpui::svg()
                                    .path(SharedString::from("icons/icon-new-chat.svg"))
                                    .text_color(rgb(t.text_dim))
                                    .group_hover("prj-act", |s| s.text_color(rgb(t.text)))
                                    .size(px(crate::ui::tokens::icon::MD))
                                    .into_any_element(),
                            ),
                    ),
            )
        })
        .into_any_element()
}
