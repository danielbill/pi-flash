//! pi-flash — desktop shell for the pi coding agent.
//!
//! v54 shell: topbar 两段（左=收放钮 chrome / 右=内容 tabs+设置+窗口控制）·
//! psp 项目+会话一体列表 · 内容区 chat/term/md 状态机 · statusbar 仅面板段。
//! Layout values come from docs/UI设计/主界面UI设计-2.html (mist tokens).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use futures::StreamExt;
use gpui::{
    actions, App, Application, Context, FocusHandle, Focusable, KeyBinding, KeyDownEvent,
    MouseButton, ParentElement,
    Render, SharedString, Styled, WindowOptions, div, prelude::*, px, rgb,
};
use pi_link::protocol::Command;
use pi_link::sessions::{SessionInfo, list_sessions_for_cwd, read_tail_messages};

mod actions_dialogs;
mod agent_session;
mod automation;
mod actions_menu;
mod actions_panels;
mod actions_rename;
mod actions_runtime;
mod actions_sessions;
mod actions_terminal;
mod appearance;
mod assets;
mod content;
mod dialogs;
mod editor;
mod ext_ui;
mod ext_ui_actions;
mod function_panel;
mod i18n;
mod models_config;
mod remote_control;
mod startup;
mod theme;
mod services;
mod session;
mod settings;
mod status_bar;
mod titlebar;
mod terminal;
mod top_panels;
mod ui;
pub(crate) use ui::ComposerInput;
pub(crate) use actions_menu::slash_menu_view;

// composer 覆盖动作（注册为 "Input" 上下文绑定，见 run() 里 bind_keys）：
// ↑/↓ 在菜单态导航补全、空输入态回溯历史，非空多行重新派发组件 MoveUp/
// MoveDown；Tab 在菜单态接受补全；Ctrl+V 截获图片粘贴（composer 附件化，
// 其余输入框经根节点兜底重派发组件 Paste）。组件默认的这些键由此被截获。
actions!(app, [ComposerUp, ComposerDown, ComposerTab, ComposerPaste, FileSave]);
use i18n::tr;
use models_config::EnabledState;
use theme::theme as T;
use services::format::*;
use services::git::*;
use services::workspace::*;
use session::messages::{Msg, msgs_from_tail};
use session::runtime::SessionRuntime;
use terminal::{TermStatus, TerminalTab};
use ui::TextInput;
use ui::icon;
use ext_ui::render_ext_dialog;

static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static PERF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------

/// Rows rendered in the model picker (and the keyboard-selection range);
/// further matches are reachable by typing to filter.
pub(crate) const MODEL_PICKER_ROWS: usize = 12;

/// 扩展页后台 CLI op（040）：转圈落点不同——Install 在安装按钮，
/// Remove 在对应包的垃圾桶上；op 泵收到完成消息即清零。
pub(crate) enum PkgOp {
    Install,
    Remove(String),
}

#[derive(Debug, Clone)]
enum Dialog {
    /// 042：新增 Provider 清单弹窗（两组：自定义 / API KEY；订阅组缓行）
    ProviderPicker { input: gpui::Entity<TextInput> },
    GitDiff { path: PathBuf, patch: String },
    SessionSearch { input: gpui::Entity<TextInput> },
    /// 004 projectManager 打开项目菜单：搜索框 + 打开文件夹 + 最近 30 天
    /// 项目列表（列表数据在 `project_hits`，扫描完异步回填）。`fresh` =
    /// 新会话页来源：选项目落**全新草稿**（new_session_in，不恢复
    /// last_open）；psp 来源保持切项目恢复上次会话的既定行为。`scroll`
    /// 挂列表自绘滚动条（须跨帧复用，vlist.rs「血案」注释）。
    ProjectPicker {
        input: gpui::Entity<TextInput>,
        fresh: bool,
        scroll: gpui::ScrollHandle,
    },
    /// composer 缩略图点击大图预览：直接持渲染源（Arc 指针拷贝，无索引
    /// 失效问题）
    ImagePreview { image: std::sync::Arc<gpui::Image> },
    /// topbar ⋯ 菜单：系统提示词 / 工具定义（窗体 = 设置弹窗那套大卡片，
    /// 见 top_panels / ui::overlay::big_card）
    SessionInfo { kind: TopPanel },
    /// 023 fileView：关闭带未保存修改的文件 tab 前确认。
    /// 持 path 不持 ix——弹窗存活期间的增删不会让索引漂移。
    FileDirty { path: PathBuf },
    /// 060 远程控制：扫码弹窗（二维码 + 绑定码）。topbar 手机图标与设置
    /// 「远程控制」页签入口已隐藏（2026-10-10 定），自动化 WX_QR_OPEN 仍可打开。
    /// 不持数据 —— 内容全从 `chat.remote.qr` 现读，泵更新即重绘。
    WxQr,
}

/// ⋯ 菜单的两个面：系统提示词原文 / 已加载工具的声明。各自画在设置弹窗
/// 同款窗体里（`Dialog::SessionInfo`），数据来自 transcript 重放。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TopPanel {
    System,
    Tools,
}

/// functionPanel active view (statusbar 三 tab): mutually exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DockPanel {
    Sessions,
    Files,
    Git,
}

impl DockPanel {
    fn as_str(&self) -> &'static str {
        match self {
            DockPanel::Sessions => "sessions",
            DockPanel::Files => "files",
            DockPanel::Git => "git",
        }
    }
}

/// 内容区视图状态机 (v54): chat 默认；terminal / markdown 预览以 topbar tab 打开。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentView {
    Chat,
    Term,
    /// 文件查看 tab（PanelTab::File；md/html 渲染、其余源码）
    File,
}

/// psp 列表方式（⋯ 菜单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ListMode {
    Grouped,
    Flat,
}

/// psp 排序方式（⋯ 菜单；手动=初始序，真拖拽待实现）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortMode {
    Time,
    Manual,
}

/// One project group of the psp (sessions sorted mtime desc at build).
#[derive(Debug, Clone)]
pub(crate) struct ProjectGroup {
    pub name: String,
    pub path: PathBuf,
    pub sessions: Vec<SessionInfo>,
}

/// 004 打开项目菜单的一行：项目名（目录名）+ 路径。
#[derive(Debug, Clone)]
pub(crate) struct ProjectEntry {
    pub name: String,
    pub path: PathBuf,
}

/// 会话 hover 详情卡状态（300ms 离行宽限 + 进卡取消隐藏）。
#[derive(Debug, Clone)]
pub(crate) struct HoverCard {
    pub path: PathBuf,
    pub y: f32,
    pub hide_at: Option<std::time::Instant>,
    /// 创建时刻：悬停满 300ms 才显示（快速滑过列表不弹卡、不干扰行悬停）
    pub show_at: std::time::Instant,
    /// show_at 到期后由 120ms tick 置 true 并渲染一次
    pub shown: bool,
    pub confirming: bool,
    /// 鼠标当前是否在卡上（gpui 的行退出/卡进入事件顺序不保证，
    /// 行退出只在 !card_hovered 时才启动消失宽限）
    pub card_hovered: bool,
    /// 标题点击后原地改名（设计稿：卡内变输入框，Enter 提交 / Esc 取消）
    pub renaming: bool,
    pub rename_input: Option<gpui::Entity<crate::ui::TextInput>>,
}

/// psp 菜单（⋯ 排序两级 / 项目菜单）；(x, y) 为事件坐标锚点。
#[derive(Debug, Clone)]
pub(crate) enum PspMenu {
    /// sub: 0=列表方式, 1=排序方式
    Sort { sub: Option<u8>, x: f32, y: f32 },
    Project { path: PathBuf, x: f32, y: f32 },
}

