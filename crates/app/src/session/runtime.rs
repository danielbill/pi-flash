//! SessionRuntime (030 会话池): ONE resident agent runtime per session —
//! pi-web `__piSessions` parity. Holds the pi process (via AgentSession),
//! the full message log, live session state, and the session's own
//! inputPanel state (031: draft text, attachments, history, thinking
//! override, tools preset, model). Created on first open, destroyed only on
//! explicit delete; switching sessions NEVER touches processes — Chat just
//! moves its attention pointer.
//!
//! Events bubble to the shell via SessionEvent; Chat does UI-side effects
//! (sidebar refresh, git refresh, ext dialogs) for the active session only —
//! except the turn-end notification sound, which is this runtime's own job:
//! only it knows when its turn ended (see `settle_turn`).

use std::path::PathBuf;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use gpui::{Context, Entity, EventEmitter, Render, prelude::*};
use pi_link::protocol::{
    AssistantEvent, Block, Command, Event, SessionState, SessionStats, SlashCommand, Usage,
    TreeNode, content_blocks, parse_tree,
};

use crate::agent_session::AgentSession;
use crate::session::chat_list::ChatList;
use crate::session::messages::{BashInfo, Msg, Role, UsageLine, merge_tool_result, msgs_from_tail, result_payload};
use pi_link::sessions::read_leaf_messages;

/// no cap for the disk-side leaf-chain rebuild: the repair must be longer
/// than the truncated RPC snapshot to win the length contest, and the RPC
/// path already renders the full (broken) chain — same render scale
const LEAF_REPAIR_MAX: usize = usize::MAX;
use crate::i18n::tr;
use crate::services::format::status_line;

/// Shell-level effects a runtime bubbles up (Chat subscribes).
pub(crate) enum SessionEvent {
    /// repaint-worthy state change
    Changed,
    /// sidebar list should refresh (rename flush, fork, file landed)
    ListDirty,
    /// extension UI request (active session only surfaces the dialog)
    ExtUi(pi_link::protocol::ExtensionUiRequest),
    /// assistant 流式输出（060 微信回推：文本累积 / tool_call 处 force flush）
    Assistant(pi_link::protocol::AssistantEvent),
    /// get_state arrived with a pending rename prefill
    RenameReady(String),
    /// get_available_models arrived: the model catalog is project-level
    /// shared state (pi-web /api/models + loadModelsWithCache parity) — the
    /// runtime only forwards it, Chat stores it per cwd for every picker
    Models(Vec<pi_link::protocol::ModelInfo>),
    /// get_commands arrived（含扩展命令；磁盘上没有包内注册命令的数据文件）：
    /// Chat 并进全局命令清单并回写自有缓存（010-启动.md §3/§4.1）
    Commands(Vec<pi_link::protocol::SlashCommand>),
    /// a draft got persisted: pi bound this process to a fresh session file
    /// (pi-web promoteNewSession parity — the pool key migrates draft-N → path)
    FileBound(PathBuf),
}

/// 033 会话导航面板的一轮摘要（runtime 缓存条目）：用户消息索引 + 前
/// 50 字，agent 首条有正文回复的 (消息索引, 首行前 50 字)。
pub(crate) struct TurnSummary {
    pub user_ix: usize,
    pub user_prefix: String,
    pub agent: Option<(usize, String)>,
}

/// 033 导航面板缓存整包：轮次摘要 + 总结栏统计。统计是「按我们自己设
/// 计」的自洽口径 N = 用户 + 助手 + 工具调用，非 pi-web totalMessages
/// （那条把 toolResult/system 条目也计入——本工程 toolResult 合并进
/// ToolCall 块、system 入库即跳过，两边数字本来就对不上）
pub(crate) struct NavData {
    pub turns: Vec<TurnSummary>,
    pub users: usize,
    pub assistants: usize,
    pub tool_calls: usize,
}

pub(crate) struct SessionRuntime {
    /// stable pool key: session file path, or "draft-N" before first prompt
    pub key: String,
    pub cwd: PathBuf,
    /// None until pi persists the draft (first prompt)
    pub file: Option<PathBuf>,
    /// disk-side message count at open time — get_messages shorter than
    /// this means pi's restored leaf chain is truncated (mis-parented
    /// non-message entry) and the display rebuilds from the file instead
    pub disk_msg_count: usize,
    /// session file size at last read — an external writer (pi-web on the
    /// same session) grows the file behind our pi process's back, which
    /// never notices; the tick compares and re-reads from disk
    pub disk_file_len: u64,
    /// pi process ownership (None while recycled/lazy)
    pub agent: AgentSession,

    // ---- message log ----
    pub messages: Vec<Msg>,
    /// 滚屏状态机（chat_list.rs）：gpui 列表 + 翻页锚点 + 垫片 + splice 记账
    pub pager: ChatList,
    /// explicit open/close overrides; absent = per-state default (thinking
    /// blocks open, process group closed when the turn has a final answer)
    pub collapsed: std::collections::HashMap<(usize, usize), bool>,
    pub phase_waiting: bool,
    /// text/thinking/toolcall deltas in flight (pi-web hasStreamingContent
    /// parity): the running/waiting phase row yields while content streams
    pub streaming_content: bool,
    /// 乐观发送的用户文本：只用于把 pi 回显的同文 user 消息就地升级（去重）。
    /// 与 phase_waiting 解耦 —— 回显到达 ≠ agent 已应答，等待行不能被它掐掉。
    pub pending_echo: Option<String>,
    pub stream_started: Option<std::time::Instant>,
    pub agent_running: bool,
    pub status: String,
    /// 「其他」页提示音开关的运行时副本（app_settings.json `sound` 是真值
    /// 来源）：轮末提示音是 runtime 的职责——只有它知道自己这一轮何时结
    /// 束，Chat 拿不到这个时刻。开关变更时由 Chat 广播给全池。
    pub sound_on: bool,

    // ---- live session state ----
    pub state: Option<SessionState>,
    pub stats: Option<SessionStats>,
    pub branch_tree: Option<(Vec<TreeNode>, Option<String>)>,
    pub active_user_entry_ids: Vec<String>,
    pub pending_rename: bool,
    /// 手动压缩进行中（圆环弹窗按钮防连点；compact 响应清除）
    pub compacting: bool,
    /// fork 在飞：agent 回复下的「新分支」显示「创建中…」并禁用（pi-web
    /// forking parity），同时防连点发出第二个 fork
    pub forking: bool,
    /// fork 之前捕获的「原会话标题」（state.session_name，缺省取首条用户
    /// 消息前 50 字）。分支落地后用它命名新会话：原名截取 20 字。
    pub fork_source_title: Option<String>,
    pub commands: Vec<SlashCommand>,
    /// 系统提示词 + 已加载工具声明 + 分类明细（top panel / token 分析块）。
    /// 数据来自 transcript 的 `role:"system"` 消息（pi 0.86+），由
    /// pi_link::transcript::transcript_system replay 得出 —— pi-web 的
    /// System/Tools 面板同源。`None` = 尚未加载（无进程或消息还没回来）。
    pub sys: Option<pi_link::transcript::TranscriptSystem>,
    /// transcript 里 `role:"system"` 消息原文。live `message_start`（pi 每个
    /// run 都先追加一条，实测 RPC 全量携带 content/sections/toolsAdded）逐条
    /// 追加；get_messages 快照到达时整表替换。`sys` 由它重放得出 —— 新会话
    /// 第一轮即可点亮面板，不必等下一次 get_messages。
    system_msgs: Vec<serde_json::Value>,

    // ---- inputPanel (031) per-session state ----
    pub input: String,
    pub pending_images: Vec<crate::AttachedImage>,
    pub history: Vec<String>,
    pub history_ix: Option<usize>,
    pub thinking_override: Option<String>,
    /// model picked while the draft had no process yet (pi-web
    /// newSessionModelOverrideRef parity) — applied as a `--model` spawn flag
    pub pending_model: Option<(String, String)>,
    /// 042：PF 默认模型（app-settings 五角星；Chat 在默认变化/建会话时
    /// 填充）。仅**全新 draft** spawn 时落 `--model`（`file` 为空）——重绑
    /// / 恢复的会话不强行切模型。用户手选（pending_model）优先于默认。
    pub default_model: Option<(String, String)>,
    /// tools preset id (configured/chat-only/read-only/default/full) —
    /// applied at spawn via CLI flags (RPC has no live tool switching)
    pub tools_preset: String,
    /// 「full+」档的会话扩展集（`entry_source()` 字符串 = `-e` 参数）。
    /// 只在本档 spawn 时使用；切别的档不清空，切回来还是上次的选择。
    pub ext_sources: Vec<String>,
    /// 040：picker 在运行中确认的清单——下一轮 send_input 空闲时重绑生效
    pub(crate) pending_ext_sources: Option<Vec<String>>,
    /// 043：MCP 菜单在运行中改过 mcp.json——下一轮 send_input 空闲时重绑
    /// 生效（新进程 session_start 重读清单；空闲 spawn 本就直接读盘，无需标记）
    pub(crate) pending_mcp_reload: bool,

