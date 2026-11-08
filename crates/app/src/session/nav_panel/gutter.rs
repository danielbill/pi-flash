//! 导航面板槽（nav_gutter）：图标列与悬浮卡装配。

use super::*;

/// 会话导航面板 (033)：右缘 6px 竖条，屏高 75%，按「屏高 − composer
/// 高度×50%」居中；生成算法（v63-5，2026-10-10 上限 20→10）：轮次均分
/// 切割整条——1 轮整条弱主题色、N 轮切 N 段（flex_1 等分 + 2px 缝），10
/// 轮后段高不再缩小，>10 走百分比桶映射（轮 k → 段 k×10/N，点击回桶首
/// 轮）。当前定位段 accent
/// 实色——选中跟随真实滚动位置（贴底 = 末段），卡片悬浮框不回写。
/// 悬停向左弹出 500px 轮次摘要卡片列表（用户前 50 字 + agent 首条回复
/// 前 50 字，编号 01-99）+ 30px 总结栏，点击卡片/段定位会话；内容溢出
/// 时右缘显示滚动条。摘要数据走 runtime 缓存（v63-3）：notify_list 置
/// 脏惰性重建，渲染帧零重算。
pub(crate) fn nav_gutter(
    chat: &mut Chat,
    rt_entity: gpui::Entity<crate::session::runtime::SessionRuntime>,
    weak: gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    // 摘要读 runtime 缓存（v63-3：notify_list 置脏、惰性重建，渲染帧
    // 零重算——摘要「一轮一次」，不随渲染帧反复拼前缀）
    let (summary, scroll_top_ix, nav_pinned) = rt_entity.update(cx, |rt, _| {
        (
            rt.nav_summary(),
            rt.pager.scroll_top_ix(),
            rt.pager.nav_pinned(),
        )
    });
    // 刻度选中 = 真实定位轮次（033）：视口顶之上最近的轮。贴底（含未
    // 滚动的短会话）时逻辑位是 None，scroll_top_ix 回退成总条目数 →
    // 自然选中末轮（对话常态在末轮）。**不能**用 pager.is_at_bottom：
    // 那是滚轮回调维护的 Cell，程序化定位不经过它——导航点击后它仍是
    // true，刻度会错停在末轮（v63-1 踩坑）。
    // 导航点击优先：nav_pinned = 被点轮次（贴底钳制会改写视口顶，位置
    // 推导会错选上一段；物理滚轮清除交还推导，v63-7）
    let active_turn: Option<usize> = nav_pinned.or_else(|| {
        summary
            .turns
            .iter()
            .rposition(|s| s.user_ix <= scroll_top_ix + 1)
    });
    // 033 段上限 10（v63-5 生成算法；2026-10-10 由 20 收紧——20 段太碎）：
    // 轮次均分切割整条竖条——N 轮切
    // N 段（flex_1 等分 + 2px 缝），10 轮后段高不再缩小，>10 走既有的
    // 百分比桶映射（轮 k 命中段 k×10/N，点击段回桶首轮 i×N/10）。定位/
    // 选中算法与刻度时代完全一致：active 跟随真实滚动位置；飞出卡片仍
    // 逐轮全列（滚动查看）
    const TICK_MAX: usize = 10;
    let n_turns = summary.turns.len();
    let tick_count = n_turns.min(TICK_MAX);
    let turn_of_tick = move |i: usize| -> usize { i * n_turns / tick_count };
    // 逆映射用桶包含式 floor(((t+1)·k − 1)/N)：与正向边界 floor(i·N/k)
    // 自洽（i = max{i : floor(i·N/k) ≤ t}）。两个错例都踩过：朴素
    // floor(t·k/N) 对桶起始轮（i·N mod k ≠ 0）floor 回上一桶——点段 i
    // 高亮 i−1；不带 −1 的 floor((t+1)·k/N) 整体偏下一桶（恒等映射下
    // = t+1，点哪段亮下一段）——v63-7 两版都踩过，此为终版
    let tick_of_turn =
        move |t: usize| -> usize { (((t + 1) * tick_count - 1) / n_turns).min(tick_count - 1) };

    let mut gutter = div()
        .id("nav-gutter")
        .relative()
        // 10 = 段命中区宽（实体 6 + 右侧 4px 透明可点，不贴窗口边；
        // 2026-11-08 二次定夺：透明余量 8→4，右缘更贴边）
        .w(px(10.))
        .h_full()
        .flex_shrink_0();
    // 悬停展开的轮次摘要列表（033：topbar 之下 100% 高、400px 宽、向左
    // 弹出、内容溢出显示滚动条）。滚动条挂在滚动容器平级的 absolute
    // 兄弟上（滚动容器内的绝对定位子元素会随内容滚走）。整包结构：
    // wrap（flex 列）= 总结栏 30px 固定 + 卡片滚动区 flex_1
    if chat.layout.nav_open && !summary.turns.is_empty() {
        // absolute 定位本身即绝对定位子孙（滚动条）的包含块，无需再挂
        // relative——且 position 是单字段，relative 会覆盖 absolute
        let mut flyout_wrap = div()
            .id("nav-flyout-wrap")
            .absolute()
            // 10 = 与段左缘齐平（gutter 10 = 段命中区 6 实体 + 右 4 透明）
            .right(px(10.))
            .top_0()
            // 下缘与 input panel 底线对齐：composer 悬浮容器的 bottom =
            // footer_bar::BAR_H（034 起永久上移，input/area.rs composer_wrap
            // 同一常量）——旧硬编码 20 与状态带高度脱钩（状态带 28 后 flyout
            // 比输入面板多探出 8px）。500 = 用户定稿（2026-11-08）：面板
            // 加宽至 500（2026-11-07 曾补偿 +50 到 450）
            .bottom(px(crate::session::footer_bar::BAR_H))
            .w(px(500.))
            .flex()
            .flex_col()
            // 全面板字体族跟随「面板」槽位设置（2026-11-08）：总结栏 + 摘要
            // 卡全部继承；卡内轮次编号仍显式 mono（右对齐等宽设计）
            .font_family(crate::appearance::panel_font().family)
            // 1px accent 描边 + 5px 圆角（2026-11-08 用户定夺）：挂在 wrap
            // 上，总结栏/滚动区子元素的方角由 overflow_hidden 裁进圆角
            .rounded(px(5.))
            .border_1()
            .border_color(rgb(t.accent))
            .overflow_hidden()
            // 悬停保持打开：挂 wrap（含总结栏）而不是滚动区——鼠标挪到
            // 总结栏不算离开面板，否则 250ms 宽限一到面板就收了
            .occlude()
            .shadow_lg()
            .on_hover(cx.listener(|this, h: &bool, _w, cx| {
                if *h {
                    this.layout.nav_open = true;
                    this.layout.nav_hide_at = None;
                    this.layout.nav_flyout_hovered = true;
                } else {
                    this.layout.nav_flyout_hovered = false;
                    this.layout.nav_hover_turn = None;
                    this.layout.nav_hide_at = Some(std::time::Instant::now());
                }
                cx.notify();
            }));
        // 总结栏（v63-4）：「icon 数字」四组统计，装配在 nav_summary_header
        let header = nav_summary_header(&summary, t);
        // 卡片列表虚拟化（v63-6）：ListState 只建可视卡片（~20）而非全
        // 量（千轮会话原先是 1.2 万+ div/帧）。轮次数仅在用户新增一轮时
        // 变化（面板开着时几乎不可能），reset 重算计数
        if chat.layout.nav_flyout_list.item_count() != summary.turns.len() {
            chat.layout.nav_flyout_list.reset(summary.turns.len());
        }
        let weak_cards = weak.clone();
        let summary_cards = summary.clone();
        let rt_cards = rt_entity.clone();
        let flyout = div()
            .id("nav-flyout")
            .w_full()
            .flex_1()
            .min_h_0()
            // list 子元素靠 flex_1 撑高——父容器必须是 flex 列：block 容
            // 器里 flex_grow 无效，list 高度 auto 塌成 0，一张卡都不渲染
            //（v63-6 踩坑；session_list 的父容器同样是 flex().flex_col()）
            .flex()
            .flex_col()
            // 背景同功能面板（左 dock t.nav）——旧值 t.bg 比面板浅一档；
            // 左描边已上收至 wrap 的整圈 accent 边框，此处不再重复
            .bg(rgb(t.nav))
            // 水平 padding 必须挂外层容器：gpui List 的 prepaint 只应用
            // 条目的 padding.top/bottom，左右 padding 对条目完全无效
            //（session_list 同款结论）
            .pl(px(6.))
            .pr(px(14.))
            .child(
                gpui::list(
                    chat.layout.nav_flyout_list.clone(),
                    move |ix, _window, cx| {
                        let Some(s) = summary_cards.turns.get(ix) else {
                            return div().into_any_element();
                        };
                        // 选中框 = 悬浮光标（enter 置位），状态在 Chat 上，
                        // 经 weak 升级读取（list 闭包只有 &mut App）
                        let on = weak_cards
                            .upgrade()
                            .is_some_and(|c| c.read(cx).layout.nav_hover_turn == Some(ix));
                        // 该轮子代理（044-0 验收⑥）：第三行数据 + hover 菜单锚
                        let sub_rows = weak_cards.upgrade().and_then(|c| {
                            let chat = c.read(cx);
                            let panel = chat.dlg.subagent_panel.as_ref()?;
                            panel.turn_agents.get(ix).filter(|v| !v.is_empty()).cloned()
                        });
                        nav_card(ix, s, on, sub_rows, &weak_cards, &rt_cards, t)
                    },
                )
                // 上下 padding 挂 list 本体（prepaint 会应用）
                .pt(px(8.))
                .pb(px(8.))
                .flex_1()
                .min_h_0(),
            );
        flyout_wrap = flyout_wrap.child(header).child(flyout);
        // 滚动条仅在实际溢出时渲染；ListStateHandle 把 ListState 适配成
        // gpui-component Scrollbar 的句柄（拖拽走 list.rs 的 scrollbar 协
        // 作接口），锚在滚动区（总结栏之下）
        if chat
            .layout
            .nav_flyout_list
            .max_offset_for_scrollbar()
            .height
            > px(0.)
        {
            let handle =
                crate::ui::list_handle::ListStateHandle(chat.layout.nav_flyout_list.clone());
            let sb_state = gpui_component::scroll::ScrollbarState::default();
            flyout_wrap = flyout_wrap.child(
                div()
                    .absolute()
                    .top(px(38.))
                    .bottom(px(8.))
                    .right(px(3.))
                    .w(px(8.))
                    .child(gpui_component::scroll::Scrollbar::vertical(
                        &sb_state, &handle,
                    )),
            );
        }
        gutter = gutter.child(flyout_wrap);
    }

    // 导航刻度条（033）：屏高 75%；居中规则 = 屏高 − inputpanel 高度×50%
    // 后居中——rail 挂 mb(0.5×composer高)，flex 连 margin 盒一起居中，
    // 视觉中心正好上移 composer/4。生成算法（v63-5）：轮次均分切割整条
    // ——flex_1 等分 + 2px 缝，1 轮 = 整条弱主题色、2 轮 = 上下两段……
    // 10 轮后段高不再缩小（>10 百分比桶映射）。段色 = 弱主题色常驻，
    // 选中 = accent 边框（不整段覆盖），hover 增亮；段命中区 10px = 实体
    // 6px + 右侧 4px 透明可点（2026-10-10：命中不满宽会点穿下方内容误收
    // 面板；2026-11-08 实体 10→6、透明余量 8→4）
    if !summary.turns.is_empty() {
        let composer_h = chat.composer.composer_h.get();
        let active_tick = active_turn.map(tick_of_turn);
        let mut rail = div()
            .id("nav-rail")
            .h_full()
            .w(px(10.))
            .flex()
            .flex_col()
            // 实体段贴左，右侧留 4px 透明命中区
            .items_start()
            .gap(px(2.));
        for i in 0..tick_count {
            let on = active_tick == Some(i);
            let target_ix = summary.turns[turn_of_tick(i)].user_ix;
            let rt_scroll = rt_entity.clone();
            rail = rail.child(
                div()
                    .id(SharedString::from(format!("nav-node-{i}")))
                    // 命中区吃满 rail 10px：左 6px 实体 + 右 4px 透明同权
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .group("navtick")
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            // 面板联动（v63-6）：面板滚到该段的区域起始卡
                            //（卡序号 = 轮序号）
                            this.layout
                                .nav_flyout_list
                                .scroll_to_reveal_item(turn_of_tick(i));
                            rt_scroll
                                .read(cx)
                                .pager
                                .nav_goto(turn_of_tick(i), target_ix);
                            window.refresh();
                        }),
                    )
                    // 可见段体只有左 6px；右 4px 透明但同属命中区
                    .child(
                        div()
                            .w(px(6.))
                            .h_full()
                            .rounded(px(2.))
                            // 常驻 1px 边框（透明↔accent）防选中时尺寸跳动；
                            // 选中 = 1px accent 实边（2026-11-08 段体 10→6，
                            // 边框 2→1：border-box 下 2px 边框会吞掉大半可见填充）
                            .border_1()
                            .bg(gpui::rgba((t.accent as u32) << 8 | 0x3d))
                            .when(on, |d| d.border_color(rgb(t.accent)))
                            .when(!on, |d| d.border_color(gpui::rgba(0x00000000)))
                            // 段 hover：整段增亮到实色（navtick 组联动——
                            // 4px 透明区悬停同样增亮）
                            .group_hover("navtick", |s| s.bg(rgb(t.accent))),
                    ),
            );
        }
        gutter = gutter.child(
            // 居中外层（与原 rail 定位一致）：h_full + justify_center 把
            // rail 垂直居中，连 margin 盒一起算（视觉中心上移 composer/4）
            div()
                .flex()
                .h_full()
                .w_full()
                .flex_col()
                .items_start()
                .justify_center()
                .child(
                    // hover 响应区 = rail 垂直带 × gutter 全宽（033「悬浮
                    // 竖条弹出」）：高度 0.75 屏 + composer 偏移与 rail 同
                    // 款，宽度 w_full 含右侧 4px 呼吸位——只挂 rail 本体
                    // 的话那 4px 是死区；挂 gutter 全高又会误触上下空白区
                    div()
                        .id("nav-rail-hit")
                        .h(relative(0.75))
                        .mb(px(composer_h * 0.5))
                        .w_full()
                        .flex()
                        .flex_col()
                        .items_start()
                        .on_hover(cx.listener(|this, h: &bool, _w, cx| {
                            if *h {
                                this.layout.nav_open = true;
                                this.layout.nav_hide_at = None;
                            } else if !this.layout.nav_flyout_hovered {
                                this.layout.nav_hide_at = Some(std::time::Instant::now());
                            }
                            cx.notify();
                        }))
                        .child(rail),
                ),
        );
    }
    gutter.into_any_element()
}
