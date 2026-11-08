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
//!
//! 域内分片：types（冒泡事件/队列条目）/ spawn（进程参数装配 + 挂载源
//! 过滤）/ pump（RPC 事件泵 + epoch 守卫）/ events（事件分发）/ response
//! （命令回执臂）/ stream（消息流三臂）/ send（发送路径）/ actions
//! （斜杠/模型/思考/压缩）/ bash（`!` shell 三件套）/ queue（本地队列）/
//! ingest（消息摄取）/ nav（磁盘对账 + 导航缓存）。

use std::path::PathBuf;

use futures::channel::mpsc::UnboundedReceiver;
use gpui::{Context, Entity, EventEmitter, Render, prelude::*};
use pi_link::protocol::{Command, Event, SessionState, SessionStats, SlashCommand, TreeNode};

use crate::agent_session::AgentSession;
use crate::session::chat_list::ChatList;
use crate::session::messages::Msg;

use super::*;

/// no cap for the disk-side leaf-chain rebuild: the repair must be longer
/// than the truncated RPC snapshot to win the length contest, and the RPC
/// path already renders the full (broken) chain — same render scale
const LEAF_REPAIR_MAX: usize = usize::MAX;
use crate::i18n::tr;
use crate::services::format::status_line;

pub(crate) struct SessionRuntime {
    /// stable pool key: session file path, or "draft-N" before first prompt
    pub key: String,
    pub cwd: PathBuf,
    /// None until pi persists the draft (first prompt)
    pub file: Option<PathBuf>,
    /// 只读观察窗（044-0 子代理会话查看）：子会话由父 pi 进程独占驱动，
    /// PF 打开它只是观察同一账本文件——永不 spawn 进程（spawn 会造出第二
    /// 驱动：分叉账本/双花 token/抢占并发槽），无输入通道（composer/操作
    /// 栏隐藏），外部追加靠 3s tick（check_external_append 空闲态天然覆盖）。
    /// open_session 按文件头部 marker 判定置位（sessions::is_subagent_session）。
    pub observer: bool,
    /// disk-side message count at open time — get_messages shorter than
    /// this means pi's restored leaf chain is truncated (mis-parented
    /// non-message entry) and the display rebuilds from the file instead
    pub disk_msg_count: usize,
    /// session file size at last read — an external writer (pi-web on the
    /// same session) grows the file behind our pi process's back, which
    /// never notices; the tick compares and re-reads from disk
    pub disk_file_len: u64,
    /// 外部追加观察（3s tick）的 leaf 重读在飞标记：重读是后台任务（秒开
    /// 深度优化 pi-flash-2o92），在飞期间不得重复调度
    pub leaf_watch_in_flight: bool,
    /// 最近一次 get_messages 快照的条数：磁盘计数（后台）晚到时用它补做
    /// leaf 修复对账——计数 > 快照才值得重读（对抗失败无副作用）
    pub last_rpc_msg_count: Option<usize>,
    /// pi process ownership (None while recycled/lazy)
    pub agent: AgentSession,

