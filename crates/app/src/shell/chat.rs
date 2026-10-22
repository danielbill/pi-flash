//! Chat 实体本体（P3 自 main.rs 拆出）：全应用状态字段簇 + 构造器
//! （new 含全部字段初始化与各泵启动）+ 跨域小 setter（set_status/
//! set_content_view）+ 焦点仲裁辅助（file_editor_focus）与 Focusable。
//! 域方法住各领域文件（session/pool、session/input、editor/tabs……）；
//! 字段数与 impl 块数不写死在注释里（随演进漂移，查数用 grep）。

use crate::function_panel::{HoverCard, ProjCard, TreeEdit, TreeMenu};
use crate::session::input::{AtFilterCache, AtIndexState};
use crate::settings::models::McProbe;
use crate::settings::pi_extensions::PkgOp;
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
}

/// 周边面板状态簇（4ea 字段重组）：扩展 UI 面 / 081 自动更新与更新日志 /
/// 060 远程控制桥 / 系统提示词·工具定义弹窗。
pub(crate) struct AuxUi {
    // extension UI surface (active session)
    pub(crate) ext_status: Vec<(String, String)>,
    pub(crate) ext_widgets: Vec<(String, Vec<String>, bool)>,
    pub(crate) ext_dialog: Option<pi_link::protocol::ExtensionUiRequest>,
    pub(crate) ext_input: gpui::Entity<TextInput>,
    /// ask_user_question 整卷问卷卡（ask_card.rs）；None = 无进行中的问卷
    pub(crate) ask: Option<ask_card::AskCard>,
    /// 问卷卡「自定义答案」输入框（active 题内嵌，切题时同步存量）
    pub(crate) ask_input: gpui::Entity<TextInput>,
    pub(crate) ext_notice: Option<(String, u8)>,
    /// 081 自动更新状态（新会话页 inputpanel 右下版本号位消费）
    pub(crate) update: services::updater::UpdateState,
    /// 081 更新日志 tab 数据（新版本首启自动打开；None = 无页）
    pub(crate) changelog: Option<services::updater::ChangelogPage>,
    /// 更新日志页滚动句柄
    pub(crate) changelog_scroll: gpui::ScrollHandle,
    /// 082 Pi 扩展更新检测状态（新会话页通知 + 设置·扩展页箭头/按钮消费）
    pub(crate) pi_extension_updates: services::pi_extension_updates::UpdatesState,
    /// 040-1 市场域状态簇（Pi 扩展市场页；后续多个市场在此并列）
    pub(crate) market: crate::market::MarketUi,
    /// 微信远程控制桥（060）：transport + 待答 ExtUi 请求
    pub(crate) remote: remote_control::RemoteControl,
    /// 工具定义弹窗左列表选中的工具名（pi-web ToolDefinitionsPanel 的
    /// selectedToolName）
    pub(crate) tool_sel: Option<String>,
    /// 系统提示词弹窗的滚动 handle（psp 滚动条要跨渲染读同一份滚动位）
    pub(crate) sysprompt_scroll: gpui::ScrollHandle,
    /// 系统提示词面板各桶「调用声明」折叠块的展开态（按 SYSTEM_BUCKETS 下标）
    pub(crate) decl_open: [bool; 7],
}

/// 设置面板状态簇（4ea 字段重组）：模型/技能/扩展/MCP 页的全部
/// 状态与 CLI op 泵通道（settings 句柄本身在 dlg 簇）。
pub(crate) struct CfgUi {
    // settings panel data
    pub(crate) mc_patterns: Option<Vec<String>>,
    pub(crate) mc_state: EnabledState,
    pub(crate) mc_creds: Vec<(String, pi_link::config::CredentialKind)>,
    pub(crate) mc_project_scope: bool,
    /// settings.json defaults a new session starts with (pi-web /api/models
    /// `defaultModel` + `defaultThinkingLevel`; selectInitialModelScope inputs)
    pub(crate) mc_default_model: Option<(String, String)>,
    pub(crate) mc_default_thinking: Option<String>,
    /// settings.modelThinkingLevels — per-`provider/modelId` recorded levels
    pub(crate) mc_model_thinking: Vec<(String, String)>,
    pub(crate) mc_skills: Vec<pi_link::skills::SkillEntry>,
    /// Settings-子代理页的 Agent 定义（044-1 §2）：内置 + 自定义（自定义在前，
    /// 同名覆盖内置）。
    pub(crate) mc_agents: Vec<pi_link::agents::AgentProfile>,
    pub(crate) mc_pkgs_global: Vec<serde_json::Value>,
    pub(crate) mc_pkgs_project: Vec<serde_json::Value>,
    /// 扩展页 CLI op（install/remove）进行中：转圈按 op 种类落按钮；op 泵
    /// 收到完成消息即清零（泵同时重读 globals + 刷新列表面板）
    pub(crate) pkg_op: Option<PkgOp>,
    /// 卸载确认浮层（040：点垃圾桶先确认）：待卸载来源
    pub(crate) pkg_confirm_remove: Option<String>,
    pub(crate) mc_default_tools: Option<Vec<String>>,
    /// models.json 编辑缓冲（设置·模型页；表单头保存即写盘，无页面级 footer）
    pub(crate) mc_models_json: serde_json::Value,
    pub(crate) mc_mj_error: Option<String>,
    /// 042：provider 通路探查状态（保存 key 后自动一次 + 详情页手动重试）
    pub(crate) mc_probe: std::collections::HashMap<String, McProbe>,
    /// 042 新增 Provider 弹窗的全量注册表（registry.rs dump；None = 未加载）
    pub(crate) provider_registry: Option<std::sync::Arc<pi_link::registry::RegistryDump>>,
    /// dump 拉取状态（Idle = 待拉/可拉，Loading = 后台进行中，Failed = 上次失败）
    pub(crate) registry_state: crate::session::model_picker::RegistryState,
    /// 042：【移除】内置商的确认弹窗（provider id；None = 没开）
    pub(crate) mc_remove_confirm: Option<String>,
    /// mcp.json 全局 + 项目服务器（设置·MCP 页）
    pub(crate) mcp_servers: Vec<pi_link::mcp::ServerEntry>,
    pub(crate) mcp_errors: Vec<String>,
    pub(crate) op_tx: Option<futures::channel::mpsc::UnboundedSender<String>>,
}

