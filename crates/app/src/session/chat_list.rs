//! 聊天列表滚屏算法——pi-web `useAgentSession.ts` + `lib/chat-lazy-load.ts`
//! 滚屏层的 gpui 对应物，自 SessionRuntime 抽出独立成模块。
//!
//! 列表条目结构恒为 `[msgs | phase 行 | spacer]`，两个状态：
//!
//! 1. **钉顶**（`content_below < 内容区高`，发送帧起）：`scroll_to(锚点, 0)`
//!    把用户消息钉在内容区顶（`padding.top` 处），用户消息下方的空白由 spacer
//!    撑出；回复在锚点下方逐字长出，锚点不动、下方空白被吃掉。
//! 2. **跟随尾部**（回复长过内容区）：spacer 归零，逻辑位交还 gpui 贴底
//!    胶水——内容末条贴住视口底，锚点退出屏顶，流式增长自动下滑
//!    （pi-web liveFollow 同款）。
//!
//! **spacer 在钉顶期是一个常数：内容区高 = 视口高 − padding.top − padding.bottom。**
//!
//! 早期版本按「内容区高 − 锚下内容」精算它，那是错的：gpui `layout_items` 在
//! 「锚下填不满视口」时会**丢掉逻辑位、改成贴底胶水**（`logical_scroll_top = None`），
//! 而胶水定位用的是列表里**缓存的条目高度**。内容一旦在两帧之间变矮（轮末把思考/
//! 工具折进「工作详情」、等待行收起、下一条消息开始），精算的垫片就相对偏大：
//! 覆盖条件成立 → 逻辑位被夺走 → 胶水按偏大的旧垫片算出更靠上的起点 → **锚点条目
//! 干脆不在绘制范围里**，可见区只剩「agent 内容尾巴贴着屏顶 + 下方一整片空白」
//! （用户实测截图的错位状态）。给足一整屏内容区高，覆盖条件
//! `pt + below + spacer + pb ≥ H` 恒成立（below ≥ 0）⇒ 钉顶只由逻辑位保证，锚点
//! 位置与测量精度、与条目缓存新鲜度彻底解耦。垫片偏大只让屏下空白多一点（看不见）。
//!
//! 于是 gpui 贴底胶水（Bottom 对齐、`logical_scroll_top = None`）只在跟尾期起作用：
//! 它把末条底边钉在 `viewport.bottom − padding.bottom`——vendor/gpui
//! `elements::list` 的语义，由 gpui 侧单测
//! `test_bottom_glue_pins_last_item_above_bottom_padding` 钉死（那个 157px 的
//! 内边距差也是「垫片必须扣掉内边距」的由来：早期漏减它，锚点会停在
//! `viewport.top − padding.bottom`，消息整条滚出屏顶）。
//!
//! **滚轮/拖动即退役锚点（pi-web 同款，勿当 bug 修）**：垫片是**真实列表条目**
//! ——那片「回复下方的空白」就是它，不是 padding。退役即把它从内容里摘掉，
//! 于是内容高度 < 视口高，gpui 贴底胶水（Bottom 对齐铁律）把末条底边拉回
//! 屏底，整块内容随之下落；且落下后再没有滚动余量（内容比视口短 ⇒ 最大滚动
//! 位就是尾部位），滑不回去——想重回钉顶只有再发一条消息（pi-web 的
//! `promptAnchorActive` 同样只由发送置位）。长回复（内容高于视口）不受影响：
//! 那时胶水本来就没接管，滚到哪停在哪儿。
//!
//! 同步入口只有 [`ChatList::sync`]：增删走 splice（保逻辑滚动位），整体重排
//! 才 reset。
use std::cell::Cell;
use std::rc::Rc;

use gpui::{ListAlignment, ListOffset, ListState, Pixels, px};

/// 首帧视口未知时的垫片过估（ListState overdraw 同值）
const VIEWPORT_FALLBACK: f32 = 1000.;

