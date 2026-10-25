//! 应用壳的界面状态类型簇（P3 自 main.rs 拆出）：弹窗/面板/内容区枚举、
//! psp 分组与悬浮卡、composer 菜单与 @ 索引状态、pill 弹层三件套、
//! 内容区 tab（PanelTab/FileTab）。全量 pub(crate)：crate 内自由可达。

use crate::*;

/// 一个项目的文件树内存态（对齐 Zed `expanded_dir_ids` 的 per-worktree
/// 结构）：展开集合 + Pin 视图开关。切项目不丢、可落盘恢复（010/021）。
#[derive(Clone, Default)]
pub(crate) struct TreeMemory {
    pub(crate) expanded: std::collections::HashSet<PathBuf>,
    /// 该项目是否停在 Pin 收藏夹视图
    pub(crate) pin: bool,
}

#[derive(Debug, Clone)]
pub(crate) enum Dialog {
    /// 042：新增 Provider 清单弹窗（两组：自定义 / API KEY；订阅组缓行）
    ProviderPicker {
        input: gpui::Entity<TextInput>,
        scroll: gpui::ScrollHandle,
    },
    GitDiff {
        path: PathBuf,
        patch: String,
    },
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
    ImagePreview {
        image: std::sync::Arc<gpui::Image>,
    },
    /// topbar ⋯ 菜单：系统提示词 / 工具定义（窗体 = 设置弹窗那套大卡片，
    /// 见 top_panels / ui::overlay::big_card）
    SessionInfo {
        kind: TopPanel,
    },
    /// 023 fileView：关闭带未保存修改的文件 tab 前确认。单关持一个 path；
    /// 标签右键批量关闭（tab 菜单移植 zed「N unsaved」）收拢整批待裁决
    /// 文件，弹窗三键对整批生效。持 path 不持 ix——弹窗存活期间的增删
    /// 不会让索引漂移。
    FileDirty {
        paths: Vec<PathBuf>,
    },
    /// 060 二维码弹窗（微信线扫码绑定/重建）：信息栏【重制二维码】触发，
    /// 未创建时打开微信线会话自动弹出；扫码成功泵自动关窗。
    WxQr,
    /// 002 §3 [新建会话] 确认（W1）：丢弃当前会话创建新会话？确认后走
    /// `wx_rotate_session`（老影子下线不删文件、新影子 `pf-wxbot-*`）。
    WxNewSession,
}

/// ⋯ 菜单的两个面：系统提示词原文 / 已加载工具的声明。各自画在设置弹窗
/// 同款窗体里（`Dialog::SessionInfo`），数据来自 transcript 重放。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TopPanel {
    System,
    Tools,
}

/// 左列面板（statusbar tab 互斥切换）。020-1 曾有三态 sessions/files/git，
/// 简化后 sessions/files 渲染已无差别（文件树并入整合面板）——2026-10
/// 重整：收敛为 `Default`（整合面板），`Git` 保留（automation 可达的
/// 独立视图），`Plugin(ix)` 为插件面板（快捷面板起）。
/// 微信面板**已迁出 dock**（2026-10-23 定夺）：入口收三横菜单，内容区

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DockPanel {
    /// 整合面板：会话列表(上) + 文件树(下)
    Default,
    /// git 独立视图（022；UI 入口已隐藏，automation panel.dock 可达）
    Git,
    /// 插件面板（ix = pf_plugin::PluginUi.plugins 下标；不跨重启持久化）
    Plugin(usize),
}

impl DockPanel {
    /// 持久化串。Plugin 态不落盘（重启回落默认整合面板）。
    pub(crate) fn as_str(&self) -> &'static str {
        match self {
            DockPanel::Default => "sessions", // 旧键名，读回兼容
            DockPanel::Git => "git",
            DockPanel::Plugin(_) => "sessions",
        }
    }

    /// automation `panel.dock` 入参 → 态（旧值 sessions/files 兼容映射）。
    pub(crate) fn from_dock_arg(panel: &str, plugin_ix: usize) -> Option<Self> {
        // "wx" 不再是 dock 面板（2026-10-23 迁内容区全屏页），automation
        // 侧在 PANEL_DOCK 入口先行截获重定向到 `Chat::open_wx`。
        match panel {
            "sessions" | "files" | "default" => Some(DockPanel::Default),
            "git" => Some(DockPanel::Git),
            "plugin" => Some(DockPanel::Plugin(plugin_ix)),
            _ => None,
        }
    }
}