#[derive(Debug, Clone)]
struct AttachedImage {
    data_b64: String,
    mime: String,
    /// 缩略图渲染源（附加时预构建）：img() 按 Image id 缓存解码，避免
    /// 逐帧重哈希/重解码大图
    thumb: Option<std::sync::Arc<gpui::Image>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MenuKind {
    Slash,
    At,
}

#[derive(Debug, Clone)]
struct MenuItem {
    insert: String,
    desc: String,
    /// @ 菜单条目的目录形态（决定插入文本 `@dir/` 不闭合 + 图标）
    is_dir: bool,
}

/// @ 文件索引的每-cwd 缓存条目（pi-web file-index route.ts cache parity：
/// 后台构建、每 cwd 一份；TTL/容量可配置——services::workspace 的
/// `at_index_ttl_secs`（默认 60s）/ `at_index_max_projects`（默认 10））。
/// `files` = git ls-files 原始清单；`entries` = 派生条目（目录 + 文件，
/// 浅层优先）——菜单打分输入。
#[derive(Clone, Default)]
struct AtIndexState {
    built: Option<std::time::Instant>,
    files: std::sync::Arc<Vec<String>>,
    entries: std::sync::Arc<Vec<crate::services::at_file::FileEntry>>,
    building: bool,
}

/// @ 菜单过滤记忆化条目：entries 的 Arc 身份 + 查询词 + 上次过滤结果。
/// 索引重建整体换新 Arc → `Arc::ptr_eq` 失配自然失效，无需手动清。
type AtFilterCache = (
    std::sync::Arc<Vec<crate::services::at_file::FileEntry>>,
    String,
    Vec<crate::services::at_file::FileEntry>,
);

/// 042：provider 通路探查状态（保存 key 自动一次 + 详情页【重新探查】）。
#[derive(Debug, Clone)]
pub(crate) enum McProbe {
    Running,
    Done(pi_link::probe::ProbeOutcome),
}

struct Chat {
    focus: FocusHandle,
    dialog_focus: FocusHandle,
    dialog: Option<Dialog>,
    dock_panel: DockPanel,
    input: String,
    pending_images: Vec<AttachedImage>,
    history: Vec<String>,
    history_ix: Option<usize>,
    session_tail_cache: std::collections::HashMap<PathBuf, Vec<Msg>>,
    sessions: Vec<SessionInfo>,
    cwd: PathBuf,
    branch: String,
    /// model catalog keyed by cwd — **进程答案覆盖层**（010-启动.md §7）：活着的
    /// runtime 的 RPC `get_available_models` 答复写这里（项目级差异），无进程时
    /// 消费方回落 `globals.models`（启动时从磁盘 ∪ 自有缓存装载）。
    models_by_cwd: std::collections::HashMap<String, Vec<pi_link::protocol::ModelInfo>>,
    /// 启动装载的全局态（模型清单 / 命令 / 默认项 / 插件 / 全局 mcp）：渲染只读，
    /// 不再扫盘也不发 RPC（010-启动.md §1、§7）。
    globals: startup::Globals,
    /// 项目上下文（启动集合内逐项目装载；切项目命中即零扫盘，见 §5）。
    project_ctx: std::collections::HashMap<String, startup::ProjectCtx>,
    runtimes: std::collections::HashMap<String, gpui::Entity<session::runtime::SessionRuntime>>,
    active_key: String,
    /// 010-启动：启动页闸门；揭幕帧由 pending_zoom 补 §4 最大化
    booted: bool,
    pending_zoom: bool,
    /// 040 延迟加载：扩展/技能说明 token 数（source → 测量值；启动时读缓存，
    /// 揭幕后后台探针刷新）。source 缺项 = 尚未测得（UI 显示「—」）
    ext_tokens: std::collections::HashMap<String, services::token_probe::TokenEntry>,
    draft_seq: usize,
    menu_ix: usize,
    /// / 菜单滚动句柄（按键选中 scroll_to_item 行跟随；输入变化回顶）
    menu_scroll: gpui::ScrollHandle,
    /// 斜杠/@ 菜单被点外收起（active_menu 据此返回 None；改输入即复位）
    menu_dismissed: bool,
    term_events: Option<futures::channel::mpsc::UnboundedSender<(usize, alacritty_terminal::event::Event)>>,
    op_tx: Option<futures::channel::mpsc::UnboundedSender<String>>,
    // editor view state（输入组件实体在 composer 首次渲染时惰性创建；
    // 真输入框 = gpui-component InputState，光标/选区/IME/滚动条全内置）
    composer: Option<gpui::Entity<ComposerInput>>,
    /// 技能展开消息的展开态（key = entry_id 或序号键）
    expanded_skills: std::collections::HashSet<String>,
    /// 消息气泡滚动句柄（key 同 expanded_skills；跨帧保持滚动位置并供
    /// gpui-component Scrollbar 读取）
    bubble_scrolls: std::rc::Rc<std::cell::RefCell<
        std::collections::HashMap<String, gpui::ScrollHandle>,
    >>,
    /// 操作栏悬停态（pi-web onMouseEnter/Leave parity，状态驱动而非
    /// group_hover）：用户消息行与 agent 轮块**共用**这一个字段，值是行在
    /// 列表里的索引（用户行 = msg_ix，agent 轮 = 轮首 msg_ix；角色不同故
    /// 永不撞车）。唯一写入点 = messages::bar_hover_wired。
    pub bar_hover: Option<usize>,
    input_focused: bool,
    // inline rename (active session)
    renaming: Option<PathBuf>,
    rename_input: Option<gpui::Entity<TextInput>>,
    confirm_delete: Option<PathBuf>,
    // session content search (013): dialog query input + background results
    search_hits: Vec<pi_link::sessions::SearchHit>,
    search_truncated: bool,
    search_running: bool,
    search_needle: String,
    search_gen: u64,
    pending_locate: Option<(PathBuf, Option<i64>, String)>,
    // shell surfaces
    pill_menu: Option<PillMenu>,
    /// 「full+」档的扩展选择面板（勾选即生效；滚动位跨帧复用）
    plugin_picker: Option<crate::session::plugin_picker::PluginPicker>,
    /// MCP 勾选菜单（043：全局 mcp.json 无条目时按钮不渲染，菜单自然不开）
    mcp_picker: Option<crate::session::mcp_picker::McpPicker>,
    /// 打开弹窗的那个按钮的锚点（水平中点 + 顶缘）——inputpanel 全部弹窗
    /// 统一定位：与按钮居中、距 5px、不遮按钮（用户定稿）
    pill_anchor: Option<PillAnchor>,
    /// 三个胶囊按钮的 paint 期 bounds（canvas 捕获，跨帧槽位）
    pill_btn: PillBtns,
    /// ctx-ring 悬浮详情（v58 响应式）：环/面板两面悬停标志 + 淡出起始
    /// 时刻（input.rs ctx_tip_hover 状态机持有）
    ctx_tip_ring_hover: bool,
    ctx_tip_panel_hover: bool,
    ctx_tip_closing: Option<std::time::Instant>,
    // git panel
    git_files: Vec<GitFile>,
    git_add_del: (u64, u64),
    git_selected: Option<PathBuf>,
    git_tab: function_panel::git_panel::GitTab,
    git_log: Vec<GitCommit>,
    git_error: Option<String>,
    git_commit_input: gpui::Entity<TextInput>,
    // workspace / project files
    project_files: Vec<String>,
    /// @ 文件索引缓存（key = cwd 字符串；031 输入面板）
    at_index: std::collections::HashMap<String, AtIndexState>,
    /// @ 菜单过滤结果记忆化（menu_items 渲染期是 &self，走 RefCell 内部
    /// 可变性，先例 bubble_scrolls；见 AtFilterCache）
    at_filter_cache: std::cell::RefCell<Option<AtFilterCache>>,
    expanded_dirs: HashSet<PathBuf>,
    /// 文件树展平缓存（services::file_tree::flatten；渲染只读这份，
    /// 重建点 = 展开/折叠、git 刷新、fs 事件、切项目）。
    tree_rows: std::sync::Arc<Vec<services::file_tree::TreeRow>>,
    file_cache: std::collections::HashMap<PathBuf, FileTab>,
    // fs watch（services::watcher）：tx 永驻（每次挂 watch clone 一份），
    // rx 被 startup 的泵取走；watcher 句柄 drop 即解除监听。
    fs_watch_tx: std::sync::mpsc::Sender<()>,
    fs_watch_rx: Option<std::sync::mpsc::Receiver<()>>,
    fs_watch: Option<services::watcher::FsWatcher>,
    /// 023：**cwd 外**打开文件的单文件监听（key = 文件所在目录，非递归）。
    /// Zed 对不在任何 worktree 里的文件建 single-file worktree（只监听其
    /// 所在目录）同款；事件走同一条 fs 通道。打开/关闭/切项目后由
    /// `retain_file_watches` 重算，不再被需要的目录即卸载。
    file_watches: std::collections::HashMap<PathBuf, services::watcher::FsWatcher>,
    // terminals (content-area tabs)
    terminals: Vec<TerminalTab>,
    active_terminal: Option<usize>,
    term_seq: usize,
    /// caret-blink heartbeat phase (1Hz, term_cursor task) + paint-time
    /// "a focused terminal is on screen" flag that gates the heartbeat
    term_cursor_on: bool,
    term_cursor_focused: bool,
    panel_tabs: Vec<PanelTab>,
    active_panel_tab: Option<usize>,
    // 023 fileView：菜单与面包屑兄弟菜单的弹层状态
    crumb_menu_dir: Option<PathBuf>,
    crumb_dd: gpui::Entity<crate::ui::DropdownState>,
    // TEMP 探针（023 调试）：外部改动检测 运行数/命中数
    ext_probe: (u32, u32),
    /// 023：打开文件后待聚焦的编辑器（Zed 行为：开文件即聚焦；渲染帧
    /// 编辑器实体就绪后消费）
    pending_focus_file: Option<PathBuf>,
    // settings panel data
    mc_patterns: Option<Vec<String>>,
    mc_state: EnabledState,
    mc_creds: Vec<(String, pi_link::config::CredentialKind)>,
    mc_project_scope: bool,
    /// settings.json defaults a new session starts with (pi-web /api/models
    /// `defaultModel` + `defaultThinkingLevel`; selectInitialModelScope inputs)
    mc_default_model: Option<(String, String)>,
    mc_default_thinking: Option<String>,
    /// settings.modelThinkingLevels — per-`provider/modelId` recorded levels
    mc_model_thinking: Vec<(String, String)>,
    mc_skills: Vec<pi_link::skills::SkillEntry>,
    mc_pkgs_global: Vec<serde_json::Value>,
    mc_pkgs_project: Vec<serde_json::Value>,
    /// 扩展页 CLI op（install/remove）进行中：转圈按 op 种类落按钮；op 泵
    /// 收到完成消息即清零（泵同时重读 globals + 刷新列表面板）
    pkg_op: Option<PkgOp>,
    /// 卸载确认浮层（040：点垃圾桶先确认）：待卸载来源
    pkg_confirm_remove: Option<String>,
    mc_default_tools: Option<Vec<String>>,
    /// models.json 编辑缓冲（设置·模型页；表单头保存即写盘，无页面级 footer）
    mc_models_json: serde_json::Value,
    mc_mj_error: Option<String>,
    /// 042：provider 通路探查状态（保存 key 后自动一次 + 详情页手动重试）
    mc_probe: std::collections::HashMap<String, McProbe>,
    /// 042 新增 Provider 弹窗的全量注册表（registry.rs dump；None = 未加载）
    provider_registry: Option<std::sync::Arc<pi_link::registry::RegistryDump>>,
    /// dump 拉取状态（Idle = 待拉/可拉，Loading = 后台进行中，Failed = 上次失败）
    registry_state: crate::session::model_picker::RegistryState,
    /// 042：inputpanel 模型菜单（锚定 pill 卡片，替代原居中 ModelSelect 弹窗）
    model_picker: Option<crate::session::model_picker::ModelPicker>,
    /// mcp.json 全局 + 项目服务器（设置·MCP 页）
    mcp_servers: Vec<pi_link::mcp::ServerEntry>,
    mcp_errors: Vec<String>,
    // extension UI surface (active session)
    ext_status: Vec<(String, String)>,
    ext_widgets: Vec<(String, Vec<String>, bool)>,
    ext_dialog: Option<pi_link::protocol::ExtensionUiRequest>,
    ext_input: gpui::Entity<TextInput>,
    ext_notice: Option<(String, u8)>,
    settings: Option<gpui::Entity<settings::SettingsPanel>>,
    // ---- v54 shell state ----
    /// psp 项目组（当前项目钉顶，其余按最近会话倒序；启动只加载
    /// 设置.默认加载会话数 N 个会话，组由这批会话的 cwd 自然形成）
    projects: Vec<ProjectGroup>,
    /// 004 打开项目菜单：最近 30 天活动项目（弹窗打开时后台扫描回填）
    project_hits: Vec<ProjectEntry>,
    /// 004 打开项目菜单的搜索词（输入框 on_change 镜像）
    project_filter: String,
    /// 当前活跃会话文件（psp 选中态；switch_to 时更新）
    active_file: Option<PathBuf>,
    list_mode: ListMode,
    sort_mode: SortMode,
    collapsed_keys: HashSet<String>,
    /// psp 分页：每组已展开的会话数（「显示更多」每次 +10；
    /// key = 组 ws_key，Flat 模式 = "__flat__"）
    psp_shown: std::collections::HashMap<String, usize>,
    slp_w: f32,
    panes_hidden: bool,
    slp_drag: Option<(f32, f32)>,
    /// 分隔线响应区悬停（线加深加粗 + col_resize 光标已由 resizer 承担）
    slp_hover: bool,
    content_view: ContentView,
    /// 浏览操作区的最后视图（Term/File）：文件树标签点击时恢复
    browse_last: ContentView,
    file_scrollbar: gpui_component::scroll::ScrollbarState,
    /// 文件视图块级虚拟化（抄 zed thread_view 的 list 架构）：md 渲染预览用，
    /// ListState 每帧只建可视块；path 记当前文件，换文件时 reset 归零滚动
    file_view_list: gpui::ListState,
    file_view_list_path: Option<PathBuf>,
    /// psp 会话列表滚动（滚动条数据源）
    psp_scroll: gpui::ScrollHandle,
    psp_sb_state: crate::ui::psp_scrollbar::PspScrollbarState,
    nav_open: bool,
    nav_hide_at: Option<std::time::Instant>,
    nav_flyout_hovered: bool,
    /// flyout 内鼠标所在轮（选择框/比例尺亮点跟随鼠标）
    nav_hover_turn: Option<usize>,
    /// 导航 flyout 卡片列表虚拟化句柄（v63-6：只建可视卡片；跨开合保持
    /// 滚动位）
    nav_flyout_list: gpui::ListState,
    /// composer 胶囊实时高度（ui::measure_height 每帧写、读到的是上一帧
    /// 值；导航刻度条「屏高 − inputpanel/2」居中用，033）
    composer_h: std::rc::Rc<std::cell::Cell<f32>>,
    unread: HashSet<PathBuf>,
    /// 微信远程控制桥（060）：transport + 待答 ExtUi 请求
    remote: remote_control::RemoteControl,
    hovered_project: Option<usize>,
    proj_tip: Option<(PathBuf, f32, f32)>,
    hover_card: Option<HoverCard>,
    psp_menu: Option<PspMenu>,
    confirm_prj_del: Option<(PathBuf, f32, f32)>,
    status_toast: Option<(String, std::time::Instant)>,
    /// topbar 会话视图的 ⋯ 更多菜单开合（打开终端 / 系统提示词 / 已加载工具）
    top_menu_open: bool,
    /// 该 dropdown 的收起防抖守卫（ui::dropdown 组件持有，点外收起不双触发）
    top_dd: gpui::Entity<crate::ui::DropdownState>,
    /// 工具定义弹窗左列表选中的工具名（pi-web ToolDefinitionsPanel 的
    /// selectedToolName）
    tool_sel: Option<String>,
    /// 系统提示词弹窗的滚动 handle（psp 滚动条要跨渲染读同一份滚动位）
    sysprompt_scroll: gpui::ScrollHandle,
    /// 系统提示词面板各桶「调用声明」折叠块的展开态（按 SYSTEM_BUCKETS 下标）
    decl_open: [bool; 7],
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum PillMenu {
    Thinking,
    Tools,
}

/// inputpanel 弹窗的统一定位锚点（用户定稿）：弹窗与触发按钮**水平居中**、
/// 底缘距按钮**顶缘 5px**（不遮按钮）。坐标来自按钮 paint 期的 canvas 捕获。
#[derive(Debug, Clone, Copy)]
pub(crate) struct PillAnchor {
    pub center_x: gpui::Pixels,
    pub top: gpui::Pixels,
}

/// 三个胶囊按钮的 bounds 槽位：canvas 在 paint 期每帧覆盖写入，弹窗
/// 打开时读取。Rc<RefCell> 是为了绕开 paint 闭包拿不到实体借用的限制。
#[derive(Clone, Default)]
pub(crate) struct PillBtns {
    pub tools: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    pub thinking: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    pub ext: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    /// MCP 按钮（043：扩展按钮右边的勾选菜单入口）
    pub mcp: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    /// 模型按钮（042：模型菜单锚定）
    pub model: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
}

impl PillBtns {
    /// 从槽位取锚点；未就绪则退回点击坐标（首帧竞态兜底）。
    pub(crate) fn anchor(
        slot: &std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
        at: gpui::Point<gpui::Pixels>,
    ) -> PillAnchor {
        slot.borrow()
            .map(|b| PillAnchor { center_x: b.center().x, top: b.origin.y })
            .unwrap_or(PillAnchor { center_x: at.x, top: at.y })
    }

