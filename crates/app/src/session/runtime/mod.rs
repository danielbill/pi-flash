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

/// Shell-level effects a runtime bubbles up (Chat subscribes).
pub(crate) enum SessionEvent {
    /// repaint-worthy state change
    Changed,
    /// 一轮终点（AgentSettled，2026-10-10）：订阅方据此给**不在场**的会话
    /// 点亮完成绿点/出错红点——结算时 agent_running 已复位，走 Changed 的
    /// 「running 时记未读」通路永远轮不到它
    TurnSettled,
    /// sidebar list should refresh (rename flush, fork, file landed)
    ListDirty,
    /// extension UI request (active session only surfaces the dialog)
    ExtUi(pi_link::protocol::ExtensionUiRequest),
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
    /// 伪标签（draft 首条消息乐观上侧栏）：Some(预览) = 挂/刷新，None = 撤。
    /// 真标签要等 pi 落盘 + 磁盘扫描（进程冷启 1-3s），伪标签让「发送瞬间」
    /// 就有标签；FileBound 落地时由 on_file_bound 摘除换真身。
    PendingTab(Option<String>),
    /// abort 撤回的排队消息原文（clear_queue response 携带）：Chat 据此回填
    /// composer 输入框（pi-web "restoring to editor when user aborts" parity）。
    /// 仅活跃会话回填可见输入框；后台会话由 runtime 自吞进 input 草稿。
    /// `images` = 本地队列（090）撤回的附件：活跃会话拼回附件条，后台会话
    /// 已由 runtime 直吞 pending_images（pi 侧撤回的条目无附件，恒空）。
    RecallInput {
        text: String,
        images: Vec<crate::AttachedImage>,
    },
}

/// 090 排队护栏：PF 本地排队条目。运行中（queue 模式）的发送扣在这里，
/// **不进 pi 队列**——pi 的 followUp 在「模型第一次停下不调工具」的
/// would-stop 点就 drain（run 内、agent_settled 之前），abort/出错还不清
/// 队，正是「排队插队」的根源。flush 时机钉死 AgentSettled（queue.rs）。
#[derive(Debug, Clone)]
pub(crate) struct QueuedInput {
    pub text: String,
    pub images: Vec<crate::AttachedImage>,
    /// 远程插件来的条目（与 composer 同一漏斗；卡片渲染不区分，仅排障用）
    pub remote: bool,
}

/// 「立即/插话」的打断投递在途关联（2026-10-20 二次验收：立即必须打断）：
/// id = 在途 prompt 命令 id；`interrupt` = 打断槽被取走的条目，被 pi 拒时
/// 放回槽位。`interrupt: None` = 普通队列 flush（队首仍留在队列里）。
#[derive(Debug, Clone)]
pub(crate) struct FlushInFlight {
    pub id: String,
    pub interrupt: Option<QueuedInput>,
}

/// 乐观上屏的待回显条目：文本 + 发送帧的钉顶决定。`pin` 随条目走到回显侧
/// （message_start user）——空闲普通发送 true；打断投递（立即/Ctrl+Enter）
/// false：上屏不钉顶，回显升级同样不翻页，否则「上屏不刷屏」会被回显补
/// 一刀，立即消息照旧把整屏顶出去（2026-10-20 三次验收根因）。
#[derive(Debug, Clone)]
pub(crate) struct PendingEcho {
    pub text: String,
    pub pin: bool,
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
    /// 「立即/插话」打断槽（2026-10-20 二次验收）：显式插入 = abort 当前
    /// 回复 + 条目进槽，本轮 aborted 的 AgentSettled 一到就由 flush 以新
    /// prompt 发出——模型立刻看到（此前走 pi steer 通道等注入点，纯文本
    /// 长回复没有边界，54s 故事讲完才被回应）。不渲染卡片：气泡即呈现。
    pub interrupt_pending: Option<QueuedInput>,
    /// 流已切断（abort 已发出）：管道里还在飞的旧轮残包（TextDelta/TextEnd/
    /// ThinkingDelta/ToolCallStart…）到达时，最后一条消息往往已经是乐观追
    /// 加的打断气泡（User）——`last_assistant` 若照常兜底推新 assistant 壳，
    /// 残包就在**气泡后面**长出旧内容，气泡视觉上「跳到 agent 输出上方」，
    /// 要等新轮事件归一才回位（2026-10-20 三次验收）。置位期间残包并入最后
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
            leaf_watch_in_flight: false,
            last_rpc_msg_count: None,
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
            pending_echoes: Vec::new(),
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
            bash_running: false,
            pending_bash: None,
            last_activity: std::time::Instant::now(),
            ext_queue: Vec::new(),