/// psp 列表簇（4ea 字段重组）：项目组/会话列表面板、会话搜索、通知点、
/// 伪标签与窗口焦点快照。
pub(crate) struct PspUi {
    // ---- v54 shell state ----
    /// psp 项目组（当前项目钉顶，其余按最近会话倒序；启动只加载
    /// 设置.默认加载会话数 N 个会话，组由这批会话的 cwd 自然形成）
    pub(crate) projects: Vec<ProjectGroup>,
    /// 004 打开项目菜单：最近 30 天活动项目（弹窗打开时后台扫描回填）
    pub(crate) project_hits: Vec<ProjectEntry>,
    /// 004 打开项目菜单的搜索词（输入框 on_change 镜像）
    pub(crate) project_filter: String,
    /// 打开项目菜单的键盘选中行：0 = 【打开文件夹】，1..=N = 过滤后的项目行。
    /// 打开/列表回填/改过滤词时落默认（当前项目行，不在列则回退），↑/↓ 移动。
    pub(crate) project_sel: usize,
    /// 当前活跃会话文件（psp 选中态；switch_to 时更新）
    pub(crate) active_file: Option<PathBuf>,
    pub(crate) list_mode: ListMode,
    pub(crate) sort_mode: SortMode,
    pub(crate) collapsed_keys: HashSet<String>,
    /// psp 分页：每组已展开的会话数（「显示更多」每次 +10；
    /// key = 组 ws_key，Flat 模式 = "__flat__"）
    pub(crate) psp_shown: std::collections::HashMap<String, usize>,
    /// psp 会话列表滚动（滚动条数据源）
    pub(crate) psp_scroll: gpui::ScrollHandle,
    pub(crate) psp_sb_state: crate::ui::psp_scrollbar::PspScrollbarState,
    // session content search (013): dialog query input + background results
    pub(crate) search_hits: Vec<pi_link::sessions::SearchHit>,
    pub(crate) search_truncated: bool,
    pub(crate) search_running: bool,
    pub(crate) search_needle: String,
    pub(crate) search_gen: u64,
    /// 会话查询面板（083 同款版式，只查询）：查询框懒建
    pub(crate) session_query: Option<gpui::Entity<crate::ui::TextInput>>,
    /// 会话查询里被收起的会话块（键 = 会话文件路径）
    pub(crate) session_collapsed: std::collections::HashSet<String>,
    /// 会话查询结果列表（变高块列表：会话块 = 组头 + 片段行；与项目搜索同款
    /// 虚拟化，避免自建滚动容器在长行时把宽度顶爆）
    pub(crate) session_list: gpui::ListState,
    pub(crate) session_sb_state: gpui_component::scroll::ScrollbarState,
    pub(crate) pending_locate: Option<(PathBuf, Option<i64>, String)>,
    pub(crate) unread: HashSet<PathBuf>,
    /// 出错红点（agent 结束时窗口无焦点就点亮，重新获得焦点后切入清除）——
    /// 与 unread 同生命周期管理，区别只是颜色语义
    pub(crate) turn_errors: HashSet<PathBuf>,
    /// 内部中断黄点（stopReason = length/aborted：上限截断、中止）
    pub(crate) turn_warnings: HashSet<PathBuf>,
    /// 伪标签（draft 首条消息乐观上侧栏）：pi 落盘前 Sidebar 的占位行。
    /// 挂 = PendingTab(Some)；摘 = FileBound 换真身 / 发送被拒 / 进程死 /
    /// 轮末确定无文件。纯内存态——没落盘的 draft 重启后本就不存在。
    pub(crate) pending_tabs: Vec<PendingTab>,
    /// 窗口焦点快照（render 每帧写；焦点变化本身会触发重绘所以不陈旧）。
    /// 「agent 结束时窗口没焦点就给点」的判定依据（2026-10-10 定夺）
    pub(crate) window_active: bool,
    pub(crate) hovered_project: Option<usize>,
    /// 020 项目用户排序（拖拽产物的 ws_key 有序表）：Some = 用户拖过，
    /// 启动与运行期重建都按这份排；None = 无记录，首建按活动序排一次、
    /// 此后运行期重建保序（切换会话不再自动置顶）。
    pub(crate) proj_order: Option<Vec<String>>,
    /// 020 项目拖拽进行中（key + 最近 drag_move 时刻）：组收起为标签与
    /// 插入指示线的依据。列表外松手取消不派发事件，staleness 由看门狗
    /// 兜底清理（psp_drag::spawn_watchdog）。
    pub(crate) proj_drag: Option<crate::function_panel::psp_drag::ProjDragState>,
    /// 020 拖拽落点插入位（projects 下标 0..=len；None = 无有效落点/原地）
    pub(crate) proj_drop_ix: Option<usize>,
    /// 020 避让位移动画实例（from/to = 前后落点 + 起始时刻）：渲染期按
    /// elapsed 插值各项目块的 ±32px 平移，到点由驱动清场落回静态位移。
    pub(crate) proj_shift: Option<crate::function_panel::psp_drag::ProjShift>,
    /// 会话置顶（pin）：绝对路径有序表，index 0 = 最近置顶（后置顶的总在
    /// 最上面）。持久化在 UiState.session_pins；pin/unpin 都走
    /// [`Chat::session_pin_toggle`] 单一写路。
    pub(crate) session_pins: Vec<PathBuf>,
    /// 悬停中的会话行（pin 槽位显隐/可点的依据；path 判等——置顶组与原组
    /// 两份行同 path，悬停其一两处同亮，无害）。与 hovered_project 同款：
    /// 只在翻转时 notify。
    pub(crate) hovered_session: Option<PathBuf>,
}

