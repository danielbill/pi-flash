use super::*;

#[cfg(test)]
mod tests {
    use super::*;

    /// 条目计数记账：msgs 增长 / phase 开关 / 垫片挂卸全走 sync
    #[test]
    fn sync_tracks_structure() {
        let p = ChatList::new();
        p.sync(3, false);
        assert_eq!(p.state.item_count(), 3);
        p.sync(5, false);
        assert_eq!(p.state.item_count(), 5);
        p.sync(5, true);
        assert_eq!(p.state.item_count(), 6);
        p.sync(4, false);
        assert_eq!(p.state.item_count(), 4);
    }

    /// 消息增长插在 phase 行之前（条目序 [msgs | phase | spacer]）
    #[test]
    fn growth_inserts_before_phase() {
        let p = ChatList::new();
        p.sync(2, true);
        assert_eq!(p.state.item_count(), 3);
        p.sync(4, true);
        assert_eq!(p.state.item_count(), 5);
    }

    /// 翻页挂垫片、退役卸垫片；首帧视口未知时垫片按过估值
    #[test]
    fn page_turn_mounts_spacer_and_release_unmounts() {
        let p = ChatList::new();
        p.sync(3, false);
        p.page_turn(2, 3, false);
        assert_eq!(p.state.item_count(), 4); // [m0 m1 m2 spacer]
        assert!(p.anchor_active());
        assert_eq!(p.spacer_px(), VIEWPORT_FALLBACK);
        // 回显升级重入：不重挂不重估
        p.page_turn(2, 3, false);
        assert_eq!(p.state.item_count(), 4);
        p.release();
        p.sync(3, false);
        assert_eq!(p.state.item_count(), 3);
        assert!(!p.anchor_active());
    }

    /// 翻页锚定期消息继续增长（steer）：新消息插在垫片之前
    #[test]
    fn growth_during_anchor_keeps_spacer_last() {
        let p = ChatList::new();
        p.sync(3, false);
        p.page_turn(2, 3, false);
        p.sync(5, false);
        assert_eq!(p.state.item_count(), 6); // [m0..m4 spacer]
    }

    /// 槽位分配只认列表侧结构：消息已进 messages、还没 splice 进列表时，
    /// 垫片槽位必须仍是 Spacer（旧实现按现算 content 派槽位，会把它当成
    /// 等待行渲染 —— 一整屏垫片塌成一行 → 内容被拽到屏底再跳回，见
    /// row_slot 注释 / 2026-11-08 报障）
    #[test]
    fn row_slot_follows_listed_structure_not_live_counts() {
        // 列表侧：[m0 m1 m2 | phase | spacer]
        let (l_msgs, l_phase, l_spacer) = (3usize, true, true);
        assert_eq!(row_slot(2, l_msgs, l_phase, l_spacer), RowSlot::Message(2));
        assert_eq!(row_slot(3, l_msgs, l_phase, l_spacer), RowSlot::Phase);
        assert_eq!(row_slot(4, l_msgs, l_phase, l_spacer), RowSlot::Spacer);
        assert_eq!(row_slot(5, l_msgs, l_phase, l_spacer), RowSlot::Empty);
        // 此刻 messages.len() = 4（新消息还没 splice）：旧实现现算 content =
        // 4 + 1 = 5，垫片下标跟着漂到 5；结构里垫片仍在 4 → 判定失配。
        let live_spacer_ix = 4 + usize::from(l_phase);
        assert_eq!(live_spacer_ix, 5);
        assert_ne!(
            row_slot(live_spacer_ix, l_msgs, l_phase, l_spacer),
            RowSlot::Spacer
        );
    }

    /// listed() 与 ListState 条目数一一对应（槽位表的前提）
    #[test]
    fn listed_matches_item_count() {
        let p = ChatList::new();
        p.sync(3, false);
        p.page_turn(2, 3, false);
        p.sync(5, true);
        let (msgs, phase, spacer) = p.listed();
        assert_eq!(
            msgs + usize::from(phase) + usize::from(spacer),
            p.state.item_count()
        );
    }

