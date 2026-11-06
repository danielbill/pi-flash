//! Chat 实体本体（P3 自 main.rs 拆出）：全应用状态实体 Chat + 工作区簇
//! WsUi + 跨域小 setter（set_status/set_content_view）。其余 7 簇界面状态住
//! ui_fields.rs；构造器（new 含全部字段初始化与各泵启动）住 boot.rs；焦点
//! 仲裁与 Focusable 住 focus.rs。域方法住各领域文件（session/pool、
//! session/input、editor/tabs……）；字段数与 impl 块数不写死在注释里
//! （随演进漂移，查数用 grep）。

use self::ui_fields::{AuxUi, CfgUi, ComposerUi, DlgUi, LayoutUi, PoolUi, PspUi};

use crate::*;

pub(crate) struct Chat {
    /// 周边面板簇（4ea）：ext UI / 更新 / 远程 / 系统提示词弹窗
    pub(crate) aux: AuxUi,
    /// 设置面板簇（4ea）：模型/技能/扩展/MCP 配置状态
    pub(crate) cfg: CfgUi,
    /// psp 列表簇（4ea）：项目/会话列表、搜索、通知点
    pub(crate) psp: PspUi,
    /// 弹窗/悬浮层簇（4ea）
    pub(crate) dlg: DlgUi,
    /// 会话池簇（4ea）：runtimes 池 + 指针 + 缓存 + 启动闸门
    pub(crate) pool: PoolUi,
    /// composer 输入面板簇（4ea，031）
    pub(crate) composer: ComposerUi,
    /// 壳布局簇（4ea）：dock/路由/分隔条/flyout/焦点
    pub(crate) layout: LayoutUi,
    /// 工作区簇（4ea）：文件树/编辑器/终端/git
    pub(crate) ws: WsUi,
    /// PF 插件（090-4/02 规范）：清单注册表 + 懒加载运行时 + 面板行缓存
    pub(crate) plugins: pf_plugin::PluginUi,
}

