//! 运行时定位与磁盘对账：消息定位/导航摘要 + 尾窗基线同步与外部
//! 追加检测（023 外部改动）。

use gpui::Context;
use pi_link::protocol::{Block, Command};

use crate::session::messages::{Role, msgs_from_tail};
use crate::session::subagent_panel::project_turn_agents;
use pi_link::sessions::read_leaf_messages;

use super::*;

impl SessionRuntime {
    /// 只读观察窗装载（044-0）：后台全量 leaf 读子会话文件 → 回灌 messages。
    /// 观察窗无进程，GetMessages 无人接，磁盘链就是唯一真相。回灌后同步
    /// disk_file_len 基线 —— 此后外部追加由 3s tick（check_external_append）
    /// 自然跟随（观察窗 agent_running 恒 false，守卫恒开）。
    pub(crate) fn spawn_observer_snapshot(&mut self, cx: &mut Context<Self>) {
        let Some(f) = self.file.clone() else { return };
        let epoch = self.agent.epoch;
        let f_read = f.clone();
        cx.spawn(async move |this, cx| {
            let msgs = cx
                .background_executor()
                .spawn(async move { msgs_from_tail(read_leaf_messages(&f_read, LEAF_REPAIR_MAX)) })
                .await;
            let _ = this.update(cx, |r, cx| {
                if r.agent.epoch != epoch || r.file.as_deref() != Some(f.as_path()) || !r.observer {
                    return; // 重绑/换身份/已退出观察态：陈旧快照作废
                }
                r.messages = msgs;
                r.sync_disk_baseline();
                r.notify_list(cx);
                r.status = "observer: synced from disk".into();
            });
        })
        .detach();
    }

    /// 绑定对账：pi 报告过的 sessionFile（state 里）若已落盘就跟随绑定
    /// （FileBound → Chat 迁移 pool key + 摘伪标签换真身）。返回 false =
    /// 报过但确定没落盘（调用方据此撤伪标签）。state 还空着时发一条
    /// GetState 兜底（响应带 sessionFile 走 follow_session_file 同一守卫）。
    pub(super) fn try_bind_reported_file(&mut self, cx: &mut Context<Self>) -> bool {
        if self.file.is_some() {
            return true;
        }
        match self
            .state
            .as_ref()
            .and_then(|s| s.session_file.clone().map(PathBuf::from))
        {
            Some(f) => {
                if !f.is_file() {
                    return false;
                }
                self.follow_session_file(f, cx);
                true
            }
            None => {
                if let Some(session) = &self.agent.session {
                    let _ = session.send(&Command::GetState);
                }
                true
            }
        }
    }

    /// Remember the session file's current size (our own writer's progress):
    /// the external-append tick only re-reads when someone ELSE grew it.
    pub(crate) fn sync_disk_baseline(&mut self) {
        if let Some(f) = &self.file {
            if let Ok(meta) = std::fs::metadata(f) {
                self.disk_file_len = meta.len();
            }
        }
    }

    /// External-append watch (pi-web session-revision parity): another
    /// writer (pi-web on the same session file) appends behind our pi
    /// process's back, and pi's in-memory chain never notices. While idle,
    /// a size change triggers a disk leaf-chain re-read.
    ///
    /// 秒开深度优化（pi-flash-2o92）：全文重读是 O(会话大小) 的主线程冻结
    /// 源（read_leaf_messages 逐行 JSON 解析，20MB 上限下 ~300ms+），改
    /// 后台线程执行；回灌守卫 =（文件路径 + epoch + 空闲态），重绑/重启/
    /// 读的这段时间会话动了都作废（外部快照过期，下轮 tick 重新发现）。
    pub(crate) fn check_external_append(&mut self, cx: &mut Context<Self>) {
        if self.agent_running || !self.pending_echoes.is_empty() {
            return;
        }
        // 上一次重读还在飞：不重复调度（文件还在长就等它落地；落地后的
        // 新增量由下一轮 tick 发现）
        if self.leaf_watch_in_flight {
            return;
        }
        let Some(f) = self.file.clone() else { return };
        let Ok(meta) = std::fs::metadata(&f) else {
            return;
        };
        let len = meta.len();
        if len == self.disk_file_len || len == 0 {
            return;
        }
        // 长度先记账（防在飞期间每 3s tick 重复调度）；重读失败时长度已
        // 前移，靠外部写入的下一次增长重新触发（与同步版同语义）
        self.disk_file_len = len;
        self.leaf_watch_in_flight = true;
        let epoch = self.agent.epoch;
        let f_read = f.clone();
        cx.spawn(async move |this, cx| {
            let msgs = cx
                .background_executor()
                .spawn(async move { msgs_from_tail(read_leaf_messages(&f_read, LEAF_REPAIR_MAX)) })
                .await;
            let _ = this.update(cx, |r, cx| {
                r.leaf_watch_in_flight = false;
                if r.agent.epoch != epoch || r.file.as_deref() != Some(f.as_path()) {
                    return; // 重启/重绑在飞期间已换身份
                }
                if r.agent_running || !r.pending_echoes.is_empty() {
                    return; // 读的这段时间会话动了：外部快照过期，下轮再看
                }
                if msgs.len() != r.messages.len() {
                    // 整表重读：锚点索引可能失效，重读后按最后一条用户消息重锚
                    r.messages = msgs;
                    // fork anchors best-effort (same as the open-time repair path)
                    let mut ids = r.active_user_entry_ids.iter();
                    for m in r.messages.iter_mut() {
                        if m.role == Role::User {
                            m.entry_id = ids.next().cloned();
                        }
                    }
                    r.apply_pending_locate(cx);
                    r.restore_anchor_after_reload();
                    r.notify_list(cx);
                    r.status = status_line(true, "synced from disk");
                }
            });
        })
        .detach();
    }