    /// 抓按钮 bounds 的隐形覆盖层：absolute inset_0 的 canvas，prepaint 期
    /// 把整颗按钮的 bounds 写进槽位（弹窗统一定位的测量端）。
    pub(crate) fn tracker(
        slot: &std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    ) -> gpui::AnyElement {
        let capture = slot.clone();
        gpui::canvas(
            move |b, _, _| *capture.borrow_mut() = Some(b),
            |_, _, _, _| {},
        )
        .absolute()
        .inset_0()
        .into_any_element()
    }
}

/// One content-area tab: a terminal session or a file viewer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PanelTab {
    Term(usize),
    File(PathBuf),
}

/// 外部改动冲突类型（023）：文件在磁盘上被改/删，而本缓冲区有未保存修改。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileConflict {
    /// 磁盘内容已变；banner 提供重新加载
    Changed,
    /// 文件已从磁盘消失
    Deleted,
}
// 023 文件编辑展示页：所有文件都进 CodeEditor（Zed parity——Zed 里不分
// 文件大小、一律可编辑）。曾按 50K 行上限加过「超限转只读虚拟化列表」的
// 特殊分支，已删除：特殊路径既不是 Zed 行为，也让大文件不可编辑。
/// 文件 tab 缓冲区状态（023 文件编辑展示页）。
///
/// `content` 是磁盘真值缓存（打开/保存/重载时更新）；`editor` 懒创建——
/// `InputState::new` 要 `&mut Window`，而 `open_file_tab` 的调用链没有，
/// 推迟到渲染帧（content.rs file_view）里补。
pub(crate) struct FileTab {
    pub(crate) content: String,
    pub(crate) editor: Option<gpui::Entity<gpui_component::input::InputState>>,
    /// 编辑器值 != content（订阅 InputEvent::Change 时比较，set_value 也发
    /// Change 事件，盲标会假脏）
    pub(crate) dirty: bool,
    /// md 默认渲染预览；eye 切源码编辑（拍板 2026-10-07）
    pub(crate) md_source: bool,
    pub(crate) conflict: Option<FileConflict>,
    /// 外部改动检测基准 (mtime, len)；打开/保存/确认时刷新
    pub(crate) disk_sig: Option<(std::time::SystemTime, u64)>,
    /// 磁盘内容已换新（自动重载路径），待渲染帧灌进 editor
    pub(crate) reload_pending: bool,
    /// 最后一次用户编辑时刻（自动保存静默期判定）
    pub(crate) last_edit: Option<std::time::Instant>,
}


impl FileTab {
    pub(crate) fn from_disk(content: String) -> Self {
        Self {
            content,
            editor: None,
            dirty: false,
            // 024 用户拍板（2026-10-10 二改）：md 默认打开 = 源码态（WYSIWYG
            // 感官完整：折叠/样式/容器 reveal/原子跳词），eye 切回 preview
            // 渲染页（两者均完好）。
            md_source: true,
            conflict: None,
            disk_sig: None,
            reload_pending: false,
            last_edit: None,
        }
    }
}

impl Chat {
    fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let dialog_focus = cx.focus_handle();
        let cwd = std::env::var("PI_FLASH_CWD")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        // startup restore decides workspace + last session BEFORE the first
        // spawn — exactly one pi process (ARCHITECTURE.md §4, was two)
        let last_ws = if startup_restore() { get_last_workspace() } else { None };
        let target_ws = last_ws.unwrap_or_else(|| cwd.to_string_lossy().to_string());
        let cwd = if !same_ws(&target_ws, &cwd.to_string_lossy()) {
            let ws_path = PathBuf::from(&target_ws);
            if ws_path.is_dir() { ws_path } else { cwd }
        } else {
            cwd
        };
        let branch = read_branch(&cwd);
        let last_open = if startup_restore() {
            get_last_open(&cwd.to_string_lossy())
                .map(PathBuf::from)
                .filter(|p| p.exists())
        } else {
            None
        };
        let ui = ui_state();
        // fs watch 通道：tx 永驻 Chat，rx 交给 startup 的泵任务
        let (fs_watch_tx, fs_watch_rx_ch) = std::sync::mpsc::channel::<()>();

