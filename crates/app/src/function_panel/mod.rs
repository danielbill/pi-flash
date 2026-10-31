//! psp / dock (v54): 项目+会话一体列表（ChatGPT 式，设计核心=隐藏复杂度）。
//! dock = statusbar 三面板容器（psp / 文件树 / git），nav 底色，右缘 6px
//! 拖拽调宽（250–500，双击复位 282）。psp 的悬浮层（路径 tooltip / 会话
//! 详情卡 / 两级菜单 / 删除确认）由 `psp_overlays` 在根渲染挂载。

pub(crate) mod file_actions;
pub(crate) mod file_tree;
pub(crate) mod file_tree_drag;
pub(crate) mod file_tree_edit;
pub(crate) mod file_tree_pins;
pub(crate) mod file_tree_sticky;
pub(crate) mod git_panel;
pub(crate) mod psp_actions;
pub(crate) mod psp_drag;
pub(crate) mod psp_menus;
pub(crate) mod psp_overlays;
pub(crate) mod psp_row_views;
pub(crate) mod tab_menu;
pub(crate) mod tree_menu;

use gpui::{Context, Entity, MouseButton, SharedString, div, prelude::*, px, rgb};

use std::path::PathBuf;

use crate::Chat;
use crate::DockPanel;
use crate::ListMode;
use crate::SortMode;
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::services::workspace::{same_path, same_ws, same_ws_key};
use crate::ui::TextInput;
use crate::ui::tokens::align;
use crate::ui::{WithTip, icon};
use psp_row_views::{pending_row_view, pinned_header_row, session_row_view};

mod project_row;
mod psp_rows;

pub(crate) use project_row::project_row;
pub(crate) use psp_rows::{PspRow, psp_page, psp_rows};

// ---------------------------------------------------------------------------
// psp view
// ---------------------------------------------------------------------------