    // ---- `!` shell 命令（031）----
    /// rpc bash 在飞（pi isBashRunning 的客户端镜像；期间再发 bash 会被
    /// pi 拒绝，这里提前拦）
    pub bash_running: bool,
    /// bash response 关联的乐观消息下标（执行中增量就地追加；快照重建后
    /// 失效 → finalize 时校验角色，失配就补一张完成卡）
    pending_bash: Option<usize>,

    /// last user-visible activity (idle recycle)
    pub last_activity: std::time::Instant,
    /// queued ext requests while not active (G+ surfaces a badge)
    pub ext_queue: Vec<pi_link::protocol::ExtensionUiRequest>,

    /// 013: jump target waiting for the message snapshot to land
    pub pending_locate: Option<(Option<i64>, String)>,

    /// 033 导航数据缓存（摘要 + 总结栏统计）：渲染层每帧读、消息变化才
    /// 重算（notify_list 置脏，nav_summary() 惰性重建）——语义上「一轮
    /// 一次」，不随渲染帧反复拼前缀/数块
    nav_cache: Option<std::rc::Rc<NavData>>,
    nav_dirty: bool,
}

impl SessionRuntime {
    pub(crate) fn new(key: String, cwd: PathBuf, file: Option<PathBuf>) -> Self {
        // 会话级扩展清单：**绑定会话的独立数组**（用户定稿）。全局 settings
        // 启用集只在「新会话创建」时**复制**为初始清单（之后两者互不跟随，
        // 设置页改动不影响已存在的会话）；台账里存的是该会话自选的清单，
        // 重启/复接原样恢复。菜单勾选态只对齐这一份数组。
        let mut ext_sources = pi_link::session_ext::store_path()
            .map(|p| pi_link::session_ext::read_for(&p, &key))
            .unwrap_or_default();
        if ext_sources.is_empty() {
            ext_sources = pi_link::skills::enabled_package_sources();
        }
        Self {
            key,
            cwd,
            file: file.clone(),
            disk_msg_count: 0,
            disk_file_len: file
                .as_deref()
                .and_then(|f| std::fs::metadata(f).ok())
                .map(|m| m.len())
                .unwrap_or(0),
            agent: AgentSession::new(1),
            messages: Vec::new(),
            pager: ChatList::new(),
            collapsed: std::collections::HashMap::new(),
            phase_waiting: false,
            streaming_content: false,
            pending_echo: None,
            stream_started: None,
            agent_running: false,
            status: String::new(),
            sound_on: crate::services::workspace::load_sound_pref(),
            state: None,
            stats: None,
            branch_tree: None,
            active_user_entry_ids: Vec::new(),
            pending_rename: false,
            compacting: false,
            forking: false,
            fork_source_title: None,
            commands: Vec::new(),
            sys: None,
            system_msgs: Vec::new(),
            input: String::new(),
            pending_images: Vec::new(),
            history: Vec::new(),
            history_ix: None,
            thinking_override: None,
            pending_model: None,
            default_model: None,
            // 默认档 = full+（full 内置 + 会话扩展清单）；旧 pi-web 的
            // "configured"（什么都不发、让 pi 自己算）已按 034 决定移除
            tools_preset: "custom".into(),
            ext_sources,
            pending_ext_sources: None,
            pending_mcp_reload: false,
            bash_running: false,
            pending_bash: None,
            last_activity: std::time::Instant::now(),
            ext_queue: Vec::new(),

            pending_locate: None,

            nav_cache: None,
            nav_dirty: true,
        }
    }

    /// Spawn (or respawn) the pi process for this session. `background`
    /// callers wrap this on the background executor; returns the event
    /// receiver for the pump.
    pub(crate) fn spawn(&mut self) -> Option<UnboundedReceiver<Event>> {
        let mut extra: Vec<String> = Vec::new();
        if let Some(f) = &self.file {
            extra.push("--session".into());
            extra.push(f.to_string_lossy().into());
        }
        match self.tools_preset.as_str() {
            // spawn-arg tool selection (pi-web uses in-process
            // setActiveToolsByName; the RPC surface has no live switch).
            // "configured" sends nothing — pi resolves settings.json
            // defaultTools exactly like the CLI does.
            "chat-only" => extra.push("--no-tools".into()),
            "read-only" => {
                extra.push("--tools".into());
                extra.push("read,grep,find,ls".into());
            }
            "default" => {
                extra.push("--tools".into());
                extra.push("read,bash,edit,write".into());
            }
            // full：内置 7 件（--tools）+ 精确集（-ne + 个人扩展 + 内置扩展）
            // ⇒ 插件一个不装 ⇒ 系统提示词零插件注入（034 §1.2）
            "full" => {
                extra.push("--tools".into());
                extra.push("bash,read,edit,write,grep,find,ls".into());
                extra.extend(crate::session::tools_recipe::full_args(&self.cwd));
            }
            // 自定义（原 full+plugins）：不发 --tools（注册级 allowlist 会把
            // 插件工具挡在注册之外），走 -ne + 逐条 -e + full_activate.ts
            "custom" => {
                extra.extend(crate::session::tools_recipe::full_plugin_args(
                    &self.ext_sources,
                    &self.cwd,
                ));
            }
            _ => {}
        }
        // pi 原则：不逐次审批工具调用（vendor security.md），任何档位都
        // 不挂 permission 门禁；扩展自己的 ctx.ui.* 请求仍照常呈现（060 翻案）。
        // draft picks made before the process existed ride the spawn flags
        // (pi CLI parity: --model provider/id, --thinking level)
        // 042：无手选时落 PF 默认模型——只在全新 draft（`file` 空）生效，
        // 重绑/恢复的会话不强行切模型
        let model = match self.pending_model.take() {
            Some(m) => Some(m),
            None if self.file.is_none() => self.default_model.clone(),
            None => None,
        };
        if let Some((provider, id)) = model {
            extra.push("--model".into());
            extra.push(format!("{provider}/{id}"));
        }
        if let Some(level) = self.thinking_override.clone() {
            extra.push("--thinking".into());
            extra.push(level);
        }
        self.agent.spawn_with(&self.cwd, &extra)
    }

    /// Kill the process but keep everything else (idle recycle / soft drop).
    pub(crate) fn shutdown_process(&mut self) {
        self.agent.session = None;
        // 进程即刻不在：跑动标志就地复位。pump 退出事件可能永不到达（渠道
        // 已死 / 事件泵先被回收），留着 true 会让状态槽的旋转圈常亮。
        self.agent_running = false;
        self.phase_waiting = false;
        self.stream_started = None;
        if let Some(s) = self.state.as_mut() {
            s.is_streaming = false;
        }
    }

    pub(crate) fn model_label_text(&self) -> String {
        self.state
            .as_ref()
            .and_then(|s| s.model_label())
            .unwrap_or_else(|| "pi".to_string())
    }

    pub(crate) fn touch(&mut self) {
        self.last_activity = std::time::Instant::now();
    }

    pub(crate) fn refresh_state(&self) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::GetState);
            let _ = session.send(&Command::GetSessionStats);
            let _ = session.send(&Command::GetCommands);
            let _ = session.send(&Command::GetAvailableModels);
        }
    }

    /// Attach the event pump for a freshly spawned receiver. Callers pass
    /// the epoch they just created — reading the entity here would be a
    /// re-entrant borrow (we are typically inside `rt.update`).
    pub(crate) fn attach_pump(
        _this: &Entity<Self>,
        rx: UnboundedReceiver<Event>,
        epoch: u64,
        cx: &mut Context<Self>,
    ) {
        cx.spawn(async move |weak, cx| {
            consume_runtime_events(weak, cx, rx, epoch).await;
        })
        .detach();
    }
}