/// 列表元素上下内边距——必须与 `session::session_list` 里 `list(...)` 元素的
/// `.pt()` / `.pb()` 一致（钉顶与贴底胶水的几何都以它为准，见模块头注释）。
pub(crate) const PAD_TOP: f32 = 22.;
/// 下边距 = 悬浮 composer 的让位：胶囊高 ~110（pt10 + 编辑区 60 + 控件行 26 +
/// pb12 + 边框）+ 底距 20 + **内容与胶囊上沿之间留 50px**（用户口径：agent
/// 输出距 input panel 上沿 50px 就该开始上滚）。跟尾时内容末条就停在
/// `屏底 − PAD_BOTTOM`；「回到最新」按钮仍悬浮在胶囊上方 20px 处（比内容末条
/// 低 30px，只在脱离尾部后出现，不与内容末条争同一条线）。
pub(crate) const PAD_BOTTOM: f32 = 180.;

/// 垫片高度只有两个取值：**钉顶期 = 一整屏内容区高，跟尾期 = 0**。
///
/// 不按「内容区高 − 锚下内容」精算——那正是钉顶会被夺走的老根因：测量一有偏差
/// （条目 reshape 变矮后 ListState 里缓存的旧高度尚未刷新、条目未测量按 0 计），
/// 垫片就偏小，gpui `layout_items` 的覆盖条件 `pt + below + spacer + pb < H`
/// 随即成立 → 逻辑位被强制改成贴底胶水（logical = None），而胶水定位又用那份旧
/// 缓存算出更靠上的起点 → **锚点条目干脆不在绘制范围里**，可见区只剩「内容尾巴
/// 贴在屏顶 + 下方空白」（用户实测截图）。给足一整屏内容区高，覆盖条件
/// `pt + below + avail + pb ≥ H` 恒成立 ⇒ 钉顶只由逻辑位保证，锚点位置与测量
/// 精度彻底解耦；垫片偏大只让屏下空白多一点（本来也看不见）。
///
/// 返回值第二项 = 是否该交还贴底胶水跟随尾部（内容已长过内容区）。
fn spacer_target(avail: f32, content_below: f32) -> (f32, bool) {
    if content_below >= avail {
        (0., true)
    } else {
        (avail, false)
    }
}

/// 列表条目分解记账：`[msgs | phase | spacer]` 外科 splice 的依据
#[derive(Clone, Copy, Default, Debug, PartialEq)]
struct Listed {
    msgs: usize,
    phase: bool,
    spacer: bool,
}

/// 共享内核：scroll handler 闭包与各方法共用（全内部可变性，&self 即可改）
struct Core {
    anchor: Cell<Option<usize>>,
    listed: Cell<Listed>,
    at_bottom: Cell<bool>,
    spacer_px: Cell<f32>,
    /// 滚动回调请求过退役（垫片待卸）——回调里严禁 splice（见 handler）
    pending_release: Cell<bool>,
    /// 垫片已按实测内容结算（之前是视口高过估，靠硬钉顶兜底）
    settled: Cell<bool>,
    /// 内容长过内容区：spacer 归零、交还贴底胶水跟随尾部
    following: Cell<bool>,
    /// 结算尝试次数（渲染帧驱动，带上限防病态循环）
    settle_attempts: Cell<u32>,
    /// 导航点击钉住的轮次（v63-7）：点击定位后选中段固定为该轮——贴底
    /// 钳制会把视口顶改写到目标之前（layout_items 向上补条目并改写
    /// logical），按「视口顶之上最近轮」推导会错选上一段；物理滚轮清除，
    /// 交还位置推导
    nav_pinned: Cell<Option<usize>>,
}

/// 聊天列表滚屏状态机。持有 gpui [`ListState`]（Bottom 对齐）+ 锚点 + 垫片
/// 高度 + splice 记账；scroll handler 内建（贴底判定、滚轮退役锚点）。
pub(crate) struct ChatList {
    state: ListState,
    core: Rc<Core>,
}