            pending_locate: None,

            nav_cache: None,
            nav_dirty: true,
            copy_flash: None,
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
            // full：内置 7 件（--tools）+ agent（045 subagent，开发旗标开着
            // 才点名——allowlist 指向不存在的注册工具没有意义）
            // + 精确集（-ne + 个人扩展 + 内置扩展）⇒ 插件一个不装 ⇒
            // 系统提示词零插件注入（034 §1.2）
            "full" => {
                extra.push("--tools".into());
                let tools = if crate::services::workspace::subagents_enabled() {
                    "bash,read,edit,write,grep,find,ls,agent"
                } else {
                    "bash,read,edit,write,grep,find,ls"
                };
                extra.push(tools.into());
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
        // 会话级扩展清单（031 session-ext.json；契约 v2.4/v3 pi_extensions
        // 的挂载通道）：**所有档位统一消费**——此前只有 custom 档读它，
        // 插件 ext 在 full/default 档下永远挂不上（-e 缺席，出码/收发无人
        // 执行）。已挂过的（custom 档经 ext_sources）不重复。
        if let Some(store) = pi_link::session_ext::store_path() {
            for src in pi_link::session_ext::read_for(&store, &self.key) {
                // 契约 v4：插件的 pi/*.mjs 是执行域入口，再当扩展挂载会整批 -e 失败
                if pi_link::service_ext::is_exec_entry(std::path::Path::new(&src)) {
                    continue;
                }
                if !self.ext_sources.iter().any(|e| e == &src) {
                    extra.push("-e".into());
                    extra.push(src);
                }
            }
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
        // 执行域挂载（90-6）：存在实例配置 → 挂宿主自带 shim + 交付配置指针。
        // 配置由 Chat 侧随插件装/卸同步（svc_sync_configs）；spawn 只消费。
        let mut envs: Vec<(String, String)> = Vec::new();
        if let Some(base) = pi_link::paths::dir() {
            if let Some(cfg) = pi_link::service_ext::config_path_if_any(&base, &self.key) {
                if cfg.is_file() {
                    match pi_link::service_ext::shim_path() {
                        Some(shim) => {
                            extra.push("-e".into());
                            extra.push(shim.to_string_lossy().into_owned());
                            // 信箱残留清零：上一轮已消费事件不得重放（至多一次）
                            if let Some(token) =
                                pi_link::service_ext::token_if_any(&base, &self.key)
                            {
                                pi_link::service_ext::reset_spools(&base, &token);
                            }
                            pi_link::service_ext::note_shim(
                                &base,
                                &format!("挂载 shim={} cfg={}", shim.display(), cfg.display()),
                            );
                            envs.push(("PF_SVC_CONFIG".into(), cfg.to_string_lossy().into_owned()));
                        }
                        None => {
                            eprintln!("[pf-svc] 本构建缺执行域 shim，service 落空（{}）", self.key);
                            pi_link::service_ext::note_shim(
                                &base,
                                &format!("shim 定位失败 key={}", self.key),
                            );
                        }
                    }
                }
            }
        }
        self.agent.spawn_with(&self.cwd, &extra, &envs)
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
            consume_runtime_events(weak, cx, rx, epoch).await;
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

// consume_runtime_events：RPC 事件泵（自由异步函数），epoch 守卫在此
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
            // 轮中死亡 = 外部错误：补全轮终语义。此前只有一条静默的
            // "pi exited" 状态行——标签栏三色点只认 TurnSettled，永不亮；
            // 半截工具卡永转；无提示音（用户实测：跑着跑着就没了，标签
            // 栏毫无动静）。
            let code = rt.exit_code.take();
            let was_running = rt.agent_running || rt.stream_started.is_some();
            if was_running {
                rt.turn_error = true;
                rt.fail_dangling_tool_calls();
            }
            rt.status = match code {
                Some(c) => crate::i18n::tf("status.pi_exited_code", &[("code", c.to_string())]),
                None => tr("status.pi_exited").to_string(),
            };
            rt.compacting = false;
            // 进程没了就不存在「在跑」：不就地复位，agent_running 与快照
            // is_streaming 会永久卡在 true——psp 状态槽的旋转圈永不灭
            //（用户实测：旧版还在跑的会话被掐掉后一直转）
            rt.agent_running = false;
            rt.phase_waiting = false;
            rt.streaming_content = false;
            // stream_started 闩交给 settle_turn 消费（有音开关才响；用户
            // 已 abort 则闩已清，静音）
            rt.settle_turn();
            if let Some(s) = rt.state.as_mut() {
                s.is_streaming = false;
            }
            // 死因留痕：stderr 尾部（node OOM / unhandled rejection /
            // 扩展 fatal 只出现在这里）
            let stderr = rt
                .agent
                .session
                .as_ref()
                .map(|s| s.stderr_tail())
                .unwrap_or_default();
            if !stderr.trim().is_empty() {
                eprintln!("pi stderr tail:\n{stderr}");
            }
            // 进程死了：文件落过盘就补绑定（真标签接管；重启后可重开），
            // 没落盘（boot 即死 / 全程无消息）撤伪标签
            if !rt.try_bind_reported_file(cx) {
                cx.emit(SessionEvent::PendingTab(None));
            }
            // 轮中死亡按轮终冒泡：订阅方给不在场的会话点红点（turn_error
            // 已置）。空闲死亡（闲置进程被系统清掉）不值得打扰，不冒泡。
            if was_running {
                cx.emit(SessionEvent::TurnSettled);
            }
            cx.emit(SessionEvent::Changed);
            cx.notify();
        }
    });
}