    /// 外部裸 reset（记账失配）→ 下一次 sync 侦测差额整体重建
    #[test]
    fn sync_recovers_from_external_reset() {
        let p = ChatList::new();
        p.sync(3, true);
        assert_eq!(p.state.item_count(), 4);
        p.state.reset(0);
        p.sync(3, true);
        assert_eq!(p.state.item_count(), 4);
    }

    /// 会话切换 reload：reset + 记账同步，后续 sync 幂等
    #[test]
    fn reload_resyncs_bookkeeping() {
        let p = ChatList::new();
        p.sync(3, true);
        p.page_turn(2, 3, true);
        p.reload(7);
        assert_eq!(p.state.item_count(), 7);
        assert!(!p.anchor_active());
        p.sync(7, false);
        assert_eq!(p.state.item_count(), 7);
    }

    /// 滚轮自由滚动（free_scroll）不退役锚点：垫片保留、sync 幂等不摘——
    /// 旧实现滚轮即退役摘垫片，短内容被 Bottom 胶水拽到屏底且余量归零
    /// （用户报障：等待回复时稍一滚就跳底，之后滚轮彻底失灵）。
    #[test]
    fn free_scroll_keeps_spacer_instead_of_unmount() {
        let p = ChatList::new();
        p.sync(3, false);
        p.page_turn(2, 3, false);
        assert_eq!(p.state.item_count(), 4);
        p.core.free_scroll.set(true);
        p.sync(3, false);
        assert!(p.anchor_active());
        assert_eq!(p.state.item_count(), 4); // 垫片保留（旧实现此处被摘掉）
        assert_eq!(p.spacer_px(), VIEWPORT_FALLBACK);
    }

    /// 帧补账查询：无锚点 false；未结算锚点 true（带上限，12 次后放弃）
    #[test]
    fn frame_sync_requests_settle_while_unsettled() {
        let p = ChatList::new();
        p.sync(2, false);
        assert!(!p.take_frame_sync()); // 无锚点
        p.page_turn(1, 2, false);
        assert!(p.take_frame_sync()); // 未结算 → 请求补账
        assert!(p.take_frame_sync());
        for _ in 0..10 {
            assert!(p.take_frame_sync());
        }
        assert!(!p.take_frame_sync()); // 尝试次数耗尽
    }

    /// scroll_to 越界钳制到列表尾（Bottom 胶水位）
    #[test]
    fn scroll_to_clamps_overflow() {
        let p = ChatList::new();
        p.sync(2, false);
        p.state.scroll_to(gpui::ListOffset {
            item_ix: 99,
            offset_in_item: px(0.),
        });
        assert_eq!(p.state.logical_scroll_top().item_ix, 2);
    }

    /// 垫片取值：钉顶期恒为「一整屏内容区高」（内容区 = 视口高 − 上下内边距，
    /// 下边距含 composer 让位），内容长过内容区则归零并交还贴底胶水跟随尾部。
    ///
    /// 钉顶期用常数而不是「内容区高 − 锚下内容」，是为了让 gpui `layout_items`
    /// 的覆盖条件 `pt + below + spacer + pb < H` 恒不成立 ⇒ 逻辑位永远不被夺走
    /// （测量偏差、条目 reshape 后的旧缓存高度都不再能破坏钉顶）。
    #[test]
    fn spacer_target_is_a_constant_while_pinned() {
        let avail = 1000. - PAD_TOP - PAD_BOTTOM;
        assert_eq!(avail, 1000. - 22. - 188.);
        assert_eq!(spacer_target(avail, 0.), (avail, false));
        assert_eq!(spacer_target(avail, 130.), (avail, false));
        // 内容刚好填满 → 垫片 0 且转跟随（胶水接住，几何连续）
        assert_eq!(spacer_target(avail, avail), (0., true));
        assert_eq!(spacer_target(avail, 2000.), (0., true));
    }
}

#[cfg(test)]
mod glue_geometry {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{
        AppContext, Bounds, Context, Entity, IntoElement, ListOffset, ParentElement, Pixels,
        Render, Styled, TestAppContext, VisualTestContext, Window, canvas, div, list, point, px,
        size,
    };

