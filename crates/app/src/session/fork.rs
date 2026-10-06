//! Fork（消息下的「新分支」）的全部实现，从 `runtime.rs` 拆出以守住单文件
//! 行数上限（ARCHITECTURE.md §6）。
//!
//! pi-web parity 三段：
//! 1. `handleFork`（hooks/useAgentSession.ts）：点「新分支」→ rpc
//!    `fork(entryId)`；in-flight 期间按钮显示「创建中…」并禁用
//! 2. pi 侧 `runtimeHost.fork`（modes/rpc/rpc-mode.js）：position=before，
//!    target = 该 user entry 的 parentId，`createBranchedSession` 复制出
//!    一份独立副本，然后**同一个进程** rebind 到新分支文件
//! 3. `onSessionForked(newSessionId)`：前端切到那个新会话——pi-flash 的等价
//!    动作是跟随新 sessionFile 迁移会话身份（follow_session_file）

use std::collections::HashMap;
use std::path::PathBuf;

use gpui::Context;
use pi_link::protocol::{Command, TreeNode};

use super::messages::Role;
use super::runtime::{SessionEvent, SessionRuntime};

/// 按钮/调用方看到的「新分支」目标。
///
/// 语义（用户定案）：按钮挂在 **agent 回复** 的操作栏上，分支保留到本轮
/// 回复为止——本轮之前（含本轮）全部带过去，其后的内容丢弃。用户在下一轮
/// 里想换个说法重来，clone 出来的会话正好停在这条回复后面。
pub(crate) struct ForkAnchor {
    /// Some(entry id) = 分支点 =「下一条用户消息之前」；pi 的 rpc `fork`
    /// 只支持 before，且目标必须是用户消息
    pub next_user: Option<String>,
    /// 本轮就是会话尾部（没有下一条用户消息）→ 只能整段复制：rpc `clone`
    /// （= fork at leaf）
    pub tail: bool,
    pub forking: bool,
}

impl ForkAnchor {
    /// 锚点齐了才算可以点：尾部轮走 clone，其它轮必须拿到下一条用户消息
    /// 的 entry id（拿不到就说明锚点还没回来，宁可不点也不能分错地方）
    pub fn clickable(&self) -> bool {
        !self.forking && (self.next_user.is_some() || self.tail)
    }
}