pub(crate) fn psp_view(
    chat: &mut Chat,
    _entity: Entity<Chat>,
    weak: gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    // 020-1 会话子面板：控制栏固定在子面板顶（操作行永远可见），只有列表
    // 滚动。之前操作行放滚动容器内时，其按钮 hitbox 被 scroll mask 裁掉，
    // 点击全部失效——这也是设计的本意：操作行不是列表内容。
    let t = T();
    // 统一对齐线（§7 / tokens::align）：控制栏钮图标与列表行图标共
    // 用视觉左缘 ICON_LINE。容器 px = LIST_PX 是唯一水平留白源，
    // 行内 pl = ROW_PL 是对齐补偿（LIST_PX + ROW_PL = ICON_LINE）；右线 =
    // LIST_PX。改线只改 tokens 里 ICON_LINE，这里全自动跟随。
    let sess_min = chat.layout.sess_min;
    let files_min = chat.layout.files_min;
    let mut panel = div()
        .id("sub-sess")
        .w_full()
        .overflow_hidden()
        .flex()
        .flex_col()
        .child(psp_ctrl_bar(chat, &weak, t));
    if !sess_min {
        let mut body = div()
            .flex_1()
            .min_h_0()
            .w_full()
            .flex()
            .flex_col()
            .px(px(align::LIST_PX))
            .pt(px(6.));
        let mut rows: Vec<gpui::AnyElement> = Vec::new();
        // 避让位移（020 拖拽）的块归属：Project 行之后的一切行都属于该
        // 项目块（Session 自带下标；Empty/More 跟随最近的 Project 行），
        // 整组一起平移让位；首个 Project 之前的孤儿伪标签不参与平移。
        let mut last_pi: Option<usize> = None;
        for row in psp_rows(chat) {
            let block = match &row {
                PspRow::Project(pi) => {
                    last_pi = Some(*pi);
                    None
                }
                // 置顶组行不参与项目块避让平移（头行在所有 Project 之前，
                // 置顶组引用的是别组的项目块）
                PspRow::PinnedHeader | PspRow::PinnedSession { .. } => None,
                PspRow::Session { p, .. } => Some(*p),
                _ => last_pi,
            };
            let shift = block.map(|b| psp_drag::row_shift(chat, b)).unwrap_or(0.);
            match row {
                PspRow::Project(pi) => rows.push(project_row(chat, pi, &weak, t)),
                PspRow::PinnedHeader => rows.push(pinned_header_row(t)),
                PspRow::Session { p, s } => {
                    let badges = row_badges(chat, p, s, cx);
                    rows.push(session_row_view(
                        chat, p, s, shift, false, &weak, t, window, cx, &badges,
                    ));
                }
                PspRow::PinnedSession { p, s } => {
                    let badges = row_badges(chat, p, s, cx);
                    rows.push(session_row_view(
                        chat, p, s, shift, true, &weak, t, window, cx, &badges,
                    ));
                }
                PspRow::Pending { key } => rows.push(pending_row_view(chat, &key, shift, &weak, t)),
                PspRow::Empty => rows.push(
                    div()
                        .id("psp-empty")
                        .relative()
                        .top(px(shift))
                        .pl(px(45.))
                        .py(px(5.5))
                        // 与「显示更多」行同款：placeholder 色 + 面板字号−1（2026-10-10 定夺）
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_faint))
                        .child(tr("psp.no_sessions"))
                        .into_any_element(),
                ),
                PspRow::More { key } => {
                    let weak_more = weak.clone();
                    let key_more = key.clone();
                    rows.push(
                        div()
                            .id(SharedString::from(format!("psp-more-{key}")))
                            .relative()
                            .top(px(shift))
                            .pl(px(45.))
                            .py(px(5.5))
                            // placeholder 色 + 面板字号−1（2026-10-10 定夺）
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text_faint))
                            .cursor_pointer()
                            .hover(|s| s.text_color(rgb(t.text)))
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let key = key_more.clone();
                                let _ = weak_more.update(cx, |chat, cx| {
                                    *chat.psp.psp_shown.entry(key).or_insert_with(psp_page) +=
                                        psp_page();
                                    cx.notify();
                                });
                            })
                            .child(tr("settings.show_more"))
                            .into_any_element(),
                    );
                }
            }
        }
        // psp 会话列表滚动 + 右缘滚动条。Scrollbar 挂在与滚动容器平级的
        // relative 包裹上（贴 dock 右缘，不吃容器内 10px 留白）；ZED Regular
        // 视觉：只有 thumb 无 track（track 色已在 sync_gpui_tokens 透明化）。
        body = body.child(
            div()
                .id("psp-scroll-wrap")
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(
                    div()
                        .id("psp-scroll")
                        .size_full()
                        .overflow_y_scroll()
                        .track_scroll(&chat.psp.psp_scroll)
                        // 鼠标进 panel → 滚动条立现；离开 → 3s 后淡出（自绘
                        // psp_scrollbar 读这两个状态自绘透明度）
                        .on_hover(cx.listener(|this, hovered: &bool, _w, cx| {
                            this.psp.psp_sb_state.set_parent_hovered(*hovered);
                            cx.notify();
                        }))
                        // 滚动时立刻关掉全部浮层（详情卡/项目tooltip/删除确认）
                        //——它们锚定的是行位置，滚动后锚点漂移，钉在原地
                        // 只会错位（用户定稿：滚动 = 菜单立即消失）
                        .on_scroll_wheel(cx.listener(|this, _: &gpui::ScrollWheelEvent, _w, cx| {
                            if this.dlg.hover_card.is_some()
                                || this.dlg.proj_card.is_some()
                                || this.dlg.proj_tip.is_some()
                            {
                                this.dlg.hover_card = None;
                                this.dlg.proj_card = None;
                                this.dlg.proj_tip = None;
                                cx.notify();
                            }
                        }))
                        // 项目拖拽（020）：容器级 drag_move = 状态同步漏斗——
                        // 列表区内续期、拖出列表区立即清态（指示线消失、收起
                        // 的组复原）；on_drop 兜底整片列表的落点提交（行内半行
                        // 判定已在 project_row 做，这里只消费 proj_drop_ix）
                        .on_drag_move::<psp_drag::ProjDragTab>({
                            let w_sync = weak.clone();
                            move |ev, window, cx| {
                                let inside = ev.bounds.contains(&ev.event.position);
                                let key = ev.drag(cx).key.clone();
                                // 拖拽全程全窗口握紧小手：AnyDrag.cursor_style
                                // 是窗口级光标（优先于一切行光标），拖拽激活时
                                // 从源行样式取值（OpenHand），这里覆写成
                                // ClosedHand；refresh 与拖拽期逐 move 刷新合并
                                let _ = cx.set_active_drag_cursor_style(
                                    gpui::CursorStyle::ClosedHand,
                                    window,
                                );
                                let _ = w_sync.update(cx, |c, cx| {
                                    psp_drag::sync_drag(c, key, inside, cx);
                                });
                            }
                        })
                        .on_drop::<psp_drag::ProjDragTab>({
                            let w_drop = weak.clone();
                            move |_, _, cx| {
                                let _ = w_drop.update(cx, |c, cx| {
                                    let target = c.psp.proj_drop_ix;
                                    psp_drag::commit_drop(c, target, cx);
                                });
                            }
                        })
                        // 水平留白由 col 外层统一（10px），此处不再叠加
                        .flex()
                        .flex_col()
                        .children(rows),
                )
                // 自绘滚动条（ZED Regular 移植）：贴边 0px；无 track；panel 内
                // 常显、离开 3s+1s 淡出；thumb 悬停加宽；事件不穿透
                .child(
                    div()
                        .absolute()
                        .top(px(0.))
                        .bottom(px(0.))
                        .right(px(-10.))
                        .w(px(10.))
                        .child(crate::ui::psp_scrollbar::psp_scrollbar(
                            &chat.psp.psp_sb_state,
                            &chat.psp.psp_scroll,
                        )),
                ),
        );
        panel = panel.child(body);
    }
    // 子面板在 dock 列里的伸缩（020-1）：对面最大化 → 本面板填满；本面板
    // 最小 → 只剩控制栏（auto 高）；常态 = 分隔线占比
    if files_min {
        panel = panel.flex_1().min_h_0();
    } else if sess_min {
        panel = panel.flex_shrink_0();
    } else {
        panel = panel
            .flex_basis(gpui::relative(chat.layout.dock_split))
            .flex_shrink_0()
            .min_h_0();
    }
    panel.into_any_element()
}