impl EventEmitter<SessionEvent> for SessionRuntime {}

// placeholder Render so the entity can exist before the view wires up
impl Render for SessionRuntime {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::div()
    }
}

/// Runtime-local event pump: routes THIS session's events into THIS runtime
/// (stale-guarded by epoch). Chat is not involved — running sessions keep
/// accumulating while parked.
async fn consume_runtime_events(
    this: gpui::WeakEntity<SessionRuntime>,
    cx: &mut gpui::AsyncApp,
    mut rx: UnboundedReceiver<Event>,
    epoch: u64,
) {
    while let Some(event) = rx.next().await {
        let stale = this
            .update(cx, |rt, cx| {
                if rt.agent.epoch != epoch {
                    return true;
                }
                rt.touch();
                rt.on_event(event, cx);
                false
            })
            .unwrap_or(true);
        if stale {
            return;
        }
    }
    let _ = this.update(cx, |rt, cx| {
        if rt.agent.epoch == epoch {
            rt.status = "pi exited".into();
            rt.compacting = false;
            // 进程没了就不存在「在跑」：不就地复位，agent_running 与快照
            // is_streaming 会永久卡在 true——psp 状态槽的旋转圈永不灭
            //（用户实测：旧版还在跑的会话被掐掉后一直转）
            rt.agent_running = false;
            rt.phase_waiting = false;
            rt.stream_started = None;
            if let Some(s) = rt.state.as_mut() {
                s.is_streaming = false;
            }
            cx.emit(SessionEvent::Changed);
            cx.notify();
        }
    });
}

// ---------------------------------------------------------------------------
// migrated from Chat (main.rs) — session-local behavior
// ---------------------------------------------------------------------------

impl SessionRuntime {
    /// 轮末提示音（「其他」页开关）。`stream_started` 兼作「本轮在飞」的闩：
    /// AgentSettled 与 AgentEnd 是同一轮的两个终止事件、都会到达，所以在这里
    /// take 掉它就恰好响一次；abort_stream() 会清掉闩，用户中止的一轮静音。
    fn settle_turn(&mut self) {
        if self.stream_started.take().is_some() && self.sound_on {
            crate::services::workspace::play_notify_sound();
        }
    }

    /// 重放 [`Self::system_msgs`] 刷新 `sys`（transcript 无 system 消息时保留
    /// 旧值，维持「尚未加载」语义）；变了就发 Changed，让开着的信息面板当场
    /// 点亮。
    fn replay_system(&mut self, cx: &mut Context<Self>) {
        if let Some(sys) = pi_link::transcript::transcript_system(&self.system_msgs) {
            let changed = match &self.sys {
                Some(prev) => prev.prompt != sys.prompt || prev.tools != sys.tools,
                None => true,
            };
            self.sys = Some(sys);
            if changed {
                cx.emit(SessionEvent::Changed);
            }
        }
    }