impl SessionRuntime {
    /// 重取 fork 锚点（用户消息的 entry id）。
    ///
    /// `get_entries` 是权威来源：扁平、不嵌套，多长的会话都拿得到。`get_tree`
    /// 只为 033 导航面板而要——pi 在 ~1200 条消息的会话上会自己 stack overflow
    /// 并回 `success:false`，那种会话下没有树，但「新分支」照常可用。
    pub(crate) fn refresh_anchors(&self) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::GetEntries);
            let _ = session.send(&Command::GetTree);
        }
    }

    /// 把 active chain 上的 user entry id 按顺序回填到用户消息上。
    /// 位置对齐的前提：消息列表里的 user 消息与链上的 user entry 一一对应
    /// （回显去重、技能信封折叠都在渲染层做；custom 条目映射成 Role::Custom
    /// 而不是 User，见 sessions.rs 的说明）。
    pub(crate) fn apply_entry_ids(&mut self) {
        let mut ids = self.active_user_entry_ids.iter();
        for m in self.messages.iter_mut() {
            if m.role == Role::User {
                m.entry_id = ids.next().cloned();
            }
        }
    }

    /// 「新分支」入口：从 agent 轮操作栏调用。
    pub(crate) fn fork_from_turn(&mut self, next_user: Option<String>, cx: &mut Context<Self>) {
        match next_user {
            // 分支点 = 下一条用户消息之前 → 保留到本轮 agent 回复为止
            Some(entry_id) => self.begin_fork(Command::Fork { entry_id }, cx),
            // 尾部轮：整段复制
            None => self.begin_fork(Command::Clone, cx),
        }
    }

    /// Fork before an arbitrary user-message entry（rpc `fork` 的原始形态）。
    ///
    /// 当前只有「新分支」按钮走 `fork_from_turn`；这个入口留给 033 导航面板
    /// 的按节点分支（面板可点任意节点 → 需要任意 entry id）。
    #[allow(dead_code)]
    pub(crate) fn fork_from_entry(&mut self, entry_id: String, cx: &mut Context<Self>) {
        self.begin_fork(Command::Fork { entry_id }, cx);
    }

    /// 前置条件 / 状态位 / 命名素材的统一入口（fork 与 clone 共用）。
    fn begin_fork(&mut self, cmd: Command, cx: &mut Context<Self>) {
        if self.forking {
            return;
        }
        if self.agent_running || self.state.as_ref().is_some_and(|s| s.is_streaming) {
            self.status = "cannot fork while running".into();
            cx.notify();
            return;
        }
        // pi-web handleFork 的前置条件：进程必须在（未落盘的 draft fork 会
        // 由 pi 报 "This session has not been saved yet"）
        let Some(session) = &self.agent.session else {
            self.status = "cannot fork before the session is saved".into();
            cx.notify();
            return;
        };
        // 分支要改名（原名截取 20 字）：原 title 必须在 fork **之前**
        // 取，fork 落地后 state 讲的已经是新会话了
        self.fork_source_title = Some(self.branch_source_title());
        // 「创建中…」直到响应落地（同时防连点：第二个 fork 会被 pi 拒）
        self.forking = true;
        self.status = "forking".into();
        let _ = session.send(&cmd);
        cx.notify();
    }

    /// 原会话标题：pi 的 session_name 优先，缺省取首条用户消息前 50 字
    /// （与「重命名」弹窗的预填口径一致）。
    fn branch_source_title(&self) -> String {
        self.state
            .as_ref()
            .and_then(|s| s.session_name.clone())
            .filter(|n| !n.trim().is_empty())
            .or_else(|| {
                self.messages
                    .iter()
                    .find(|m| m.role == Role::User)
                    .map(|m| m.plain_text())
            })
            .map(|v| v.trim().chars().take(50).collect::<String>())
            .unwrap_or_default()
    }

    /// pi 换绑了会话文件（draft 首条 prompt 落盘、fork/clone 换分支）时的
    /// 跟随动作。`self.file` 是 shell 的会话身份来源（pool key、侧栏高亮、
    /// 重开目标、recents）——落后一步就是「分支已生效、界面仍挂在父会话」。
    /// 磁盘侧探针也要同步重取：分支文件的内容已经不是父会话的了。
    pub(crate) fn follow_session_file(&mut self, f: PathBuf, cx: &mut Context<Self>) {
        if self.file.as_deref() == Some(f.as_path()) {
            return;
        }
        self.file = Some(f.clone());
        self.disk_file_len = std::fs::metadata(&f).map(|m| m.len()).unwrap_or(0);
        self.disk_msg_count = pi_link::sessions::count_message_entries(&f) as usize;
        // Chat 迁移 pool key（draft-N / 父路径 → 新分支路径）、刷新侧栏、
        // 记 recents 与 last-open
        cx.emit(SessionEvent::FileBound(f));
        cx.notify();
    }

    /// `fork` 响应落地（pi-web useAgentSession.handleFork 的落地端）。
    ///
    /// pi 把同一个进程重绑到新分支会话文件上（runtimeHost.fork →
    /// createBranchedSession + rebind），并把「新身份」放在紧随其后的
    /// get_state.sessionFile 里带回来 → follow_session_file → FileBound →
    /// Chat 迁移 pool key / 侧栏高亮 / recents。因此这里只负责把展示面清空
    /// 成「一条刚开出来的新分支会话」。
    pub(crate) fn on_fork_response(
        &mut self,
        success: bool,
        error: Option<&str>,
        cx: &mut Context<Self>,
    ) {
        // in-flight 按钮（「创建中…」）到此为止
        self.forking = false;
        // 命名素材在这里就消费掉：失败分支也要清，免得残留到下一次
        let source_title = self.fork_source_title.take();
        if !success {
            self.status = format!("fork failed: {}", error.unwrap_or_default());
            cx.notify();
            return;
        }
        self.branch_tree = None;
        self.active_user_entry_ids.clear();
        // 换了条会话（分支）：锚点必须退役——留着就会把新分支里「最后一条用户
        // 消息」重新钉顶（restore_anchor_after_reload 的兜底），而分支该从
        // 尾部开始看
        self.pager.release();
        self.messages.clear();
        self.pending_echo = None;
        self.phase_waiting = false;
        self.streaming_content = false;
        self.agent_running = false;
        // branched file 的 RPC 快照即权威（fork 点之前的路径已被 pi 复制进新文件）
        self.disk_msg_count = 0;
        self.notify_list(cx);
        if let Some(s) = self.agent.session.as_ref() {
            let _ = s.send(&Command::GetState);
            let _ = s.send(&Command::GetSessionStats);
            let _ = s.send(&Command::GetMessages);
        }
        self.refresh_anchors();
        // 新分支自动改名：原名截取 20 字（用户定案）。set_session_name 写在
        // pi 已 rebind 的新文件上，响应回来后再刷一次侧栏（沿用既有 500ms
        // 延迟刷新路径）。
        let name = source_title.as_deref().map(branch_name).unwrap_or_default();
        if !name.is_empty() {
            if let Some(s) = self.agent.session.as_ref() {
                let _ = s.send(&Command::SetSessionName { name: name.clone() });
            }
            self.status = format!("forked: {name}");
        } else {
            // 原名取不到（空会话）：交给 pi 的默认命名，不发明名字
            self.status = "forked".into();
        }
        // branch_tree / fork 锚点由上面的 get_entries / get_tree 重建
        cx.notify();
    }

}