    // ---- message log ----
    pub messages: Vec<Msg>,
    /// 会话内容是否已权威就位（打开加载页的放行标记，2026-11-07 用户定夺）：
    /// 文件已绑定但内容未到第一份（尾窗空 + get_messages 投影未落）时渲染
    /// 空白页 + 「会话载入中…」，不再拿新会话欢迎页冒充加载态。tail 非空即
    /// 就位（有东西可看）；投影/失败响应落定也置位（真空会话不能永久钉在
    /// 加载页）。
    pub messages_loaded: bool,
    /// 滚屏状态机（chat_list.rs）：gpui 列表 + 翻页锚点 + 垫片 + splice 记账
    pub pager: ChatList,
    /// explicit open/close overrides; absent = per-state default (thinking
    /// blocks open, process group closed when the turn has a final answer)
    pub collapsed: std::collections::HashMap<(usize, usize), bool>,
    pub phase_waiting: bool,
    /// text/thinking/toolcall deltas in flight (pi-web hasStreamingContent
    /// parity): the running/waiting phase row yields while content streams
    pub streaming_content: bool,
    /// 乐观发送的用户文本队列（FIFO）：只用于把 pi 回显的同文 user 消息
    /// 就地升级（去重）。排队发送（follow_up）一次可积多条，且回显往往
    /// 晚到（下一轮 turn 边界才emit message_start），所以是列表而非单值；
    /// 匹配也不看「是不是最后一条」——排队的回显前面必然已经堆了本轮的
    /// assistant 消息（2026-10-16 修：单值 + last() 口径下 steer/排队回显
    /// 必然重复成两条气泡）。`pin` = 发送帧的钉顶决定，随条目走到回显侧
    /// （[`PendingEcho`]）：打断投递的回显同样不翻页。
    /// 与 phase_waiting 解耦 —— 回显到达 ≠ agent 已应答，等待行不能被它掐掉。
    pub pending_echoes: Vec<PendingEcho>,
    /// 乐观发送的入是斜杠命令（pi-web isSlashCommandPrompt → running_command
    /// 相位）：等待行显示「执行扩展命令中...」而非「正在等待模型...」——
    /// 命令 handler 在跑（阻塞菜单/查询都算），与模型无关（#7 体验修正）。
    /// 生命期：optimistic_send 置位；真轮 AgentStart / handled 收场 /
    /// settled / abort / 发送失败 复位。
    pub phase_command: bool,
    /// pi 侧 steering/followUp 队列镜像（`queue_update` 全量快照）：「已排
    /// 队 N 条」状态行不再本地猜。仅显示用——abort 撤回的对账真值来自
    /// `clear_queue` response（pi 权威回传），不读这里。090 后 composer 消息
    /// 不再进 pi 队列，这里只剩 pif-ui 自动化 steer/follow_up 动作的条目。
    pub queued_steering: Vec<String>,
    pub queued_followups: Vec<String>,
    /// 090：PF 本地排队（queue 模式运行中的发送 + Ctrl+Enter 显式入队 +
    /// 远程插件端）。AgentSettled 逐条 flush，见 queue.rs。
    pub queued_inputs: Vec<QueuedInput>,
    /// 在途 flush 的命令 id（Prompt{None} 发出后等 response）：success =
    /// 摘掉队首；failure = pi 忙拒绝，放回队首等下一次 settled 重试。
    /// Some 期间卡片按钮禁用（条目正被取出派发，ZCode dispatch 态 parity）。
    pub flush_in_flight: Option<FlushInFlight>,
    /// 「立即/插话」打断槽：显式插入 = abort 当前
    /// 回复 + 条目进槽，本轮 aborted 的 AgentSettled 一到就由 flush 以新
    /// prompt 发出——模型立刻看到（此前走 pi steer 通道等注入点，纯文本
    /// 长回复没有边界，54s 故事讲完才被回应）。不渲染卡片：气泡即呈现。
    pub interrupt_pending: Option<QueuedInput>,
    /// 流已切断（abort 已发出）：管道里还在飞的旧轮残包（TextDelta/TextEnd/
    /// ThinkingDelta/ToolCallStart…）到达时，最后一条消息往往已经是乐观追
    /// 加的打断气泡（User）——`last_assistant` 若照常兜底推新 assistant 壳，
    /// 残包就在**气泡后面**长出旧内容，气泡视觉上「跳到 agent 输出上方」，
    /// 要等新轮事件归一才回位。置位期间残包并入最后
    /// 一条既有 assistant（ingest.rs `last_assistant`）；新轮 assistant 消息
    /// 到达 / AgentSettled 清除。
    pub stream_cut: bool,
    pub stream_started: Option<std::time::Instant>,
    pub agent_running: bool,
    /// 本轮结局（三色通知的红档）：**最后一条 assistant 消息**
    /// stopReason=="error"（模型/接口层错误），或 prompt 发送失败。
    /// 定义 = 「用户消息起、最后一条助手消息止」——每条 assistant 消息
    /// 覆盖这两个旗标（MessageEnd）、每条用户消息归零（MessageStart），
    /// 与转写区 final_error/final_truncated 同口径。**工具调用失败不算**
    /// （toolResult.isError 是 agent 工作过程的一部分，工具报错后 agent
    /// 照常自愈并答完——2026-10-12 实测：grep 未命中被误报红点）。
    /// AgentSettled 时随 TurnSettled 冒泡给订阅方（出错红点）。
    pub turn_error: bool,
    /// 本轮结局（黄档）：最后一条 assistant 消息 stopReason = length/aborted
    /// （输出上限截断 / 中止——非外部错误也非正常完成）
    pub turn_interrupted: bool,
    /// pi 侧车退出码（合成 ProcessExited 事件携带；进程死亡轮终时进状态行）
    exit_code: Option<i64>,
    /// wire 上无法识别的事件类型（按 type 去重留痕：新 pi 版本加事件的哨兵，
    /// 不再完全静默）
    unparsed_types: std::collections::HashSet<String>,
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
    /// 043：MCP 菜单/设置页改过 mcp.json——下一轮 send_input 空闲发送前
    /// 重绑生效。**无论运行与否都必须标记**：MCP 装载只在 pi 进程启动时
    /// 读 mcp.json（扩展加载期 + session_start），空闲但活着的进程永远不会
    /// 自己重读；「空闲 spawn 本就直接读盘」只对还没 spawn 的会话成立
    /// （真机 2026-11-03：空闲取消勾选 ghzw 后下一轮照旧挂载，根因即
    /// 此处旧注释误导出的 agent_running 守卫）。运行中由 rebind_pending
    /// 推迟，flag 不清。
    pub(crate) pending_mcp_reload: bool,
    /// 044-0：设置-子代理页模块开关翻转过——工具清单（`agent` /
    /// `subagent_*`）是 spawn 参数，活着的 pi 进程不会自己重读。闲时
    /// 重绑前生效，运行中由 `rebind_pending` 推迟到下一轮发送。
    pub(crate) pending_subagents_reload: bool,
    /// 自动压缩开关（zi2s.5 /auto-compact）：pi 的 get_state 不回显该值
    /// （rpc-types.d.ts RpcSessionState 无此字段），本地跟踪；None = 默认开。
    pub(crate) auto_compaction: Option<bool>,

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
    /// 已提醒过「在等你回答」的阻塞请求 id（pi-web
    /// claimExtensionAttentionNotification parity：每请求只提醒一次）
    pub notified_ext_ids: std::collections::HashSet<String>,