// ---------------------------------------------------------------------------
// migrated from Chat (main.rs) — session-local behavior
// ---------------------------------------------------------------------------

mod events;
mod ingest;
mod nav;
mod queue;
mod send;

#[cfg(test)]
mod turn_outcome_tests {
    //! 轮末结局（三色通知点）的定义锁：结局 = **最后一条 assistant 消息的
    //! stopReason**，由用户消息归零、由每条 assistant 消息覆盖。
    //!
    //! 两个真机回归（2026-10-12）：
    //! 1. pi 的 `agent_end.willRetry` 是乐观预判（先报 true，再看
    //!    `_prepareRetry` 决定是否真重试）。原先在这里清旗 → 退避期间被
    //!    中止/未重试时，错误结局被清掉 → 错误的轮次报青蓝完成点。
    //! 2. `toolResult.isError`（grep 未命中、命令非零退出）被当成轮末错误 →
    //!    正常完成的轮次报红点。
    use super::*;
    use gpui::TestAppContext;

    fn ev_start(role: &str, is_error: bool) -> Event {
        Event::MessageStart {
            role: role.into(),
            blocks: Vec::new(),
            timestamp: None,
            is_error,
            tool_call_id: None,
            custom_type: None,
            custom_display: true,
            details: None,
            raw_system: None,
        }
    }