impl ChatList {
    pub(crate) fn new() -> Self {
        let state = ListState::new(0, ListAlignment::Bottom, px(VIEWPORT_FALLBACK));
        let core = Rc::new(Core {
            anchor: Cell::new(None),
            listed: Cell::new(Listed::default()),
            at_bottom: Cell::new(true),
            spacer_px: Cell::new(0.),
            pending_release: Cell::new(false),
            settled: Cell::new(false),
            following: Cell::new(false),
            settle_attempts: Cell::new(0),
            nav_pinned: Cell::new(None),
        });
        {
            let core = core.clone();
            // 贴底判定（pi-web isNearBottom）：is_scrolled = 用户主动滚动，
            // 锚点就地退役；翻转时 refresh 重绘悬浮按钮（滚动事件不触发
            // 实体 notify）。
            // 铁律：此处严禁触碰 ListState——gpui scroll() 持有 RefCell
            // 可变借用期间回调本闭包，再 splice/borrow 即 BorrowMutError
            // 崩溃。退役只置标记，卸垫片由下一帧渲染补 sync（渲染层经
            // take_frame_sync 驱动）。
            state.set_scroll_handler(move |ev, window, _| {
                if ev.is_scrolled {
                    core.request_detach();
                    // 物理滚动交还位置推导（v63-7）：导航钉住的选中段失效
                    core.nav_pinned.set(None);
                }
                let now = !ev.is_scrolled;
                if core.at_bottom.replace(now) != now {
                    window.refresh();
                }
            });
        }
        Self { state, core }
    }

    // ---- 只读查询（渲染层） ----

    /// 列表状态克隆（交给 `list()` 元素 / 导航点击跳转）
    pub(crate) fn state(&self) -> ListState {
        self.state.clone()
    }

    /// 锚点是否激活（spacer 条目渲染依据）
    pub(crate) fn anchor_active(&self) -> bool {
        self.core.anchor.get().is_some()
    }

    /// 锚点条目索引（整表重读后校验/重锚用）
    pub(crate) fn anchor_ix(&self) -> Option<usize> {
        self.core.anchor.get()
    }

    /// 垫片当前高度 px
    pub(crate) fn spacer_px(&self) -> f32 {
        self.core.spacer_px.get()
    }

    /// 视口贴底标记（「回到最新」按钮显隐）
    pub(crate) fn is_at_bottom(&self) -> bool {
        self.core.at_bottom.get()
    }

    /// 视口顶所在条目（会话导航比例尺）
    pub(crate) fn scroll_top_ix(&self) -> usize {
        self.state.logical_scroll_top().item_ix
    }

    /// 渲染层每帧消费一次：要不要补一次 sync？三种触发：
    ///
    /// 1. 滚动回调请求过退役（垫片待卸）——回调里不可 splice
    /// 2. 锚点未结算（还没拿到实测锚下高度，钉顶/跟尾没定），带 12 次上限防病态循环
    /// 3. **钉顶/跟尾判定已过期**：内容跨过内容区高（流式增长长过一屏、轮末把
    ///    思考/工具折进「工作详情」又缩回来）——这类形状变化不经过 pi 事件，
    ///    没有补账就会一直停在旧判定上（该跟尾时不跟、该钉顶时压在屏底）。只在
    ///    判定**翻转**时请求，静止时完全不空转。
    ///
    /// 滚动回调与列表布局期都不可触碰 ListState（RefCell 冲突），这里是补账入口。
    pub(crate) fn take_frame_sync(&self) -> bool {
        let release = self.core.pending_release.replace(false);
        let Some(ix) = self.core.anchor.get() else {
            return release;
        };
        if !self.core.settled.get() {
            let n = self.core.settle_attempts.get();
            self.core.settle_attempts.set(n.saturating_add(1));
            return release || n < 12;
        }
        release || self.follow_decision_stale(ix)
    }

    /// 搜索命中 / 导航点击定位：reveal 条目
    pub(crate) fn reveal(&self, ix: usize) {
        self.state.scroll_to_reveal_item(ix);
    }

    /// 导航点击定位（033）：目标条目置顶。不用 reveal——它向下跳时把目标
    /// 底边贴视口底，视口顶落进上一轮内容，刻度的 active 判定（视口顶之
    /// 上最近轮）会算到前一轮；置顶后视口顶即目标，刻度精确命中被点轮。
    /// 程序化定位不经过滚轮回调，at_bottom 在此补正（导航跳走即离开贴底，
    /// 「回到最新」按钮随之出现）。
    ///
    /// `turn` = 被点轮次，钉入 nav_pinned：目标靠近会话尾部时下方内容不
    /// 足填满视口，layout_items 向上补条目并改写 logical_scroll_top（贴底
    /// 钳制），视口顶不在目标轮——选中段若仍按位置推导会错选上一段
    ///（v63-7），钉住后由点击轮次直接决定，物理滚轮清除。
    pub(crate) fn nav_goto(&self, turn: usize, ix: usize) {
        self.state
            .scroll_to(ListOffset { item_ix: ix, offset_in_item: px(0.) });
        self.core.nav_pinned.set(Some(turn));
        self.core.at_bottom.set(false);
    }