/// 弹窗/悬浮层簇（4ea 字段重组）：模态槽、pill 弹层三件套、各域右键
/// 菜单与行内编辑/确认槽、ctx 提示悬停、toast、设置面板句柄。
pub(crate) struct DlgUi {
    pub(crate) dialog_focus: FocusHandle,
    pub(crate) dialog: Option<Dialog>,
    // shell surfaces
    pub(crate) pill_menu: Option<PillMenu>,
    /// 「full+」档的扩展选择面板（勾选即生效；滚动位跨帧复用）
    pub(crate) pi_extension_picker: Option<crate::session::pi_extension_picker::PiExtensionPicker>,
    /// MCP 勾选菜单（043；按钮常驻渲染，菜单从按钮开合）
    pub(crate) mcp_picker: Option<crate::session::mcp_picker::McpPicker>,
    /// 042：inputpanel 模型菜单（锚定 pill 卡片，替代原居中 ModelSelect 弹窗）
    pub(crate) model_picker: Option<crate::session::model_picker::ModelPicker>,
    /// 打开弹窗的那个按钮的锚点（水平中点 + 顶缘）——inputpanel 全部弹窗
    /// 统一定位：与按钮居中、距 5px、不遮按钮（用户定稿）
    pub(crate) pill_anchor: Option<PillAnchor>,
    /// 三个胶囊按钮的 paint 期 bounds（canvas 捕获，跨帧槽位）
    pub(crate) pill_btn: PillBtns,
    /// ctx-ring 悬浮详情（v58 响应式）：环/面板两面悬停标志 + 淡出起始
    /// 时刻（input.rs ctx_tip_hover 状态机持有）
    pub(crate) ctx_tip_ring_hover: bool,
    pub(crate) ctx_tip_panel_hover: bool,
    pub(crate) ctx_tip_closing: Option<std::time::Instant>,
    // inline rename (active session)
    pub(crate) renaming: Option<PathBuf>,
    pub(crate) rename_input: Option<gpui::Entity<TextInput>>,
    pub(crate) confirm_delete: Option<PathBuf>,
    pub(crate) proj_tip: Option<(PathBuf, f32, f32)>,
    pub(crate) hover_card: Option<HoverCard>,
    /// 项目行右键详情卡（020：样式对齐会话详情卡；渲染挂 psp_overlays）
    pub(crate) proj_card: Option<ProjCard>,
    /// 021 文件树右键菜单状态槽（渲染挂 psp_overlays 同层）
    pub(crate) tree_menu: Option<TreeMenu>,
    /// 文件树标题项目下拉（树根跨项目快切）：Some = 开着，sel = 键盘高亮
    /// 行（psp.projects 下标，默认落当前项目）。渲染挂 psp_overlays。
    pub(crate) tree_proj_menu: Option<crate::function_panel::TreeProjMenu>,
    /// 021 行内新建/改名（Zed EditState 移植）：非空 = 树里有一行编辑器
    /// （渲染挂 file_tree；焦点走主 render 焦点保持链）
    pub(crate) tree_edit: Option<TreeEdit>,
    /// 编辑区右键菜单（zed 式：编辑块 + 文件动作块；渲染挂 psp_overlays
    /// 同层）。(文件路径, 鼠标 x, y)
    pub(crate) editor_menu: Option<(PathBuf, f32, f32)>,
    /// 终端右键菜单（复制 / 粘贴 / 全选；渲染挂 psp_overlays 同层）。
    /// (终端 tab id, 鼠标 x, y)
    pub(crate) term_menu: Option<(usize, f32, f32)>,
    /// 输入面板右键菜单（031：编辑块 撤销/恢复/全选 + 剪切/复制/粘贴/
    /// 删除；渲染挂 psp_overlays 同层）。(鼠标 x, y)
    pub(crate) input_menu: Option<(f32, f32)>,
    /// 消息内容区右键菜单（单项「复制」= 复制当前消息划选；渲染挂
    /// psp_overlays 同层）。(鼠标 x, y)
    pub(crate) msg_menu: Option<(f32, f32)>,
    /// topbar 标签右键菜单（zed tab_bar 菜单移植：Close/Others/Left/
    /// Right/All；渲染挂 psp_overlays 同层）。(目标 tab, 鼠标 x, y)
    pub(crate) tab_menu: Option<(crate::TabMenuTarget, f32, f32)>,
    /// 021 文件树删除确认（统一 overlay::confirm；待删路径）
    pub(crate) tree_confirm_del: Option<PathBuf>,
    /// 删除失败持久弹层（trash Err 不再只靠状态栏一闪，用户可直接回报
    /// 具体错误）
    pub(crate) tree_del_error: Option<String>,
    /// 删除在飞清单（trash 已下沉后台线程：Windows 回收站删大目录可达
    /// 分钟级，不能占主线程）。用于同路径去重投递 + 完成归位。
    pub(crate) tree_deleting: Vec<PathBuf>,
    pub(crate) status_toast: Option<(String, std::time::Instant)>,
    pub(crate) settings: Option<gpui::Entity<settings::SettingsPanel>>,
    /// 010 重设计 send 门：无项目态下用户敲下的文字在弹「打开项目」前
    /// 暂存于此，switch_project 选定目录后回填并自动发送（话不丢）
    pub(crate) pending_send: Option<String>,
    /// 子代理浮层面板（044-1 §1）：Some = 开着（1s 轮询启动），None = 关闭
    /// （轮询停止）。rows 是累积的 descriptor 投影，offset 是下一轮续读起点。
    pub(crate) subagent_panel: Option<crate::session::SubagentPanel>,
    /// 040-1 topbar 三横「菜单」浮层：Some = 开着，值为按钮锚点（x, y）。
    /// 渲染挂 render.rs 根（与 psp_overlays 同层），菜单项目前只有
    /// 「Pi扩展市场」。
    pub(crate) market_menu: Option<(f32, f32)>,
}