    fn ev_end(role: &str, stop: &str, err: Option<&str>) -> Event {
        Event::MessageEnd {
            role: role.into(),
            blocks: Vec::new(),
            usage: None,
            timestamp: None,
            stop_reason: Some(stop.into()),
            error_message: err.map(str::to_string),
            is_error: false,
            tool_call_id: None,
            custom_type: None,
            custom_display: true,
            model: Some("m".into()),
        }
    }

    fn runtime(cx: &mut TestAppContext) -> gpui::Entity<SessionRuntime> {
        cx.new(|_| {
            let mut r = SessionRuntime::new("t".into(), PathBuf::from("C:\t"), None);
            r.sound_on = false;
            r
        })
    }

    fn feed(rt: &gpui::Entity<SessionRuntime>, cx: &mut TestAppContext, ev: Event) {
        rt.update(cx, |r, cx| r.on_event(ev, cx));
    }

    /// 真机回归 1：错误尝试之后 willRetry=true，但重试没跑起来（退避期间被
    /// 中止/新消息打断）→ 结算必须仍是「出错」，不能被乐观预判清成完成。
    #[gpui::test]
    fn error_survives_optimistic_will_retry(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        feed(&rt, cx, ev_start("user", false));
        feed(&rt, cx, ev_start("assistant", false));
        feed(
            &rt,
            cx,
            ev_end("assistant", "error", Some("Connection error.")),
        );
        assert!(rt.read_with(cx, |r, _| r.turn_error), "error 消息当即置红");
        feed(&rt, cx, Event::AgentEnd { will_retry: true });
        assert!(
            rt.read_with(cx, |r, _| r.turn_error),
            "乐观 willRetry 不得清掉已发生的错误结局"
        );
        feed(&rt, cx, Event::AgentSettled);
        assert!(rt.read_with(cx, |r, _| r.turn_error), "结算时仍是红档");
    }

    /// 重试真的成功：最后一条 assistant 的 stop 覆盖错误结局 → 青蓝。
    #[gpui::test]
    fn retry_success_overwrites_error(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        feed(&rt, cx, ev_start("user", false));
        feed(&rt, cx, ev_start("assistant", false));
        feed(
            &rt,
            cx,
            ev_end("assistant", "error", Some("Connection error.")),
        );
        // 重试：pi 会再发一次 agent_start + 一条成功的 assistant 消息
        feed(&rt, cx, Event::AgentStart);
        feed(&rt, cx, ev_start("assistant", false));
        feed(&rt, cx, ev_end("assistant", "stop", None));
        feed(&rt, cx, Event::AgentEnd { will_retry: false });
        feed(&rt, cx, Event::AgentSettled);
        assert!(!rt.read_with(cx, |r, _| r.turn_error));
        assert!(!rt.read_with(cx, |r, _| r.turn_interrupted));
    }

    /// pi-flash-7ep 回归：退避窗口轮**未**结束——agent_end{willRetry:true}
    /// 不得复位 agent_running（composer 停止按钮/输入框提示/等待行同源，
    /// 就地复位会在退避的 2-8s 里把「停止」翻回「发送」）；auto_retry_start
    /// 报倒计时；真轮末（willRetry=false + settled）才回落 idle。
    #[gpui::test]
    fn retry_backoff_keeps_running_state(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        feed(&rt, cx, Event::AgentStart);
        feed(&rt, cx, ev_start("assistant", false));
        feed(
            &rt,
            cx,
            ev_end("assistant", "error", Some("429: too many requests")),
        );
        feed(&rt, cx, Event::AgentEnd { will_retry: true });
        assert!(
            rt.read_with(cx, |r, _| r.agent_running),
            "退避窗口必须保持运行态，否则停止按钮翻回发送"
        );
        feed(
            &rt,
            cx,
            Event::AutoRetryStart {
                attempt: 1,
                max_attempts: 3,
                delay_ms: 2000,
                error_message: Some("429".into()),
            },
        );
        assert!(rt.read_with(cx, |r, _| r.agent_running));
        assert!(
            rt.read_with(cx, |r, _| r.status.contains("2s")),
            "状态行报退避倒计时，实际：{}",
            rt.read_with(cx, |r, _| r.status.clone())
        );
        // 重试跑起来并成功收尾
        feed(&rt, cx, Event::AgentStart);
        feed(&rt, cx, ev_start("assistant", false));
        feed(&rt, cx, ev_end("assistant", "stop", None));
        feed(
            &rt,
            cx,
            Event::AutoRetryEnd {
                success: true,
                attempt: 1,
                final_error: None,
            },
        );
        feed(&rt, cx, Event::AgentEnd { will_retry: false });
        feed(&rt, cx, Event::AgentSettled);
        assert!(!rt.read_with(cx, |r, _| r.agent_running), "真轮末回落 idle");
    }