    /// 034 状态带数据面：本会话 pi 侧 `ctx.ui.setStatus(key, text)` 的当前快照
    /// （到达序；渲染侧按 key 字母序排）。**per-runtime 而不是 Chat 级**：pi 在
    /// `resetExtensionUI()`（换会话/重载）里清扩展状态，PF 池子里每个 pi 进程
    /// 各持一份 —— 放 Chat 上会让切会话串味（上一会话的额度行跟到新会话）。
    pub ext_status: Vec<(String, String)>,

    /// (下标, 点亮时刻) — 复制 pill 的「已复制」反馈（032 恢复）：下标对
    /// 用户栏是 msg_ix、对 agent 轮栏是轮头 start_ix（两者互不重叠）。
    /// `spawn_flash_clear` 1.5s 后清空。渲染侧经
    /// `copy_flash.is_some_and(...elapsed < 1500ms)` 判定 copied 态。
    pub copy_flash: Option<(usize, std::time::Instant)>,

    /// 013: jump target waiting for the message snapshot to land
    pub pending_locate: Option<(Option<i64>, String)>,

    /// 033 导航数据缓存（摘要 + 总结栏统计）：渲染层每帧读、消息变化才
    /// 重算（notify_list 置脏，nav_summary() 惰性重建）——语义上「一轮
    /// 一次」，不随渲染帧反复拼前缀/数块
    nav_cache: Option<std::rc::Rc<NavData>>,
    nav_dirty: bool,