    /// 磁盘计数后台化（秒开深度优化 pi-flash-2o92）：`count_message_entries`
    /// 在主线程 `fs::read` 整个会话文件（O(文件大小)，20MB ≈ 几十 ms 且
    /// 随文件线性放大）——open 路径改为调度后台任务算完回灌。
    ///
    /// 计数晚到的补偿：get_messages 快照到达时计数还是 0，修复守卫
    /// （`rpc < disk`）空转过一轮；计数落地后若快照比磁盘短，补一次
    /// 后台 leaf 修复对账（对抗失败无副作用——显示维持 RPC 权威投影）。
    pub(crate) fn load_disk_msg_count(&mut self, cx: &mut Context<Self>) {
        let Some(f) = self.file.clone() else { return };
        let epoch = self.agent.epoch;
        let f_count = f.clone();
        cx.spawn(async move |this, cx| {
            let n = cx
                .background_executor()
                .spawn(async move { pi_link::sessions::count_message_entries(&f_count) as usize })
                .await;
            let _ = this.update(cx, |r, cx| {
                if r.agent.epoch != epoch || r.file.as_deref() != Some(f.as_path()) {
                    return; // 重启/重绑在飞期间已换身份，旧文件的计数作废
                }
                r.disk_msg_count = n;
                if let Some(rpc_len) = r.last_rpc_msg_count {
                    if n > rpc_len && !r.agent_running {
                        r.spawn_leaf_repair(rpc_len, None, cx);
                    }
                }
            });
        })
        .detach();
    }

    /// 后台 leaf 修复（get_messages 快照短于磁盘计数时的磁盘侧重建）：
    /// 磁盘链必须长于快照才换入（与原同步版长度竞赛同规则）；`fallback`
    /// = 落败分支的 RPC 权威投影（deferred 调用方把投影延后到这里，
    /// None = 计数晚到补偿，投影早已上屏）。回灌守卫同
    /// check_external_append；读的这段时间有新快照到达
    /// （last_rpc_msg_count 变了）也作废，避免陈旧投影回退。
    pub(crate) fn spawn_leaf_repair(
        &mut self,
        rpc_len: usize,
        fallback: Option<Vec<serde_json::Value>>,
        cx: &mut Context<Self>,
    ) {
        let Some(f) = self.file.clone() else { return };
        let epoch = self.agent.epoch;
        let f_read = f.clone();
        cx.spawn(async move |this, cx| {
            let disk = cx
                .background_executor()
                .spawn(async move { msgs_from_tail(read_leaf_messages(&f_read, LEAF_REPAIR_MAX)) })
                .await;
            let _ = this.update(cx, |r, cx| {
                if r.agent.epoch != epoch || r.file.as_deref() != Some(f.as_path()) {
                    return;
                }
                if r.agent_running || r.last_rpc_msg_count != Some(rpc_len) {
                    return; // 会话动了 / 已有更新的快照：陈旧对账作废
                }
                if disk.len() > rpc_len {
                    r.messages = disk;
                    r.pending_echoes.clear();
                    // entry-id mapping stays the RPC/get_entries view
                    // (fork anchors best-effort on repaired chains)
                    let mut ids = r.active_user_entry_ids.iter();
                    for m in r.messages.iter_mut() {
                        if m.role == Role::User {
                            m.entry_id = ids.next().cloned();
                        }
                    }
                    r.apply_pending_locate(cx);
                    r.notify_list(cx);
                    r.status = "resumed (leaf chain repaired from disk)".into();
                } else if let Some(rpc) = fallback {
                    // 磁盘没赢 → RPC 权威投影（原同步版的落败分支，延后到
                    // 后台重读落地才执行——延后期间显示维持 tail 预渲染）
                    r.project_rpc_messages(rpc, cx);
                    r.status = status_line(true, "resumed");
                }
                // 计数晚到补偿（无 fallback）：显示维持已上屏的权威投影
            });
        })
        .detach();
    }