    /// 导航点击钉住的轮次（无则按滚动位置推导选中段）
    pub(crate) fn nav_pinned(&self) -> Option<usize> {
        self.core.nav_pinned.get()
    }

    /// 「回到最新」：钉顶期（锚点仍激活）就是 `scroll_to(锚点, 0)` —— 内容短于
    /// 内容区时「最新」= 锚点顶 + 全部内容，钉顶位本来就是看着最新；锚点已退役
    /// （用户滚走过）才把逻辑位交还贴底胶水 `scroll_to(count)`（下一帧归一化回
    /// Bottom 跟随位，pi-web scrollToBottom + isNearBottom=true 同款）。
    ///
    /// 注意**别在垫片还是正数时用胶水**：钉顶期垫片是一整屏内容区高，胶水会把
    /// 末条底边钉在屏底、把「内容末尾」推到屏顶（下方一片空白）——正是用户截到
    /// 的那个错位状态。
    pub(crate) fn jump_to_bottom(&self) {
        let target = if self.core.listed.get().spacer {
            self.core.anchor.get().unwrap_or(self.state.item_count())
        } else {
            self.state.item_count()
        };
        self.state
            .scroll_to(ListOffset { item_ix: target, offset_in_item: px(0.) });
        self.core.at_bottom.set(true);
    }

    // ---- 翻页与同步 ----

    /// 用户发言翻页：锚定 `anchor_ix`（最后一条用户消息）钉内容区顶，历史滚
    /// 出屏上方。发送帧新消息未测量 → 垫片先给满一整屏内容区高（这是钉顶期唯一
    /// 的垫片取值，见 [`spacer_target`]），钉顶由 [`ChatList::sync`] 第 6 步硬置。
    /// 同锚点重入（回显升级）不重钉不重估。
    pub(crate) fn page_turn(&self, anchor_ix: usize, msgs: usize, phase: bool) {
        let fresh = self.core.anchor.get() != Some(anchor_ix);
        self.core.anchor.set(Some(anchor_ix));
        if fresh {
            let vh = f32::from(self.state.viewport_bounds().size.height);
            let avail = (vh - PAD_TOP - PAD_BOTTOM).max(0.);
            self.core
                .spacer_px
                .set(if avail > 0. { avail } else { VIEWPORT_FALLBACK });
            self.core.settled.set(false);
            self.core.following.set(false);
            self.core.settle_attempts.set(0);
        }
        // 先落账：条目进树后 scroll_to 才有得钉（越界会被钳到列表尾）
        self.sync(msgs, phase);
        if fresh {
            self.state
                .scroll_to(ListOffset { item_ix: anchor_ix, offset_in_item: px(0.) });
        }
    }

    /// 整表重读（工具预设换绑的 `get_messages`、压缩后重拉、会话文件重读）后
    /// 把锚点落到 `ix`（调用方确认它是一条用户消息）：**垫片与跟尾状态原样保留**，
    /// 只把 settled 打回未结算让下一帧重新实测、并由 [`ChatList::sync`] 第 6 步
    /// 重新钉顶。
    ///
    /// 为什么不能沿用「整表重读就 release」：release 会把锚点退役并卸掉垫片，
    /// 于是 Bottom 对齐把「等待模型响应」这类短内容连同刚上翻的历史一起拽回屏底
    /// （用户实测：换工具选项时消息全部回落）。
    pub(crate) fn reanchor(&self, ix: usize, msgs: usize, phase: bool) {
        self.core.anchor.set(Some(ix));
        self.core.settled.set(false);
        self.core.settle_attempts.set(0);
        self.sync(msgs, phase);
    }