    /// 真机回归 2：工具报错（isError）是工作过程，不是轮末结局。
    #[gpui::test]
    fn tool_error_is_not_a_turn_error(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        feed(&rt, cx, ev_start("user", false));
        feed(&rt, cx, ev_start("assistant", false));
        feed(&rt, cx, ev_start("toolResult", true));
        feed(&rt, cx, ev_end("toolResult", "stop", None));
        feed(&rt, cx, ev_start("assistant", false));
        feed(&rt, cx, ev_end("assistant", "stop", None));
        feed(&rt, cx, Event::AgentSettled);
        assert!(!rt.read_with(cx, |r, _| r.turn_error));
    }

    /// 黄色档：length/aborted 是内部中断，非外部错误。
    #[gpui::test]
    fn length_stop_is_warning_not_error(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        feed(&rt, cx, ev_start("user", false));
        feed(&rt, cx, ev_start("assistant", false));
        feed(&rt, cx, ev_end("assistant", "length", None));
        feed(&rt, cx, Event::AgentSettled);
        assert!(!rt.read_with(cx, |r, _| r.turn_error));
        assert!(rt.read_with(cx, |r, _| r.turn_interrupted));
    }

    /// 新一轮用户消息把上轮结局归零（上一轮的黄/红不得泄漏进下一轮）。
    #[gpui::test]
    fn new_user_turn_resets_outcome(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        feed(&rt, cx, ev_start("user", false));
        feed(&rt, cx, ev_start("assistant", false));
        feed(&rt, cx, ev_end("assistant", "error", Some("boom")));
        assert!(rt.read_with(cx, |r, _| r.turn_error));
        // 用户再发一条 → 归零（重试不发用户消息，所以重试不会误清）
        feed(&rt, cx, ev_start("user", false));
        assert!(!rt.read_with(cx, |r, _| r.turn_error));
        assert!(!rt.read_with(cx, |r, _| r.turn_interrupted));
    }

    /// 进程死亡收尾（pi-flash-7r0）：半截工具卡就地落错误态
    /// （红框 + 中断文案），已回结果的卡不动。
    #[gpui::test]
    fn process_death_closes_dangling_tool_calls(cx: &mut TestAppContext) {
        use pi_link::protocol::{AssistantEvent, Block};
        let rt = runtime(cx);
        feed(&rt, cx, ev_start("assistant", false));
        feed(
            &rt,
            cx,
            Event::MessageUpdate(AssistantEvent::ToolCallStart {
                content_index: 0,
                id: "a".into(),
                tool_name: "bash".into(),
            }),
        );
        feed(
            &rt,
            cx,
            Event::MessageUpdate(AssistantEvent::ToolCallStart {
                content_index: 1,
                id: "b".into(),
                tool_name: "read".into(),
            }),
        );
        // 卡 B 先回了结果：进程死时不得被改写
        rt.update(cx, |r, _| {
            if let Block::ToolCall {
                result,
                result_arrived,
                ..
            } = &mut r.messages[0].blocks[1]
            {
                *result = "ok".into();
                *result_arrived = true;
            }
        });
        rt.update(cx, |r, _| r.fail_dangling_tool_calls());
        rt.read_with(cx, |r, _| {
            let blocks = &r.messages[0].blocks;
            let Block::ToolCall {
                result: ra,
                is_error: ea,
                args_partial: pa,
                result_arrived: va,
                ..
            } = &blocks[0]
            else {
                panic!("block 0 not toolcall")
            };
            assert!(*va && *ea && !*pa, "半截卡落错误态");
            assert!(!ra.is_empty(), "中断文案已填");
            let Block::ToolCall {
                result: rb,
                is_error: eb,
                result_arrived: vb,
                ..
            } = &blocks[1]
            else {
                panic!("block 1 not toolcall")
            };
            assert_eq!(rb, "ok", "已回结果的卡不动");
            assert!(*vb && !*eb);
        });
    }