    use super::*;
    /// 端到端几何锁：真 gpui 布局 + 真 [`ChatList`]。上面的 `tests` 模块是
    /// 无窗口单测（`viewport_bounds` 恒 0，结算根本不跑）；v59 三次回归全栽
    /// 在「公式对、但 gpui 贴底胶水的语义不对」上。这里用 gpui 测试夹具走
    /// 一遍真实帧序列：发送帧钉顶 → 下一帧结算 → 回复逐帧增长 → 长过内容区
    /// 交还胶水跟随尾部；断言用条目实际绘制位置（canvas prepaint 抓 bounds）。

    const VIEWPORT: f32 = 1000.;
    /// 内容区高 = 视口 − 上下内边距
    const AVAIL: f32 = VIEWPORT - PAD_TOP - PAD_BOTTOM;
    /// 等待行（phase）高度
    const PHASE_H: f32 = 40.;

    struct TestView {
        pager: Rc<ChatList>,
        msg_heights: Vec<f32>,
        phase: bool,
        seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>>,
    }

    impl Render for TestView {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            let pager = self.pager.clone();
            let seen = self.seen.clone();
            let msg_heights = self.msg_heights.clone();
            let msgs = msg_heights.len();
            let phase = self.phase;
            // 与 session::session_list 同构：条目序 [msgs | phase | spacer]，
            // 垫片高度取自 pager.spacer_px()，上下内边距取自同一组常量
            list(pager.state(), move |ix, _, _| {
                let seen = seen.clone();
                let height = if ix < msgs {
                    msg_heights[ix]
                } else if phase && ix == msgs {
                    PHASE_H
                } else {
                    pager.spacer_px()
                };
                div()
                    .h(px(height))
                    .w_full()
                    .child(
                        canvas(
                            move |bounds, _, _| seen.borrow_mut().push((ix, bounds)),
                            |_, _, _, _| {},
                        )
                        .size_full(),
                    )
                    .into_any_element()
            })
            .w_full()
            .h_full()
            .pt(px(PAD_TOP))
            .pb(px(PAD_BOTTOM))
        }
    }

    /// 画一帧，返回各条目实际绘制位置
    fn paint(
        cx: &mut VisualTestContext,
        pager: &Rc<ChatList>,
        seen: &Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>>,
        msg_heights: &[f32],
        phase: bool,
    ) -> Items {
        seen.borrow_mut().clear();
        cx.draw::<Entity<TestView>>(
            point(px(0.), px(0.)),
            size(px(400.), px(VIEWPORT)),
            |_, cx| {
                cx.new(|_| TestView {
                    pager: pager.clone(),
                    msg_heights: msg_heights.to_vec(),
                    phase,
                    seen: seen.clone(),
                })
            },
        );
        seen.borrow().clone()
    }

    /// 一次事件帧：先 sync（结算用上一帧布局的测量），再画一帧（重测尾部条目）
    fn step(
        window: &mut VisualTestContext,
        pager: &Rc<ChatList>,
        seen: &Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>>,
        msg_heights: &[f32],
        phase: bool,
    ) -> Items {
        pager.sync(msg_heights.len(), phase);
        paint(window, pager, seen, msg_heights, phase)
    }

    type Items = Vec<(usize, Bounds<Pixels>)>;

    fn top(items: &Items, ix: usize) -> Option<Pixels> {
        items.iter().find(|(i, _)| *i == ix).map(|(_, b)| b.top())
    }

    fn bottom(items: &Items, ix: usize) -> Option<Pixels> {
        items
            .iter()
            .find(|(i, _)| *i == ix)
            .map(|(_, b)| b.bottom())
    }

    #[gpui::test]
    fn prompt_stays_pinned_while_reply_grows_then_follows_tail(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        // 历史 m0 高 500：reload 后是普通贴底列表（末条贴住屏底 − padding.bottom）
        let mut heights = vec![500.];
        pager.reload(heights.len());
        let items = paint(window, &pager, &seen, &heights, false);
        assert_eq!(bottom(&items, 0), Some(px(VIEWPORT - PAD_BOTTOM)));

        // 发送帧：乐观气泡进树（列表里还没这个条目 = 未测量）→ 垫片只能过估
        // （视口高），钉顶由 sync 硬置。若此处「猜」出垫片，钉顶会被胶水顶掉。
        heights.push(100.);
        pager.page_turn(1, heights.len(), true);
        assert!(pager.anchor_active());
        assert_eq!(pager.spacer_px(), AVAIL);
        let items = paint(window, &pager, &seen, &heights, true);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));

        // 下一帧结算：垫片 = 内容区高 − 锚下内容（用户消息 100 + 等待行 40）。
        // 结算前后锚点都在 padding.top —— 胶水与钉顶必须同几何，否则这里会跳。
        let items = step(window, &pager, &seen, &heights, true);
        assert_eq!(pager.spacer_px(), AVAIL);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));
        assert_eq!(bottom(&items, 2), Some(px(PAD_TOP + 140.))); // 内容末条（phase 行）紧跟锚点

        // 回复到达（等待行退场）+ 流式增长：锚点全程钉在 padding.top。
        // 注意 sync 先结算后 splice，所以新条目那一帧的垫片还沿用上一帧测量
        // （偏大 300），下一帧收敛——钉顶不受影响，这正是「钉顶由逻辑位硬置、
        // 不依赖垫片精度」的意义。
        heights.push(300.);
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), AVAIL); // 上一帧测量
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), AVAIL);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));

        let last = heights.len() - 1;
        heights[last] = AVAIL - 200.; // 仍短于内容区（内容区高随 PAD_BOTTOM 变，勿写死）
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), AVAIL);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));

        // 回复长过内容区：垫片归零 → 交还贴底胶水跟随尾部（锚点退出屏顶，
        // 内容末条贴住屏底 − padding.bottom）
        heights[last] = 1200.;
        step(window, &pager, &seen, &heights, false);
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), 0.);
        assert_eq!(top(&items, 1), None);
        assert_eq!(bottom(&items, last), Some(px(VIEWPORT - PAD_BOTTOM)));

        // 用户滚轮 → free_scroll 软状态：锚点与垫片保留，sync 不退役不重钉
        pager.core.free_scroll.set(true);
        pager.sync(heights.len(), false);
        assert!(pager.anchor_active());
        assert_eq!(pager.state().item_count(), heights.len() + 1); // 垫片条目还在
    }

    /// 用户报障场景（滚轮跳底回归锁）：钉顶期内容短于内容区、等待回复时用户
    /// 轻滚——旧实现滚轮退役锚点摘垫片，Bottom 胶水把整块内容拽到屏底且滚动
    /// 余量归零。free_scroll 软状态下垫片保留，视口平滑滑动；滚回贴底（几何
    /// 上≡钉顶位）sync 恢复钉顶。
    #[gpui::test]
    fn free_scroll_keeps_spacer_no_jump_to_bottom(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        let heights = vec![500., 60.];
        pager.reload(heights.len());
        paint(window, &pager, &seen, &heights, false);
        pager.page_turn(1, heights.len(), false);
        paint(window, &pager, &seen, &heights, false);
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), AVAIL);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));

        // 用户轻滚：逻辑位下移 30px（滚轮后），free_scroll 置位
        pager.core.free_scroll.set(true);
        pager.state().scroll_to(ListOffset {
            item_ix: 1,
            offset_in_item: px(30.),
        });
        let items = step(window, &pager, &seen, &heights, false);
        // 内容没有坠到屏底：垫片条目还在，锚点只是随滚动上移了 30px
        assert!(pager.anchor_active());
        assert_eq!(pager.state().item_count(), heights.len() + 1);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP - 30.)));

        // 滚回贴底（≡钉顶位）→ free_scroll 复位 → sync 恢复钉顶
        pager.core.free_scroll.set(false);
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));
    }

    /// 自由滚动中回复长过内容区：垫片照常归零交还胶水，但**不拽底**——用户
    /// 读历史的位置保持（旧实现 settle 翻转无条件 scroll_to(count)，读历史时
    /// 被硬拽到屏底）。
    #[gpui::test]
    fn follow_flip_while_free_scroll_does_not_yank(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        let mut heights = vec![500., 60.];
        pager.reload(heights.len());
        paint(window, &pager, &seen, &heights, false);
        pager.page_turn(1, heights.len(), false);
        paint(window, &pager, &seen, &heights, false);
        step(window, &pager, &seen, &heights, false);

        // 用户上滚进历史第一屏（free_scroll），随后回复长过内容区
        pager.core.free_scroll.set(true);
        pager.state().scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        heights.push(1200.);
        step(window, &pager, &seen, &heights, false); // 新条目未测量：垫片沿用上一帧
        let items = step(window, &pager, &seen, &heights, false); // 收敛：垫片归零
        assert_eq!(pager.spacer_px(), 0.); // 垫片照常归零交几何
        assert_eq!(pager.state().item_count(), heights.len() + 1);
        assert_eq!(top(&items, 0), Some(px(PAD_TOP))); // 视口停在原地，没被拽走
    }

    /// 用户报障回归锁（2026-10「生成中拖拽滚屏无法下拉、乱跳」）：跟尾期用户
    /// 上滚读历史时流式 delta 连发，sync 第 5 步不得把尾部条目打回 Unmeasured
    /// ——否则 summary 高度当场塌陷（整轮回复按 0 计），滚轮 scroll_max 钳死
    /// 在塌缩值，下拉即被钳回；判定也不得据低估的 below 把跟尾翻回钉顶（垫片
    /// 涨回一整屏，尾部凭空多一屏空白）。
    #[gpui::test]
    fn streaming_while_reading_history_keeps_tail_measured(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        let mut heights = vec![500., 60.];
        pager.reload(heights.len());
        paint(window, &pager, &seen, &heights, false);
        pager.page_turn(1, heights.len(), false);
        paint(window, &pager, &seen, &heights, false);
        step(window, &pager, &seen, &heights, false);

        // 回复长过内容区 → 跟尾（垫片归零、末条贴住屏底）
        heights.push(1200.);
        step(window, &pager, &seen, &heights, false);
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), 0.);
        assert_eq!(bottom(&items, 2), Some(px(VIEWPORT - PAD_BOTTOM)));

        // summary 完好（滚轮 scroll_max 的数据源）
        let full = pager.state().max_offset_for_scrollbar().height;
        assert!(full > px(0.));

        // 用户上滚进历史（free_scroll），流式增长只 sync 不 paint
        //（delta 连发、视口停在历史处，尾部不进绘制范围）
        pager.core.free_scroll.set(true);
        pager.state().scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        heights[2] = 3200.;
        pager.sync(heights.len(), false);
        pager.sync(heights.len(), false);

        // 判定冻结：垫片不回涨、scroll_max 不塌、帧自检安静
        assert!(pager.core.following.get());
        assert_eq!(pager.spacer_px(), 0.);
        assert_eq!(pager.state().max_offset_for_scrollbar().height, full);
        assert!(!pager.take_frame_sync());

        // 滚回贴底（滚轮到底 ≡ 胶水位，同 jump_to_bottom）：free_scroll 复位
        // → 补测 → 跟尾照常（末条贴住屏底）
        pager.core.free_scroll.set(false);
        pager.jump_to_bottom();
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), 0.);
        assert_eq!(bottom(&items, 2), Some(px(VIEWPORT - PAD_BOTTOM)));
    }

    /// 内容**形状**变化（轮末把思考/工具折进「工作详情」→ 锚下内容大幅变矮）
    /// 之后，帧自检必须发现垫片过期并补账，把锚点重新钉回 `padding.top`。
    /// 旧实现只在 unsettled 时补账，形状变化只能等下一条 pi 事件——中间这段
    /// 时间贴底胶水会把锚点摆到屏幕中段（用户实测「翻页混乱」）。
    #[gpui::test]
    fn shape_change_triggers_frame_sync(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        // 发送 → 结算 → 长回复（垫片归零、跟尾）
        let mut heights = vec![500., 100.];
        pager.reload(heights.len());
        paint(window, &pager, &seen, &heights, false);
        pager.page_turn(1, heights.len(), false);
        paint(window, &pager, &seen, &heights, false);
        step(window, &pager, &seen, &heights, false);
        heights.push(1200.);
        step(window, &pager, &seen, &heights, false);
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), 0.);
        assert_eq!(top(&items, 1), None);

        // 整形：工作内容折起来，锚下内容 1300 → 300
        heights[2] = 300.;
        paint(window, &pager, &seen, &heights, false);
        assert!(pager.take_frame_sync()); // 帧自检发现垫片过期
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), AVAIL);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP))); // 锚点回到屏顶

        // 形状没再变 → 自检安静下来（不空转补账）
        paint(window, &pager, &seen, &heights, false);
        assert!(!pager.take_frame_sync());
    }

    /// agent 运行中，轮中工具卡/子代理卡形状变化（toolResult 到达、状态切换、
    /// details 回写）让锚下内容缩水：**不得**翻回钉顶——此时翻会把视口拽回上
    /// 一轮用户消息（乱跳），且翻转帧按旧逻辑位渲染满屏垫片、内容顶出视口上
    /// 沿（屏幕全空一帧）。轮末整形（AgentEnd，值翻 false）照常补账钉顶。
    #[gpui::test]
    fn agent_running_blocks_midturn_flip_settles_at_end(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        // 发送 → 结算 → 长回复（垫片归零、跟尾）
        let mut heights = vec![500., 100.];
        pager.reload(heights.len());
        paint(window, &pager, &seen, &heights, false);
        pager.page_turn(1, heights.len(), false);
        paint(window, &pager, &seen, &heights, false);
        step(window, &pager, &seen, &heights, false);
        heights.push(1200.);
        step(window, &pager, &seen, &heights, false);
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), 0.);

        // 轮中缩水（工具卡收拢 1200→300）：运行态下帧自检不报翻转、垫片不涨、
        // 不钉顶（内容短于视口会暂被胶水拽到屏底，属可接受的中间态，等轮末整形）
        pager.set_agent_running(true);
        heights[2] = 300.;
        let items = paint(window, &pager, &seen, &heights, false);
        assert!(!pager.take_frame_sync());
        assert_eq!(pager.spacer_px(), 0.);
        assert_ne!(top(&items, 1), Some(px(PAD_TOP)));

        // 轮末整形（AgentEnd）：照常补账钉顶，锚点回屏顶
        pager.set_agent_running(false);
        paint(window, &pager, &seen, &heights, false);
        assert!(pager.take_frame_sync());
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), AVAIL);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));
    }

    /// 内容**变矮**（轮末把思考/工具折进「工作详情」）且**没有 pi 事件**时，
    /// 钉顶必须纹丝不动：垫片是常数（一整屏内容区高），gpui「锚下填不满视口就
    /// 强制贴底」的覆盖条件恒不成立，逻辑位不会被夺走。
    ///
    /// 旧实现（垫片 = 内容区高 − 锚下内容）正是在这里翻车：整形后内容变矮，
    /// `pt + below + spacer + pb < H` 成立 → 逻辑位被改成贴底胶水，而垫片还是
    /// 按整形前的高度算的（偏大），胶水就把整段内容推到屏顶之上——只剩 agent
    /// 内容尾巴贴着屏顶、下方一片空白，用户消息整个不见（实测截图）。
    #[gpui::test]
    fn shape_shrink_without_event_keeps_pin(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        let mut heights = vec![500., 60.];
        pager.reload(heights.len());
        paint(window, &pager, &seen, &heights, false);
        pager.page_turn(1, heights.len(), false);
        paint(window, &pager, &seen, &heights, false);
        step(window, &pager, &seen, &heights, false);
        heights.push(300.); // 回合内容（短于内容区 843）→ 钉顶
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), AVAIL);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));

        // 整形变矮：只重画一帧、不补账（模拟「整形不经过 pi 事件」）
        heights[2] = 100.;
        let items = paint(window, &pager, &seen, &heights, false);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP))); // 锚点仍在屏顶
        assert!(!pager.take_frame_sync()); // 判定没翻转 → 不必补账

        // 长过内容区：帧自检发现判定翻转 → 补账 → 交还胶水跟尾
        heights[2] = 1200.;
        paint(window, &pager, &seen, &heights, false);
        assert!(pager.take_frame_sync());
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), 0.);
        assert_eq!(bottom(&items, 2), Some(px(VIEWPORT - PAD_BOTTOM)));
        assert_eq!(top(&items, 1), None); // 锚点退出屏顶（跟尾）
    }

    /// 整表重读（换工具预设的 `get_messages`、压缩后重拉、会话文件重读）之后
    /// 必须**保住钉顶**：`reanchor` 只挪锚点索引、保留垫片与跟尾状态；如果照旧
    /// `release`，就没有钉顶了，Bottom 对齐会把「等待模型响应」这类短内容连同刚
    /// 上翻的历史一起拽回屏底（用户实测：改工具选项后消息全部回落）。
    #[gpui::test]
    fn reload_keeps_pin_or_falls_to_bottom(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        let heights = vec![500., 60., 300.];
        pager.reload(heights.len());
        paint(window, &pager, &seen, &heights, false);
        pager.page_turn(1, heights.len(), false);
        paint(window, &pager, &seen, &heights, false);
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), AVAIL);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));

        // 整表重读：reanchor 同一索引 -> 钉顶纹丝不动
        pager.reanchor(1, heights.len(), false);
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), AVAIL);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));

        // 反例（旧行为）：退役锚点 -> 垫片卸掉 -> 内容整块回落到屏底
        pager.release();
        let items = paint(window, &pager, &seen, &heights, false);
        assert!(!pager.anchor_active()); // 锚点退役、垫片卸掉
        assert_eq!(pager.state().item_count(), heights.len());
        assert!(top(&items, 1).unwrap() > px(300.)); // 用户消息掉到屏幕下半部
    }

    /// 用户报障回归锁（2026-10-21）：首条消息发出 → 钉顶、agent 思考中再发
    /// 一条 steer（运行中打断投递）——打断**只能在跟尾期**退役锚点。钉顶期
    /// 退役就是摘垫片：内容还短于一屏，Bottom 胶水把整块内容拽到屏底（与上一
    /// 例的反例同几何）——用户原话「所有消息落到了屏幕下半部分」。
    #[gpui::test]
    fn interrupt_while_pinned_keeps_position(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        // 首条用户消息：发送帧钉顶（还带着等待行）
        let mut heights = vec![80.];
        pager.reload(heights.len());
        paint(window, &pager, &seen, &heights, false);
        pager.page_turn(0, heights.len(), true);
        paint(window, &pager, &seen, &heights, true);
        let items = step(window, &pager, &seen, &heights, true);
        assert_eq!(pager.spacer_px(), AVAIL);
        assert_eq!(top(&items, 0), Some(px(PAD_TOP)));

        // 思考中插话（运行中打断）：钉顶期不退役锚点，气泡只是追加在下面
        assert!(!pager.release_if_following(), "钉顶期打断不得退役锚点");
        heights.push(60.);
        let items = step(window, &pager, &seen, &heights, true);
        assert!(pager.anchor_active());
        assert_eq!(pager.spacer_px(), AVAIL);
        assert_eq!(
            top(&items, 0),
            Some(px(PAD_TOP)),
            "首条用户消息仍钉在 padding.top"
        );
        assert_eq!(
            top(&items, 1),
            Some(px(PAD_TOP + 80.)),
            "插话气泡接在其下，几何原地没动"
        );

        // 新回复流式（等待行收起）并长过内容区 → 跟尾；此后插话才退役锚点
        step(window, &pager, &seen, &heights, false);
        heights.push(1200.);
        step(window, &pager, &seen, &heights, false);
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), 0.);
        assert_eq!(bottom(&items, 2), Some(px(VIEWPORT - PAD_BOTTOM)));

        // 跟尾期插话：锚点照常退役（垫片已归零，几何纹丝不动）
        assert!(pager.release_if_following());
        assert!(!pager.anchor_active());
        let items = paint(window, &pager, &seen, &heights, false);
        assert_eq!(bottom(&items, 2), Some(px(VIEWPORT - PAD_BOTTOM)));
    }

    /// 用户报障回归锁：跟尾期（内容长过内容区）读历史后点「回到最新」，必须
    /// 落到贴底胶水位（末条贴住屏底 − padding.bottom）。旧实现只看垫片条目在
    /// 不在就 `scroll_to(锚点, 0)`——跟尾期锚点（最后一条用户消息）顶早退到屏底
    /// 之上数屏之外，视口只挪到锚点顶、按钮消失但内容根本没到底。
    #[gpui::test]
    fn jump_to_bottom_reaches_glue_while_following(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        let mut heights = vec![500., 60.];
        pager.reload(heights.len());
        paint(window, &pager, &seen, &heights, false);
        pager.page_turn(1, heights.len(), false);
        paint(window, &pager, &seen, &heights, false);
        step(window, &pager, &seen, &heights, false);

        // 回复长过内容区 → 跟尾；用户上滚回首条读历史（free_scroll）
        heights.push(1200.);
        step(window, &pager, &seen, &heights, false);
        step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), 0.);
        pager.core.free_scroll.set(true);
        pager.state().scroll_to(ListOffset {
            item_ix: 0,
            offset_in_item: px(0.),
        });
        paint(window, &pager, &seen, &heights, false);

        // 点「回到最新」：末条必须贴住屏底 − padding.bottom，且随后的 sync
        // （流式事件）不得把视口拽离胶水位（following 期间第 6 步不钉顶）
        pager.jump_to_bottom();
        assert!(pager.is_at_bottom());
        let last = heights.len() - 1;
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(bottom(&items, last), Some(px(VIEWPORT - PAD_BOTTOM)));
    }

    /// 钉顶期（内容短于内容区）点「回到最新」仍回锚点顶（≡看着最新）——
    /// 此期**不能**走胶水：垫片是一整屏内容区高，胶水会把内容末尾推到屏顶。
    #[gpui::test]
    fn jump_to_bottom_returns_to_pin_while_pinned(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        let heights = vec![500., 60., 300.];
        pager.reload(heights.len());
        paint(window, &pager, &seen, &heights, false);
        pager.page_turn(1, heights.len(), false);
        paint(window, &pager, &seen, &heights, false);
        step(window, &pager, &seen, &heights, false);
        assert_eq!(pager.spacer_px(), AVAIL);

        // 用户轻滚离位（free_scroll），随后点「回到最新」：锚点钉回 padding.top
        pager.core.free_scroll.set(true);
        pager.state().scroll_to(ListOffset {
            item_ix: 1,
            offset_in_item: px(30.),
        });
        pager.jump_to_bottom();
        let items = step(window, &pager, &seen, &heights, false);
        assert_eq!(top(&items, 1), Some(px(PAD_TOP)));
    }

    /// 快流（一帧内多个 pi 事件）下，「钉顶 / 跟尾」判定不能被**刚重测（splice）
    /// 过的条目**带偏：sync 第 5 步会把尾部条目打回 Unmeasured，而 ListState 的
    /// 未测量条目按 0 计——若被重测的正是承载整轮内容的「轮首条目」，同一帧里
    /// 第二个事件结算时就会量到 0 → 每个事件翻转一次判定 → 视图在「钉顶」与
    /// 「跟尾」之间抖，表现就是**永不自动上滚**、内容一路顶出屏幕（用户实测）。
    #[gpui::test]
    fn burst_events_do_not_flip_follow_decision(cx: &mut TestAppContext) {
        let window = cx.add_empty_window();
        let seen: Rc<RefCell<Vec<(usize, Bounds<Pixels>)>>> = Default::default();
        let pager = Rc::new(ChatList::new());

        let mut heights = vec![500., 60.];
        pager.reload(heights.len());
        paint(window, &pager, &seen, &heights, false);
        pager.page_turn(1, heights.len(), false);
        paint(window, &pager, &seen, &heights, false);
        step(window, &pager, &seen, &heights, false);

        // 回复到达（新条目未测量 → 这一帧只能钉顶）→ 布局测出 1200（长过内容区）
        heights.push(1200.);
        pager.sync(heights.len(), false);
        paint(window, &pager, &seen, &heights, false);
        // 同一帧里再有 2 个流式 delta（230 tok/s ≈ 一帧 2~4 个事件）：
        // 判定必须保持「跟尾」，不能被第二个事件弹回钉顶
        pager.sync(heights.len(), false);
        pager.sync(heights.len(), false);
        assert_eq!(
            pager.spacer_px(),
            0.,
            "跟尾必须保持，不能被第二个事件弹回钉顶"
        );
        let items = paint(window, &pager, &seen, &heights, false);
        assert_eq!(bottom(&items, 2), Some(px(VIEWPORT - PAD_BOTTOM))); // 尾巴贴屏底
        assert_eq!(top(&items, 1), None); // 锚点已退出屏顶（在自动上滚）
    }
}
