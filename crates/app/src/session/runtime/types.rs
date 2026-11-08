//! runtime 域共享类型：冒泡事件（SessionEvent）与队列/回显/导航缓存
//! 条目。实体本体在 mod.rs，这些类型由 mod.rs `pub(crate) use` 原路径
//! 转发（runtime::SessionEvent 等外部路径不变）。

use std::path::PathBuf;

/// Shell-level effects a runtime bubbles up (Chat subscribes).
pub(crate) enum SessionEvent {
    /// repaint-worthy state change
    Changed,
    /// 用户可见的瞬时提示（status_toast 通道）：runtime 层发生的用户动作
    /// 终局（compact 完成/失败等）——r.status 是无渲染死字段，用户可见
    /// 反馈只有 Chat::set_status 的 toast（真机验收实踩）
    Toast(String),
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
    /// pi 进程死亡（泵尾通道关闭分支）：在途扩展弹窗/问卷卡指向死管道，
    /// Chat 侧须 ext_cancel_all 清理（zi2s.3）。后台会话不弹窗也就无弹窗可
    /// 清，订阅侧按 is_active 闸掉。
    ExtUiDied,
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

/// 「立即/插话」的打断投递在途关联：
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
/// 一刀，立即消息照旧把整屏顶出去。
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
    /// 按轮子代理状态点（第 n 条 User 消息 = 第 n 轮）：导航卡第三行数据源。
    /// 与 turns 同一次构建、同一个缓存——渲染层不允许另存一份（两份 = 两个渲染态）。
    pub turn_agents: Vec<Vec<crate::session::subagent_panel::DotTone>>,
    pub users: usize,
    pub assistants: usize,
    pub tool_calls: usize,
}