    /// ProcessExited 存退出码；Unparsed 按 type 去重留痕。
    #[gpui::test]
    fn exit_code_and_unparsed_dedup(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        feed(&rt, cx, Event::ProcessExited { code: Some(1) });
        feed(
            &rt,
            cx,
            Event::Unparsed(serde_json::json!({"type":"mystery"})),
        );
        feed(
            &rt,
            cx,
            Event::Unparsed(serde_json::json!({"type":"mystery"})),
        );
        feed(
            &rt,
            cx,
            Event::Unparsed(serde_json::json!({"type":"other"})),
        );
        rt.read_with(cx, |r, _| {
            assert_eq!(r.exit_code, Some(1));
            assert_eq!(r.unparsed_types.len(), 2, "同 type 只记一次");
        });
    }
}

#[cfg(test)]
mod binding_guard_tests {
    //! 幻影绑定守卫 + 绑定对账（伪标签前置）：pi 在 SessionManager 构造期
    //! 就报 sessionFile，文件本体要等首条 user/assistant 消息才落盘
    //! （_hasConversation/_persist，#10000）。没落盘不得绑定——boot 草稿保
    //! 持 draft 身份、recents/last-open 不记幻影路径；落盘了才迁移。

    use super::*;
    use gpui::TestAppContext;

    fn runtime(cx: &mut TestAppContext) -> gpui::Entity<SessionRuntime> {
        cx.new(|_| {
            let mut r = SessionRuntime::new("draft-t".into(), PathBuf::from("C:/t"), None);
            r.sound_on = false;
            r
        })
    }

    /// follow_session_file 存在性守卫：幻影路径不绑定，已落盘文件正常绑定。
    #[gpui::test]
    fn follow_requires_existing_file(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        rt.update(cx, |r, cx| {
            r.follow_session_file(PathBuf::from("Z:/pf-not-landed/x.jsonl"), cx);
            assert!(r.file.is_none(), "幻影路径不得绑定");
        });
        let real = std::env::temp_dir().join(format!("pf-guard-{}.jsonl", std::process::id()));
        std::fs::write(&real, "{}\n").unwrap();
        rt.update(cx, |r, cx| {
            r.follow_session_file(real.clone(), cx);
            assert_eq!(
                r.file.as_deref(),
                Some(real.as_path()),
                "已落盘文件正常绑定"
            );
        });
        let _ = std::fs::remove_file(&real);
    }

    /// 绑定对账三态：state 空 → GetState 兜底返回 true；报过未落盘 →
    /// false（调用方撤伪标签）；已落盘 → 绑定 + true。
    #[gpui::test]
    fn try_bind_reported_file_states(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        rt.update(cx, |r, cx| {
            assert!(
                r.try_bind_reported_file(cx),
                "state 空 = 绑定在途，伪标签先留着"
            );
            assert!(r.file.is_none());
        });
        rt.update(cx, |r, cx| {
            r.state = Some(SessionState {
                session_file: Some("Z:/pf-not-landed/x.jsonl".into()),
                ..Default::default()
            });
            assert!(
                !r.try_bind_reported_file(cx),
                "未落盘的报备文件不得视为已绑定"
            );
            assert!(r.file.is_none());
        });
        let real = std::env::temp_dir().join(format!("pf-guard-bind-{}.jsonl", std::process::id()));
        std::fs::write(&real, "{}\n").unwrap();
        rt.update(cx, |r, cx| {
            r.state = Some(SessionState {
                session_file: Some(real.to_string_lossy().into_owned()),
                ..Default::default()
            });
            assert!(r.try_bind_reported_file(cx));
            assert_eq!(r.file.as_deref(), Some(real.as_path()));
        });
        let _ = std::fs::remove_file(&real);
    }
}

