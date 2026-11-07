//! 贴底跟随与滚屏算法（reload/release/settle_spacer/follow_decision 等）
//! 与 Core 卸载钩子。拆自 chat_list（itlq.5，只搬不改）。

use super::*;

impl ChatList {
    /// 会话切换 / tail 预渲染：整体 reset 到 msgs 条并同步记账（fresh
    /// runtime 专用——phase/锚点均未激活）。
    pub(crate) fn reload(&self, msgs: usize) {
        self.core.anchor.set(None);
        self.core.spacer_px.set(0.);
        self.core.settled.set(false);
        self.core.following.set(false);
        self.core.free_scroll.set(false);
        self.state.reset(msgs);
        self.core.listed.set(Listed {
            msgs,
            phase: false,
            spacer: false,
        });
    }

    /// 锚点退役（快照重建 / 发送失败 / fork / 外部整表重读）。就地卸载垫片，
    /// 切回普通贴底列表——注意 Bottom 对齐下内容短于视口会被拽到屏底，所以轮末
    /// （AgentEnd/AgentSettled）**不**走这里，否则刚钉到屏顶的消息立刻跳回屏底。
    /// 普通滚轮走 free_scroll 软状态，不进这里。
    pub(crate) fn release(&self) {
        self.core.detach(&self.state);
    }

    /// 打断投递（运行中插话）的锚点处置：**只在跟尾期退役**，钉顶期原样保留。
    ///
    /// 跟尾期退役是有意义的：垫片早已归零（不承载几何，卸掉不跳），而锚点一
    /// 退役就不会在轮末内容缩水（思考/工具折进「工作详情」）时被
    /// [`ChatList::settle_spacer`] 翻回钉顶、把视口拽回上一轮的用户消息。
    ///
    /// 钉顶期（内容还短于内容区）绝不能退役：垫片是一整屏内容区高，摘掉它
    /// `layout_items` 的覆盖条件立刻成立 → 逻辑位被夺、Bottom 胶水把**整块
    /// 短内容拽到屏底**（用户报障：首条消息已钉顶、agent 思考中再发一条 steer，
    /// 所有消息落到屏幕下半部分）。钉顶期保留锚点则消息只是追加在下面——内容
    /// 还装在一屏里，几何原地不动，等新回复长过内容区再交还胶水跟尾。
    ///
    /// 返回是否真的退役了（调用方日志/断言用）。
    pub(crate) fn release_if_following(&self) -> bool {
        if self.core.anchor.get().is_none() || !self.core.following.get() {
            return false;
        }
        self.core.detach(&self.state);
        true
    }

    /// 垫片结算：唯一定「钉顶 or 跟尾」的地方（垫片取值见 [`spacer_target`]）。
    ///
    /// - 锚下内容短于内容区：垫片给满一整屏、逻辑位由 [`ChatList::sync`] 第 6 步
    ///   硬停在锚点顶（与测量精度解耦）
    /// - 内容长过内容区：垫片归零，逻辑位交还贴底胶水跟随尾部（只在翻过来的
    ///   那一次交还；翻回去由 sync 第 6 步重新钉顶）
    ///
    /// 实测不可用（条目刚 splice 进来还没测量 / 还没布局）则保持现值，等下一次
    /// sync——反正钉顶期垫片是个常数，不影响几何。
    pub(super) fn settle_spacer(&self, msgs: usize, phase: bool) {
        let Some(ix) = self.core.anchor.get() else {
            return;
        };
        let vh = f32::from(self.state.viewport_bounds().size.height);
        if vh <= 0. {
            return;
        }
        // 末条**内容**条目：结算发生在 splice 之前，此刻列表里可能还是「旧结构 +
        // 垫片」，必须按实际条目数夹紧——否则会把垫片自己算成「锚下内容」，below
        // 凭空多出一整屏，钉顶被误判成跟尾（实测踩过）。夹到已存在的最后一条内容
        // 条目，等价于用「还没进树的新条目高度 = 0」的下界，方向安全（垫片偏大）。
        let listed_spacer = usize::from(self.core.listed.get().spacer);
        let last_content = self.state.item_count().saturating_sub(1 + listed_spacer);
        let last = (msgs + usize::from(phase))
            .saturating_sub(1)
            .min(last_content);
        if last < ix {
            return; // 锚点之后还没有内容条目（发送帧）：保持现值，下一帧再看
        }
        // 内容区高 = 视口 − 上下内边距（见模块头注释）
        let avail = (vh - PAD_TOP - PAD_BOTTOM).max(0.);
        let Some(below) = self.content_below(ix, last).map(f32::from) else {
            return;
        };
        let (spacer_px, follow) = spacer_target(avail, below);
        // 自由滚动中禁止「跟尾 → 钉顶」回退：below 依赖尾部实测高度，用户滚离
        // 尾部后尾部条目不可见（第 5 步也跳过重测），below 是陈旧/低估值，不足以
        // 推翻已确立的跟尾——误翻会把垫片涨回一整屏（尾部凭空多一屏空白），滚回
        // 贴底先看到空白、判定翻回又瞬移。真发生轮末缩水也不急：滚回贴底（
        // free_scroll 复位）后下一帧 take_frame_sync 照常补账重钉。
        //
        // agent 运行中同样禁止：轮中工具卡/子代理卡形状变化（toolResult 到达、
        // 状态切换、poll 回写 details）会让锚下内容缩水几十~几百px，below 跌破
        // 内容区被判「该钉顶」——但轮还没结束，翻回钉顶会把视口拽回上一轮用户
        // 消息（乱跳），翻转帧按旧逻辑位渲染满屏垫片、内容顶出视口上沿（屏幕
        // 全空一帧）。轮末整形（工作详情折叠）发生在 AgentEnd 之后，此时
        // agent_running 已翻 false，照常放行。两条拦截各管各的误翻源；反向翻转
        // （钉顶 → 跟尾）照旧放行（follow_flip_while_free_scroll_does_not_yank）。
        if self.core.following.get()
            && !follow
            && (self.core.free_scroll.get() || self.core.agent_running.get())
        {
            return;
        }
        self.core.settled.set(true);
        self.core.spacer_px.set(spacer_px);
        let was_following = self.core.following.replace(follow);
        if follow && !was_following && !self.core.free_scroll.get() {
            // scroll_to(count) ≡ Bottom 逻辑位 None（下一帧 layout 归一化）。
            // 自由滚动中不拽底：用户在读历史，垫片归零交几何即可，视口保持
            self.state.scroll_to(ListOffset {
                item_ix: self.state.item_count(),
                offset_in_item: px(0.),
            });
        }
    }