    /// notify_list 节流门（2026-11-06 流式卡死修复 A2）：列内容类拍的最小
    /// 间隔（33ms ≈ 30fps 上限）。流式快模型 100+ delta/s 时每条全窗失效
    /// 会把前台队列塞满、输入饿死——间隔未到时置 pending，由 trailing
    /// timer 补一拍（事件停了最后一拍也能落地）。
    notify_last: Option<std::time::Instant>,
    notify_pending: bool,
}

impl SessionRuntime {
    pub(crate) fn new(key: String, cwd: PathBuf, file: Option<PathBuf>) -> Self {
        // 会话级扩展清单：**绑定会话的独立数组**（用户定稿）。全局 settings
        // 启用集只在「新会话创建」时**复制**为初始清单（之后两者互不跟随，
        // 设置页改动不影响已存在的会话）；台账里存的是该会话自选的清单，
        // 重启/复接原样恢复。菜单勾选态只对齐这一份数组。
        let mut ext_sources = pi_link::session_ext::store_path()
            .map(|p| {
                let raw = pi_link::session_ext::read_for(&p, &key);
                // 090-10 版本层自愈：台账存的是含版本目录的绝对路径，插件
                // 升级后旧版本目录被删 → spawn 把死路径当 `-e` 传给 pi，扩展
                // 加载失败会让**整个进程退出**（真机 2026-11-03：绑定微信后
                // 重启会话起不来，ext 无处安身，收发全哑）。重解析后变动了
                // 就回写台账，下一次 spawn 不再付这份 cost。
                let (healed, changed) = pi_link::session_ext::resolve_sources(&raw);
                if changed {
                    let _ = pi_link::session_ext::write_for(&p, &key, &healed);
                }
                healed
            })
            .unwrap_or_default();
        // 台账是混合通道：scheme 源 = 用户自选；路径型条目 = PF 插件挂载通道
        // （session_ext::is_mount_path），只归 spawn 前的台账 -e 段管，不进
        // ext_sources——否则胶囊计数 ≠ 勾选面板（真机 2026-10-31：勾 2 显 [3]）。
        // 台账没有 scheme 自选（空，或只剩路径条目——历史版本 attach 整表覆写
        // npm 清单的残留）→ 回落全局启用集
        if !ext_sources
            .iter()
            .any(|s| !pi_link::session_ext::is_mount_path(s))
        {
            ext_sources = pi_link::skills::enabled_package_sources();
        }
        // 停用/卸载插件的残留挂载源失效（tc6）：创建时 store 读入的清单只增
        // 不减，spawn 前按注册状态过滤（PF 扩展源不过问）
        {
            let ledger = pi_link::pf_plugins::load_ledger();
            ext_sources.retain(|src| spawn::ext_source_allowed(src, &ledger));
        }
        Self {
            key,
            cwd,
            file: file.clone(),
            disk_msg_count: 0,
            leaf_watch_in_flight: false,
            last_rpc_msg_count: None,
            disk_file_len: file
                .as_deref()
                .and_then(|f| std::fs::metadata(f).ok())
                .map(|m| m.len())
                .unwrap_or(0),
            agent: AgentSession::new(1),
            messages: Vec::new(),
            messages_loaded: false,
            pager: ChatList::new(),
            collapsed: std::collections::HashMap::new(),
            observer: false,
            phase_waiting: false,
            streaming_content: false,
            pending_echoes: Vec::new(),
            phase_command: false,
            queued_steering: Vec::new(),
            queued_followups: Vec::new(),
            queued_inputs: Vec::new(),
            flush_in_flight: None,
            interrupt_pending: None,
            stream_cut: false,
            stream_started: None,
            agent_running: false,
            turn_error: false,
            turn_interrupted: false,
            exit_code: None,
            unparsed_types: std::collections::HashSet::new(),
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
            pending_subagents_reload: false,
            auto_compaction: None,
            bash_running: false,
            pending_bash: None,
            last_activity: std::time::Instant::now(),
            ext_queue: Vec::new(),
            notified_ext_ids: std::collections::HashSet::new(),
            ext_status: Vec::new(),

            pending_locate: None,

            nav_cache: None,
            nav_dirty: true,
            copy_flash: None,
            notify_last: None,
            notify_pending: false,
        }
    }