    fn on_event(&mut self, event: Event, cx: &mut Context<Self>) {
        // MessageStart(user) 置位翻页锚点（见函数尾 page_turn）
        let mut user_arrived = false;
        match event {
            Event::Response { command, success, error, data, .. } => {
                if command == "get_state" && success {
                    if let Some(data) = &data {
                        let st = SessionState::parse(data);
                        // pi 换了绑定的会话文件（draft 首条 prompt 落盘 /
                        // fork / clone 换分支）→ shell 身份必须跟着换
                        let rebound = st.session_file.clone().map(PathBuf::from);
                        self.state = Some(st);
                        if let Some(f) = rebound {
                            self.follow_session_file(f, cx);
                        }
                    }
                    if self.pending_rename {
                        self.pending_rename = false;
                        let prefill = self
                            .state
                            .as_ref()
                            .and_then(|s| s.session_name.clone())
                            .or_else(|| {
                                self.messages
                                    .iter()
                                    .find(|m| matches!(m.role, Role::User))
                                    .map(|m| m.plain_text())
                            })
                            .map(|v| v.chars().take(50).collect::<String>())
                            .unwrap_or_default();
                        cx.emit(SessionEvent::RenameReady(prefill));
                    }
                } else if command == "get_session_stats" && success {
                    if let Some(data) = &data {
                        self.stats = Some(SessionStats::parse(data));
                        if let Some(f) = data["sessionFile"].as_str().map(PathBuf::from) {
                            // same follow-the-rebind path as get_state: a draft
                            // persisted by its first prompt, or a fork/clone
                            // that moved pi onto the branched file
                            self.follow_session_file(f, cx);
                        }
                    }
                } else if command == "set_session_name" && success {
                    // pi flushed the name to the session file — reload the
                    // sidebar labels (pi-web onRenamed → loadSessions), with
                    // one delayed pass to cover flush lag
                    cx.emit(SessionEvent::ListDirty);
                    cx.notify();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(std::time::Duration::from_millis(500))
                            .await;
                        let _ = this.update(cx, |_c, cx| {
                            cx.emit(SessionEvent::ListDirty);
                            cx.notify();
                        });
                    })
                    .detach();
                } else if command == "get_commands" && success {
                    if let Some(data) = &data {
                        let commands = SlashCommand::parse_list(data);
                        // 转发给 Chat：并进全局清单 + 回写自有缓存（010-启动.md §3）
                        cx.emit(SessionEvent::Commands(commands.clone()));
                        self.commands = commands;
                    }
                } else if command == "get_available_models" && success {
                    if let Some(data) = &data {
                        // project-level shared catalog: forward, don't store
                        cx.emit(SessionEvent::Models(pi_link::protocol::parse_model_list(data)));
                    }
                } else if command == "set_model" && success {
                    // pi 1.0 swaps the model asynchronously — a get_state sent
                    // right after the command still reports the PREVIOUS model
                    // (measured), which made the pill lag one click behind.
                    // The set_model response carries the swapped model: apply
                    // it immediately, then re-sync state (thinking level now
                    // reflects the new model, e.g. "off" for non-reasoning).
                    if let Some(data) = &data {
                        if let Some(st) = self.state.as_mut() {
                            st.model = pi_link::protocol::parse_model_info(data);
                        }
                    }
                    self.refresh_state();
                } else if command == "get_entries" && success {
                    // fork 锚点的唯一可靠来源：扁平 entries + parentId 回溯。
                    // get_tree 在长会话上会被 pi 自己拒绝（见 GetEntries 注释），
                    // 033 导航面板因此降级为空，但「新分支」必须一直可用。
                    if let Some(data) = &data {
                        let (entries, leaf) = pi_link::protocol::parse_entries(data);
                        self.active_user_entry_ids =
                            pi_link::protocol::active_user_entry_ids(&entries, leaf.as_deref());
                        self.apply_entry_ids();
                    }
                } else if command == "get_tree" && success {
                    if let Some(data) = &data {
                        let (tree, leaf) = parse_tree(data);
                        // 树只是 033 导航面板的数据源；锚点以 get_entries 为准
                        // （同一条链、同一顺序），这里同步一份作为短会话的兜底
                        self.active_user_entry_ids =
                            super::fork::collect_path_user_ids(&tree, leaf.as_deref());
                        self.apply_entry_ids();
                        self.branch_tree = Some((tree, leaf));
                    }
                } else if command == "fork" || command == "clone" {
                    // clone = 整段复制（rpc clone → fork at leaf），与 fork
                    // 的落地动作完全一样
                    self.on_fork_response(success, error.as_deref(), cx);
                } else if command == "get_messages" && success {
                    if let Some(data) = &data {
                        let rpc_msgs = data["messages"].as_array().cloned().unwrap_or_default();
                        // 系统提示词 + 工具声明：pi 0.86+ 把它们写进 transcript
                        // 的 system 消息（sections / toolsAdded），replay 是唯一读法
                        // （pi-ai transcript.js；pi-web 的 System/Tools 面板同源）。
                        // 快照是权威源：system 消息整表替换后重放（live 流式追加
                        // 的增量以快照为准对齐）。在叶子链修复之前算——那份回退只
                        // 喂显示用的消息。
                        self.system_msgs = rpc_msgs
                            .iter()
                            .map(|e| e.get("message").unwrap_or(e))
                            .filter(|m| m["role"].as_str() == Some("system"))
                            .cloned()
                            .collect();
                        self.replay_system(cx);
                        // leaf-chain repair: pi anchors its restored leaf at the
                        // last entry of ANY type; a mis-parented custom entry
                        // (plan-mode-state 实测) strands whole turns off the
                        // chain and get_messages comes back short. Rebuild from
                        // the file anchored at the last message entry instead.
                        let mut repaired = false;
                        if rpc_msgs.len() < self.disk_msg_count {
                            if let Some(path) = &self.file {
                                let msgs =
                                    msgs_from_tail(read_leaf_messages(path, LEAF_REPAIR_MAX));
                                if msgs.len() > rpc_msgs.len() {
                                    self.messages = msgs;
                                    self.pending_echo = None;
                                    // entry-id mapping stays the RPC/get_entries view
                                    // (fork anchors best-effort on repaired chains)
                                    let mut ids = self.active_user_entry_ids.iter();
                                    for m in self.messages.iter_mut() {
                                        if m.role == Role::User {
                                            m.entry_id = ids.next().cloned();
                                        }
                                    }
                                    self.apply_pending_locate(cx);
                                    self.notify_list(cx);
                                    self.status =
                                        "resumed (leaf chain repaired from disk)".into();
                                    repaired = true;
                                }
                            }
                        }
                        if repaired {
                            return;
                        }
                        // authoritative projection: replace any disk-direct /
                        // cached pre-render instead of appending (fixes
                        // doubled rows after the tail pre-render)
                        self.messages.clear();
                        self.pending_echo = None;
                        for msg in &rpc_msgs {
                            let blocks = content_blocks(&msg["content"]);
                            let usage = Usage::parse(&msg["usage"]);
                            self.ingest_message(msg, blocks, usage, None, cx);
                        }
                        // map user messages to active-path entry ids (fork anchors)
                        let mut ids = self.active_user_entry_ids.iter();
                        for m in self.messages.iter_mut() {
                            if m.role == Role::User {
                                m.entry_id = ids.next().cloned();
                            }
                        }
                        // get_messages payloads carry generation-START stamps
                        // only; completion stamps (回复用时) come from the
                        // session file's entry write-times
                        self.merge_tail_stamps();
                        self.apply_pending_locate(cx);
                        // 同一会话的重读保住钉顶（换工具预设/压缩后重拉都走这里）
                        self.restore_anchor_after_reload();
                        self.notify_list(cx);
                    }
                    self.status = status_line(true, "resumed");
                } else if command == "compact" {
                    // 手动压缩（圆环弹窗按钮）：response 在摘要 LLM 完成后
                    // 才回（带 summary/usage）——清防连点标志并重拉
                    // stats/messages（compaction 卡片 + 上下文环跟着变）
                    self.compacting = false;
                    if success {
                        self.refresh_state();
                        if let Some(s) = self.agent.session.as_ref() {
                            let _ = s.send(&Command::GetMessages);
                        }
                    } else {
                        self.status =
                            format!("compact failed: {}", error.unwrap_or_default());
                    }
                } else if command == "bash" {
                    // `!` shell 命令最终结果（流式增量已就地追加；这里回填
                    // 退出码/截断/取消并点亮完成态）。失败响应（会话忙被 pi
                    // 拒等）同样要收尾乐观卡片，否则「执行中」永不消失。
                    let result = if success {
                        data.map(|d| pi_link::protocol::BashResult::parse(&d))
                            .unwrap_or_default()
                    } else {
                        pi_link::protocol::BashResult {
                            output: error.clone().unwrap_or_default(),
                            exit_code: Some(1),
                            ..Default::default()
                        }
                    };
                    self.finalize_bash(result, cx);
                } else if success {
                    self.status = format!("{command} ok");
                } else {
                    self.status =
                        format!("{command} failed: {}", error.unwrap_or_default());
                    if command == "prompt" {
                        // 发送失败：等待行不能一直转下去；锚点也无从跟随
                        self.phase_waiting = false;
                        self.pending_echo = None;
                        self.pager.release();
                    }
                }
            }
            Event::MessageStart { role, blocks, timestamp, is_error, tool_call_id, custom_type: _, custom_display: _, details, raw_system } => {
                match role.as_str() {
                    "user" => {
                        // upgrade the optimistic send bubble in place instead
                        // of pushing a duplicate echo (pi-web
                        // optimisticUserMessageKey parity); image blocks and
                        // entry ids ride in with the echo
                        let echo_text = blocks
                            .iter()
                            .map(|b| match b {
                                Block::Text { text, .. } => text.as_str(),
                                _ => "",
                            })
                            .collect::<Vec<_>>()
                            .join("");
                        // 去重只看 pending_echo：pi 回显 user 消息 ≠ agent 已开始应答，
                        // 等待行（spark + 正在思考… + shimmer）必须活到第一个 assistant 事件
                        let optimistic = self.pending_echo.is_some()
                            && matches!(self.messages.last(), Some(m)
                                if m.role == Role::User && m.plain_text() == echo_text);
                        self.pending_echo = None;
                        if optimistic {
                            if let Some(m) = self.messages.last_mut() {
                                m.blocks = blocks;
                            }
                        } else {
                            self.messages.push(Msg {
                                role: Role::User,
                                bash: None,
                                blocks,
                                usage: None,
                                entry_id: None,
                                ts: timestamp,
                                end_ts: None,
                                stop_reason: None,
                                error_message: None,
                                custom_type: None,
                                custom_display: true,
                                details: None,
                                model: None,
                            });
                        }
                        user_arrived = true;
                    }
                    "assistant" => {
                        self.phase_waiting = false;
                        self.pending_echo = None;
                        self.streaming_content = false;
                        self.messages.push(Msg {
                            role: Role::Assistant,
                            bash: None,
                            blocks,
                            usage: None,
                            entry_id: None,
                            ts: timestamp,
                            end_ts: None,
                            stop_reason: None,
                            error_message: None,
                            custom_type: None,
                            custom_display: true,
                            details: None,
                            model: None,
                        });
                    }
                    "toolResult" => {
                        let (text, images) = result_payload(&blocks);
                        if let Some(m) = self.messages.last_mut() {
                            merge_tool_result(
                                m,
                                tool_call_id.as_deref(),
                                is_error,
                                &text,
                                images,
                                details.clone(),
                                timestamp,
                            );
                        }
                    }
                    "custom" => {
                        let _ = &details;
                        self.messages.push(Msg {
                            role: Role::Custom,
                            bash: None,
                            blocks,
                            usage: None,
                            entry_id: None,
                            ts: timestamp,
                            end_ts: None,
                            stop_reason: None,
                            error_message: None,
                            custom_type: None,
                            custom_display: true,
                            details: None,
                            model: None,
                        });
                    }
                    "system" => {
                        // pi 每个 run 都会先追加一条 role:"system"（content /
                        // sections / toolsAdded 补丁），RPC message_start 全量
                        // 携带——收进 transcript 重放缓存，面板实时点亮
                        // （新会话第一轮就有，不必等下一次 get_messages）。
                        // message_end 不重复处理（start 已带全量）。
                        if let Some(raw) = raw_system {
                            self.system_msgs.push(raw);
                            self.replay_system(cx);
                        }
                    }
                    _ => {}
                }
                let _ = timestamp;
            }
            Event::MessageUpdate(assistant_event) => {
                self.phase_waiting = false;
                self.pending_echo = None;
                self.streaming_content = true;
                // 060 微信回推：把流式事件原样冒给订阅方，由它自己决定何时
                // flush（ZCode assistantReplyBuffer 语义，§6 坑1 边界不按 chunk）
                cx.emit(SessionEvent::Assistant(assistant_event.clone()));
                match assistant_event {
                AssistantEvent::TextDelta { content_index, delta } => {
                    if let Block::Text { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Text { content_index, text: String::new() },
                    ) {
                        text.push_str(&delta);
                    }
                }
                AssistantEvent::TextEnd { content_index, content } => {
                    if let Block::Text { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Text { content_index, text: String::new() },
                    ) {
                        *text = content;
                    }
                }
                AssistantEvent::ThinkingStart { content_index } => {
                    self.assistant_slot(
                        content_index,
                        Block::Thinking { content_index, text: String::new() },
                    );
                }
                AssistantEvent::ThinkingDelta { content_index, delta } => {
                    if let Block::Thinking { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Thinking { content_index, text: String::new() },
                    ) {
                        text.push_str(&delta);
                    }
                }
                AssistantEvent::ThinkingEnd { content_index, content } => {
                    if let Block::Thinking { text, .. } = self.assistant_slot(
                        content_index,
                        Block::Thinking { content_index, text: String::new() },
                    ) {
                        *text = content;
                        // pi-web parity: collapse thinking once it completes
                        let msg_ix = self.messages.len().saturating_sub(1);
                        self.collapsed.insert((msg_ix, content_index), false);
                    }
                }
                AssistantEvent::ToolCallStart { content_index, id, tool_name } => {
                    self.assistant_slot(
                        content_index,
                        Block::ToolCall {
                            content_index,
                            id,
                            name: tool_name,
                            args: String::new(),
                            result: String::new(),
                            is_error: false,
                            images: Vec::new(),
                            duration_s: None,
                            details: None,
                            args_partial: true,
                            result_arrived: false,
                        },
                    );
                }
                AssistantEvent::ToolCallDelta { content_index, delta } => {
                    if let Block::ToolCall { args, .. } = self.assistant_slot(
                        content_index,
                        Block::ToolCall {
                            content_index,
                            id: String::new(),
                            name: String::new(),
                            args: String::new(),
                            result: String::new(),
                            is_error: false,
                            images: Vec::new(),
                            duration_s: None,
                            details: None,
                            args_partial: true,
                            result_arrived: false,
                        },
                    ) {
                        args.push_str(&delta);
                    }
                }
                AssistantEvent::ToolCallEnd { content_index, tool_call } => {
                    let name = tool_call["toolName"]
                        .as_str()
                        .or_else(|| tool_call["name"].as_str())
                        .unwrap_or("")
                        .to_string();
                    let args = tool_call["arguments"]
                        .as_object()
                        .filter(|o| !o.is_empty())
                        .map(|o| serde_json::Value::Object(o.clone()).to_string())
                        .unwrap_or_default();
                    let name_c = name;
                    let args_c = args;
                    if let Block::ToolCall { name, args, .. } = self.assistant_slot(
                        content_index,
                        Block::ToolCall {
                            content_index,
                            id: String::new(),
                            name: name_c.clone(),
                            args: args_c.clone(),
                            result: String::new(),
                            is_error: false,
                            images: Vec::new(),
                            duration_s: None,
                            details: None,
                            args_partial: false,
                            result_arrived: false,
                        },
                    ) {
                        *name = name_c;
                        *args = args_c;
                    }
                }
                AssistantEvent::Other(_) => {}
                }
            }
            Event::MessageEnd { role, blocks, usage, timestamp, stop_reason, error_message, model, .. } => {
                if role == "assistant" {
                    if let Some(m) = self.messages.last_mut() {
                        if m.role == Role::Assistant {
                            m.blocks = blocks;
                            m.usage = usage.map(|u| UsageLine {
                                input: u.input,
                                output: u.output,
                                cache_read: u.cache_read,
                                cache_write: u.cache_write,
                                cost: u.cost,
                            });
                            m.ts = timestamp.or(m.ts);
                            m.end_ts = Some(crate::services::format::now_ms());
                            m.stop_reason = stop_reason;
                            m.error_message = error_message;
                            m.model = model;
                            self.streaming_content = false;
                        }
                    }
                }
            }
            Event::AgentStart => {
                self.agent_running = true;
                self.status = "running".into();
                if self.stream_started.is_none() {
                    self.stream_started = Some(std::time::Instant::now());
                }
                // 跑动状态的订阅方（psp 状态槽的未读绿点）只挂在 Changed 上，
                // 而 Changed 只在进程退出 / 压缩 / 换工具预设时才发——轮次起止
                // 必须自己冒泡，否则后台会话跑起来也点不亮未读。
                cx.emit(SessionEvent::Changed);
            }
            Event::AgentSettled => {
                self.agent_running = false;
                self.phase_waiting = false;
                self.pending_echo = None;
                self.streaming_content = false;
                // 一轮结束**不**退役锚点：pi-web 只是 promptAnchorActive=false
                // 让垫片收敛，容器 scrollTop 保持——短回复留在屏顶、长回复由
                // 胶水跟随尾部。锚点在此退役会把内容拽回屏底（消息从屏顶跳走，
                // 等于立刻撤销「发言钉顶」）。锚点留待用户滚轮 / 发送失败 /
                // 快照重建 / 会话切换退役。
                self.status = status_line(true, "idle");
                self.settle_turn();
                self.refresh_state();
            }
            Event::AgentEnd { .. } => {
                self.agent_running = false;
                self.phase_waiting = false;
                self.pending_echo = None;
                self.streaming_content = false;
                // 同上：轮末不退役锚点（短回复继续钉在屏顶）
                self.settle_turn();
                // our own writer advanced the file — re-baseline so the
                // external-append tick doesn't re-read our own turn
                self.sync_disk_baseline();
                // the session file exists now — make the new session show up
                // in the sidebar (pi-web refreshKey-on-agent_end parity)
                cx.emit(SessionEvent::ListDirty);
                // refresh fork anchors so newly-sent user messages gain entry ids
                self.refresh_anchors();
                // agent may have written files: refresh git status
                            }
            Event::ExtensionUi(req) => cx.emit(SessionEvent::ExtUi(req)),
            // `!` 执行的流式输出增量：就地追加进乐观卡片（pi-web 不消费这个
            // 事件、靠会话重载——这里直接流式渲染，严格更好）
            Event::BashExecutionUpdate { delta, .. } => {
                if let Some(ix) = self.pending_bash {
                    if let Some(m) = self.messages.get_mut(ix) {
                        if let Some(b) = m.bash.as_mut() {
                            b.output.push_str(&delta);
                        }
                    }
                    self.notify_list(cx);
                }
            }
            Event::Unparsed(_) => {}
        }
        // 用户消息到达（发送回显升级 / steer 推入）→ 翻页：该消息钉视口
        // 顶，历史滚出屏（回显升级同锚点重入不重钉，见 page_turn）
        if user_arrived {
            let ix = self.messages.len() - 1;
            self.pager
                .page_turn(ix, self.messages.len(), self.phase_row_visible());
        }
        self.notify_list(cx);
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
    pub(crate) fn check_external_append(&mut self, cx: &mut Context<Self>) {
        if self.agent_running || self.pending_echo.is_some() {
            return;
        }
        let Some(f) = self.file.clone() else { return };
        let Ok(meta) = std::fs::metadata(&f) else { return };
        let len = meta.len();
        if len == self.disk_file_len || len == 0 {
            return;
        }
        let msgs = msgs_from_tail(read_leaf_messages(&f, LEAF_REPAIR_MAX));
        self.disk_file_len = len;
        if msgs.len() != self.messages.len() {
            // 整表重读：锚点索引可能失效，重读后按最后一条用户消息重锚
            self.messages = msgs;
            // fork anchors best-effort (same as the open-time repair path)
            let mut ids = self.active_user_entry_ids.iter();
            for m in self.messages.iter_mut() {
                if m.role == Role::User {
                    m.entry_id = ids.next().cloned();
                }
            }
            self.apply_pending_locate(cx);
            self.restore_anchor_after_reload();
            self.notify_list(cx);
            self.status = status_line(true, "synced from disk");
        }
    }

    /// Backfill message stamps from the session-file tail: snapshots never
    /// carry completion times, and entry write-times are the only end-stamp
    /// source for history (aligned by suffix — tail conversion mirrors the
    /// same ingest semantics).
    fn merge_tail_stamps(&mut self) {
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
    pub fn locate_message(&mut self, ts: Option<i64>, needle: &str, _cx: &mut Context<Self>) -> bool {
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
    fn apply_pending_locate(&mut self, _cx: &mut Context<Self>) {
        if let Some((ts, needle)) = self.pending_locate.take() {
            if let Some(ix) = self.hit_index(ts, &needle) {
                self.pager.reveal(ix);
            }
        }
    }

    /// Convert one wire message (get_messages snapshot shape) into render
    /// state. Mirrors msgs_from_tail semantics exactly: user/assistant push,
    /// toolResult merges into the paired tool call, other roles skipped.
    fn ingest_message(
        &mut self,
        msg: &serde_json::Value,
        blocks: Vec<Block>,
        usage: Option<Usage>,
        entry_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let role = msg["role"].as_str().unwrap_or("");
        let ts = msg["timestamp"].as_i64();
        match role {
            "user" => {
                // 快照重建路径（打开会话/刷新循环调用）：不做清屏，
                // reset 后 Bottom 对齐保持贴底；清屏只属活事件路径
                self.messages.push(Msg {
                    role: Role::User,
                    bash: None,
                    blocks,
                    usage: None,
                    entry_id,
                    ts,
                    end_ts: None,
                    stop_reason: None,
                    error_message: None,
                    custom_type: None,
                    custom_display: true,
                    details: None,
                    model: None,
                });
            }
            "assistant" => {
                self.messages.push(Msg {
                    role: Role::Assistant,
                    bash: None,
                    blocks,
                    usage: usage.map(|u| UsageLine {
                        input: u.input,
                        output: u.output,
                        cache_read: u.cache_read,
                        cache_write: u.cache_write,
                        cost: u.cost,
                    }),
                    entry_id: None,
                    ts,
                    end_ts: None,
                    stop_reason: msg["stopReason"].as_str().map(str::to_string),
                    error_message: msg["errorMessage"].as_str().map(str::to_string),
                    model: msg["model"].as_str().map(str::to_string),
                    custom_type: None,
                    custom_display: true,
                    details: None,
                });
            }
            "toolResult" => {
                let (text, images) = result_payload(&blocks);
                let is_error = msg["isError"].as_bool().unwrap_or(false);
                let tcid = msg["toolCallId"].as_str();
                let details = msg["details"].as_object().map(|_| msg["details"].clone());
                if let Some(m) = self.messages.last_mut() {
                    merge_tool_result(m, tcid, is_error, &text, images, details, ts);
                }
            }
            "custom" => {
                self.messages.push(Msg {
                    role: Role::Custom,
                    bash: None,
                    blocks,
                    usage: None,
                    entry_id: None,
                    ts,
                    end_ts: None,
                    stop_reason: None,
                    error_message: None,
                    custom_type: Some(msg["customType"].as_str().unwrap_or("").to_string()),
                    custom_display: msg["display"].as_bool().unwrap_or(true),
                    details: msg["details"].as_object().map(|_| msg["details"].clone()),
                    model: None,
                });
            }
            // `!` 执行记录：get_messages 快照（agent 投影）不携带，快照里
            // 出现只可能是未来 pi 行为——照磁盘口径转成 Role::Bash
            "bashExecution" => {
                self.messages.push(Msg {
                    role: Role::Bash,
                    blocks: Vec::new(),
                    usage: None,
                    entry_id: None,
                    ts,
                    end_ts: None,
                    stop_reason: None,
                    error_message: None,
                    custom_type: None,
                    custom_display: true,
                    details: None,
                    model: None,
                    bash: Some(BashInfo {
                        command: msg["command"].as_str().unwrap_or("").to_string(),
                        output: msg["output"].as_str().unwrap_or("").to_string(),
                        exit_code: msg["exitCode"].as_i64(),
                        cancelled: msg["cancelled"].as_bool().unwrap_or(false),
                        truncated: msg["truncated"].as_bool().unwrap_or(false),
                        full_output_path: msg["fullOutputPath"].as_str().map(str::to_string),
                        excluded: msg["excludeFromContext"].as_bool().unwrap_or(false),
                        running: false,
                    }),
                });
            }
            _ => {}
        }
        self.notify_list(cx);
    }

    fn last_assistant(&mut self) -> &mut Msg {
        if !matches!(self.messages.last(), Some(m) if m.role == Role::Assistant) {
            self.messages
                .push(Msg {
                    role: Role::Assistant,
                    bash: None,
                    blocks: Vec::new(),
                    usage: None,
                    entry_id: None,
                    ts: None,
                    end_ts: None,
                    stop_reason: None,
                    error_message: None,
                    custom_type: None,
                    custom_display: true,
                    details: None,
                    model: None,
                });
        }
        self.messages.last_mut().expect("just pushed")
    }

    fn assistant_slot(&mut self, content_index: usize, fresh: Block) -> &mut Block {
        let msg = self.last_assistant();
        while msg.blocks.len() <= content_index {
            let pad = msg.blocks.len();
            msg.blocks.push(Block::Text { content_index: pad, text: String::new() });
        }
        let same_kind = match (&msg.blocks[content_index], &fresh) {
            (Block::Text { .. }, Block::Text { .. })
            | (Block::Thinking { .. }, Block::Thinking { .. })
            | (Block::ToolCall { .. }, Block::ToolCall { .. }) => true,
            _ => false,
        };
        if !same_kind {
            msg.blocks[content_index] = fresh;
        }
        &mut msg.blocks[content_index]
    }

    /// pi-web ChatWindow agentRunning && !hasStreamingContent parity: the
    /// waiting/running row shows while no content is streaming — before the
    /// first assistant event (waiting) and during tool execution (running).
    pub(crate) fn phase_row_visible(&self) -> bool {
        (self.phase_waiting || self.agent_running) && !self.streaming_content
    }

    /// pi-web phaseLabel parity: running tool names derive from the last
    /// assistant message's unanswered tool calls; none pending = waiting.
    pub(crate) fn phase_label(&self) -> String {
        let tools: Vec<String> = self
            .messages
            .last()
            .filter(|m| m.role == Role::Assistant)
            .map(|m| {
                m.blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::ToolCall { name, result, .. }
                            if !name.is_empty() && result.is_empty() =>
                        {
                            Some(name.to_string())
                        }
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        match tools.len() {
            0 => tr("正在等待模型...").to_string(),
            1 => crate::i18n::tf("正在运行 {name}...", &[("name", tools[0].clone())]),
            n if n <= 3 => crate::i18n::tf("正在运行 {names}...", &[("names", tools.join(", "))]),
            n => crate::i18n::tf(
                "正在运行 {names}（另有 {count} 个）...",
                &[
                    ("names", tools[..2].join(", ")),
                    ("count", (n - 2).to_string()),
                ],
            ),
        }
    }

    /// 列表同步：结构手术（splice/reset）与翻页垫片结算全在 ChatList
    /// （chat_list.rs，pi-web useAgentSession 滚屏层的移植）。
    pub(crate) fn notify_list(&mut self, cx: &mut Context<Self>) {
        self.pager.sync(self.messages.len(), self.phase_row_visible());
        // 消息变化（含流式 delta 路径）→ 导航摘要置脏，渲染帧惰性重建
        self.nav_dirty = true;
        cx.notify();
    }

    // ---- 033 会话导航面板 ----

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
            turns,
            users,
            assistants,
            tool_calls,
        }
    }

    /// 引导：中断当前运行并立即注入此消息（rpc steer）。乐观上屏与翻页
    /// 和 prompt 发送一致——发送帧气泡钉视口顶，不等回显。
    pub(crate) fn steer_input(&mut self, cx: &mut Context<Self>) {
        let text = self.input.trim().to_string();
        if text.is_empty() && self.pending_images.is_empty() {
            return;
        }
        let images: Vec<serde_json::Value> = self
            .pending_images
            .iter()
            .map(|img| {
                serde_json::json!({
                    "type": "image", "data": img.data_b64, "mimeType": img.mime
                })
            })
            .collect();
        match self
            .agent
            .session
            .as_ref()
            .map(|session| session.send(&Command::Steer { message: text.clone(), images }))
        {
            Some(Ok(_)) => {
                self.input.clear();
                self.pending_images.clear();
                if !text.is_empty() && !text.starts_with("/skill:") {
                    self.optimistic_send(text, cx);
                } else {
                    cx.notify();
                }
            }
            Some(Err(e)) => self.status = e,
            None => {}
        }
    }

    /// 乐观上屏 + 发送帧翻页（prompt/steer 共用，pi-web optimistic user
    /// message parity）：气泡立即显示并钉视口顶，RPC 回显经 pending_echo
    /// 去重就地升级。
    fn optimistic_send(&mut self, text: String, cx: &mut Context<Self>) {
        self.pending_echo = Some(text.clone());
        self.messages.push(Msg {
            role: Role::User,
            bash: None,
            blocks: vec![Block::Text { content_index: 0, text }],
            usage: None,
            entry_id: None,
            ts: Some(crate::services::format::now_ms()),
            end_ts: None,
            stop_reason: None,
            error_message: None,
            custom_type: None,
            custom_display: true,
            details: None,
            model: None,
        });
        self.phase_waiting = true;
        // 发送帧翻页：新用户消息立即钉视口顶、历史滚出屏，不等回显不等
        // bounds（page_turn 内部完成 sync 落账）
        let ix = self.messages.len() - 1;
        let msgs = self.messages.len();
        let phase = self.phase_row_visible();
        self.pager.page_turn(ix, msgs, phase);
        cx.notify();
    }

    /// 后续消息：Agent 完成后排队此消息（rpc follow_up，pi 1.0 原生携带
    /// images）。
    pub(crate) fn follow_up_input(&mut self, cx: &mut Context<Self>) {
        // 压缩锁：UI 已锁死，这里是绕过 UI 调用的兜底
        if self.compacting {
            return;
        }
        let text = self.input.trim().to_string();
        if text.is_empty() && self.pending_images.is_empty() {
            return;
        }
        let images: Vec<serde_json::Value> = self
            .pending_images
            .iter()
            .map(|img| {
                serde_json::json!({
                    "type": "image", "data": img.data_b64, "mimeType": img.mime
                })
            })
            .collect();
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::FollowUp { message: text, images });
        }
        self.input.clear();
        self.pending_images.clear();
        cx.notify();
    }

    /// 停止（rpc abort）。
    pub(crate) fn abort_stream(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::Abort);
        }
        self.stream_started = None;
        // 中止：等待行立即收起（不等 agent_settled）
        self.phase_waiting = false;
        self.pending_echo = None;
        // 中止即「不在跑」：状态槽转圈 / composer 停止按钮都读 agent_running，
        // 若 pi 的终止事件迟到或压根不来（进程被掐），不就地复位就会永久显示
        // 运行中——同一条「立即收起」语义
        self.agent_running = false;
        if let Some(s) = self.state.as_mut() {
            s.is_streaming = false;
        }
        self.notify_list(cx);
        cx.notify();
    }

    /// 「回到最新」按钮（pi-web scrollToBottom）：reveal 列表末条（锚点期
    /// 含垫片——reveal 垫片底 = 回到钉顶位，与 pi-web scrollToBottom 落在
    /// 垫片底同款）。落底后 Bottom 对齐自动归 None 恢复跟随。
    pub(crate) fn scroll_to_bottom(&mut self) {
        self.pager.jump_to_bottom();
    }

    /// 整表重读（`get_messages` / 会话文件重读）之后恢复锚点，**不要**无条件
    /// `release`：工具预设换绑就是一次 `GetMessages`，退役锚点会让「等待模型
    /// 响应」等短内容连同刚上翻的历史一起掉回屏底（用户实测）。锚点仍指着一条
    /// 用户消息就原样保留；否则按「最后一条用户消息」重锚；确实找不到才退役。
    fn restore_anchor_after_reload(&mut self) {
        if !self.pager.anchor_active() {
            return;
        }
        let msgs = self.messages.len();
        let phase = self.phase_row_visible();
        let valid = self
            .pager
            .anchor_ix()
            .filter(|ix| self.messages.get(*ix).is_some_and(|m| m.role == Role::User));
        if let Some(ix) = valid {
            self.pager.reanchor(ix, msgs, phase);
        } else if let Some(ix) = self.messages.iter().rposition(|m| m.role == Role::User) {
            self.pager.reanchor(ix, msgs, phase);
        } else {
            self.pager.release();
        }
    }

    /// `!` / `!!` shell 命令发送（031 对齐 pi-web executeBash）：立即执行、
    /// 不进模型；乐观卡片流式回显输出，response 回填终态。发出后
    /// `bash_running` = true（调用方据此清空输入）。
    fn send_bash(&mut self, text: &str, cx: &mut Context<Self>) {
        let trimmed = text.trim_start();
        let excluded = trimmed.starts_with("!!");
        let command = trimmed[if excluded { 2 } else { 1 }..].trim();
        if command.is_empty() {
            self.status = tr("命令为空：! 后面接 shell 命令").to_string();
            cx.notify();
            return;
        }
        // pi 侧拒绝条件（rpc-mode bash：busy 即 error）——提前拦，输入保留
        if self.bash_running || self.agent_running || self.compacting {
            self.status = tr("会话忙，无法执行 shell 命令").to_string();
            cx.notify();
            return;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("未连接").into();
            cx.notify();
            return;
        };
        let cmd = Command::Bash {
            command: command.to_string(),
            exclude_from_context: excluded,
        };
        match session.send(&cmd) {
            Ok(_) => {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .ok();
                self.pending_bash = Some(self.messages.len());
                self.messages.push(Msg {
                    role: Role::Bash,
                    bash: Some(BashInfo {
                        command: command.to_string(),
                        excluded,
                        running: true,
                        ..Default::default()
                    }),
                    blocks: Vec::new(),
                    usage: None,
                    entry_id: None,
                    ts,
                    end_ts: None,
                    stop_reason: None,
                    error_message: None,
                    custom_type: None,
                    custom_display: true,
                    details: None,
                    model: None,
                });
                self.bash_running = true;
                self.status = if excluded {
                    "shell（输出仅本地）".into()
                } else {
                    "shell（输出发给模型）".into()
                };
                self.notify_list(cx);
            }
            Err(e) => self.status = e,
        }
    }

    /// bash response 回填终态（pi BashResult）。乐观卡片按 pending_bash 定位，
    /// 索引失效（快照重建/换档重读）时补一张独立完成卡。
    fn finalize_bash(&mut self, result: pi_link::protocol::BashResult, cx: &mut Context<Self>) {
        self.bash_running = false;
        let end_ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .ok();
        let applied = self
            .pending_bash
            .take()
            .and_then(|ix| self.messages.get_mut(ix))
            .filter(|m| m.role == Role::Bash)
            .map(|m| {
                if let Some(b) = m.bash.as_mut() {
                    b.output = result.output.clone();
                    b.exit_code = result.exit_code;
                    b.cancelled = result.cancelled;
                    b.truncated = result.truncated;
                    b.full_output_path = result.full_output_path.clone();
                    b.running = false;
                }
                m.end_ts = end_ts;
            })
            .is_some();
        if !applied {
            // 卡片丢了：补一张完整卡（命令名无从找回时留空——正常链路
            // pending_bash 不会失效）
            self.messages.push(Msg {
                role: Role::Bash,
                bash: Some(BashInfo {
                    output: result.output,
                    exit_code: result.exit_code,
                    cancelled: result.cancelled,
                    truncated: result.truncated,
                    full_output_path: result.full_output_path,
                    ..Default::default()
                }),
                blocks: Vec::new(),
                usage: None,
                entry_id: None,
                ts: None,
                end_ts,
                stop_reason: None,
                error_message: None,
                custom_type: None,
                custom_display: true,
                details: None,
                model: None,
            });
        }
        self.status = if result.cancelled {
            "shell 中止".into()
        } else {
            match result.exit_code {
                Some(0) => "shell 完成".into(),
                Some(c) => format!("shell 退出码 {c}"),
                None => "shell 完成".into(),
            }
        };
        self.notify_list(cx);
    }

    /// Esc / 停止按钮的 bash 分支（rpc abort_bash；pi 会把已启动的命令
    /// 取消掉，response 带 cancelled=true 收尾卡片）
    pub(crate) fn abort_bash(&mut self, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::AbortBash);
        }
        cx.notify();
    }

    pub(crate) fn send_input(&mut self, cx: &mut Context<Self>) {
        // 压缩锁：UI 已锁死，这里是绕过 UI 调用的兜底
        if self.compacting {
            return;
        }
        let text = self.input.trim().to_string();
        // 空文本+图片可发送（pi-web handleSend：!msg && !images 才拦）
        if text.is_empty() && self.pending_images.is_empty() {
            return;
        }
        // 040：picker 运行中确认的清单，下一轮（空闲发送）开始前生效——
        // 换包集必须重绑进程（RPC 无运行时切换），`--session` 复接不丢消息。
        // 043：MCP 运行中改过 mcp.json 同走这条重绑通路（新进程重读清单）。
        if !self.agent_running {
            let pending_ext = self.pending_ext_sources.take();
            if pending_ext.is_some() || self.pending_mcp_reload {
                self.pending_mcp_reload = false;
                if let Some(list) = pending_ext {
                    self.ext_sources = list;
                    if let Some(p) = pi_link::session_ext::store_path() {
                        let _ = pi_link::session_ext::write_for(&p, &self.key, &self.ext_sources);
                    }
                }
                let this = cx.entity();
                match self.spawn() {
                    Some(rx) => {
                        let epoch = self.agent.epoch;
                        Self::attach_pump(&this, rx, epoch, cx);
                    }
                    None => {
                        self.status = tr("未连接").into();
                        cx.notify();
                        return;
                    }
                }
                if let Some(s) = &self.agent.session {
                    let _ = s.send(&Command::GetMessages);
                }
                self.refresh_state();
            }
        }
        if self.agent.session.is_none() {
            // lazy draft: the pi process spawns on the first prompt
            // (pi-web ensureNewSession parity). spawn_with returns the event
            // receiver — attach the pump here or the session goes deaf.
            let this = cx.entity();
            match self.spawn() {
                Some(rx) => {
                    let epoch = self.agent.epoch;
                    Self::attach_pump(&this, rx, epoch, cx);
                }
                None => {
                    self.status = tr("未连接").into();
                    cx.notify();
                    return;
                }
            }
            // state/stats/commands/models; the file-bound ping that promotes
            // the draft comes from AgentEnd's refresh_state (the file exists
            // only after the first turn)
            self.refresh_state();
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("未连接").into();
            cx.notify();
            return;
        };
        // `!` / `!!` shell 命令（031）：不经 prompt/steer，走 rpc bash。
        // 带图片时 ! 只当普通文本（pi-web bashMode 需无附件）。
        if self.pending_images.is_empty() && text.starts_with('!') {
            self.send_bash(&text, cx);
            if self.bash_running {
                // 与普通消息同口径：进 ↑ 历史、清输入
                if self.history.last().map(|h| h != &text).unwrap_or(true) {
                    self.history.push(text.clone());
                }
                self.history_ix = None;
                self.input.clear();
            }
            cx.notify();
            return;
        }
        // 运行中发消息 = steer（事件驱动 agent_running 优先：快照 is_streaming
        // 一轮内恒 false，会让「引导」变成新 prompt）
        let streaming = self.agent_running || self.state.as_ref().is_some_and(|s| s.is_streaming);
        let images: Vec<serde_json::Value> = self
            .pending_images
            .iter()
            .map(|img| {
                serde_json::json!({
                    "type": "image",
                    "data": img.data_b64,
                    "mimeType": img.mime
                })
            })
            .collect();
        let cmd = if streaming {
            Command::Steer { message: text.clone(), images: images.clone() }
        } else {
            Command::Prompt { message: text.clone(), images }
        };
        match session.send(&cmd) {
            Ok(_) => {
                // 纯图片发送不进历史、不做乐观回显（空文本气泡无内容可显，
                // 等 RPC 回显带图的完整用户消息）
                if !text.is_empty() && self.history.last().map(|h| h != &text).unwrap_or(true) {
                    self.history.push(text.clone());
                }
                self.history_ix = None;
                self.input.clear();
                self.pending_images.clear();
                self.status = if streaming { "steering" } else { "running" }.into();
                // 技能命令不做乐观回显（pi-web parity）：RPC 回显的是展开
                // 信封文本（渲染层折叠成紧凑命令），乐观插入裸 "/skill:xxx"
                // 会多出一条重复气泡。prompt 与 steer 一致——发送帧即上屏
                // 翻页，不等回显
                if !text.is_empty() && !text.starts_with("/skill:") {
                    self.optimistic_send(text, cx);
                }
            }
            Err(e) => self.status = e,
        }
        cx.notify();
    }


    /// 远程（微信）来的一条文本 —— 与 [`send_input`] **同口径**，但不碰
    /// `self.input` / `pending_images`（那两个是 composer 专属的）。
    ///
    /// 关键：草稿**还没有 pi 进程**时必须先 `spawn()` + `attach_pump`，
    /// 否则 `agent.session` 是 `None`、命令直接丢掉 —— 真机表现就是
    /// 「发消息没有任何应答，重启后才有人回」。
    ///
    /// 返回是否真的发出去了（失败时写 `status`，不静默）。
    pub(crate) fn send_remote_text(&mut self, text: String, cx: &mut Context<Self>) -> bool {
        if self.compacting {
            return false;
        }
        if self.agent.session.is_none() {
            let this = cx.entity();
            match self.spawn() {
                Some(rx) => {
                    let epoch = self.agent.epoch;
                    Self::attach_pump(&this, rx, epoch, cx);
                }
                None => {
                    self.status = tr("未连接").into();
                    cx.notify();
                    return false;
                }
            }
            self.refresh_state();
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("未连接").into();
            cx.notify();
            return false;
        };
        // 运行中 = steer，否则 prompt（与 composer 完全一致）
        let streaming =
            self.agent_running || self.state.as_ref().is_some_and(|s| s.is_streaming);
        let cmd = if streaming {
            Command::Steer { message: text.clone(), images: Vec::new() }
        } else {
            Command::Prompt { message: text.clone(), images: Vec::new() }
        };
        let sent = session.send(&cmd).is_ok();
        if sent {
            if self.history.last().map(|h| h != &text).unwrap_or(true) {
                self.history.push(text.clone());
            }
            self.history_ix = None;
            self.status = if streaming { "steering" } else { "running" }.into();
            // 让桌面端也看到这条消息（微信发的同样属于这轮对话）
            if !text.starts_with("/skill:") {
                self.optimistic_send(text, cx);
            }
        } else {
            self.status = tr("发送失败").into();
        }
        cx.notify();
        sent
    }

    /// 远程来的非文本命令（`/停止`、ExtUi 回填）：**不 spawn** ——
    /// 没有进程就明确返回失败，由调用方出声。
    pub(crate) fn send_remote_command(&mut self, cmd: &Command, cx: &mut Context<Self>) -> bool {
        if self.compacting {
            return false;
        }
        let Some(session) = &self.agent.session else {
            self.status = tr("未连接").into();
            cx.notify();
            return false;
        };
        let ok = session.send(cmd).is_ok();
        if !ok {
            self.status = tr("发送失败").into();
        }
        cx.notify();
        ok
    }

    /// LLM session title (pi-web lib/session-title.ts parity via a one-off
    /// `pi --no-session --print` run; the in-process SDK call pi-web uses is
    /// not reachable over RPC).
    pub(crate) fn set_thinking_level(&mut self, level: &str, cx: &mut Context<Self>) {
        if level == "auto" {
            self.thinking_override = None;
            cx.notify();
            return;
        }
        self.thinking_override = Some(level.to_string());
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetThinkingLevel { level: level.to_string() });
        }
        cx.notify();
    }

    pub(crate) fn select_model(&mut self, provider: String, id: String, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetModel { provider, model: id });
        } else {
            // draft with no process: remember the pick, spawn carries it as
            // --model (pi-web newSessionModel parity)
            self.pending_model = Some((provider, id));
        }
        // no immediate refresh_state here: pi 1.0 completes the swap
        // asynchronously, so an eager get_state reports the PREVIOUS model —
        // the label updates from the set_model response instead (see the
        // set_model response arm)
        cx.notify();
    }

    /// 压缩按钮可用性：agent 跑动中不可（compact 会中止当前 turn）、
    /// 压缩中防连点、未连接不可。除此之外不设门槛——用不用由用户决定。
    pub(crate) fn can_compact(&self) -> bool {
        !self.compacting && !self.agent_running && self.agent.session.is_some()
    }

    /// 圆环弹窗底部手动压缩：rpc compact。`compacting` 期间输入面板锁死
    /// （placeholder 提示 + 变更丢弃）；完成后 pi 回 compact response，
    /// 由其分支清标志并刷新 stats/messages。
    pub(crate) fn compact(&mut self, cx: &mut Context<Self>) {
        if !self.can_compact() {
            return;
        }
        match self.agent.session.as_ref().map(|s| s.send(&Command::Compact)) {
            Some(Ok(_)) => {
                self.compacting = true;
                self.status = tr("压缩中…").into();
            }
            Some(Err(e)) => self.status = e,
            None => self.status = tr("未连接").into(),
        }
        cx.emit(SessionEvent::Changed);
        cx.notify();
    }

    pub(crate) fn tool_preset_key(&self) -> &str {
        &self.tools_preset
    }

    pub(crate) fn tool_preset_label(&self) -> String {
        let key = self.tool_preset_key();
        if key.is_empty() {
            return "configured".into();
        }
        if key == "custom" {
            // 档名 full+；扩展数量由右边的「扩展(n)」按钮承载，不重复
            return crate::i18n::tr("full+").to_string();
        }
        key.to_string()
    }

}