/// 020-1 会话子面板控制栏（38px、无下 border）：左二 icon+文字（大图标
/// LG18）= 项目（打开项目）/ 会话（新建会话）；右三纯 icon（小图标 MD16）
/// = 搜索 + 列表切换（项目分组 ↔ 最近会话，图标随模式换）+ 最大化（本面板
/// 填满、文件树收起到操作栏）。搜索已降级为小 icon 并入右侧组（去掉文字，
/// 与右侧钮同宽 26 同档 MD16）。左缘 = ICON_LINE 12（容器 pl4 + 钮 px8）、
/// 右缘 = LIST_PX 10，与文件树操作栏同线。
///
/// **自动隐藏**：右侧「搜索」「列表切换」「最大化」三颗常态不可见
/// （opacity 0），鼠标进入本控制栏（group `psp-bar-reveal`）才显影——少三
/// 颗粒常驻图标减视觉冗余。只用绘制期 opacity，钮盒位置/命中区不变（列
/// 对齐不抖）。受设置-界面「部分操作隐藏」总开关（`autohide_icons`，默认
/// on）控制。
fn psp_ctrl_bar(chat: &Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> gpui::AnyElement {
    let grouped = chat.psp.list_mode == ListMode::Grouped;
    let w_open = weak.clone();
    let w_new = weak.clone();
    let w_search = weak.clone();
    let w_toggle = weak.clone();
    let w_max = weak.clone();
    // 钮盒：h30（tokens::panel_ctrl::BTN_H）px8 gap6，左二带文字（ui_size
    // 12.5 Semibold），右三定宽 26 纯 icon（与文件树操作栏钮盒同宽，列对
    // 齐见下方注释）；hover = bg_hover + 前景变亮（icon 走同名 group 的
    // group_hover 同步变色）
    let btn = |id: &'static str| {
        div()
            .id(id)
            .h(px(crate::ui::tokens::panel_ctrl::BTN_H))
            .px(px(8.))
            .flex()
            .items_center()
            .gap(px(crate::ui::tokens::space::SP3))
            .rounded(px(8.))
            .cursor_pointer()
            .group("pc-btn")
            .text_size(crate::appearance::ui_size(12.5))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(t.text_dim))
            .hover(move |s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
    };
    let icon = |name: &'static str, size: f32| {
        gpui::svg()
            .path(SharedString::from(format!("icons/{name}.svg")))
            .text_color(rgb(t.text_dim))
            .group_hover("pc-btn", |s| s.text_color(rgb(t.text)))
            .size(px(size))
            .into_any_element()
    };
    // 右缘三颗纯 icon 钮链外组好再进 child（child 实参位置不能写 let）。
    // 自动隐藏（设置-界面「部分操作隐藏」，默认 on）= 常态透明、控制栏
    // hover 显影；命中区/布局不动，关掉开关则恒显。搜索/列表切换/最大化
    // 三颗低频入口一并纳入显影组
    let autohide = |b: gpui::Stateful<gpui::Div>| {
        if crate::services::workspace::autohide_icons() {
            b.opacity(0.)
                .group_hover("psp-bar-reveal", |s| s.opacity(1.))
        } else {
            b
        }
    };
    let search_btn = autohide(btn("psp-search").w(px(26.)).px(px(0.)).justify_center());
    let list_toggle = autohide(
        btn("psp-list-toggle")
            .w(px(26.))
            .px(px(0.))
            .justify_center(),
    );
    div()
        .id("psp-ctrl")
        // 自动隐藏的显影组：鼠标在本控制栏内 → 右侧三颗低频钮显影
        .group("psp-bar-reveal")
        .w_full()
        .flex_shrink_0()
        .h(px(crate::ui::tokens::panel_ctrl::BAR_H))
        .flex()
        .items_center()
        .gap(px(2.))
        // 左线 = ICON_LINE（4 + 钮内边距 8）；右缘 = LIST_PX
        .pl(px(4.))
        .pr(px(align::LIST_PX))
        // 左二：icon+文字
        .child(
            btn("psp-open-project")
                .with_tip_action("psp.open_project", &crate::OpenProjectPicker)
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_open.update(cx, |c, cx| c.open_project_picker(false, cx));
                })
                .child(icon("icon-project", crate::ui::tokens::icon::LG))
                .child(tr("scope.project")),
        )
        .child(
            btn("psp-new-session")
                .with_tip_action("psp.new_session", &crate::NewSession)
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_new.update(cx, |c, cx| c.new_session(cx));
                })
                // 会话 icon 是 iconfont 实底形（viewBox 824×767，笔画内容
                // 占 ~79%），比 lucide 描边形（~83%）光学上小一圈——19 =
                // 旧操作行验证过的补偿值（其余两颗 18 时它 19），维持同视高
                .child(icon("icon-new-chat", 19.))
                .child(tr("psp.session_label")),
        )
        .child(div().flex_1())
        // 右三：纯 icon（钮盒 26 = 文件树操作栏 bar_btn 同款，bar gap 2 →
        // 与下方操作栏右缘逐列对齐：最大化列、次列图标中心严格同 x）。
        // 搜索居本组最左 = 右缘定宽对齐不动，只往左多占一列
        .child(
            search_btn
                .with_tip_action("psp.search_sessions", &crate::OpenSessionSearch)
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_search.update(cx, |c, cx| c.open_session_search_panel(cx));
                })
                .child(icon("search", crate::ui::tokens::icon::MD)),
        )
        .child(
            // 列表切换：图标随当前模式换（分组 = folder-list，扁平 =
            // icon-flat），点击直切，不再走 ⋯ 菜单（020-1 定稿）
            list_toggle
                .with_tip(if grouped {
                    "psp.switch_to_flat"
                } else {
                    "psp.switch_to_grouped"
                })
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_toggle.update(cx, |c, cx| {
                        c.psp.list_mode = match c.psp.list_mode {
                            ListMode::Grouped => ListMode::Flat,
                            ListMode::Flat => ListMode::Grouped,
                        };
                        c.persist_ui();
                        cx.notify();
                    });
                })
                .child(icon(
                    if grouped {
                        "icon-folder-list"
                    } else {
                        "icon-flat"
                    },
                    crate::ui::tokens::icon::MD,
                )),
        )
        .child(
            // 最大化（单钮语义）：本面板填满，文件树收起到操作栏；拖分隔线
            // 即退出最大态。Alt+1 = 同一键（082 全局绑定，chip 反查 keymap）。
            // 与搜索/列表切换同入显影组（自动隐藏，见 autohide 头注）
            autohide(btn("psp-panel-max").w(px(26.)).px(px(0.)).justify_center())
                .with_tip_action("psp.maximize_session_list", &crate::MaximizeSessionList)
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = w_max.update(cx, |c, cx| {
                        c.layout.files_min = true;
                        c.layout.sess_min = false;
                        // 最大化态立即落盘（不必等关窗，防异常退出丢失）
                        c.persist_ui();
                        cx.notify();
                    });
                })
                .child(icon("maximize", crate::ui::tokens::icon::MD)),
        )
        .into_any_element()
}