    /// 列表结构同步：垫片结算、msgs 增长、phase 行增删、垫片挂卸、整体重
    /// 排 reset、尾部外科重测。幂等，事件层每次 notify 调用。
    pub(crate) fn sync(&self, msgs: usize, phase: bool) {
        self.settle_spacer(msgs, phase);
        let want_spacer = self.core.anchor.get().is_some();
        let l = self.core.listed.get();

        // 1. 消息增长：插在 phase/垫片之前
        if msgs > l.msgs {
            self.state.splice(l.msgs..l.msgs, msgs - l.msgs);
        }
        // 2. phase 行增删（挂在 msgs 之后）
        if phase != l.phase {
            let at = l.msgs;
            if phase {
                self.state.splice(at..at, 1);
            } else {
                self.state.splice(at..at + 1, 0);
            }
        }
        // 3. 垫片增删（挂在最尾）
        let content = msgs + usize::from(phase);
        if want_spacer != l.spacer {
            if want_spacer {
                self.state.splice(content..content, 1);
            } else {
                self.state.splice(content..content + 1, 0);
            }
        }
        // 4. 仍有差额 = 整体重排（快照重建/回退/外部裸 reset）→ reset 回贴底
        let target = content + usize::from(want_spacer);
        if self.state.item_count() != target {
            self.state.reset(target);
        } else if msgs > 0 {
            // 5. 外科重测尾部：最后一条消息 + phase + 垫片（流式增长/垫片缩放）。
            //    **但绝不重测「轮首条目」**（锚点 + 1，它承载整轮内容）：把它打回
            //    Unmeasured 后，同一帧里第二个 pi 事件结算时 ListState 会按 0 计它
            //    的高度 → below 只剩锚点的高度 → 钉顶/跟尾判定每事件翻转一次 → 快流
            //    （一帧 2~4 个 delta）下视图在「钉顶」与「跟尾」之间抖，表现就是
            //    **永不自动上滚**、内容一路顶出屏幕。它本来每帧都会被布局重测：
            //    钉顶期在首屏内、跟尾期在胶水 walk-up 里都会被渲染。
            let from = msgs - 1;
            let from = match self.core.anchor.get() {
                Some(ix) if from <= ix + 1 => ix + 2,
                _ => from,
            };
            if from < target {
                self.state.splice(from..target, target - from);
            }
        }
        self.core.listed.set(Listed { msgs, phase, spacer: want_spacer });
        // 6. 钉顶：内容仍短于内容区（!following）时逻辑位恒停在锚点顶。配合
        //    钉顶期「垫片 = 一整屏内容区高」这个常数，gpui「锚下填不满视口就
        //    强制贴底」的覆盖条件恒不成立 ⇒ 逻辑位不会被夺走，锚点永远画在
        //    padding.top（与测量精度、条目缓存新鲜度无关）。内容长过内容区
        //    （following）时不钉，交给胶水跟随尾部。
        if want_spacer
            && !self.core.following.get()
            && let Some(ix) = self.core.anchor.get()
        {
            self.state
                .scroll_to(ListOffset { item_ix: ix, offset_in_item: px(0.) });
        }
    }

    /// 会话切换 / tail 预渲染：整体 reset 到 msgs 条并同步记账（fresh
    /// runtime 专用——phase/锚点均未激活）。
    pub(crate) fn reload(&self, msgs: usize) {
        self.core.anchor.set(None);
        self.core.spacer_px.set(0.);
        self.core.settled.set(false);
        self.core.following.set(false);
        self.state.reset(msgs);
        self.core.listed.set(Listed { msgs, phase: false, spacer: false });
    }