#[cfg(test)]
mod pin_regression_tests {
    //! 发言钉顶回归锁（2026-10-20）：空闲普通发送 = 发送帧 + 回显双钉顶；
    //! 打断投递（立即/Ctrl+Enter）全程不钉（上屏与回显同决）；
    //! stream_cut 残包并入旧 assistant，不得在打断气泡后长出新壳。

    use super::*;
    use gpui::TestAppContext;

    fn runtime(cx: &mut TestAppContext) -> gpui::Entity<SessionRuntime> {
        cx.new(|_| {
            let mut r = SessionRuntime::new("pin-t".into(), PathBuf::from("C:/t"), None);
            r.sound_on = false;
            r
        })
    }

    fn feed(rt: &gpui::Entity<SessionRuntime>, cx: &mut TestAppContext, ev: Event) {
        rt.update(cx, |r, cx| r.on_event(ev, cx));
    }

    fn msg_start(role: &str, text: &str) -> Event {
        Event::MessageStart {
            role: role.into(),
            blocks: vec![pi_link::protocol::Block::Text {
                content_index: 0,
                text: text.into(),
            }],
            timestamp: None,
            is_error: false,
            tool_call_id: None,
            custom_type: None,
            custom_display: true,
            details: None,
            raw_system: None,
        }
    }

    fn text_delta(delta: &str) -> Event {
        Event::MessageUpdate(pi_link::protocol::AssistantEvent::TextDelta {
            content_index: 0,
            delta: delta.into(),
        })
    }

    /// 普通发送：发送帧 page_turn，回显按发送帧的 pin=true 再钉（不回退）。
    #[gpui::test]
    fn normal_send_echo_keeps_pin(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        rt.update(cx, |r, cx| {
            r.optimistic_send("你好".into(), true, cx);
            assert_eq!(r.pager.anchor_ix(), Some(0), "发送帧钉顶");
            assert!(
                r.pending_echoes.last().is_some_and(|p| p.pin),
                "普通发送 pin=true 随条目入列"
            );
        });
        feed(&rt, cx, msg_start("user", "你好"));
        rt.read_with(cx, |r, _| {
            assert_eq!(r.messages.len(), 1, "回显就地升级，不重复推气泡");
            assert!(r.pending_echoes.is_empty());
            assert_eq!(r.pager.anchor_ix(), Some(0), "回显按 pin=true 继续钉顶");
        });
    }

    /// 空闲投递（steer 档 Enter / 空闲点立即）= **普通发送**：agent 没在
    /// 说话无所谓插话，照常发言钉顶，回显继续钉（2026-10-20 验收回归）。
    #[gpui::test]
    fn idle_delivery_pins_like_normal_send(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        rt.update(cx, |r, cx| {
            r.queued_inputs.push(QueuedInput {
                text: "插队".into(),
                images: Vec::new(),
                remote: false,
            });
            r.queue_send_now(0, cx);
            assert_eq!(
                r.pager.anchor_ix(),
                Some(0),
                "空闲投递 = 普通发送，照常钉顶"
            );
            assert_eq!(r.messages.len(), 1);
            assert!(r.interrupt_pending.is_some());
            assert!(!r.stream_cut, "空闲打断无 abort，不置切断标记");
        });
        feed(&rt, cx, msg_start("user", "插队"));
        rt.read_with(cx, |r, _| {
            assert_eq!(r.messages.len(), 1, "回显就地升级");
            assert_eq!(r.pager.anchor_ix(), Some(0), "回显继续钉顶");
        });
    }