/// 项目行：folder 开合图标 + 名字；点击 = 组收起/展开；hover 显新会话。
/// 拖拽排序三件套挂本行（020）：`on_drag` 发起（幽灵 = 全行宽项目行小卡，
/// 「收起为标签」的可见形态；源行 opacity 0、组体折叠在 [`psp_rows`] 做）、
/// `on_drag_move` 按鼠标在行的上/下半判定插入位（提交与取消清理走容器）；
/// 避让让位 = 本行按所属块的动画位移整体平移（`.top()`，空槽即落点）。
// ---------------------------------------------------------------------------
// dock container (020-1 v54): nav 底色 + 上下子面板（会话/文件树）+ 分隔线
// ---------------------------------------------------------------------------

pub(crate) fn dock(
    chat: &mut Chat,
    entity: Entity<Chat>,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    // git 面板保留独立视图（022；状态栏 tab 已隐藏，automation panel.dock 可达）
    if chat.layout.dock_panel == DockPanel::Git {
        return div()
            .id("dock")
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(t.nav))
            .child(git_panel::view(chat, weak, cx))
            .into_any_element();
    }
    // 插件面板（beads 360 反馈②）：左列格子视图，互斥于整合面板/git。
    // **停用/条目不在（已被摘掉）时直接漏到下面的默认 psp 列表**——
    // 左列是「停用后仍旧挂着旧面板」的最后一道口，不能只倚靠状态侧去撤 dock。
    if let DockPanel::Plugin(ix) = chat.layout.dock_panel
        && chat.plugins.plugins.get(ix).is_some_and(|e| !e.disabled)
    {
        return div()
            .id("dock")
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(t.nav))
            .child(crate::pf_plugin::plugin_panel_view(chat, ix, weak))
            .into_any_element();
    }
    // 020-1 整合面板：会话列表(上) + 文件树(下)，默认 50/50；分隔线拖拽
    // 改占比（拖拽即退出两侧最小态、双击复位 50%），各自可收起到控制栏
    //（单最大化钮，语义见 LayoutUi::sess_min / files_min）
    let bounds_slot = chat.layout.dock_bounds.clone();
    // §5.12 统一分隔线：常驻 1px；**hover 与拖拽同为 4px accent**
    let split_drag = chat.layout.split_drag;
    let split_active = split_drag || chat.layout.split_hover;
    let d_hit = crate::ui::tokens::divider::HIT;
    div()
        .id("dock")
        .flex_1()
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .bg(rgb(t.nav))
        // dock 容器 bounds 每帧捕获（PillBtns::tracker 同款 canvas）：
        // app-body 层的拖拽跟踪要把窗口 y 换算成占比
        .child(
            gpui::canvas(
                move |bounds, _, _| *bounds_slot.borrow_mut() = Some(bounds),
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0(),
        )
        .child(psp_view(chat, entity, weak.clone(), window, cx))
        // 子面板分隔线：7px 命中带 + 常驻 1px 视觉线（左右缩进 10 = 列表
        // 右线），hover/拖拽按 §5.12 统一规范换 4px accent 覆盖线（absolute
        // 改 top/h 不占布局零推挤，带内居中 = 与常驻线同一中线）
        .child(
            div()
                .id("sp-divider")
                .with_tip("psp.split_tip")
                .h(px(d_hit))
                .flex_shrink_0()
                .relative()
                .cursor_row_resize()
                // hover 状态存进字段（与 content 侧 slp_hover 同一机制），
                // 不用 group_hover——后者受 has_active_drag 门控，粗细拿不到
                .on_hover({
                    let weak_hv = weak.clone();
                    move |h: &bool, _, cx| {
                        let _ = weak_hv.update(cx, |c, cx| {
                            if c.layout.split_hover != *h {
                                c.layout.split_hover = *h;
                                cx.notify();
                            }
                        });
                    }
                })
                .on_mouse_down(MouseButton::Left, {
                    let weak_dz = weak.clone();
                    move |ev: &gpui::MouseDownEvent, _, cx| {
                        let _ = weak_dz.update(cx, |c, cx| {
                            if ev.click_count == 2 {
                                c.layout.dock_split = 0.5;
                                c.layout.sess_min = false;
                                c.layout.files_min = false;
                                c.persist_ui();
                            } else {
                                c.layout.split_drag = true;
                            }
                            cx.notify();
                        });
                    }
                })
                // 线形交给统一实现（ui::divider）：1px 常驻 / 4px accent
                // 高亮，带内垂直居中 = 两种粗细同一中线
                .child(crate::ui::divider::line(
                    crate::ui::divider::LineSpec::horizontal(10., d_hit),
                    split_active,
                    t,
                )),
        )
        .child(files_view(chat, weak, cx))
        .into_any_element()
}