    /// Backfill message stamps from the session-file tail: snapshots never
    /// carry completion times, and entry write-times are the only end-stamp
    /// source for history (aligned by suffix — tail conversion mirrors the
    /// same ingest semantics).
    pub(super) fn merge_tail_stamps(&mut self) {
        let Some(f) = self.file.clone() else {
            return;
        };
        let entries = pi_link::sessions::read_tail_messages(&f, 256 * 1024, 400);
        if entries.is_empty() {
            return;
        }
        let tail = msgs_from_tail(entries);
        if tail.len() > self.messages.len() {
            return; // projection shorter than file (compaction) — skip unsafe align
        }
        let start = self.messages.len() - tail.len();
        for (i, tm) in tail.into_iter().enumerate() {
            let m = &mut self.messages[start + i];
            if m.role == tm.role {
                if m.ts.is_none() {
                    m.ts = tm.ts;
                }
                if m.end_ts.is_none() {
                    m.end_ts = tm.end_ts;
                }
            }
        }
    }

    /// Reveal a search hit (013): scroll the message list to the matched row —
    /// by payload timestamp, falling back to the first message containing the
    /// needle. Returns false when messages aren't loaded yet (caller parks a
    /// pending_locate for the reconcile to apply).
    pub fn locate_message(
        &mut self,
        ts: Option<i64>,
        needle: &str,
        _cx: &mut Context<Self>,
    ) -> bool {
        if self.messages.is_empty() {
            self.pending_locate = Some((ts, needle.to_string()));
            return false;
        }
        let ix = self.hit_index(ts, needle);
        if let Some(ix) = ix {
            self.pager.reveal(ix);
        }
        ix.is_some()
    }

    fn hit_index(&self, ts: Option<i64>, needle: &str) -> Option<usize> {
        if let Some(ts) = ts {
            if let Some(ix) = self.messages.iter().rposition(|m| m.ts == Some(ts)) {
                return Some(ix);
            }
        }
        let needle_lc = needle.to_lowercase();
        if needle_lc.is_empty() {
            return None;
        }
        self.messages
            .iter()
            .position(|m| m.plain_text().to_lowercase().contains(&needle_lc))
    }

    /// Apply a parked 013 jump once messages exist (reconcile path).
    pub(super) fn apply_pending_locate(&mut self, _cx: &mut Context<Self>) {
        if let Some((ts, needle)) = self.pending_locate.take() {
            if let Some(ix) = self.hit_index(ts, &needle) {
                self.pager.reveal(ix);
            }
        }
    }

    /// 导航数据（每轮摘要 + 总结栏统计，Rc 共享）。渲染层每帧调用：命中
    /// 缓存零成本，notify_list 置脏后下一帧重建一次。
    pub(crate) fn nav_summary(&mut self) -> std::rc::Rc<NavData> {
        if self.nav_dirty {
            self.nav_dirty = false;
            let built = self.build_nav_summary();
            self.nav_cache = Some(std::rc::Rc::new(built));
        }
        self.nav_cache.clone().unwrap_or_else(|| {
            std::rc::Rc::new(NavData {
                turns: Vec::new(),
                turn_agents: Vec::new(),
                users: 0,
                assistants: 0,
                tool_calls: 0,
            })
        })
    }

    fn build_nav_summary(&self) -> NavData {
        // 总结栏统计（自洽口径）：用户/助手按消息条数，工具调用按
        // assistant 消息内的 ToolCall 块数（结果是否合并到达不影响）；
        // custom（压缩摘要/extension 卡）不计
        let mut users = 0usize;
        let mut assistants = 0usize;
        let mut tool_calls = 0usize;
        for m in &self.messages {
            match m.role {
                Role::User => users += 1,
                Role::Assistant => {
                    assistants += 1;
                    tool_calls += m
                        .blocks
                        .iter()
                        .filter(|b| matches!(b, Block::ToolCall { .. }))
                        .count();
                }
                Role::Custom => {}
                // `!` shell 卡不是 assistant 消息，不计统计（034 自洽口径）
                Role::Bash => {}
            }
        }
        // turns = 用户消息锚点（ChatMinimap.tsx parity：每轮=用户消息 +
        // 其后的全部 assistant 回复）；摘要 033：用户前 50 字 + agent
        // 首条有正文回复首行前 50 字（定位器不是会话副本——agent 说
        // 多少话也只摘要一行）
        let turns: Vec<usize> = self
            .messages
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == Role::User)
            .map(|(i, _)| i)
            .collect();
        let turns = turns
            .iter()
            .enumerate()
            .map(|(ti, &ux)| {
                let user_prefix = self
                    .messages
                    .get(ux)
                    .map(|m| m.plain_text_prefix(50))
                    .unwrap_or_default();
                let end = turns.get(ti + 1).copied().unwrap_or(self.messages.len());
                let agent = (ux + 1..end).find_map(|i| {
                    let m = self.messages.get(i)?;
                    if m.role != Role::Assistant {
                        return None;
                    }
                    let head = m.plain_text_prefix(80);
                    let line: String = head
                        .trim_start()
                        .lines()
                        .next()
                        .unwrap_or("")
                        .chars()
                        .take(50)
                        .collect();
                    (!line.is_empty()).then_some((i, line))
                });
                TurnSummary {
                    user_ix: ux,
                    user_prefix,
                    agent,
                }
            })
            .collect();
        NavData {
            turn_agents: project_turn_agents(&self.messages),
            turns,
            users,
            assistants,
            tool_calls,
        }
    }
}