/// 会话池簇（4ea 字段重组）：常驻 runtime 池 + 注意力指针 + 索引缓存
/// + 启动装载的全局态与闸门（010）。
pub(crate) struct PoolUi {
    pub(crate) runtimes:
        std::collections::HashMap<String, gpui::Entity<session::runtime::SessionRuntime>>,
    pub(crate) active_key: String,
    pub(crate) sessions: Vec<SessionInfo>,
    pub(crate) session_tail_cache: std::collections::HashMap<PathBuf, Vec<Msg>>,
    pub(crate) draft_seq: usize,
    /// model catalog keyed by cwd — **进程答案覆盖层**（010-启动.md §7）：活着的
    /// runtime 的 RPC `get_available_models` 答复写这里（项目级差异），无进程时
    /// 消费方回落 `globals.models`（启动时从磁盘 ∪ 自有缓存装载）。
    pub(crate) models_by_cwd: std::collections::HashMap<String, Vec<pi_link::protocol::ModelInfo>>,
    /// 启动装载的全局态（模型清单 / 命令 / 默认项 / 插件 / 全局 mcp）：渲染只读，
    /// 不再扫盘也不发 RPC（010-启动.md §1、§7）。
    pub(crate) globals: startup::Globals,
    /// 项目上下文（启动集合内逐项目装载；切项目命中即零扫盘，见 §5）。
    pub(crate) project_ctx: std::collections::HashMap<String, startup::ProjectCtx>,
    /// 040 延迟加载：扩展/技能说明 token 数（source → 测量值；启动时读缓存，
    /// 揭幕后后台探针刷新）。source 缺项 = 尚未测得（UI 显示「—」）
    pub(crate) ext_tokens: std::collections::HashMap<String, services::token_probe::TokenEntry>,
    /// 010-启动：启动页闸门；揭幕帧由 pending_zoom 补 §4 最大化
    pub(crate) booted: bool,
    /// 010 重设计：启动恢复已定案（进入项目或维持无项目态）。无项目态没有
    /// 可附着的进程，揭幕闸门据此放行，不必等 SPLASH_TIMEOUT 兜底。
    pub(crate) boot_resolved: bool,
    pub(crate) pending_zoom: bool,
}