    /// 锚点退役（用户滚轮 / 快照重建 / 发送失败 / 外部整表重读）。就地卸载垫片，
    /// 切回普通贴底列表——注意 Bottom 对齐下内容短于视口会被拽到屏底，所以轮末
    /// （AgentEnd/AgentSettled）**不**走这里，否则刚钉到屏顶的消息立刻跳回屏底。
    pub(crate) fn release(&self) {
        self.core.detach(&self.state);
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
    fn settle_spacer(&self, msgs: usize, phase: bool) {
        let Some(ix) = self.core.anchor.get() else { return };
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
        let last = (msgs + usize::from(phase)).saturating_sub(1).min(last_content);
        if last < ix {
            return; // 锚点之后还没有内容条目（发送帧）：保持现值，下一帧再看
        }
        // 内容区高 = 视口 − 上下内边距（见模块头注释）
        let avail = (vh - PAD_TOP - PAD_BOTTOM).max(0.);
        let Some(below) = self.content_below(ix, last).map(f32::from) else { return };
        let (spacer_px, follow) = spacer_target(avail, below);
        self.core.settled.set(true);
        self.core.spacer_px.set(spacer_px);
        let was_following = self.core.following.replace(follow);
        if follow && !was_following {
            // scroll_to(count) ≡ Bottom 逻辑位 None（下一帧 layout 归一化）
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
    fn follow_decision_stale(&self, ix: usize) -> bool {
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
    /// 滚动回调路径的锚点退役：只清锚 + 置标记，严禁在此 splice ListState
    /// （scroll() 持有 RefCell 可变借用）。实际卸垫片由下一次 sync 完成
    /// （渲染层 take_frame_sync 驱动）。
    fn request_detach(&self) {
        if self.anchor.take().is_some() {
            self.pending_release.set(true);
        }
    }

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
    }
}

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

    /// 滚动回调退役只置标记：卸垫片延迟到下一次 sync（RefCell 约束——
    /// scroll() 持有可变借用期间 splice 必崩）
    #[test]
    fn scroll_detach_defers_spacer_unmount() {
        let p = ChatList::new();
        p.sync(3, false);
        p.page_turn(2, 3, false);
        assert_eq!(p.state.item_count(), 4);
        p.core.request_detach();
        assert!(!p.anchor_active());
        assert!(p.take_frame_sync());
        assert_eq!(p.state.item_count(), 4); // 垫片还在（延迟卸载）
        p.sync(3, false);
        assert_eq!(p.state.item_count(), 3);
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
        p.state
            .scroll_to(gpui::ListOffset { item_ix: 99, offset_in_item: px(0.) });
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
        assert_eq!(avail, 1000. - 22. - 180.);
        assert_eq!(spacer_target(avail, 0.), (avail, false));
        assert_eq!(spacer_target(avail, 130.), (avail, false));
        // 内容刚好填满 → 垫片 0 且转跟随（胶水接住，几何连续）
        assert_eq!(spacer_target(avail, avail), (0., true));
        assert_eq!(spacer_target(avail, 2000.), (0., true));
    }
}

/// 端到端几何锁：真 gpui 布局 + 真 [`ChatList`]。
///
/// 上面的 `tests` 模块是无窗口单测（`viewport_bounds` 恒 0，结算根本不跑），
/// 而 v59 三次回归全栽在「公式对、但 gpui 贴底胶水的语义不对」上。这里用 gpui
/// 测试夹具走一遍真实帧序列：发送帧钉顶 → 下一帧结算 → 回复逐帧增长 → 长过
/// 内容区交还胶水跟随尾部；断言用条目实际绘制位置（canvas prepaint 抓 bounds）。
#[cfg(test)]
mod glue_geometry {
    use std::cell::RefCell;
    use std::rc::Rc;

    use gpui::{
        AppContext, Bounds, Context, Entity, IntoElement, ParentElement, Pixels, Render, Styled,
        TestAppContext, VisualTestContext, Window, canvas, div, list, point, px, size,
    };

    use super::{ChatList, PAD_BOTTOM, PAD_TOP};

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
        items.iter().find(|(i, _)| *i == ix).map(|(_, b)| b.bottom())
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

        // 用户滚轮 → 锚点退役、垫片就地卸载（回到普通贴底列表）
        pager.core.request_detach();
        pager.sync(heights.len(), false);
        assert!(!pager.anchor_active());
        assert_eq!(pager.state().item_count(), heights.len());
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
        assert_eq!(pager.spacer_px(), 0., "跟尾必须保持，不能被第二个事件弹回钉顶");
        let items = paint(window, &pager, &seen, &heights, false);
        assert_eq!(bottom(&items, 2), Some(px(VIEWPORT - PAD_BOTTOM))); // 尾巴贴屏底
        assert_eq!(top(&items, 1), None); // 锚点已退出屏顶（在自动上滚）
    }
}