/// User-message entry ids along the root→leaf path (fork anchors for the
/// per-message fork button). Ordering matches the projected user messages.
pub(crate) fn collect_path_user_ids(nodes: &[TreeNode], leaf_id: Option<&str>) -> Vec<String> {
    let Some(target) = leaf_id else {
        return Vec::new();
    };
    fn flatten<'a>(nodes: &'a [TreeNode], map: &mut HashMap<String, &'a TreeNode>) {
        for n in nodes {
            map.insert(n.id.clone(), n);
            flatten(&n.children, map);
        }
    }
    let mut map = HashMap::new();
    flatten(nodes, &mut map);
    let mut chain: Vec<TreeNode> = Vec::new();
    let mut cur = map.get(target);
    while let Some(n) = cur {
        chain.push((*n).clone());
        cur = n.parent_id.as_deref().and_then(|pid| map.get(pid));
    }
    chain.reverse();
    chain
        .into_iter()
        .filter(|n| n.role.as_deref() == Some("user"))
        .map(|n| n.id)
        .collect()
}

/// 分支会话名 = 原名截取 20 个字（超长补 "…"）。
///
/// 历史：最初按「前 15 字 + "2"」命名，用户连续 clone 两次后实测后缀会叠成
/// `…22`，判定很蠢 —— 改为只截断，不加任何后缀（同名分支由侧栏位置/时间区分）。
pub(crate) const BRANCH_NAME_CHARS: usize = 20;

pub(crate) fn branch_name(base: &str) -> String {
    let trimmed = base.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let mut out: String = trimmed.chars().take(BRANCH_NAME_CHARS).collect();
    if trimmed.chars().count() > BRANCH_NAME_CHARS {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{branch_name, BRANCH_NAME_CHARS};

    #[test]
    fn branch_name_truncates_to_20_chars() {
        // 短名原样
        assert_eq!(branch_name("test"), "test");
        assert_eq!(branch_name("  修一下登录 bug  "), "修一下登录 bug");
        // 正好 20 字：不截断、不加省略号
        let exactly20: String = "12345678901234567890".to_string();
        assert_eq!(branch_name(&exactly20), exactly20);
        // 超 20 字：截断 + "…"
        assert_eq!(branch_name("123456789012345678901"), "12345678901234567890…");
        // 空标题：不发明名字（调用方跳过 set_session_name）
        assert_eq!(branch_name("   "), "");
        assert_eq!(BRANCH_NAME_CHARS, 20);
    }

    /// 按「字」截断而不是按字节：中文标题不能切出半个字符
    #[test]
    fn branch_name_counts_chars_not_bytes() {
        let name = branch_name("观宏知微的文章标题很长很长的说法还有更多字");
        assert_eq!(name.chars().count(), BRANCH_NAME_CHARS + 1); // 20 字 + …
        assert!(name.ends_with('…'));
    }

    /// 连续 clone 不再叠后缀（旧规则的 `…22` 事故）
    #[test]
    fn repeated_clone_keeps_the_same_name() {
        let long = "观宏知微的文章标题很长很长的说法还有更多字";
        let once = branch_name(long);
        assert_eq!(branch_name(&once), once);
    }
}