/// composer 输入面板簇（4ea 字段重组，031）：草稿/附件/历史、输入实体
/// 惰性槽、@·斜杠补全菜单状态、消息气泡渲染辅助。
pub(crate) struct ComposerUi {
    pub(crate) input: String,
    pub(crate) pending_images: Vec<AttachedImage>,
    pub(crate) history_ix: Option<usize>,
    pub(crate) history: Vec<String>,
    // editor view state（输入组件实体在 composer 首次渲染时惰性创建；
    // 真输入框 = gpui-component InputState，光标/选区/IME/滚动条全内置）
    pub(crate) composer: Option<gpui::Entity<ComposerInput>>,
    /// composer 胶囊实时高度（ui::measure_height 每帧写、读到的是上一帧
    /// 值；导航刻度条「屏高 − inputpanel/2」居中用，033）
    pub(crate) composer_h: std::rc::Rc<std::cell::Cell<f32>>,
    pub(crate) input_focused: bool,
    pub(crate) menu_ix: usize,
    /// / 菜单滚动句柄（按键选中 scroll_to_item 行跟随；输入变化回顶）
    pub(crate) menu_scroll: gpui::ScrollHandle,
    /// 斜杠/@ 菜单被点外收起（active_menu 据此返回 None；改输入即复位）
    pub(crate) menu_dismissed: bool,
    /// @ 文件索引缓存（key = cwd 字符串；031 输入面板）
    pub(crate) at_index: std::collections::HashMap<String, AtIndexState>,
    /// @ 菜单过滤结果记忆化（menu_items 渲染期是 &self，走 RefCell 内部
    /// 可变性，先例 bubble_scrolls；见 AtFilterCache）
    pub(crate) at_filter_cache: std::cell::RefCell<Option<AtFilterCache>>,
    /// 技能展开消息的展开态（key = entry_id 或序号键）
    pub(crate) expanded_skills: std::collections::HashSet<String>,
    /// 操作栏悬停态（pi-web onMouseEnter/Leave parity，状态驱动而非
    /// group_hover）：用户消息行与 agent 轮块**共用**这一个字段，值是行在
    /// 列表里的索引（用户行 = msg_ix，agent 轮 = 轮首 msg_ix；角色不同故
    /// 永不撞车）。唯一写入点 = messages::bar_hover_wired。
    pub bar_hover: Option<usize>,
    /// 090 排队卡拖拽进行中 + 落点（queue_cards.rs 三件套；ComposerUi 持
    /// 是因为拖拽是 composer 的 UI 态，队列数据本体在 SessionRuntime）
    pub(crate) queue_drag: Option<QueueDragState>,
    pub(crate) queue_drop_ix: Option<usize>,
    /// 消息气泡滚动句柄（key 同 expanded_skills；跨帧保持滚动位置并供
    /// gpui-component Scrollbar 读取）
    pub(crate) bubble_scrolls:
        std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, gpui::ScrollHandle>>>,
}