    /// 运行中打断 + abort 在途残包：残包并入旧 assistant（气泡上方收尾），
    /// 不得在气泡后长新壳；回显升级不钉顶。
    #[gpui::test]
    fn stale_deltas_merge_into_old_assistant(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        rt.update(cx, |r, cx| {
            r.on_event(Event::AgentStart, cx);
            r.on_event(msg_start("assistant", ""), cx);
            r.on_event(text_delta("旧"), cx);
        });
        rt.update(cx, |r, cx| {
            r.queued_inputs.push(QueuedInput {
                text: "插".into(),
                images: Vec::new(),
                remote: false,
            });
            r.queue_send_now(0, cx);
            assert!(r.stream_cut, "运行中打断置切断标记");
            assert_eq!(r.pager.anchor_ix(), None, "运行中打断不钉顶（跟流不刷屏）");
            assert_eq!(r.messages.len(), 2, "assistant + 打断气泡");
            assert_eq!(
                r.messages[1].role,
                crate::session::messages::Role::User,
                "气泡在末尾"
            );
        });
        // abort 前入管道的残包此刻才到
        rt.update(cx, |r, cx| {
            r.on_event(text_delta("残包"), cx);
        });
        rt.read_with(cx, |r, _| {
            assert_eq!(r.messages.len(), 2, "残包不得在气泡后长出新 assistant");
            let merged = match r.messages[0].blocks.first() {
                Some(pi_link::protocol::Block::Text { text, .. }) => text.clone(),
                other => panic!("expected text block, got {other:?}"),
            };
            assert!(
                merged.contains("旧") && merged.contains("残包"),
                "残包并入旧 assistant：{merged}"
            );
            assert_eq!(
                r.messages[1].role,
                crate::session::messages::Role::User,
                "气泡保持在末尾"
            );
        });
        // abort 收尾 → AgentSettled（清切断标记 + flush 槽内条目）→ 新轮回显
        feed(&rt, cx, Event::AgentSettled);
        rt.read_with(cx, |r, _| {
            assert!(!r.stream_cut, "结算清除切断标记");
        });
        feed(&rt, cx, msg_start("user", "插"));
        rt.read_with(cx, |r, _| {
            assert_eq!(r.messages.len(), 2, "回显升级打断气泡");
            assert_eq!(r.pager.anchor_ix(), None, "打断回显不钉顶");
        });
    }

    /// 用户报障回归锁（2026-10-21）：首条消息已钉顶、agent 思考中再发一条
    /// steer——打断投递不得退役钉顶锚点。钉顶期（内容还短于一屏）退役 = 摘
    /// 垫片，Bottom 胶水把整块内容拽到屏底（「所有消息落到屏幕下半部分」）。
    /// 几何锁见 chat_list.rs `interrupt_while_pinned_keeps_position`。
    #[gpui::test]
    fn steer_while_pinned_keeps_anchor(cx: &mut TestAppContext) {
        let rt = runtime(cx);
        rt.update(cx, |r, cx| {
            r.optimistic_send("第一条".into(), true, cx);
            assert_eq!(r.pager.anchor_ix(), Some(0), "首条照常钉顶");
            r.agent_running = true;
            r.input = "第二条".into();
            r.steer_input(cx);
        });
        rt.read_with(cx, |r, _| {
            assert_eq!(
                r.pager.anchor_ix(),
                Some(0),
                "钉顶期打断不退役锚点（昵了就落到屏幕下半部）"
            );
            assert_eq!(r.messages.len(), 2, "插话气泡接在下面");
            assert_eq!(r.messages[1].role, crate::session::messages::Role::User);
            assert!(r.interrupt_pending.is_some(), "打断槽持有本条");
        });
    }
}
