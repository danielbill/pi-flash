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
//! **滚轮 = free_scroll 软状态（有意偏离 pi-web）**：垫片是**真实列表条目**
//! ——那片「回复下方的空白」就是它，不是 padding。早期滚轮即退役锚点、摘掉
//! 垫片（pi-web `promptAnchorActive` 同款，滚轮即清锚），但摘垫片后内容高
//! < 视口，Bottom 贴底胶水把整块内容拽回屏底、滚动余量归零（用户报障：等待
//! 回复时稍一滚就跳底，且之后滚轮彻底失灵）。现为软状态：滚轮只置
//! `free_scroll`，暂停 sync 第 6 步重钉与跟尾翻转拽底，**锚点与垫片保留**——
//! 微滚就是平滑滑动，滚回贴底（几何上≡钉顶位）自动恢复钉顶，读历史不被流式
//! 拽走；长回复照常由 settle_spacer 归零垫片、交还胶水跟尾。真正的退役
//! （`release` / 打断投递的 [`ChatList::release_if_following`]）只留给发送
//! 失败 / fork / 整表重读等重置场景——钉顶期退役必把短内容拽到屏底。
//!
//! 自由滚动期间**钉顶/跟尾判定整体冻结**（第 5 步尾部重测、settle 回判、帧
//! 自检全部暂停）：判定依据是尾部实测高度，滚离尾部后尾部条目不在绘制范围、
//! 实测不可得，据陈旧/低估的 below 回判会把跟尾误翻回钉顶、垫片涨回一整屏，
//! summary 塌陷还会把滚轮 scroll_max 钳死——生成中读历史因此「无法下拉、各种
//! 乱跳」（2026-10 用户报障）。滚回贴底（free_scroll 复位）后下一帧照常补账。
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
///
/// 034：胶囊底距 20 → 28（让位底部状态带）+ 内部 pb 12 → 7，净上浮 8 →
/// 180 → 188（不同步就会让最后一条消息被胶囊压住）。
pub(crate) const PAD_BOTTOM: f32 = 188.;

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
pub(crate) mod scroll;
#[cfg(test)]
mod tests;
impl ChatList {
    pub(crate) fn new() -> Self {
        let state = ListState::new(0, ListAlignment::Bottom, px(VIEWPORT_FALLBACK));
        let core = Rc::new(Core {
            anchor: Cell::new(None),
            listed: Cell::new(Listed::default()),
            at_bottom: Cell::new(true),
            spacer_px: Cell::new(0.),
            free_scroll: Cell::new(false),
            settled: Cell::new(false),
            following: Cell::new(false),
            agent_running: Cell::new(false),
            settle_attempts: Cell::new(0),
            nav_pinned: Cell::new(None),
        });
        {
            let core = core.clone();
            // 贴底判定（pi-web isNearBottom）：is_scrolled = 离开贴底/钉顶几何
            // → free_scroll 软状态（暂停重钉，锚点与垫片保留）；滚回贴底自动
            // 复位。翻转时 refresh 重绘悬浮按钮（滚动事件不触发实体 notify）。
            // 铁律：此处严禁触碰 ListState——gpui scroll() 持有 RefCell
            // 可变借用期间回调本闭包，再 splice/borrow 即 BorrowMutError
            // 崩溃。
            state.set_scroll_handler(move |ev, window, _| {
                core.free_scroll.set(ev.is_scrolled);
                if ev.is_scrolled {
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

    /// 渲染层每帧消费一次：要不要补一次 sync？两种触发：
    ///
    /// 1. 锚点未结算（还没拿到实测锚下高度，钉顶/跟尾没定），带 12 次上限防病态循环
    /// 2. **钉顶/跟尾判定已过期**：内容跨过内容区高（流式增长长过一屏、轮末把
    ///    思考/工具折进「工作详情」又缩回来）——这类形状变化不经过 pi 事件，
    ///    没有补账就会一直停在旧判定上（该跟尾时不跟、该钉顶时压在屏底）。只在
    ///    判定**翻转**时请求，静止时完全不空转。
    ///
    /// 滚动回调与列表布局期都不可触碰 ListState（RefCell 冲突），这里是补账入口。
    pub(crate) fn take_frame_sync(&self) -> bool {
        let Some(ix) = self.core.anchor.get() else {
            return false;
        };
        if !self.core.settled.get() {
            let n = self.core.settle_attempts.get();
            self.core.settle_attempts.set(n.saturating_add(1));
            return n < 12;
        }
        // 自由滚动中判定冻结（数据不可信，见 settle_spacer）：follow_decision_stale
        // 用同一份低估的 below 会误报「判定过期」→ 每帧 notify → sync 空转
        if self.core.free_scroll.get() {
            return false;
        }
        self.follow_decision_stale(ix)
    }

    /// 同步 agent 运行态（事件层 notify_list 两处 sync 调用点每拍前置调用；
    /// pi 事件尾部统一走 notify_list，值不会滞后于真实状态）
    pub(crate) fn set_agent_running(&self, running: bool) {
        self.core.agent_running.set(running);
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
    ///
    /// 同时置 free_scroll：导航定位是程序化跳转，若不清，流式期间 sync 第
    /// 6 步会把视口硬拽回钉顶锚点，定位立刻被覆盖。
    pub(crate) fn nav_goto(&self, turn: usize, ix: usize) {
        self.state.scroll_to(ListOffset {
            item_ix: ix,
            offset_in_item: px(0.),
        });
        self.core.nav_pinned.set(Some(turn));
        self.core.at_bottom.set(false);
        self.core.free_scroll.set(true);
    }

    /// 导航点击钉住的轮次（无则按滚动位置推导选中段）
    pub(crate) fn nav_pinned(&self) -> Option<usize> {
        self.core.nav_pinned.get()
    }

    /// 「回到最新」：钉顶期（垫片还是正数、内容短于内容区）就是
    /// `scroll_to(锚点, 0)` —— 「最新」= 锚点顶 + 全部内容，钉顶位本来就是看着
    /// 最新；**跟尾期**（内容长过内容区）「最新」= 贴底胶水位 `scroll_to(count)`
    /// （下一帧归一化回 Bottom 跟随位，pi-web scrollToBottom + isNearBottom=true
    /// 同款）。
    ///
    /// 跟尾期**不能用锚点顶**：free_scroll 软状态下锚点仍激活、垫片条目以 0 高
    /// 挂着，若只看「垫片在不在」就 `scroll_to(锚点, 0)`，视口只跳到最后一条用户
    /// 消息顶——它早已退到屏底之上数屏之外，随后 at_bottom 硬置 true（按钮消失）、
    /// sync 第 6 步因 following 不再钉顶，内容就停在半空（用户报障：点按钮到不了底）。
    ///
    /// 反过来钉顶期**别用胶水**：垫片是一整屏内容区高，胶水会把末条底边钉在屏底、
    /// 把「内容末尾」推到屏顶（下方一片空白）——正是用户截到的那个错位状态。
    pub(crate) fn jump_to_bottom(&self) {
        let pin = self.core.listed.get().spacer && !self.core.following.get();
        let target = if pin {
            self.core.anchor.get().unwrap_or(self.state.item_count())
        } else {
            self.state.item_count()
        };
        self.state.scroll_to(ListOffset {
            item_ix: target,
            offset_in_item: px(0.),
        });
        self.core.at_bottom.set(true);
        self.core.free_scroll.set(false);
        // 程序化跳底交还位置推导（同物理滚动），钉住的选中段失效
        self.core.nav_pinned.set(None);
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
            self.core.free_scroll.set(false);
            self.core.settle_attempts.set(0);
        }
        // 先落账：条目进树后 scroll_to 才有得钉（越界会被钳到列表尾）
        self.sync(msgs, phase);
        if fresh {
            self.state.scroll_to(ListOffset {
                item_ix: anchor_ix,
                offset_in_item: px(0.),
            });
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
        } else if msgs > 0 && !self.core.free_scroll.get() {
            // 5. 外科重测尾部：最后一条消息 + phase + 垫片（流式增长/垫片缩放）。
            //    **自由滚动中整步跳过**：用户滚离尾部后，尾部条目在下一帧布局的
            //    绘制范围之外，打回 Unmeasured 不会被重测——summary 高度当场塌陷
            //    （生长中的整轮回复按 0 计），滚轮 scroll_max 钳死在塌缩值：下拉
            //    被钳回半空、位置乱跳（2026-10 用户报障「生成中无法下拉、乱跳」）。
            //    贴底时尾部每帧都会被布局重测（钉顶期在首屏内、跟尾期在胶水
            //    walk-up 里），跳过无损失；流式增长在条目重新可见那一帧自会补测。
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
        self.core.listed.set(Listed {
            msgs,
            phase,
            spacer: want_spacer,
        });
        // 6. 钉顶：内容仍短于内容区（!following）时逻辑位恒停在锚点顶。配合
        //    钉顶期「垫片 = 一整屏内容区高」这个常数，gpui「锚下填不满视口就
        //    强制贴底」的覆盖条件恒不成立 ⇒ 逻辑位不会被夺走，锚点永远画在
        //    padding.top（与测量精度、条目缓存新鲜度无关）。内容长过内容区
        //    （following）时不钉，交给胶水跟随尾部。用户自由滚动中（free_scroll）
        //    也不钉——流式 chunk 不得把读历史的视口硬拽回锚点；滚回贴底自动
        //    恢复钉顶（free_scroll 由滚动回调复位）。
        if want_spacer
            && !self.core.following.get()
            && !self.core.free_scroll.get()
            && let Some(ix) = self.core.anchor.get()
        {
            self.state.scroll_to(ListOffset {
                item_ix: ix,
                offset_in_item: px(0.),
            });
        }
    }
}
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
    /// 用户自由滚动中（滚轮离位）：暂停钉顶重钉与跟尾翻转拽底，锚点与垫片
    /// 保留；滚回贴底（几何上≡钉顶位）自动复位
    free_scroll: Cell<bool>,
    /// 垫片已按实测内容结算（之前是视口高过估，靠硬钉顶兜底）
    settled: Cell<bool>,
    /// 内容长过内容区：spacer 归零、交还贴底胶水跟随尾部
    following: Cell<bool>,
    /// agent 运行中（事件层 notify_list 每拍同步）：跟尾期禁止「跟尾 → 钉顶」
    /// 回退——轮中工具卡/子代理卡形状变化（toolResult 到达、状态切换、details
    /// 回写）会让锚下内容缩水，below 跌破内容区纯属运行期噪声，不是轮末整形；
    /// 此时翻回钉顶会把视口拽回上一轮用户消息（乱跳），翻转帧按旧逻辑位渲染
    /// 满屏垫片、内容顶出视口（屏幕全空一帧）。轮末整形发生在 AgentEnd 之后，
    /// 值已翻 false，照常放行
    agent_running: Cell<bool>,
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