/// 壳布局簇（4ea 字段重组）：dock 面板、内容区路由、分隔条拖拽、
/// 导航 flyout 与焦点仲裁锚。
pub(crate) struct LayoutUi {
    pub(crate) dock_panel: DockPanel,
    pub(crate) slp_w: f32,
    pub(crate) panes_hidden: bool,
    pub(crate) slp_drag: Option<(f32, f32)>,
    /// 分隔线响应区悬停（线加深加粗 + col_resize 光标已由 resizer 承担）
    pub(crate) slp_hover: bool,
    /// 020-1 整合面板：会话子面板高度占比（0..1，分隔线拖拽/双击复位 0.5，
    /// 持久化在 UiState.dock_split）
    pub(crate) dock_split: f32,
    /// 020-1 子面板最小态（= 对面最大化）。最大化语义：点谁谁填满、对面
    /// 收起到控制栏；拖分隔线即双双退出最小态。会话级，不持久化。
    pub(crate) sess_min: bool,
    pub(crate) files_min: bool,
    /// 子面板分隔线拖拽进行中（app-body 层跟踪 mouse move/up）
    pub(crate) split_drag: bool,
    /// dock 容器 bounds（canvas 每帧捕获，分隔线拖拽换算占比用；PillBtns
    /// 同款 Rc<RefCell> 绕 paint 期实体借用限制）
    pub(crate) dock_bounds: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    /// 文件树操作栏项目名（ft-project）paint 期 bounds：项目下拉的锚点
    /// （贴标题下缘左对齐；dock_bounds 同款 canvas 捕获）
    pub(crate) ft_title_bounds: std::rc::Rc<std::cell::RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    pub(crate) content_view: ContentView,
    /// 浏览操作区的最后视图（Term/File）：文件树标签点击时恢复
    pub(crate) browse_last: ContentView,
    pub(crate) nav_open: bool,
    pub(crate) nav_hide_at: Option<std::time::Instant>,
    pub(crate) nav_flyout_hovered: bool,
    /// flyout 内鼠标所在轮（选择框/比例尺亮点跟随鼠标）
    pub(crate) nav_hover_turn: Option<usize>,
    /// 导航 flyout 卡片列表虚拟化句柄（v63-6：只建可视卡片；跨开合保持
    /// 滚动位）
    pub(crate) nav_flyout_list: gpui::ListState,
    /// topbar 自由标签区的横向滚动句柄（zed `tab_bar_scroll_handle` 同款）：
    /// 供 `activate_panel_tab` 调 `scroll_to_item(ix)` 让激活 tab 跟随滚进
    /// 视口（Ctrl+←/→ 切到屏外 tab 时不再「激活了但看不见/点不到」）；
    /// 必须跨帧复用（每帧 new = 滚动位全丢，见 docs/agents/gpui.md）
    pub(crate) tabs_scroll: gpui::ScrollHandle,
    pub(crate) focus: FocusHandle,
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
    pub(crate) fn new(cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let dialog_focus = cx.focus_handle();
        // 010 重设计（2026-10-17 用户定案）：**cwd 是会话的属性，不是应用状态**
        // ——启动永远从无项目态出发（ws.cwd = None），应用不捏造任何当前目录
        //（进程 cwd / exe 目录 / 主目录兜底整族删除；payload 死循环一类的根
        // 因不复存在）。cwd 的合法来源只剩：
        //   ① 启动恢复「最后工作的会话」（startup_restore=on，recents 全局
        //      最新一条，其所属项目随之成为 cwd）——spawn_boot_restore 负责；
        //   ② 显式意图：切项目/打开项目、PI_FLASH_CWD（开发覆盖，同走 ① 通道）。
        // 无项目态下 composer 发送会弹「打开项目」先要一个目录（send 门）。
        let ui = ui_state();
        // fs watch 通道：tx 永驻 Chat，rx 交给 startup 的泵任务
        let (fs_watch_tx, fs_watch_rx_ch) = std::sync::mpsc::channel::<()>();

        let mut chat = Self {
            // v54.4 定案：启动固定会话界面（面板/内容区绑定关系见
            // status_bar；不恢复上次离开时的面板）
            ws: WsUi {
                project_files: Vec::new(),
                expanded_dirs: HashSet::new(),
                tree_rows: std::sync::Arc::new(Vec::new()),
                tree_drag_expand: None,
                tree_selected: None,
                tree_sb_state: crate::ui::psp_scrollbar::PspScrollbarState::new(),
                fs_watch: None,
                file_watches: std::collections::HashMap::new(),
                changes_only: ui.tree_changes_only,
                pin_view: false,
                pinned: Vec::new(),
                tree_memory: std::collections::HashMap::new(),
                tree_owner: None,
                file_cache: std::collections::HashMap::new(),
                search: crate::search::SearchUi::default(),
                pending_jump: None,
                panel_tabs: Vec::new(),
                active_panel_tab: None,
                crumb_menu_dir: None,
                crumb_dd: cx.new(|_| crate::ui::DropdownState::new()),
                ext_probe: (0, 0),
                pending_focus_file: None,
                terminals: Vec::new(),
                active_terminal: None,
                term_seq: 0,
                term_cursor_on: true,
                term_cursor_focused: false,
                term_events: None,
                branch: String::new(),
                git_files: Vec::new(),
                git_selected: None,
                git_tab: function_panel::git_panel::GitTab::Changes,
                git_log: Vec::new(),
                git_error: None,
                git_add_del: (0, 0),
                git_refresh_gen: 0,
                git_files_cwd: PathBuf::new(),
                md_preview_scrollbar: gpui_component::scroll::ScrollbarState::default(),
                md_preview_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
                md_preview_list_path: None,
                md_preview_focus: cx.focus_handle(),
                fs_watch_tx,
                fs_watch_rx: Some(fs_watch_rx_ch),
                cwd: None,
                git_commit_input: {
                    let input =
                        cx.new(|cx| TextInput::new(cx).placeholder(tr("git.commit_placeholder")));
                    input
                },
            },
            layout: LayoutUi {
                focus,
                dock_panel: DockPanel::Sessions,
                slp_w: ui.slp_w,
                panes_hidden: ui.panes_hidden,
                slp_drag: None,
                slp_hover: false,
                dock_split: ui.dock_split.unwrap_or(0.5),
                // dock_max = 哪个子面板最大化（映射见 workspace::min_from_dock_max）
                sess_min: crate::services::workspace::min_from_dock_max(&ui.dock_max).0,
                files_min: crate::services::workspace::min_from_dock_max(&ui.dock_max).1,
                split_drag: false,
                dock_bounds: std::rc::Rc::new(std::cell::RefCell::new(None)),
                ft_title_bounds: std::rc::Rc::new(std::cell::RefCell::new(None)),
                content_view: ContentView::Chat,
                browse_last: ContentView::Term,
                nav_open: false,
                nav_hide_at: None,
                nav_flyout_hovered: false,
                nav_hover_turn: None,
                nav_flyout_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
                tabs_scroll: gpui::ScrollHandle::new(),
            },
            composer: ComposerUi {
                input: String::new(),
                pending_images: Vec::new(),
                history: Vec::new(),
                history_ix: None,
                composer: None,
                input_focused: false,
                menu_ix: 0,
                menu_scroll: gpui::ScrollHandle::new(),
                menu_dismissed: false,
                at_index: std::collections::HashMap::new(),
                at_filter_cache: std::cell::RefCell::new(None),
                expanded_skills: std::collections::HashSet::new(),
                bar_hover: None,
                queue_drag: None,
                queue_drop_ix: None,
                composer_h: std::rc::Rc::new(std::cell::Cell::new(0.)),
                bubble_scrolls: std::rc::Rc::new(std::cell::RefCell::new(
                    std::collections::HashMap::new(),
                )),
            },
            pool: PoolUi {
                runtimes: std::collections::HashMap::new(),
                active_key: String::new(),
                sessions: Vec::new(),
                session_tail_cache: std::collections::HashMap::new(),
                draft_seq: 0,
                models_by_cwd: std::collections::HashMap::new(),
                globals: startup::Globals::default(),
                project_ctx: std::collections::HashMap::new(),
                ext_tokens: services::token_probe::read_cache(),
                booted: false,
                boot_resolved: false,
                pending_zoom: false,
            },
            dlg: DlgUi {
                dialog_focus,
                dialog: None,
                pill_menu: None,
                pi_extension_picker: None,
                mcp_picker: None,
                model_picker: None,
                pill_anchor: None,
                pill_btn: PillBtns::default(),
                ctx_tip_ring_hover: false,
                ctx_tip_panel_hover: false,
                ctx_tip_closing: None,
                renaming: None,
                rename_input: None,
                confirm_delete: None,
                proj_tip: None,
                hover_card: None,
                proj_card: None,
                tree_menu: None,
                tree_proj_menu: None,
                tree_edit: None,
                editor_menu: None,
                term_menu: None,
                input_menu: None,
                msg_menu: None,
                tab_menu: None,
                tree_confirm_del: None,
                tree_del_error: None,
                tree_deleting: Vec::new(),
                status_toast: None,
                settings: None,
                pending_send: None,
                subagent_panel: None,
                market_menu: None,
            },
            psp: PspUi {
                projects: Vec::new(),
                project_hits: Vec::new(),
                project_filter: String::new(),
                project_sel: 0,
                active_file: None,
                list_mode: if ui.list_mode == "flat" {
                    ListMode::Flat
                } else {
                    ListMode::Grouped
                },
                sort_mode: if ui.sort_mode == "manual" {
                    SortMode::Manual
                } else {
                    SortMode::Time
                },
                collapsed_keys: ui.collapsed.iter().cloned().collect(),
                psp_shown: std::collections::HashMap::new(),
                psp_scroll: gpui::ScrollHandle::new(),
                psp_sb_state: crate::ui::psp_scrollbar::PspScrollbarState::new(),
                search_hits: Vec::new(),
                search_truncated: false,
                search_running: false,
                search_needle: String::new(),
                search_gen: 0,
                session_query: None,
                session_collapsed: std::collections::HashSet::new(),
                session_list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
                session_sb_state: gpui_component::scroll::ScrollbarState::default(),
                pending_locate: None,
                unread: HashSet::new(),
                turn_errors: HashSet::new(),
                turn_warnings: HashSet::new(),
                pending_tabs: Vec::new(),
                window_active: true,
                hovered_project: None,
                proj_order: (!ui.proj_order.is_empty()).then(|| ui.proj_order.clone()),
                proj_drag: None,
                proj_drop_ix: None,
                proj_shift: None,
                session_pins: ui.session_pins.iter().map(|p| PathBuf::from(p)).collect(),
                hovered_session: None,
            },
            cfg: CfgUi {
                mc_patterns: None,
                mc_state: EnabledState::default(),
                mc_creds: Vec::new(),
                mc_project_scope: false,
                mc_default_model: None,
                mc_default_thinking: None,
                mc_model_thinking: Vec::new(),
                mc_skills: Vec::new(),
                mc_agents: Vec::new(),
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
                mc_remove_confirm: None,
                mcp_servers: Vec::new(),
                mcp_errors: Vec::new(),
                op_tx: None,
            },
            aux: AuxUi {
                ext_status: Vec::new(),
                ext_widgets: Vec::new(),
                ext_dialog: None,
                ext_notice: None,
                ask: None,
                ask_input: cx.new(|cx| TextInput::new(cx)),
                update: services::updater::initial_state(),
                changelog: None,
                changelog_scroll: gpui::ScrollHandle::new(),
                pi_extension_updates: services::pi_extension_updates::UpdatesState::default(),
                market: crate::market::MarketUi::default(),
                remote: remote_control::RemoteControl::new(),
                tool_sel: None,
                sysprompt_scroll: gpui::ScrollHandle::new(),
                decl_open: [false; 7],
                ext_input: cx.new(|cx| TextInput::new(cx)),
            },
        };
        // 启动阶段：全局态一次性装载（010-启动.md §1/§6）——模型清单（磁盘 ∪ 自有
        // 缓存）、命令、默认项、插件、全局 mcp，全部先于第一帧进内存，**不 spawn 进程**。
        chat.pool.globals = startup::load_globals();
        // new-session defaults (default model / thinking / enabledModels
        // scope) before the first frame — the draft pills render from them
        //（无项目态：project scope 恒 false；恢复挂载后 spawn_boot_restore 重算）
        chat.reload_model_defaults();
        // wire input callbacks that need the root entity handle
        let weak_ext = cx.entity().downgrade();
        chat.aux.ext_input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ext.update(cx, |c, cx| {
                    c.ext_respond(Some(v.to_string()), None, false, cx);
                });
            }));
        });
        // 问卷卡自定义答案输入框：Enter = 跳下一未答题 / 全答完则提交
        let weak_ask = cx.entity().downgrade();
        chat.aux.ask_input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |_v, cx| {
                let _ = weak_ask.update(cx, |c, cx| c.ask_enter(cx));
            }));
        });
        // 文件树 / git / fs watch 的挂载全部随「进入项目」动作走
        //（spawn_boot_restore / switch_project），无项目态零挂载。

        // settings panel CLI op pump (pi install/remove runs in background)
        let (op_tx, mut op_rx) = futures::channel::mpsc::unbounded::<String>();
        chat.cfg.op_tx = Some(op_tx);
        cx.spawn(async move |this, cx| {
            while let Some(msg) = op_rx.next().await {
                if this
                    .update(cx, |chat, cx| {
                        chat.cfg.pkg_op = None;
                        // 040-1 / 040-2：市场页行内按钮的转圈随 op 收尾清零
                        //（漏清哪一项，哪个页的按钮就永远转圈——040-2 首踩）
                        chat.aux.market.pi_ext.pending = None;
                        chat.aux.market.native.pending = None;
                        chat.set_status(msg, cx);
                        // CLI op 改的是盘上的 settings.json；globals 是启动时
                        // 的内存副本，不重读则 reload_settings_panel 只会拷到
                        // 旧列表（040：安装后列表不刷新的根因）
                        chat.pool.globals = startup::load_globals();
                        chat.reload_settings_panel();
                        // 082：CLI op（装/卸/更）动了包 → 重跑扩展更新检测，
                        // 刷新新会话页通知与扩展页箭头
                        chat.spawn_pi_extension_updates_check(cx);
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
        chat.ws.term_events = Some(term_tx);
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
                    if chat.ws.term_cursor_focused {
                        chat.ws.term_cursor_on = !chat.ws.term_cursor_on;
                        cx.notify();
                    }
                    chat.ws.term_cursor_focused
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
        // 初始 runtime = 无项目态占位（draft-0）：cwd 为空、永不 spawn——
        // 需要项目的动作全部在入口被门拦下（composer 发送 → 打开项目；
        // 文件树/git/终端空态）。占位只为让 rt() 全路径安全（rpc 对未附着
        // 进程是 no-op）；真正的恢复/挂载在 spawn_boot_restore。
        let rt = cx.new(|_| {
            let mut r =
                session::runtime::SessionRuntime::new("draft-0".to_string(), PathBuf::new(), None);
            r.default_model = services::workspace::default_model_pref();
            r
        });
        chat.pool.draft_seq = 1; // "draft-0" is taken by the initial runtime above
        chat.pool.runtimes.insert("draft-0".to_string(), rt.clone());
        chat.pool.active_key = "draft-0".to_string();
        chat.subscribe_runtime(&rt, cx);
        // 010-启动.md §9：启动期的一切（120ms 泵 / 3s 外部追加观察 / 60s 空闲
        // 回收 / 30s recents 对账 / 启动页闸门）都由 startup 发起；恢复最后
        // 会话 + 进入其项目 + 会话清单装载 = spawn_boot_restore（cwd 不再在
        // Chat::new 捏造）。进程附着随恢复路径走（open_session 自带 spawn +
        // GetMessages），无项目态零进程。
        startup::spawn_boot_tasks(cx);
        startup::spawn_boot_restore(cx);
        if PERF.load(std::sync::atomic::Ordering::Relaxed) {
            if let Some(t0) = T0.get() {
                eprintln!("[perf] last-session tail rendered: {:?}", t0.elapsed());
            }
        }
        // git panel: Enter in the commit box commits staged changes
        let entity_for_git = cx.entity();
        chat.ws.git_commit_input.update(cx, |ti, _| {
            let weak_git = entity_for_git.downgrade();
            ti.set_on_submit(Box::new(move |_, cx| {
                let _ = weak_git.update(cx, |c, cx| c.git_commit_staged(cx));
            }));
        });
        chat
    }

    /// 当前项目目录——None = 无项目态（启动未恢复、也未用任何显式动作进入
    /// 过项目）。cwd 的合法来源只有显式动作：恢复最后会话（其项目随之成为
    /// cwd）、切项目/打开项目、PI_FLASH_CWD。应用永不捏造（010 重设计）。
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
    }
}

impl Chat {
    /// 当前文件 tab 编辑器的焦点句柄（持焦时 Some）——render 帧级焦点回收
    /// 的白名单，与终端/对话框同级（023）。
    pub(crate) fn file_editor_focus(
        &self,
        window: &gpui::Window,
        cx: &App,
    ) -> Option<gpui::FocusHandle> {
        if self.layout.content_view != ContentView::File {
            return None;
        }
        let path = self.active_file_path()?;
        let ed = self.ws.file_cache.get(&path)?.editor.clone()?;
        let st = ed.read(cx);
        let handle = st.focus_handle(cx);
        // 搜索面板（Ctrl+F）输入框持焦同样不回收——它不是编辑器本体的
        // focus_handle，漏判会被下方仲裁抢回 composer，面板打不了字
        (handle.is_focused(window) || st.search_panel_focused(window, cx)).then_some(handle)
    }

    /// 082 三期：当前文件 tab 是否处于 **markdown 渲染预览态**（File 视图
    /// + md kind + md_source=false）——逐帧仲裁据此把焦点交给预览容器。
    pub(crate) fn md_preview_active(&self) -> bool {
        if self.layout.content_view != ContentView::File {
            return false;
        }
        let Some(path) = self.active_file_path() else {
            return false;
        };
        crate::editor::kind::is_markdown(&path)
            && self
                .ws
                .file_cache
                .get(&path)
                .is_some_and(|f| !f.md_source && !f.binary)
    }

    /// 082：编辑器搜索面板（Ctrl+F）输入框持焦判定——ViewLeft/ViewRight
    /// 在此退回组件词移动（查词打字时 Ctrl+←/→ 仍是跳词，不能切走 tab
    /// 把面板焦点弄丢）。编辑器本体持焦不算（那里切 tab 正是需求）。
    pub(crate) fn editor_search_panel_focused(&self, window: &gpui::Window, cx: &App) -> bool {
        if self.layout.content_view != ContentView::File {
            return false;
        }
        let Some(path) = self.active_file_path() else {
            return false;
        };
        let Some(ed) = self.ws.file_cache.get(&path).and_then(|f| f.editor.clone()) else {
            return false;
        };
        ed.read(cx).search_panel_focused(window, cx)
    }
}

impl Focusable for Chat {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        // composer 就绪后，"聚焦聊天"即落到输入组件（真输入框：光标/
        // 选区/IME 全可用），chat.layout.focus 仅作组件创建前的回退
        self.composer
            .composer
            .as_ref()
            .map(|c| c.read(cx).focus_handle_in(cx))
            .unwrap_or_else(|| self.layout.focus.clone())
    }
}