    /// 钉顶/跟尾判定是否已过期：末条内容条目取「条目数 − 垫片」推得
    /// （`[msgs | phase | spacer]` 里垫片恒在最尾），故不必知道 msgs/phase 拆分。
    /// 只看判定是否翻转——垫片在钉顶期是常数（一整屏），几何不受测量影响，
    /// 不需要每帧重算。
    pub(super) fn follow_decision_stale(&self, ix: usize) -> bool {
        let vh = f32::from(self.state.viewport_bounds().size.height);
        if vh <= 0. {
            return false;
        }
        let avail = (vh - PAD_TOP - PAD_BOTTOM).max(0.);
        let spacer_listed = self.core.listed.get().spacer;
        let last = self
            .state
            .item_count()
            .saturating_sub(1 + usize::from(spacer_listed));
        let Some(below) = self.content_below(ix, last).map(f32::from) else {
            return false;
        };
        let (_, follow) = spacer_target(avail, below);
        // agent 运行中只放行「钉顶 → 跟尾」正向翻转（流式内容涨过一屏必须能转
        // 跟尾）；「跟尾 → 钉顶」与 settle_spacer 同源拦截（理由见彼处）：轮中
        // 工具卡缩水是运行期噪声，误报会让每帧翻转请求 → sync → 满屏垫片过渡
        // 帧（屏幕全空）。轮末整形在 AgentEnd 后，值已翻 false，照常补账重钉。
        if self.core.agent_running.get() && !follow && self.core.following.get() {
            return false;
        }
        follow != self.core.following.get()
    }

    /// 锚点条目顶 → 内容末条（含 phase 行，不含垫片）底的高度（含锚点自身）。
    /// 一条都还没测量（首帧 / 刚 splice）返回 None。
    ///
    /// 用 [`ListState::measured_height_in`] 而非 `bounds_for_item`：贴底胶水下
    /// 逻辑位是 `item_count`，后者的「条目必须 ≥ 逻辑位」前置判断会把锚点直接判成
    /// None，垫片便再也结算不了（旧实现只能结算一次，之后流式增长全靠一帧前的
    /// 高度，锚点随内容越长越往屏顶外漂）。区间求和还不要求末条本身已测量——
    /// 超长回合里渲染范围到不了尾部（尾部条目高度多为 0，不参与求和），此时得到
    /// 的是**下界**：垫片偏大、钉顶更稳，且「下界已填满内容区」仍足以判定转跟随。
    fn content_below(&self, ix: usize, last: usize) -> Option<Pixels> {
        let last = last.max(ix);
        let below = self.state.measured_height_in(ix..last + 1);
        (below > px(0.)).then_some(below)
    }
}
impl Core {
    /// 清锚 + 就地卸载垫片（release 专用，运行在实体事件上下文，可安全
    /// splice；记账同步回写，后续 sync 幂等）。
    fn detach(&self, state: &ListState) {
        if self.anchor.take().is_none() {
            return;
        }
        let l = self.listed.get();
        if l.spacer {
            let content = l.msgs + usize::from(l.phase);
            state.splice(content..content + 1, 0);
        }
        self.listed.set(Listed { spacer: false, ..l });
        self.settled.set(false);
        self.following.set(false);
        self.free_scroll.set(false);
    }
}