        let mut chat = Self {
            focus,
            dialog_focus,
            dialog: None,
            input: String::new(),
            pending_images: Vec::new(),
            session_tail_cache: std::collections::HashMap::new(),
            sessions: Vec::new(),
            search_hits: Vec::new(),
            search_truncated: false,
            search_running: false,
            search_needle: String::new(),
            search_gen: 0,
            pending_locate: None,
            // v54.4 定案：启动固定会话界面（面板/内容区绑定关系见
            // status_bar；不恢复上次离开时的面板）
            dock_panel: DockPanel::Sessions,
            cwd: cwd.clone(),
            branch,
            runtimes: std::collections::HashMap::new(),
            active_key: String::new(),
            booted: false,
            pending_zoom: false,
            draft_seq: 0,
            models_by_cwd: std::collections::HashMap::new(),
            globals: startup::Globals::default(),
            project_ctx: std::collections::HashMap::new(),
            expanded_dirs: HashSet::new(),
            tree_rows: std::sync::Arc::new(Vec::new()),
            git_files: Vec::new(),
            git_selected: None,
            git_tab: function_panel::git_panel::GitTab::Changes,
            git_log: Vec::new(),
            git_error: None,
            git_commit_input: {
                let input = cx.new(|cx| TextInput::new(cx).placeholder(tr("提交信息（提交已暂存更改）")));
                input
            },
            git_add_del: (0, 0),
            project_files: Vec::new(),
            at_index: std::collections::HashMap::new(),
            at_filter_cache: std::cell::RefCell::new(None),
            history: Vec::new(),
            history_ix: None,
            menu_ix: 0,
            menu_scroll: gpui::ScrollHandle::new(),
            menu_dismissed: false,
            terminals: Vec::new(),
            active_terminal: None,
            term_seq: 0,
            term_cursor_on: true,
            term_cursor_focused: false,
            term_events: None,
            mc_patterns: None,
            mc_state: EnabledState::default(),
            mc_creds: Vec::new(),
            mc_project_scope: false,
            mc_default_model: None,
            mc_default_thinking: None,
            mc_model_thinking: Vec::new(),
            mc_skills: Vec::new(),
            mc_pkgs_global: Vec::new(),
            mc_pkgs_project: Vec::new(),
            pkg_op: None,
            pkg_confirm_remove: None,
            mc_default_tools: None,
            mc_models_json: serde_json::json!({}),
            mc_mj_error: None,
            mc_probe: std::collections::HashMap::new(),
            provider_registry: None,
            registry_state: crate::session::model_picker::RegistryState::Idle,
            model_picker: None,
            mcp_servers: Vec::new(),
            mcp_errors: Vec::new(),
            op_tx: None,
            ext_status: Vec::new(),
            ext_widgets: Vec::new(),
            ext_dialog: None,
            ext_input: cx.new(|cx| TextInput::new(cx)),
            ext_notice: None,
            panel_tabs: Vec::new(),
            active_panel_tab: None,
            file_cache: std::collections::HashMap::new(),
            fs_watch_tx,
            fs_watch_rx: Some(fs_watch_rx_ch),
            fs_watch: None,
            file_watches: std::collections::HashMap::new(),
            composer: None,
            expanded_skills: std::collections::HashSet::new(),
            bubble_scrolls: std::rc::Rc::new(std::cell::RefCell::new(
                std::collections::HashMap::new(),
            )),
            bar_hover: None,
            input_focused: false,
            pill_menu: None,
            plugin_picker: None,
            mcp_picker: None,
            pill_anchor: None,
            pill_btn: PillBtns::default(),
            ext_tokens: services::token_probe::read_cache(),
            ctx_tip_ring_hover: false,
            ctx_tip_panel_hover: false,
            ctx_tip_closing: None,
            settings: None,
            renaming: None,
            rename_input: None,
            confirm_delete: None,
            projects: Vec::new(),
            project_hits: Vec::new(),
            project_filter: String::new(),
            active_file: last_open.clone(),
            list_mode: if ui.list_mode == "flat" { ListMode::Flat } else { ListMode::Grouped },
            sort_mode: if ui.sort_mode == "manual" { SortMode::Manual } else { SortMode::Time },
            collapsed_keys: ui.collapsed.iter().cloned().collect(),
            psp_shown: std::collections::HashMap::new(),
            slp_w: ui.slp_w,
            panes_hidden: ui.panes_hidden,
            slp_drag: None,
            slp_hover: false,
            content_view: ContentView::Chat,
            browse_last: ContentView::Term,
            file_scrollbar: gpui_component::scroll::ScrollbarState::default(),
            file_view_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
            file_view_list_path: None,
            psp_scroll: gpui::ScrollHandle::new(),
            psp_sb_state: crate::ui::psp_scrollbar::PspScrollbarState::new(),
            nav_open: false,
            nav_hide_at: None,
            nav_flyout_hovered: false,
            nav_hover_turn: None,
            nav_flyout_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
            composer_h: std::rc::Rc::new(std::cell::Cell::new(0.)),
            unread: HashSet::new(),
            remote: remote_control::RemoteControl::new(),
            hovered_project: None,
            proj_tip: None,
            hover_card: None,
            psp_menu: None,
            confirm_prj_del: None,
            status_toast: None,
            top_menu_open: false,
            top_dd: cx.new(|_| crate::ui::DropdownState::new()),
            crumb_menu_dir: None,
            crumb_dd: cx.new(|_| crate::ui::DropdownState::new()),
            ext_probe: (0, 0),
            pending_focus_file: None,
            tool_sel: None,
            sysprompt_scroll: gpui::ScrollHandle::new(),
            decl_open: [false; 7],
        };
        // 启动阶段：全局态一次性装载（010-启动.md §1/§6）——模型清单（磁盘 ∪ 自有
        // 缓存）、命令、默认项、插件、全局 mcp，全部先于第一帧进内存，**不 spawn 进程**。
        chat.globals = startup::load_globals();
        let cwd_key = chat.cwd.to_string_lossy().to_string();
        chat.project_ctx
            .insert(cwd_key, startup::load_project(&chat.cwd.clone()));
        // new-session defaults (default model / thinking / enabledModels
        // scope) before the first frame — the draft pills render from them
        chat.reload_model_defaults();
        // wire input callbacks that need the root entity handle
        let weak_ext = cx.entity().downgrade();
        chat.ext_input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ext.update(cx, |c, cx| {
                    c.ext_respond(Some(v.to_string()), None, false, cx);
                });
            }));
        });
        chat.load_project_files();
        // 文件树：根项目行默认展开
        chat.expanded_dirs.insert(chat.cwd.clone());
        chat.refresh_git();

        // settings panel CLI op pump (pi install/remove runs in background)
        let (op_tx, mut op_rx) = futures::channel::mpsc::unbounded::<String>();
        chat.op_tx = Some(op_tx);
        cx.spawn(async move |this, cx| {
            while let Some(msg) = op_rx.next().await {
                if this
                    .update(cx, |chat, cx| {
                        chat.pkg_op = None;
                        chat.set_status(msg, cx);
                        // CLI op 改的是盘上的 settings.json；globals 是启动时
                        // 的内存副本，不重读则 reload_settings_panel 只会拷到
                        // 旧列表（040：安装后列表不刷新的根因）
                        chat.globals = startup::load_globals();
                        chat.reload_settings_panel();
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        // terminal event pump: alacritty EventListener -> Chat (single channel)
        let (term_tx, term_rx) =
            futures::channel::mpsc::unbounded::<(usize, alacritty_terminal::event::Event)>();
        chat.term_events = Some(term_tx);
        cx.spawn(async move |this, cx| {
            let mut rx = term_rx;
            while let Some((tab_id, event)) = rx.next().await {
                if this
                    .update(cx, |chat, cx| chat.on_term_event(tab_id, event, cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        // terminal caret blink (~1Hz): 500ms heartbeat flips the phase and
        // repaints only while a focused terminal is on screen (paint writes
        // term_cursor_focused); phase lives on Chat so repaints stay
        // tick-aligned instead of drifting against the wall clock
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;
                let focused = this.update(cx, |chat, cx| {
                    if chat.term_cursor_focused {
                        chat.term_cursor_on = !chat.term_cursor_on;
                        cx.notify();
                    }
                    chat.term_cursor_focused
                });
                if focused.is_err() {
                    break;
                }
            }
        })
        .detach();
        // models panel state (enabledModels whitelist + credentials) for the
        // picker filter — loaded once at startup, refreshed when opened
        chat.reload_settings_panel();
        // initial runtime: restore the last session (disk-direct tail) or a
        // lazy draft. The process spawns AFTER first paint.
        let rt_key = last_open
            .as_deref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "draft-0".to_string());
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(
                rt_key.clone(),
                cwd.clone(),
                last_open.clone(),
            );
            r.default_model = services::workspace::default_model_pref();
            if let Some(path) = &last_open {
                r.messages = msgs_from_tail(read_tail_messages(path, 256 * 1024, 100));
                r.disk_msg_count = pi_link::sessions::count_message_entries(path) as usize;
                r.pager.reload(r.messages.len());
                r.status = "resuming".into();
            }
            r
        });
        chat.draft_seq = 1; // "draft-0" is taken by the initial runtime above
        chat.runtimes.insert(rt_key.clone(), rt.clone());
        chat.active_key = rt_key.clone();
        chat.subscribe_runtime(&rt, cx);
        // 010-启动.md §9：启动期的一切（首帧附着 / 120ms 泵 / 3s 外部追加观察 /
        // 60s 空闲回收 / 30s recents 对账 / 启动页闸门 / 会话清单与项目集装载）
        // 都由 startup 发起——启动阶段做了什么全在 startup.rs 里可见。
        startup::spawn_boot_tasks(rt.clone(), cx);
        startup::spawn_session_list_load(
            chat.cwd.to_string_lossy().to_string(),
            last_open.clone(),
            cx,
        );
        if PERF.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(t0) = T0.get() {
                eprintln!("[perf] last-session tail rendered: {:?}", t0.elapsed());
            }
        }
        // git panel: Enter in the commit box commits staged changes
        let entity_for_git = cx.entity();
        chat.git_commit_input.update(cx, |ti, _| {
            let weak_git = entity_for_git.downgrade();
            ti.set_on_submit(Box::new(move |_, cx| {
                let _ = weak_git.update(cx, |c, cx| c.git_commit_staged(cx));
            }));
        });
        chat
    }

    fn refresh_state(&self, cx: &mut gpui::App) {
        self.rt().update(cx, |r, _| r.refresh_state());
    }

    fn refresh_sessions(&mut self) {
        let cwd = self.cwd.to_string_lossy().to_string();
        let full = list_sessions_for_cwd(&cwd, 500)
            .into_iter()
            .filter(|s| same_ws(&s.cwd, &cwd))
            .collect::<Vec<_>>();
        // v60: 加载语义 = 时间窗口（设置.加载时间窗口）——当前项目只保留
        // 窗口内活跃的会话（mtime 近似清单 last_active）+ 当前激活会话
        // 兜底（保证改名/打开的旧会话始终可见）；显示量由 psp 分页控制
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(load_window_days() * 86_400);
        let mut sessions: Vec<SessionInfo> = full
            .iter()
            .filter(|s| s.modified >= cutoff)
            .cloned()
            .collect();
        if let Some(active) = &self.active_file {
            if !sessions.iter().any(|s| &s.path == active) {
                if let Some(info) = full.iter().find(|s| &s.path == active) {
                    sessions.push(info.clone());
                }
            }
        }
        self.sessions = sessions;
        // sync the current project's group, then re-pin project order
        let all: Vec<SessionInfo> = self
            .projects
            .iter()
            .flat_map(|g| {
                if same_ws(&g.path.to_string_lossy(), &cwd) {
                    self.sessions.clone()
                } else {
                    g.sessions.clone()
                }
            })
            .collect();
        self.rebuild_projects(all);
    }

    fn refresh_git(&mut self) {
        self.git_files = git_status_files(&self.cwd);
        self.git_add_del = git_numstat(&self.cwd);
        self.rebuild_tree();
    }

    /// 文件树展平缓存重建（services::file_tree::flatten：每展开目录读一次
    /// 盘 + gitignore 过滤 + Zed 语义排序 + git 徽标回填）。展开/折叠、
    /// git 刷新、fs 事件、切项目后都走这里。
    fn rebuild_tree(&mut self) {
        self.tree_rows = std::sync::Arc::new(services::file_tree::flatten(
            &self.cwd,
            &self.expanded_dirs,
            &self.git_files,
        ));
    }

    /// 挂上 fs watcher（递归 watch cwd；旧句柄 drop 即解除）。切项目时
    /// 重挂。事件由 startup::spawn_fs_watch_pump 合批后驱动 refresh_git。
    fn attach_fs_watch(&mut self) {
        self.fs_watch = services::watcher::watch(&self.cwd, self.fs_watch_tx.clone()).ok();
    }

    /// 023 外部改动检测的监听面维护（对齐 Zed 的 worktree 监听面）：
    /// - cwd 内文件：cwd 的**递归** watch 已覆盖；
    /// - cwd 外文件（Obsidian 库 / pick_open_files / 远程 FILE_OPEN）：此前
    ///   完全没监听——磁盘怎么改都没有信号，编辑器不跟随。现按**所在目录**
    ///   挂非递归 watch（Zed single-file worktree 同款），事件与 cwd 事件
    ///   走同一条通道合批，`check_external_file_changes` 按 disk_sig 复查。
    ///
    /// 打开/关闭文件 tab、切项目后调用；不再被任何打开文件需要的目录卸掉
    /// （句柄 drop = 停止监听），避免长期持有无关目录的监听。
    fn retain_file_watches(&mut self) {
        let mut wanted: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for t in &self.panel_tabs {
            if let PanelTab::File(p) = t
                && let Some(dir) = p.parent()
                && !services::workspace::is_under(dir, &self.cwd)
            {
                wanted.insert(dir.to_path_buf());
            }
        }
        self.file_watches.retain(|dir, _| wanted.contains(dir));
        for dir in wanted {
            if self.file_watches.contains_key(&dir) {
                continue;
            }
            if let Ok(w) = services::watcher::watch_dir(&dir, self.fs_watch_tx.clone()) {
                self.file_watches.insert(dir, w);
            }
        }
    }

    // -----------------------------------------------------------------------
    // built-in terminal (pi-web TerminalPanel parity)
    // -----------------------------------------------------------------------

    fn on_term_event(
        &mut self,
        tab_id: usize,
        event: alacritty_terminal::event::Event,
        cx: &mut Context<Self>,
    ) {
        use alacritty_terminal::event_loop::Msg;
        let Some(tab) = self.terminals.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        match event {
            alacritty_terminal::event::Event::Wakeup => {
                cx.notify();
            }
            alacritty_terminal::event::Event::PtyWrite(s) => {
                let _ = tab.pty.send(Msg::Input(s.into_bytes().into()));
            }
            alacritty_terminal::event::Event::ClipboardStore(_, text) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
            alacritty_terminal::event::Event::ClipboardLoad(_, fmt) => {
                let text = cx
                    .read_from_clipboard()
                    .and_then(|i| i.text())
                    .unwrap_or_default();
                let _ = tab.pty.send(Msg::Input(fmt(&text).into_bytes().into()));
            }
            alacritty_terminal::event::Event::ColorRequest(i, fmt) => {
                let _ = tab
                    .pty
                    .send(Msg::Input(fmt(terminal::term_rgb(i)).into_bytes().into()));
            }
            alacritty_terminal::event::Event::TextAreaSizeRequest(fmt) => {
                let ws = alacritty_terminal::event::WindowSize {
                    num_cols: tab.cols as u16,
                    num_lines: tab.rows as u16,
                    cell_width: tab.cell_w as u16,
                    cell_height: tab.line_h as u16,
                };
                let _ = tab.pty.send(Msg::Input(fmt(ws).into_bytes().into()));
            }
            alacritty_terminal::event::Event::ChildExit(status) => {
                tab.status = TermStatus::Exited(status.code());
                cx.notify();
            }
            alacritty_terminal::event::Event::Exit => {
                if tab.status == TermStatus::Ready {
                    tab.status = TermStatus::Exited(None);
                }
                cx.notify();
            }
            _ => {}
        }
    }

    fn switch_project(&mut self, cwd: PathBuf, cx: &mut Context<Self>) {
        self.cwd = cwd;
        // mark as the globally-last active workspace for startup restore
        set_last_workspace(&self.cwd.to_string_lossy());
        self.branch = read_branch(&self.cwd);
        self.refresh_sessions();
        self.dialog = None;
        self.expanded_dirs.clear();
        // 文件树：根项目行默认展开
        self.expanded_dirs.insert(self.cwd.clone());
        self.refresh_git();
        self.load_project_files();
        self.attach_fs_watch();
        // cwd 变了：cwd 外单文件监听面跟着重算（旧 cwd 下的文件现在要单挂）
        self.retain_file_watches();
        if let Some(p) = get_last_open(&self.cwd.to_string_lossy()) {
            let path = PathBuf::from(&p);
            if path.exists() {
                self.open_session(path, false, cx);
                cx.notify();
                return;
            }
        }
        self.new_session(cx);
        cx.notify();
    }

    /// chat.input 唯一写入口：同步镜像进输入组件（组件是渲染真值源，
    /// 手写 input 不再存在）。历史/草稿/清空/补全改写都走这里。
    pub(crate) fn set_input(&mut self, v: String, cx: &mut Context<Self>) {
        self.input = v;
        self.menu_dismissed = false;
        if let Some(c) = &self.composer {
            let v = self.input.clone();
            c.update(cx, |f, fcx| f.set_value(v, fcx));
        }
        cx.notify();
    }

    /// set_input 的光标落点版（031 @ 补全：确认后光标停在插入 token 之后）。
    /// cursor_byte = 新值的字节偏移。
    pub(crate) fn set_input_with_cursor(
        &mut self,
        v: String,
        cursor_byte: usize,
        cx: &mut Context<Self>,
    ) {
        self.input = v;
        self.menu_dismissed = false;
        if let Some(c) = &self.composer {
            let v = self.input.clone();
            c.update(cx, |f, fcx| f.set_value_with_cursor(v, cursor_byte, fcx));
        }
        cx.notify();
    }

    /// @ 文件索引惰性构建（031 对齐 pi-web file-index：TTL 内复用，过期/
    /// 缺失在后台线程重建；旧清单在重建期间先顶上。TTL/容量读
    /// app-settings.json，默认 60s / 10 项目）。
    pub(crate) fn ensure_at_index(&mut self, cx: &mut Context<Self>) {
        let ttl = std::time::Duration::from_secs(services::workspace::at_index_ttl_secs());
        let key = self.cwd.to_string_lossy().to_string();
        let fresh = match self.at_index.get(&key) {
            Some(e) => {
                e.building || e.built.is_some_and(|t| t.elapsed() < ttl)
            }
            None => false,
        };
        if fresh {
            return;
        }
        let entry = self.at_index.entry(key).or_default();
        entry.building = true;
        entry.built = None;
        // 缓存上限（pi-web：超限整表清空——每 cwd 一份，重建很便宜）
        if self.at_index.len() >= services::workspace::at_index_max_projects() {
            self.at_index.clear();
        }
        let cwd = self.cwd.clone();
        let key = cwd.to_string_lossy().to_string();
        cx.spawn(async move |this, cx| {
            // entries 派生也在后台：大仓库（20 万条）下主线程只做 Arc 换手
            let (files, entries) = cx
                .background_executor()
                .spawn(async move {
                    let listing = crate::services::file_index::load_listing(&cwd);
                    let entries =
                        crate::services::at_file::build_entries_from_files(&listing.files);
                    (listing.files, entries)
                })
                .await;
            let _ = this.update(cx, |chat, cx| {
                // key = 构建时的 cwd（期间切项目也不会写错条目）
                let e = chat.at_index.entry(key).or_default();
                e.files = std::sync::Arc::new(files);
                e.entries = std::sync::Arc::new(entries);
                e.built = Some(std::time::Instant::now());
                e.building = false;
                cx.notify();
            });
        })
        .detach();
    }

    fn with_active_editor<Act: FnOnce(&mut SessionRuntime, &mut Context<SessionRuntime>)>(
        &mut self,
        cx: &mut Context<Self>,
        act: Act,
    ) {
        let rt = self.rt();
        let (input, images, history) = (
            self.input.clone(),
            self.pending_images.clone(),
            self.history.clone(),
        );
        rt.update(cx, |r, cx| {
            r.input = input;
            r.pending_images = images;
            r.history = history;
            act(r, cx);
        });
        // pull post-action state back (history push, input clear)
        let (input, images, history) = {
            let r = rt.read(cx);
            (r.input.clone(), r.pending_images.clone(), r.history.clone())
        };
        self.set_input(input, cx);
        self.pending_images = images;
        self.history = history;
        self.history_ix = None;
        cx.notify();
    }

    fn send_input(&mut self, cx: &mut Context<Self>) {
        self.with_active_editor(cx, |r, cx| r.send_input(cx));
    }

    fn steer_input(&mut self, cx: &mut Context<Self>) {
        self.with_active_editor(cx, |r, cx| r.steer_input(cx));
    }

    fn follow_up_input(&mut self, cx: &mut Context<Self>) {
        self.with_active_editor(cx, |r, cx| r.follow_up_input(cx));
    }

    fn abort_stream(&mut self, cx: &mut Context<Self>) {
        self.rt().update(cx, |r, cx| r.abort_stream(cx));
        cx.notify();
    }

    /// `!` bash 执行中 Esc / 停止按钮（031）：rpc abort_bash
    pub(crate) fn abort_bash(&mut self, cx: &mut Context<Self>) {
        self.rt().update(cx, |r, cx| r.abort_bash(cx));
        cx.notify();
    }

    fn set_thinking_level(&mut self, key: &str, cx: &mut Context<Self>) {
        self.rt().update(cx, |r, cx| r.set_thinking_level(key, cx));
    }

    fn mc_set_tools_preset(&mut self, key: &str, cx: &mut Context<Self>) {
        // 工具预设是 spawn 参数（`--tools` / `--no-tools`），换它必须重绑会话
        // 进程（kill + 新开）+ 整表重读：运行中换会把正在跑的这一轮直接掐掉，
        // 顺便让「等待模型响应」掉回屏底。UI 已置灰，这里是绕过 UI 的兜底。
        if self.rt().read(cx).agent_running {
            self.set_status(crate::i18n::tr("运行中不能更换工具预设").to_string(), cx);
            return;
        }
        let rt = self.rt();
        rt.update(cx, |r, cx| {
            r.tools_preset = key.to_string();
            if let Some(rx) = r.spawn() {
                let epoch = r.agent.epoch;
                session::runtime::SessionRuntime::attach_pump(&rt, rx, epoch, cx);
            }
            if let Some(s) = &r.agent.session {
                let _ = s.send(&Command::GetMessages);
            }
            r.refresh_anchors();
            r.refresh_state();
            // 状态行与胶囊同文案（full+）
            let label = r.tool_preset_label();
            r.status = crate::i18n::tf("工具预设: {k} (会话进程已重绑)", &[("k", label)]);
            cx.emit(session::runtime::SessionEvent::Changed);
        });
        cx.notify();
    }

    fn rt(&self) -> gpui::Entity<session::runtime::SessionRuntime> {
        self.runtimes
            .get(&self.active_key)
            .expect("active runtime exists")
            .clone()
    }

    /// 「其他」页提示音开关 → 全池广播。轮末提示音归 runtime 职责，每个常驻
    /// runtime 各持一份副本，所以新开关必须扇出到**所有** runtime 而不只是活跃
    /// 那个——后台会话同样会跑 agent、同样需要提示。
    pub(crate) fn broadcast_sound_on(&self, on: bool, cx: &mut Context<Self>) {
        for (_, rt) in &self.runtimes {
            rt.update(cx, |r, _| r.sound_on = on);
        }
    }

    /// 一个项目的模型目录（010-启动.md §7）：**进程答案优先**（`models_by_cwd`
    /// 有该 cwd 的非空条目 → 用它），否则回落启动装载的全局清单（磁盘 ∪ 自有缓存）——
    /// 于是 lazy draft / 冷启动也有清单，不再出现「选择模型」「no models match」。
    pub(crate) fn catalog_for(&self, cwd: &std::path::Path) -> &[pi_link::protocol::ModelInfo] {
        self.models_by_cwd
            .get(&cwd.to_string_lossy().to_string())
            .map(|v| v.as_slice())
            .filter(|v| !v.is_empty())
            .unwrap_or(self.globals.models.as_slice())
    }

    /// 当前项目的上下文（010-启动.md §5）：命中启动装载的集合就直接返回，
    /// 未命中（新打开的项目）用同一个装载函数现算一次并纳入集合。
    pub(crate) fn project_ctx_now(&mut self) -> startup::ProjectCtx {
        let key = self.cwd.to_string_lossy().to_string();
        if let Some(ctx) = self.project_ctx.get(&key) {
            return ctx.clone();
        }
        let ctx = startup::load_project(&self.cwd.clone());
        self.project_ctx.insert(key, ctx.clone());
        ctx
    }

    /// `/` 菜单命令清单（010-启动.md §3）：会话进程答过 → 用它（含项目 skill）；
    /// 否则 = 项目 skill 派生（`skill:<name>`）+ 全局扩展命令（内置表 + 缓存）。
    /// 草稿态（无进程）因此也有完整菜单。
    pub(crate) fn slash_commands(&self, cx: &gpui::App) -> Vec<pi_link::protocol::SlashCommand> {
        let rt = self.rt().read(cx);
        if !rt.commands.is_empty() {
            return rt.commands.clone();
        }
        let mut out: Vec<pi_link::protocol::SlashCommand> = self
            .project_ctx
            .get(&self.cwd.to_string_lossy().to_string())
            .map(|ctx| ctx.skill_commands.clone())
            .unwrap_or_default();
        out.extend(self.globals.commands.iter().cloned());
        out
    }

    /// Picker fallback: catalog entry for the active session's cwd is empty →
    /// ask once through a live runtime of the SAME cwd (the response flows
    /// back via its pump into models_by_cwd). Never spawns a process for
    /// this — model selection must not conjure session processes.
    pub(crate) fn ensure_models_requested(&mut self, cx: &mut Context<Self>) {
        let active = self.rt();
        let (cwd_key, has_session) = {
            let r = active.read(cx);
            (
                r.cwd.to_string_lossy().to_string(),
                r.agent.session.is_some(),
            )
        };
        if self
            .models_by_cwd
            .get(&cwd_key)
            .is_some_and(|v| !v.is_empty())
        {
            return;
        }
        if has_session {
            active.update(cx, |r, _| {
                if let Some(s) = r.agent.session.as_ref() {
                    let _ = s.send(&pi_link::protocol::Command::GetAvailableModels);
                }
            });
            return;
        }
        // active is a draft: any other live runtime of the same cwd answers too
        for (_, rt) in &self.runtimes {
            let r = rt.read(cx);
            if r.cwd.to_string_lossy().to_string() == cwd_key
                && r.agent.session.is_some()
            {
                if let Some(s) = r.agent.session.as_ref() {
                    let _ = s.send(&pi_link::protocol::Command::GetAvailableModels);
                }
                return;
            }
        }
    }

    fn set_status(&mut self, msg: String, cx: &mut Context<Self>) {
        self.status_toast = Some((msg, std::time::Instant::now()));
        self.rt().update(cx, |r, _| r.status = String::new());
        let _ = cx;
    }

    fn load_project_files(&mut self) {
        self.project_files = walk_files(&self.cwd, 3, 400);
    }

    /// 当前会话标题（topbar 会话视图展示；≤30 字截断）。优先 pi 会话名，
    /// 回落首条用户消息。
    pub(crate) fn session_title(&self, cx: &gpui::App) -> String {
        let file = self
            .runtimes
            .get(&self.active_key)
            .and_then(|rt| rt.read(cx).file.clone());
        let raw = file
            .and_then(|f| {
                self.projects
                    .iter()
                    .flat_map(|g| &g.sessions)
                    .find(|s| same_path(&s.path, &f))
                    .and_then(|s| {
                        s.name
                            .clone()
                            .filter(|n| !n.trim().is_empty())
                            .or_else(|| {
                                Some(s.preview.clone()).filter(|p| !p.trim().is_empty())
                            })
                    })
            })
            .unwrap_or_else(|| tr("新会话").to_string());
        // 首行 + ≤30 字
        let first_line = raw.lines().next().unwrap_or("").trim().to_string();
        let mut out: String = first_line.chars().take(30).collect();
        if first_line.chars().count() > 30 {
            out.push('…');
        }
        out
    }

    /// 切内容区视图；落在浏览操作区（Term/Md）时记住，供文件树标签恢复
    pub(crate) fn set_content_view(&mut self, v: ContentView) {
        self.content_view = v;
        if matches!(v, ContentView::Term | ContentView::File) {
            self.browse_last = v;
        }
    }
}

impl Chat {
    /// 当前文件 tab 编辑器的焦点句柄（持焦时 Some）——render 帧级焦点回收
    /// 的白名单，与终端/对话框同级（023）。
    fn file_editor_focus(&self, window: &gpui::Window, cx: &App) -> Option<gpui::FocusHandle> {
        if self.content_view != ContentView::File {
            return None;
        }
        let path = self.active_file_path()?;
        let ed = self.file_cache.get(&path)?.editor.clone()?;
        let handle = ed.read(cx).focus_handle(cx);
        handle.is_focused(window).then_some(handle)
    }
}

impl Focusable for Chat {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        // composer 就绪后，"聚焦聊天"即落到输入组件（真输入框：光标/
        // 选区/IME 全可用），chat.focus 仅作组件创建前的回退
        self.composer
            .as_ref()
            .map(|c| c.read(cx).focus_handle_in(cx))
            .unwrap_or_else(|| self.focus.clone())
    }
}

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