/// 内容区视图状态机 (v54): chat 默认；terminal / markdown 预览以 topbar tab 打开。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContentView {
    Chat,
    Term,
    /// 文件查看 tab（PanelTab::File；主体渲染算法由 editor/kind.rs 的
    /// FileTypeHandler 按后缀决定：md 双态、图片查看器、代码/文本编辑器、
    /// 未注册走默认文本）
    File,
    /// 081 更新日志 tab（PanelTab::Changelog；markdown 渲染）
    Changelog,
    /// 040-1 Pi 扩展市场 tab（PanelTab::PiExtMarket；标准页）
    PiExtMarket,
    /// 040-2 Pi 原生扩展市场 tab（PanelTab::PiNativeMarket；pi.dev 目录页移植）
    PiNativeMarket,
    /// 090-4 PF 插件市场 tab（PanelTab::PfMarket；内置索引 + 台账）
    PfMarket,
    /// 083 项目搜索 tab（PanelTab::Search；搜索面板 + 结果列表）
    Search,
    /// 会话查询 tab（PanelTab::SessionSearch；同版式，只查询）
    SessionSearch,
    /// 060/002【微信通话】功能页：从管理（绑定/设置会话）到内部自管会话
    /// 的完整页（remote_control::panel::page）。就绪后由 open_wx/泵切回
    /// Chat 视图的微信线会话（wx_tab 依旧高亮）。
    Wx,
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
    /// PF 项目注册表最近打开时刻（ms；0 = 未注册）。仅作 newest() 排序
    /// 兑底：零会话的注册项目按打开时刻参与活动排序，不再垫底（004）。
    pub opened_ms: i64,
}

/// 004 打开项目菜单的一行：项目名（目录名）+ 路径。
#[derive(Debug, Clone)]
pub(crate) struct ProjectEntry {
    pub name: String,
    pub path: PathBuf,
}