/// 工作区簇（4ea 字段重组）：文件树/fs 监听、编辑器 tab/md 预览、终端、
/// git 面板与 cwd/branch 根（021/022/023）。
pub(crate) struct WsUi {
    // workspace / project files
    pub(crate) project_files: Vec<String>,
    pub(crate) expanded_dirs: HashSet<PathBuf>,
    /// 文件树展平缓存（services::file_tree::flatten；渲染只读这份，
    /// 重建点 = 展开/折叠、git 刷新、fs 事件、切项目）。
    pub(crate) tree_rows: std::sync::Arc<Vec<services::file_tree::TreeRow>>,
    /// 021 拖拽悬停待自动展开的折叠目录（Zed hover_expand 同款）：
    /// 同目标去重定时器，离开即清（file_tree::tree_drag_wiring 维护）。
    pub(crate) tree_drag_expand: Option<PathBuf>,
    /// 082 文件树选中条目（点击/右键留存，行高亮 + F2 改名的对象；纯内存
    /// 态不持久化）。行存在性在使用点对 tree_rows 复核（外部删除/切项目
    /// 后残留 → F2 静默忽略）。
    pub(crate) tree_selected: Option<PathBuf>,
    /// 文件树滚动条共享状态（与会话列表 `psp.psp_sb_state` 同款分体：
    /// psp_scrollbar 的 hover 常显 + autohide 挂在状态上，两面板各持一份
    /// ——共用会让悬停一边持续续上另一边的淡出计时）
    pub(crate) tree_sb_state: crate::ui::psp_scrollbar::PspScrollbarState,
    // fs watch（services::watcher）：tx 永驻（每次挂 watch clone 一份），
    // rx 被 startup 的泵取走；watcher 句柄 drop 即解除监听。
    pub(crate) fs_watch_tx: std::sync::mpsc::Sender<()>,
    pub(crate) fs_watch_rx: Option<std::sync::mpsc::Receiver<()>>,
    pub(crate) fs_watch: Option<services::watcher::FsWatcher>,
    /// 023：**cwd 外**打开文件的单文件监听（key = 文件所在目录，非递归）。
    /// Zed 对不在任何 worktree 里的文件建 single-file worktree（只监听其
    /// 所在目录）同款；事件走同一条 fs 通道。打开/关闭/切项目后由
    /// `retain_file_watches` 重算，不再被需要的目录即卸载。
    pub(crate) file_watches: std::collections::HashMap<PathBuf, services::watcher::FsWatcher>,
    /// 021 操作栏「切换显示变更」：只列 git 变更文件（flatten changes_only）
    pub(crate) changes_only: bool,
    /// 021 Pin 收藏夹视图（与「只看变更」同级的第三种树视图，本质是文件
    /// 树过滤器）：开 = 树体列 flatten_pins 的收藏森林。会话态不持久化
    /// （启动默认文件树）；与 changes_only 互斥。
    pub(crate) pin_view: bool,
    /// 021 Pin 收藏夹：当前项目的收藏条目（绝对路径，存储序；展示时按
    /// 名称字母排）。装载点 = 启动 / remount_workspace（切项目换清单），
    /// 落盘走 services::workspace::save_pins_for。
    pub(crate) pinned: Vec<PathBuf>,
    /// 文件树状态（010/021，对齐 Zed 的 per-worktree 内存态）：每项目一份
    /// 展开集合 + Pin 视图开关。切项目不丢（Zed `expanded_dir_ids` 同构），
    /// 另加磁盘持久化（`__tree`，Zed 不做）。
    pub(crate) tree_memory: std::collections::HashMap<String, TreeMemory>,
    /// `expanded_dirs` / `pin_view` 当前属于哪个项目（切项目时据此归档，
    /// 用归档时的原拼写路径写盘）
    pub(crate) tree_owner: Option<(String, PathBuf)>,
    pub(crate) file_cache: std::collections::HashMap<PathBuf, FileTab>,
    /// 083 项目搜索状态（搜索条输入/开关/结果行模型；tab 单例，切 tab 不丢）
    pub(crate) search: crate::search::SearchUi,
    /// 083 搜索结果点击的行跳转请求（文件 tab 渲染帧消费：把编辑器光标
    /// 挪到目标行——`InputState::set_cursor_position` 要 `&mut Window`，
    /// 只有渲染帧同时拿得到 window 与 Chat）
    pub(crate) pending_jump: Option<(PathBuf, u32)>,
    pub(crate) panel_tabs: Vec<PanelTab>,
    pub(crate) active_panel_tab: Option<usize>,
    /// md 预览滚动条状态（MarkdownKind preview 面板的 v63-6 配方）
    pub(crate) md_preview_scrollbar: gpui_component::scroll::ScrollbarState,
    /// md 渲染预览块级虚拟化（抄 zed thread_view 的 list 架构）：MarkdownKind
    /// preview 面板用，ListState 每帧只建可视块；path 记当前文件，换文件时
    /// reset 归零滚动
    pub(crate) md_preview_list: gpui::ListState,
    pub(crate) md_preview_list_path: Option<PathBuf>,
    /// 082 三期：md 预览容器焦点句柄（key_context "MdPreview"，track_focus
    /// 挂 render_preview_body 容器）——预览态的键盘滚动焦点，逐帧仲裁在
    /// File+预览态补焦；切换/回收永远不碰内容区，预览是被动只读视图
    pub(crate) md_preview_focus: gpui::FocusHandle,
    // TEMP 探针（023 调试）：外部改动检测 运行数/命中数
    pub(crate) ext_probe: (u32, u32),
    /// 023：打开文件后待聚焦的编辑器（Zed 行为：开文件即聚焦；渲染帧
    /// 编辑器实体就绪后消费）
    pub(crate) pending_focus_file: Option<PathBuf>,
    /// tab 带落盘抑制（pi-flash-weia）：启动恢复期/恢复应用中不许中间态
    /// 写穿存档（open_session 的 set_content_view(Chat) 会触发 persist）
    pub(crate) tab_restore_busy: bool,
    // 023 fileView：菜单与面包屑兄弟菜单的弹层状态
    pub(crate) crumb_menu_dir: Option<PathBuf>,
    pub(crate) crumb_dd: gpui::Entity<crate::ui::DropdownState>,
    // terminals (content-area tabs)
    pub(crate) terminals: Vec<TerminalTab>,
    pub(crate) active_terminal: Option<usize>,
    pub(crate) term_seq: usize,
    /// caret-blink heartbeat phase (1Hz, term_cursor task) + paint-time
    /// "a focused terminal is on screen" flag that gates the heartbeat
    pub(crate) term_cursor_on: bool,
    pub(crate) term_cursor_focused: bool,
    pub(crate) term_events:
        Option<futures::channel::mpsc::UnboundedSender<(usize, alacritty_terminal::event::Event)>>,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) branch: String,
    // git panel
    pub(crate) git_files: Vec<GitFile>,
    pub(crate) git_add_del: (u64, u64),
    /// refresh_git（9rk0 后台化）的请求序号：后台结果落地时校验仍是最新
    /// 一次请求，陈旧结果作废
    pub(crate) git_refresh_gen: u64,
    /// git_files 是哪个 cwd 的数据（9rk0 后台化换入窗口里防串项目）
    pub(crate) git_files_cwd: PathBuf,
    pub(crate) git_selected: Option<PathBuf>,
    pub(crate) git_tab: function_panel::git_panel::GitTab,
    pub(crate) git_log: Vec<GitCommit>,
    pub(crate) git_error: Option<String>,
    pub(crate) git_commit_input: gpui::Entity<TextInput>,
}

impl Chat {
    /// 当前项目目录——None = 无项目态（启动未恢复、也未用任何显式动作进入
    /// 过项目）。cwd 的合法来源只有显式动作：恢复最后会话（其项目随之成为
    /// cwd）、切项目/打开项目、PI_FLASH_CWD。应用永不捏造（010 重设计）。
    ///
    /// 只给**展示/定向**类消费方用（会话栏项目名、会话目录钮、终端起点、
    /// 项目选择器勾选）。业务侧（会话归属、last_open、项目注册表、文件树）
    /// 仍走 [`Self::project`]——插件认领线不碰桌面项目（模块独立）。
    pub(crate) fn ui_project(&self) -> Option<&Path> {
        self.ws.cwd.as_deref()
    }

    pub(crate) fn project(&self) -> Option<&Path> {
        self.ws.cwd.as_deref()
    }

    /// 无项目态的键占位（空串）：只允许用于查内存 map（永不命中），禁止
    /// 做任何 FS/进程语义——那类路径必须走 [`Self::project`] 的 None 分支。
    pub(crate) fn project_key(&self) -> String {
        self.project()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default()
    }

    pub(crate) fn set_status(&mut self, msg: String, cx: &mut Context<Self>) {
        self.dlg.status_toast = Some((msg, std::time::Instant::now()));
        self.rt().update(cx, |r, _| r.status = String::new());
        let _ = cx;
    }

    pub(crate) fn set_content_view(&mut self, v: ContentView) {
        self.layout.content_view = v;
        if matches!(v, ContentView::Term | ContentView::File) {
            self.layout.browse_last = v;
        }
        // 前台视图变化也是 tab 带状态（重启要复原前台），变更点即落盘
        //（内容相同自动跳过写盘；启动恢复期被 tab_restore_busy 抑制）
        self.persist_tabs();
    }
}

mod boot;
mod focus;
mod ui_fields;