/// files view：树渲染在 `file_tree.rs`（vlist 虚拟化 + 展平缓存，render
/// 不碰磁盘；Zed 图标主题 + gitignore 过滤在 services::file_tree/。
/// file_icons）。
pub(crate) fn files_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    _cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    file_tree::files_view(chat, weak, _cx)
}

// ---- psp/文件树 overlay 状态类型（自 shell/types.rs 下沉，本域自持）----

/// 会话右键详情卡（2026-10-15 定夺：悬停自动弹窗整体撤销，改右键召唤的
/// 上下文卡——定位右键点、上/下方弹出、点外即收；名称沿用 hover 时代）。
#[derive(Debug, Clone)]
pub(crate) struct HoverCard {
    pub path: PathBuf,
    /// 右键点视口坐标：卡左缘对齐 x、上/下方 12px 弹出（PopupEdges 四象限）
    pub x: f32,
    pub y: f32,
    /// 点「删除」后卡内换 取消/确认 两键
    pub confirming: bool,
    /// 标题点击后原地改名（卡内变输入框，Enter 提交 / Esc 取消）
    pub renaming: bool,
    pub rename_input: Option<gpui::Entity<crate::ui::TextInput>>,
}

/// 项目行右键详情卡（020：样式对齐会话详情卡）：标题加粗 + 路径/会话数
/// 信息行 + 分隔线下 标签栏项目行同款双图标动作栏（删除带卡内确认态）。
#[derive(Debug, Clone)]
pub(crate) struct ProjCard {
    pub path: PathBuf,
    /// 右键点视口坐标：卡左缘对齐 x、上/下方 12px 弹出（PopupEdges 四象限）
    pub x: f32,
    pub y: f32,
    /// 点「删除」后动作栏换 取消/确认 两键
    pub confirming: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum TreeMenu {
    Dir { path: PathBuf, x: f32, y: f32 },
    File { path: PathBuf, x: f32, y: f32 },
}

/// 文件树标题项目下拉（树根跨项目快切）：一览 psp.projects（会话列表里
/// 的全部项目），↑/↓ 高亮 + Enter 切换、点击直切。`sel` = 键盘高亮行
/// （psp.projects 下标，打开时默认落当前项目）。切换 = **switch_project
/// 完整切项目**：树/侧栏全套随新 cwd 重挂；会话标签去留见 switch_project
/// 的 2026-12-06 保留规则（有上次会话则恢复；没有则进行中的原会话原地
/// 保留，不落新草稿）。内容视图保持不动。
#[derive(Debug, Clone)]
pub(crate) struct TreeProjMenu {
    pub(crate) sel: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct TreeEdit {
    pub(crate) kind: TreeEditKind,
    pub(crate) is_dir: bool,
    pub(crate) input: gpui::Entity<TextInput>,
    /// 实时校验态（键入即查，Zed populate_validation_error 移植）
    pub(crate) validation: TreeValidation,
}

#[derive(Debug, Clone)]
pub(crate) enum TreeEditKind {
    /// 在该目录下新建（Zed leaf_entry_id: None——is_new_entry 判据）
    New { dir: PathBuf },
    /// 同目录改名该条目
    Rename { path: PathBuf },
}

/// Zed ValidationState 移植：错误（重名/非法）拦 Enter 提交，警告放行。
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) enum TreeValidation {
    #[default]
    None,
    Error(String),
    Warning(String),
}

/// 行徽标预算：PF 钩子 session_list_render（每行每插件一次，宿主 1s 缓存）。
fn row_badges(
    chat: &mut Chat,
    p: usize,
    s: usize,
    cx: &mut gpui::Context<Chat>,
) -> Vec<(gpui::SharedString, String)> {
    let path = chat
        .psp
        .projects
        .get(p)
        .and_then(|g| g.sessions.get(s))
        .map(|i| i.path.clone());
    match path {
        Some(path) => chat.plugin_badge(&path, cx),
        None => Vec::new(),
    }
}