impl Render for Chat {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if PERF.load(std::sync::atomic::Ordering::Relaxed)
            && !FIRST.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            if let Some(t0) = T0.get() {
                eprintln!("[perf] first frame: {:?}", t0.elapsed());
            }
        }
        // [perf] 仪器：Chat 每秒实际渲染次数（PI_FLASH_PERF=1 开启）。idle
        // 应≈1（光标闪烁）；编辑/拖选时≈事件率；无事件却持续 >30/s = 存在
        // 自持重绘循环（v62 排查文本编辑卡顿的探针，留作常备仪器）。
        static RENDER_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        static RENDER_T: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
        static RENDER_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *RENDER_ON.get_or_init(|| std::env::var("PI_FLASH_PERF").is_ok()) {
            let n = RENDER_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let mut slot = RENDER_T.lock().unwrap();
            let report = match *slot {
                Some(t0) if t0.elapsed().as_secs_f32() >= 1.0 => {
                    Some(n as f32 / t0.elapsed().as_secs_f32())
                }
                Some(_) => None,
                None => {
                    *slot = Some(std::time::Instant::now());
                    None
                }
            };
            if let Some(rate) = report {
                eprintln!("[perf] chat renders/s: {rate:.0}");
                RENDER_N.store(0, std::sync::atomic::Ordering::Relaxed);
                *slot = Some(std::time::Instant::now());
            }
        }
        // 010-启动：揭幕前整幅启动页（黑底、居中 logo、非最大化）；揭幕帧
        // 再按 §4 默认最大化（0.2.2 建窗即 Maximized 不可靠，显式 zoom 是
        // 项目已验证路径——原建窗回调里的 zoom 后移到了这里）。
        if !self.booted {
            startup::mark_splash_painted();
            return startup::splash_view();
        }
        if self.pending_zoom {
            self.pending_zoom = false;
            window.zoom_window();
        }
        // keep terminal focus alive across frames (render focuses chat input
        // otherwise, which would steal it back every redraw)
        let dialog_input = match &self.dialog {
            Some(Dialog::ProviderPicker { input }) | Some(Dialog::SessionSearch { input })
            | Some(Dialog::ProjectPicker { input, .. }) => {
                Some(input.clone())
            }
            _ => None,
        }
        .or_else(|| {
            // 042：模型菜单的过滤输入同享焦点保持（每帧不被 composer 抢回）；
            // 过滤框未渲染（<10 条）时不抢——菜单退化为纯鼠标操作
            self.model_picker
                .as_ref()
                .filter(|_| self.picker_filter_shown(cx))
                .map(|p| p.input.clone())
        });
        // 详情卡的原地改名输入框也要持有焦点——否则每帧的焦点回收
        // 会把它抢回主输入框，键盘输入进不去
        let card_rename_focus = self
            .hover_card
            .as_ref()
            .and_then(|c| c.rename_input.clone());
        let rename_focus = self.rename_input.clone().or(card_rename_focus);
        if let Some(input) = rename_focus.or(dialog_input) {
            let handle = input.read(cx).focus_handle_in(cx);
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
        } else if self.ext_dialog.is_some() {
            let ext_text = matches!(
                self.ext_dialog.as_ref().map(|r| &r.method),
                Some(
                    pi_link::protocol::ExtUiMethod::Input { .. }
                        | pi_link::protocol::ExtUiMethod::Editor { .. }
                )
            );
            let handle = if ext_text {
                self.ext_input.read(cx).focus_handle()
            } else {
                self.dialog_focus.clone()
            };
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
        } else if self.dialog.is_some() {
            if !self.dialog_focus.is_focused(window) {
                window.focus(&self.dialog_focus);
            }
        } else if let Some(panel) = self.settings.as_ref() {
            // 焦点在面板任意输入框内就不抢（白名单在 v60/v70 两度漏新输入框，
            // 现由 SettingsPanel::focus_within 结构化自检）
            if !panel.read(cx).focus_within(window, cx)
                && !self.dialog_focus.is_focused(window)
            {
                window.focus(&self.dialog_focus);
            }
        } else if self.file_editor_focus(window, cx).is_some() {
            // 023：文件编辑器持有焦点时不回收——否则点进去下一帧就被抢回
            // composer，打字全失效（编辑器此前不在白名单）
        } else if !self.terminals.iter().any(|t| t.focus.is_focused(window)) {
            window.focus(&self.focus_handle(cx));
        }
        let t = T();

        let entity = cx.entity();
        let weak = entity.downgrade();
        let weak_for_dialog = weak.clone();

        // popup menu overlay anchored above the composer controls row
        let (thinking_override, preset_key) = {
            let rt = self.rt();
            let r = rt.read(cx);
            (r.thinking_override.clone(), r.tools_preset.clone())
        };
        let pill_menu_el = self.pill_menu.map(|menu| {
            let weak_menu = weak.clone();
            let rows: Vec<(String, String, bool)> = match menu {
                PillMenu::Thinking => [
                    ("auto", tr("使用 pi 默认设置"), thinking_override.is_none()),
                    ("off", tr("关闭推理"), thinking_override.as_deref() == Some("off")),
                    ("minimal", tr("最低限度推理"), thinking_override.as_deref() == Some("minimal")),
                    ("low", tr("低强度推理"), thinking_override.as_deref() == Some("low")),
                    ("medium", tr("中等强度推理"), thinking_override.as_deref() == Some("medium")),
                    ("high", tr("高强度推理"), thinking_override.as_deref() == Some("high")),
                    ("xhigh", tr("超高强度推理"), thinking_override.as_deref() == Some("xhigh")),
                    ("max", tr("最高强度推理"), thinking_override.as_deref() == Some("max")),
                ]
                .iter()
                .map(|(k, d, on)| (k.to_string(), d.to_string(), *on))
                .collect(),
                PillMenu::Tools => [
                    ("chat-only", tr("仅聊天"), preset_key == "chat-only"),
                    ("read-only", tr("4 个只读内置工具"), preset_key == "read-only"),
                    ("default", tr("4 个内置工具"), preset_key == "default"),
                    // full：内置 7 件 + 精确集（-ne + 个人扩展/内置扩展，包零注入）
                    ("full", tr("全部内置工具（不装任何扩展）"), preset_key == "full"),
                    // full+ = full + 本会话选中扩展（默认档；清单按会话保存）
                    ("custom", tr("full + 自定义扩展"), preset_key == "custom"),
                ]
                .iter()
                .map(|(k, d, on)| (k.to_string(), d.to_string(), *on))
                .collect(),
            };
            let is_thinking = menu == PillMenu::Thinking;
            let items = rows
                .into_iter()
                .map(|(key, desc, active)| {
                    let weak_row = weak_menu.clone();
                    let key_for_rpc = key.clone();
                    div()
                        .id(SharedString::from(format!(
                            "pm-{}-{}",
                            if is_thinking { "t" } else { "w" },
                            key
                        )))
                        .min_h(px(40.))
                        .px_3()
                        .py_2()
                        .flex()
                        .items_center()
                        .gap_2()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            let _ = weak_row.update(cx, |c, cx| {
                                c.pill_menu = None;
                                if is_thinking {
                                    c.set_thinking_level(&key_for_rpc, cx);
                                } else {
                                    c.mc_set_tools_preset(&key_for_rpc, cx);
                                }
                            });
                        })
                        .child(
                            div()
                                .w(px(16.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .children(active.then(|| icon("check", 13., t.accent))),
                        )
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(14.))
                                .font_weight(if active {
                                    gpui::FontWeight::SEMIBOLD
                                } else {
                                    gpui::FontWeight::NORMAL
                                })
                                .text_color(rgb(t.text))
                                // full+ 档显示自定义名（其余档沿用内部键）
                                .child(SharedString::from(if key == "custom" {
                                    tr("full+").to_string()
                                } else {
                                    key
                                })),
                        )
                        .child(
                            div()
                                .ml_auto()
                                .text_size(crate::appearance::ui_size(11.))
                                .text_color(rgb(t.text_dim))
                                .child(desc),
                        )
                        .into_any_element()
                })
                .collect::<Vec<_>>();
            // 浮层公共基座：遮挡不穿透 + 点外关闭（ESC 走 composer 的输入框焦点，
            // 这里不抢焦）
            {
                let weak_menu = weak_menu.clone();
                crate::ui::overlay::layer(false, None, move |_w, cx| {
                    let _ = weak_menu.update(cx, |c, cx| {
                        if c.pill_menu.is_some() {
                            c.pill_menu = None;
                            cx.notify();
                        }
                    });
                })
            }
                .child(
                    {
                        // 统一定位（用户定稿）：弹窗与按钮水平居中、底缘距
                        // 按钮顶缘 5px（不遮按钮）；左右钳在视口内
                        let vp = window.viewport_size();
                        let gap = px(5.);
                        let menu_w = px(320.);
                        let (anchor_bottom, anchor_left) = match self.pill_anchor {
                            Some(a) => {
                                let bottom = (vp.height - a.top + gap).max(px(8.));
                                let mut left = a.center_x - menu_w / 2.;
                                if left + menu_w > vp.width - px(8.) {
                                    left = vp.width - menu_w - px(8.);
                                }
                                (bottom, left.max(px(8.)))
                            }
                            None => (px(64.), vp.width - menu_w - px(24.)),
                        };
                        div()
                            .absolute()
                            .bottom(anchor_bottom)
                            .left(anchor_left)
                            .min_w(menu_w)
                            .rounded(px(8.))
                            .border_1()
                            .border_color(rgb(t.border))
                            .bg(rgb(t.bg))
                            .shadow_lg()
                            .overflow_hidden()
                            .flex()
                            .flex_col()
                            // 浮层规则 4：点卡片本身不关（菜单项自己关）
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .children(items)
                    }
                )
                .into_any_element()
        });

        // ---- v54 body: panel-col (topbar-l + dock + statusbar) | content-col
        let entity_for_body = entity.clone();
        let weak_for_body = weak.clone();
        let panes_hidden = self.panes_hidden;
        let slp_dragging = self.slp_drag.is_some();
        let slp_hovered = self.slp_hover;
        let mut body = div()
            .id("app-body")
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            // slp 宽度拖拽（resizer 按下时在此跟踪）
            .on_mouse_move(move |ev: &gpui::MouseMoveEvent, _, cx| {
                let _ = weak_for_body.update(cx, |c, cx| {
                    if let Some((start_x, start_w)) = c.slp_drag {
                        c.slp_w = (start_w + f32::from(ev.position.x) - start_x).clamp(300., 500.);
                        cx.notify();
                    }
                });
            })
            .on_mouse_up(
                MouseButton::Left,
                {
                    let weak = weak.clone();
                    move |_, _, cx| {
                        let _ = weak.update(cx, |c, cx| {
                            if c.slp_drag.take().is_some() {
                                c.persist_ui();
                                cx.notify();
                            }
                        });
                    }
                },
            );
        if !panes_hidden {
            // 分隔线响应区（跨线）：panel 内 3px + 内容区 9px（避开滚动条
            // 命中区）；可视线由 content-col 的 border 承担（hover 2px 深线）
            body = body.child(
                div()
                    .id("slp-resizer")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(self.slp_w - 3.))
                    .w(px(12.))
                    .cursor_col_resize()
                    .on_hover(cx.listener(|this, h: &bool, _w, cx| {
                        if this.slp_hover != *h {
                            this.slp_hover = *h;
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(MouseButton::Left, cx.listener(
                        |this, ev: &gpui::MouseDownEvent, _w, cx| {
                            if ev.click_count == 2 {
                                this.slp_w = 300.;
                                this.persist_ui();
                                cx.notify();
                            } else {
                                this.slp_drag =
                                    Some((f32::from(ev.position.x), this.slp_w));
                            }
                        },
                    ))
            );
            body = body.child(
                div()
                    .id("panel-col")
                    .w(px(self.slp_w))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .bg(rgb(t.chrome))
                    .child(titlebar::topbar_l(self, cx))
                    .child(function_panel::dock(
                        self,
                        entity_for_body.clone(),
                        &weak,
                        window,
                        cx,
                    ))
                    .child(status_bar::control_bar(self, cx)),
            );
        }
        body = body.child(
            div()
                .id("content-col")
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .relative()
                .bg(rgb(t.bg))
                .when(
                    !panes_hidden && !slp_dragging && !self.slp_hover,
                    |d| {
                        d.border_l_1()
                            .border_color(gpui::rgba(crate::theme::border_alpha(t, 0x99)))
                    },
                )
                // hover/拖拽：2px 深色线替换 1px 常驻线（画在左缘 = 交界，
                // content-col 在 panel-col 之后渲染，z 序安全）
                .when(slp_hovered || slp_dragging, |d| {
                    d.border_l_2()
                        .border_color(gpui::rgba(crate::theme::border_alpha(t, 0xc8)))
                })
                .child(titlebar::topbar_r(self, window, cx))
                .child(content::content_main(
                    self,
                    entity_for_body,
                    &weak,
                    window,
                    cx,
                )),
        );

        let mut root = div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(rgb(t.bg))
            .text_color(rgb(t.text))
            .font_family(crate::appearance::panel_font().family.clone())
            // ComposerPaste 兜底：焦点在对话框等非 composer 输入框时，ctrl-v
            // 截获后走到这里——原样重派发组件 Paste，普通文本粘贴不受影响
            //（composer 胶囊内有更近的 on_action，bubble 最内层先停）
            .on_action(cx.listener(|_, _: &ComposerPaste, window, cx| {
                window.dispatch_action(Box::new(gpui_component::input::Paste), cx);
            }))
            // 023：同款兜底 ×3——composer 绑定在 "Input" 上下文全局劫持了
            // up/down/tab（同深度后注册者优先），焦点在文件编辑器时这四个
            // 动作无人处理=按键全死；bubble 走到根=非 composer，重派发组件
            // 原动作（composer 内层有更近 handler，不会到这里）
            .on_action(cx.listener(|_, _: &ComposerUp, window, cx| {
                window.dispatch_action(Box::new(gpui_component::input::MoveUp), cx);
            }))
            .on_action(cx.listener(|_, _: &ComposerDown, window, cx| {
                window.dispatch_action(Box::new(gpui_component::input::MoveDown), cx);
            }))
            .on_action(cx.listener(|_, _: &ComposerTab, window, cx| {
                window.dispatch_action(Box::new(gpui_component::input::IndentInline), cx);
            }))
            .child(body);

        if let Some(panel) = self.settings.clone() {
            let data = settings::SettingsFormData::snapshot(panel.read(cx), cx);
            root = root.child(settings::render_settings(self, &weak_for_dialog, &data));
        }
        // 弹窗必须挂在 settings **之后**：后渲染者在上——原顺序（dialogs →
        // settings）让设置面板盖住从设置页打开的 ProviderPicker，遮挡层吃掉
        // 全部点击（"看得见点不着"）。弹窗永远是顶层模态，统一放 settings 上。
        root = dialogs::render_dialogs(root, self, &weak_for_dialog, t, cx);
        // 040 卸载扩展确认浮层（叠在设置弹窗之上）
        if let Some(el) = settings::plugins::remove_confirm_overlay(self, cx) {
            root = root.child(el);
        }
        // psp 悬浮层（tooltip / 详情卡 / 菜单 / 确认）
        root = root.child(function_panel::psp_overlays::psp_overlays(self, cx));
        // toolbar pill popup menus
        if let Some(el) = pill_menu_el {
            root = root.child(el);
        }
        // 插件勾选面板：**独立于工具菜单**。千万别再塞回上面那个 if-let ——
        // 插件按钮点击时 pill_menu 是 None，面板会整块不渲染（"点不开"的根因）。
        if let Some(el) = session::plugin_picker::view(self, &weak_for_dialog, window) {
            root = root.child(el);
        }
        // MCP 勾选面板（043）：同上，独立挂载，锚点 = 扩展按钮右边的 MCP 按钮
        if let Some(el) = session::mcp_picker::view(self, &weak_for_dialog, window) {
            root = root.child(el);
        }
        // 模型菜单（042）：同上，独立挂载，锚点 = 输入面板的模型按钮
        if let Some(el) = session::model_picker::view(self, &weak_for_dialog, window, cx) {
            root = root.child(el);
        }
        // status toast（v54: statusbar 无状态文本，改瞬时提示）。2026-10-10
        // 定夺：配色并入主题系（与 ext notice 同族 bg_panel+border+text），
        // 尺寸自适应——内容撑宽、max 600×200、超高滚动、自动换行、左右 padding 20
        if let Some((msg, _)) = &self.status_toast {
            let text: SharedString = msg.clone().into();
            root = root.child(
                div()
                    .absolute()
                    .top(px(44.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .id("status-toast")
                            .max_w(px(600.))
                            .max_h(px(200.))
                            .overflow_y_scroll()
                            .px(px(20.))
                            .py(px(7.))
                            .rounded(px(8.))
                            // 2px 边框 + 加大阴影：与内容区拉开（2026-10-10 定夺）
                            .border_2()
                            .border_color(rgb(t.border))
                            .bg(rgb(t.bg_panel))
                            .shadow_2xl()
                            .text_size(crate::appearance::ui_size(12.))
                            .text_color(rgb(t.text))
                            // 非 flex 块级容器：文本按容器宽度自动换行
                            //（flex 会让文本 item 按最大内容宽撑开不换行）
                            .child(text),
                    ),
            );
        }
        // extension notify toast (top-right)。上限与 status toast 对齐
        // （max 600×200、超高滚动、自动换行；2026-10-10 定夺）
        if let Some((message, ty)) = &self.ext_notice {
            let color = match ty {
                1 => 0xfacc15,
                2 => 0xf87171,
                _ => 0x4ade80,
            };
            let text: SharedString = message.clone().into();
            root = root.child(
                div()
                    .id("ext-notice")
                    .absolute()
                    .top(px(12.))
                    .right(px(12.))
                    .max_w(px(600.))
                    .max_h(px(200.))
                    .overflow_y_scroll()
                    .px(px(20.))
                    .py(px(8.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(rgb(color))
                    .bg(rgb(t.bg_panel))
                    .shadow_lg()
                    .flex()
                    .items_start()
                    .gap_2()
                    .child(div().size(px(7.)).rounded_full().mt(px(4.)).bg(rgb(color)))
                    .child(div().text_size(crate::appearance::ui_size(12.)).text_color(rgb(t.text)).child(text)),
            );
        }
        // blocking extension dialog (select/confirm/input/editor)
        if let Some(req) = &self.ext_dialog {
            root = root.child(render_ext_dialog(self, req.clone(), &weak_for_dialog));
        }
        root
    }
}

fn main() {
    let _ = T0.set(std::time::Instant::now());
    // 启动第一步（010-启动.md §4/§10）：建 ~/.pi-flash/、旧 pi-flash-*.json 搬家、
    // recents 首启种子。必须早于任何读盘者（workspace 记忆 / recents / 会话扫描器
    // 都是进程级惰性单例）。
    startup::boot();
    // [perf] 仪器总开关：PI_FLASH_PERF=1 才开（1452 RENDER_ON 同款门控）。
    // 此处曾被无条件 store(true)，每次启动都往 stderr 打 [perf] 行，release 烟测必炸。
    PERF.store(
        std::env::var("PI_FLASH_PERF").is_ok(),
        std::sync::atomic::Ordering::Relaxed,
    );
    // theme: PI_FLASH_THEME (dev override) > app_settings.json（pi-flash 专
    // 属配置，绝不碰 pi 的 settings.json——pi 的 settings schema 有自己的
    // theme 键，写入未知主题名会让 pi 每次启动报错）
    if std::env::var("PI_FLASH_THEME").ok().and_then(|n| theme::set_by_name(&n).then_some(())).is_none() {
        if let Some(name) = app_settings().theme {
            theme::set_by_name(&name);
        }
    }
    // language: persisted workspace-memory preference
    if let Some(ix) = load_lang_pref() {
        i18n::set_lang(ix);
    }
    Application::new()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            // JetBrains Mono 随二进制打包：全应用唯一内置 mono（会话区 mono
            // + 终端网格；三档字重 + Italic 覆盖 400/600/700 与斜体）。
            // 注册失败仅回退系统字体，不致命。
            cx.text_system().add_fonts(vec![
                std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf").as_slice()),
                std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/JetBrainsMono-SemiBold.ttf").as_slice()),
                std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/JetBrainsMono-Bold.ttf").as_slice()),
                std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/JetBrainsMono-Italic.ttf").as_slice()),
            ])
            .expect("embedded mono fonts are valid TTF");
            // 系统字体目录（设置页字体下拉数据源，字母序；一次性枚举）
            crate::appearance::init_font_catalog(cx);
            // 面板字号 → 全局 UI 缩放（ui_size 基准）
            crate::appearance::sync_ui_scale();
            // gpui-component (widget library powering TextInput): global
            // init + token mapping from the active app theme
            gpui_component::init(cx);
            // composer 覆盖绑定：同深度("Input")后注册者优先，必须排在
            // gpui_component::init 之后（其绑定含 up/down/tab→组件移动/
            // 缩进）。被截获的键由 composer 的 on_action 处理（菜单导航/
            // 历史回溯/补全接受），非空多行时重新派发 MoveUp/MoveDown。
            cx.bind_keys([
                KeyBinding::new("up", ComposerUp, Some("Input")),
                KeyBinding::new("down", ComposerDown, Some("Input")),
                KeyBinding::new("tab", ComposerTab, Some("Input")),
                // 图片粘贴：同深度后注册者优先（覆盖组件 ctrl-v→Paste），
                // composer 内层处理图片附件，其余输入框由根节点兜底重派发
                KeyBinding::new("ctrl-v", ComposerPaste, Some("Input")),
                KeyBinding::new("cmd-v", ComposerPaste, Some("Input")),
                // 023 fileView：编辑器里 Ctrl+S 保存（composer 等其他 Input
                // 上下文内无人监听该 action，自然空转）
                KeyBinding::new("ctrl-s", FileSave, Some("Input")),
                KeyBinding::new("cmd-s", FileSave, Some("Input")),
            ]);
            appearance::sync_gpui_tokens(cx);
            // startup restore (§4)：每次启动默认最大化（位置不持久化——
            // gpui Windows 的外框/客户区坐标在存取间不对称，每个周期漂移
            // 一个边框宽）。取消最大化后的尺寸仍保存，供会话内还原参考。
            // 010-启动：先以普通窗口呈现启动页（不全屏），揭幕后由 render
            // 的 pending_zoom 补最大化——0.2.2 建窗即 Maximized 的延迟处理
            // 实测不生效，显式 zoom 是项目已验证路径（从建窗回调后移）。
            let _restored = get_window_state();
            let bounds = gpui::Bounds::centered(None, gpui::size(px(1180.), px(760.)), cx);
            let window_bounds = gpui::WindowBounds::Windowed(bounds);
            let automation_chat: std::sync::OnceLock<gpui::WeakEntity<Chat>> =
                std::sync::OnceLock::new();
            let window_handle = cx.open_window(
                WindowOptions {
                    window_bounds: Some(window_bounds),
                    // 最小宽度 900px（逻辑像素，gpui 按 scale_factor 换算再交给
                    // WM_GETMINMAXINFO）：窗口再缩也不至于把聊天列挤到不可用。
                    // 高度不设底（0 = 只保留边框开销），需要时再单独限。
                    window_min_size: Some(gpui::size(px(900.), px(0.))),
                    titlebar: Some(gpui::TitlebarOptions {
                        title: Some("pi-flash".into()),
                        // client-side title bar (v54 topbar 两段, window
                        // control hitboxes registered by titlebar.rs)
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                |window, cx| {
                    // gpui-component widgets require their Root as the window
                    // root view (renders their context-menu/popover layers)
                    let chat = cx.new(Chat::new);
                    let weak = chat.downgrade();
                    let _ = automation_chat.set(weak.clone());
                    // persist window bounds + shell layout on close so the
                    // startup restore layer has data (§4)
                    window.on_window_should_close(cx, move |window, cx| {
                        let b = window.bounds();
                        save_window_state(&WindowState {
                            x: f64::from(b.origin.x),
                            y: f64::from(b.origin.y),
                            w: f64::from(b.size.width),
                            h: f64::from(b.size.height),
                            maximized: window.is_maximized(),
                        });
                        if let Some(chat) = weak.upgrade() {
                            chat.update(cx, |chat, _cx| chat.persist_ui());
                        }
                        true
                    });
                    cx.new(|cx| gpui_component::Root::new(chat.into(), window, cx))
                },
            )
            .unwrap();
            // UI 自动化服务（pi-flash-2kq）：PI_FLASH_AUTOMATION=<port|auto|1>
            // 显式开启，缺省关闭——agent 调试用，不抢真实屏幕/鼠标。
            if let (Some(weak), Ok(spec)) = (
                automation_chat.get(),
                std::env::var("PI_FLASH_AUTOMATION"),
            ) {
                if let Err(e) =
                    automation::start(cx, window_handle.into(), weak.clone(), &spec)
                {
                    eprintln!("pi-flash automation: {e}");
                }
            }
            cx.activate(true);
        });
}