    /// Kill the process but keep everything else (idle recycle / soft drop).
    pub(crate) fn shutdown_process(&mut self) {
        self.agent.session = None;
        // 进程即刻不在：跑动标志就地复位。pump 退出事件可能永不到达（渠道
        // 已死 / 事件泵先被回收），留着 true 会让状态槽的旋转圈常亮。
        self.agent_running = false;
        self.phase_waiting = false;
        self.phase_command = false;
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

    /// 探针（automation snapshot）：（消息条数，system 消息条数）——
    /// 退役回收的前后对照用。
    pub(crate) fn mem_probe(&self) -> (usize, usize) {
        (self.messages.len(), self.system_msgs.len())
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
            pump::consume_runtime_events(weak, cx, rx, epoch).await;
        })
        .detach();
    }

    /// spawn pi 进程并挂事件泵；失败写 `status` 并 notify，返回 false。
    /// 「工具预设/MCP 重绑」与「草稿懒 spawn」共用此内核。
    fn spawn_attached(&mut self, cx: &mut Context<Self>) -> bool {
        let this = cx.entity();
        match self.spawn() {
            Some(rx) => {
                // 新进程队列必为空：换绑/重 spawn 后镜像不得残留旧进程的
                // 队列快照（queue_update 只在变动时发，残留会一直错下去）
                self.queued_steering.clear();
                self.queued_followups.clear();
                // 034：扩展状态同理——旧 pi 进程的 setStatus 快照随进程作废，
                // 新进程重新发（否则 /reload、空闲回收重建后带子上挂旧文案）
                self.ext_status.clear();
                let epoch = self.agent.epoch;
                Self::attach_pump(&this, rx, epoch, cx);
                true
            }
            None => {
                self.status = tr("status.disconnected").into();
                cx.notify();
                false
            }
        }
    }

    /// 草稿懒 spawn（pi-web ensureNewSession parity）：还没有 pi 进程时
    /// spawn + attach_pump + 拉初始状态。失败返回 false（调用方提前 return）。
    pub(crate) fn ensure_spawned(&mut self, cx: &mut Context<Self>) -> bool {
        // 观察窗无输入通道，发送链不会走到这；防御性拦截（never spawn）
        if self.observer {
            return false;
        }
        if self.agent.session.is_some() {
            return true;
        }
        if !self.spawn_attached(cx) {
            return false;
        }
        // state/stats/commands/models；文件绑定的 ping 由 AgentEnd 的
        // refresh_state 补（首轮之前会话文件还不存在）
        self.refresh_state();
        true
    }
}

impl EventEmitter<SessionEvent> for SessionRuntime {}

// placeholder Render so the entity can exist before the view wires up
impl Render for SessionRuntime {
    fn render(&mut self, _window: &mut gpui::Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui::div()
    }
}

// ---------------------------------------------------------------------------
// migrated from Chat (main.rs) — session-local behavior
// ---------------------------------------------------------------------------

mod actions;
mod bash;
mod events;
mod ingest;
mod nav;
mod pump;
mod queue;
mod response;
mod send;
mod spawn;
mod stream;
mod types;

pub(crate) use types::{
    FlushInFlight, NavData, PendingEcho, QueuedInput, SessionEvent, TurnSummary,
};

#[cfg(test)]
mod binding_guard_tests;
#[cfg(test)]
mod custom_msg_tests;
#[cfg(test)]
mod ext_source_registered_tests;
#[cfg(test)]
mod pin_regression_tests;
#[cfg(test)]
mod turn_outcome_tests;