/// 伪标签（draft 首条消息乐观上侧栏）：pi 还没落盘、没有真实 SessionInfo
/// 时的占位行。FileBound 落地时摘除换真身；prompt 被拒 / 进程死 / 轮末无
/// 文件时撤除。Chat 侧 Vec 保插入序（侧栏按挂标签先后排）。
#[derive(Debug, Clone)]
pub(crate) struct PendingTab {
    /// 所属 runtime 的池 key（draft-N）——点击据此 switch_to
    pub key: String,
    /// 首条消息首行（真标签落地后的 preview 与它一致，视觉无缝衔接）
    pub preview: String,
    /// 归属项目（draft 的 cwd）：渲染时插进匹配的项目组
    pub cwd: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AttachedImage {
    pub(crate) data_b64: String,
    pub(crate) mime: String,
    /// 缩略图渲染源（附加时预构建）：img() 按 Image id 缓存解码，避免
    /// 逐帧重哈希/重解码大图
    pub(crate) thumb: Option<std::sync::Arc<gpui::Image>>,
}

/// 090 排队卡拖拽进行中（psp 项目行拖拽同款三件套）：`at` = 最近一次
/// drag_move 时刻——gpui 对「窗口外松手」不派发任何事件，看门狗按
/// staleness 兜底清态（queue_cards.rs）。
#[derive(Debug, Clone)]
pub(crate) struct QueueDragState {
    pub(crate) at: std::time::Instant,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum MenuKind {
    Slash,
    At,
}

#[derive(Debug, Clone)]
pub(crate) struct MenuItem {
    pub(crate) insert: String,
    pub(crate) desc: String,
    /// @ 菜单条目的目录形态（决定插入文本 `@dir/` 不闭合 + 图标）
    pub(crate) is_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PillMenu {
    Thinking,
    Tools,
}

/// inputpanel 弹窗的统一定位锚点（用户定稿）：弹窗与触发按钮**水平居中**、
/// 底缘距按钮**顶缘 5px**（不遮按钮）。坐标来自按钮 paint 期的 canvas 捕获。
/// `right_x` 供右侧按钮（如 tools）做**右对齐**锚定：菜单右缘贴按钮右缘。
#[derive(Debug, Clone, Copy)]
pub(crate) struct PillAnchor {
    pub center_x: gpui::Pixels,
    pub right_x: gpui::Pixels,
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
            .map(|b| PillAnchor {
                center_x: b.center().x,
                right_x: b.right(),
                top: b.origin.y,
            })
            .unwrap_or(PillAnchor {
                center_x: at.x,
                right_x: at.x,
                top: at.y,
            })
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

/// One content-area tab: a terminal session, a file viewer, or the changelog.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PanelTab {
    Term(usize),
    File(PathBuf),
    /// 081 更新日志（新版本首启自动打开；无外部资源，纯渲染）
    Changelog,
    /// 040-1 Pi 扩展市场（标准页；菜单-三横按钮打开）
    PiExtMarket,
    /// 040-2 Pi 原生扩展市场（pi.dev Package Catalog 移植；菜单-三横按钮打开）
    PiNativeMarket,
    /// 090-4 PF 插件市场（内置索引 + 台账；菜单-三横按钮打开）
    PfMarket,
    /// 083 项目搜索（单例 tab；Ctrl+Shift+F 打开，Zed ProjectSearchView 同位）
    Search,
    /// 会话查询（单例 tab；Ctrl+K 打开，与 083 同版式但**只查询**）
    SessionSearch,
}

/// tab 稳定键（2026-10-14 标签拖拽换位引入）：拖拽发起时记 key，drop 时
/// 按 key 重查下标再 move——拖拽中途列表变化（异步关 tab 等）不会把落
/// 位算到漂移后的错误下标上（zed 用 item id 同语义）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TabKey {
    Term(usize),
    File(PathBuf),
    Changelog,
    PiExtMarket,
    PiNativeMarket,
    PfMarket,
    Search,
    SessionSearch,
}

impl PanelTab {
    pub(crate) fn key(&self) -> TabKey {
        match self {
            PanelTab::Term(id) => TabKey::Term(*id),
            PanelTab::File(p) => TabKey::File(p.clone()),
            PanelTab::Changelog => TabKey::Changelog,
            PanelTab::PiExtMarket => TabKey::PiExtMarket,
            PanelTab::PiNativeMarket => TabKey::PiNativeMarket,
            PanelTab::PfMarket => TabKey::PfMarket,
            PanelTab::Search => TabKey::Search,
            PanelTab::SessionSearch => TabKey::SessionSearch,
        }
    }
}

/// topbar 标签右键菜单的目标 tab（zed tab_bar 菜单移植）：置顶会话 tab
/// 恒存不可关（菜单无 Close、Close Left 恒灰）；自由 tab 按下标定位
/// （菜单存续期 panel_tabs 冻结——浮层 occlude 全窗，无人能动它）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TabMenuTarget {
    Session,
    Panel(usize),
}

impl PanelTab {
    /// 该 tab 对应的内容区视图（切换/关闭回退共用）。
    pub(crate) fn view(&self) -> ContentView {
        match self {
            PanelTab::Term(_) => ContentView::Term,
            PanelTab::File(_) => ContentView::File,
            PanelTab::Changelog => ContentView::Changelog,
            PanelTab::PiExtMarket => ContentView::PiExtMarket,
            PanelTab::PiNativeMarket => ContentView::PiNativeMarket,
            PanelTab::PfMarket => ContentView::PfMarket,
            PanelTab::Search => ContentView::Search,
            PanelTab::SessionSearch => ContentView::SessionSearch,
        }
    }
}

/// 外部改动冲突类型（023）：文件在磁盘上被改/删，而本缓冲区有未保存修改。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FileConflict {
    /// 磁盘内容已变；banner 提供重新加载
    Changed,
    /// 文件已从磁盘消失（zed DiskState::Deleted 同构，2026-10-11 改版）：
    /// 缓冲保留可继续编辑、tab 标题删除线、不弹横幅；手动保存即重建
    /// （含父目录），自动保存对它保持沉默（防误重建）
    Deleted,
}
// 023 文件编辑展示页：编辑器型文件（代码/文本/md 源码态，由 editor/kind.rs
// 注册表决定）都进 CodeEditor（Zed parity——Zed 里不分文件大小、一律可
// 编辑）。曾按 50K 行上限加过「超限转只读虚拟化列表」的特殊分支，已删除：
// 特殊路径既不是 Zed 行为，也让大文件不可编辑。图片走 ImageKind 查看器
// （无编辑器）；html/htm 直接开源码 tab（2026-10-12 二改拍板），「在浏览
// 器中打开」保留为右键菜单首位文件动作。
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
    /// 非文本 tab（zed：ImageItem / InvalidItemView 都是「非 Buffer 视图」
    /// 同构）：无文本缓冲无编辑器——保存/自动保存/外部改动检测按此位全跳
    /// 过。binary=true 且 image=Some → 图片查看 tab（zed ImageView）；
    /// image=None → 「无法打开」灰屏（zed InvalidItemView）
    pub(crate) binary: bool,
    /// 图片 tab 的渲染格式（open_file_tab 认领时 magic 嗅探所得；灰屏为
    /// None）。渲染帧 view.rs 按它分流 render_image_surface/unsupported_body
    pub(crate) image: Option<gpui::ImageFormat>,
    /// 磁盘文件编码（zed Buffer.encoding 同语义，file_content::decode_text
    /// 所得）：保存按它编码回盘，BOM 往返保持
    pub(crate) encoding: &'static encoding_rs::Encoding,
    pub(crate) has_bom: bool,
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
            binary: false,
            image: None,
            // 打开/重载路径由 decode_text 覆写；UTF_8 是磁盘缺省（zed 同）
            encoding: encoding_rs::UTF_8,
            has_bom: false,
        }
    }
}
