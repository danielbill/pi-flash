//! Chat 的界面状态字段簇（8 簇中除 WsUi 外的 7 簇；自 chat.rs 外移）：
//! aux 周边 / cfg 设置 / psp 列表 / dlg 弹窗 / pool 会话池 / composer 输入 /
//! layout 壳布局。字段数与簇内清单不写死在注释里（随演进漂移，查数用 grep）。

use crate::function_panel::{HoverCard, ProjCard, TreeEdit, TreeMenu};
use crate::session::input::{AtFilterCache, AtIndexState};
use crate::settings::models::{McProbe, MjCatalogState, MjSpeedState, MjTestState};
use crate::settings::pi_extensions::PkgOp;
use crate::*;

/// 周边面板状态簇（4ea 字段重组）：扩展 UI 面 / 081 自动更新与更新日志 /
/// 系统提示词·工具定义弹窗。
pub(crate) struct AuxUi {
    // extension UI surface (active session)
    // 034：ext_status 已下沉到 SessionRuntime（per-session，防切会话串味）
    pub(crate) ext_widgets: Vec<(String, Vec<String>, bool)>,
    pub(crate) ext_dialog: Option<pi_link::protocol::ExtensionUiRequest>,
    /// 阻塞弹窗两个内容区的滚动句柄（上：信息面板；下：操作面板）。
    /// 扩展标题可长到 40+ 行（pi-usage 的 /usage「View all」），小窗口
    /// 不限高不滚动就会把底部按钮顶出视口、点击全部落在卡片上，弹窗关不掉
    pub(crate) ext_dialog_info_scroll: gpui::ScrollHandle,
    pub(crate) ext_dialog_op_scroll: gpui::ScrollHandle,
    /// 扩展阻塞请求队列（pi-web extension-ui-queue parity）：队头在屏上
    /// （ext_dialog），其余按到达序在后面等。多个阻塞请求可同时存活
    /// （并行工具各自被扩展门控），pi 侧每个都持有到应答为止——单槽位
    /// 会让后来的请求遮住先来的，先来的 handler 永远等不到应答，它的
    /// prompt 回执也不再来（#7 相位永久挂起的根因之一）
    pub(crate) ext_dialog_waiting: Vec<pi_link::protocol::ExtensionUiRequest>,
    pub(crate) ext_input: gpui::Entity<TextInput>,
    /// editor 弹窗专用多行输入（zi2s.6）：editor 语义 Enter 提交，prefill 带
    /// 真实换行（单行 ext_input 渲染遇换行直接 panic——GPUI text 契约）
    pub(crate) ext_editor_input: gpui::Entity<TextInput>,
    /// select 弹窗键盘导航高亮行（zi2s.7）：↑↓ 移动、Enter 选中；ext_show
    /// 归零（晋升等待项时同款）
    pub(crate) ext_sel_ix: usize,
    /// ctx.ui.setTitle 目标（zi2s.9，pi-web document.title parity）：None =
    /// 未收到过；render 帧发现与已应用值不同就 set_window_title
    pub(crate) ext_title: Option<String>,
    /// 已应用到 OS 窗口标题的值（apply-diff 守卫，防每帧 set_title 抖动）
    pub(crate) ext_title_applied: String,
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
    /// 自定义商模型清单仓镜像（custom-provider.json；2026-11-01 定稿：完整
    /// 定义存 PF 侧，models.json 只放勾选子集）。reload_settings_panel 装载、
    /// 探测/保存/勾选时更新——渲染路径只读内存不扫盘
    pub(crate) mj_probe_cache: crate::services::custom_models_store::Store,
    pub(crate) mc_project_scope: bool,
    /// PF 五角星默认模型（`app-settings.json`，042）：全新草稿 spawn 时以
    /// `--model` 带上。**不是** pi 的 settings.json 默认——两份各管各的（2026-11-02
    /// 定稿：pi-web 还在用 settings.json 那份，暂不统一）。
    pub(crate) mc_default_model: Option<(String, String)>,
    /// pi `settings.json` 的 `defaultProvider/defaultModel`：PF **只在无星标时**
    /// 用于草稿 pill 的显示兜底（镜像 pi `findInitialModel` 第 2 步，起会话
    /// 前后 pill 不再跳），不参与 spawn、不写回。
    pub(crate) mc_pi_default_model: Option<(String, String)>,
    /// settings.json `defaultThinkingLevel`（pi-web /api/models 同源）
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
    pub(crate) mc_mj_error: Option<String>,
    /// 自定义商【删除】确认浮层：待删 provider 名（含其下全部模型）
    pub(crate) mc_mj_delete_confirm: Option<String>,
    /// 子代理【删除】确认浮层（044，2026-11-10 加）：待删 agent id
    pub(crate) mc_agent_delete_confirm: Option<String>,
    /// 042：provider 通路探查状态（保存 key 后自动一次 + 详情页手动重试）
    pub(crate) mc_probe: std::collections::HashMap<String, McProbe>,
    /// 042-1 自定义商「测速」状态（单槽：详情页同屏只有一个）
    pub(crate) mj_speed: Option<MjSpeedState>,
    /// 042 M2 自定义模型「测试」状态（单槽：同一时刻只测一个模型）
    pub(crate) mj_test: Option<MjTestState>,
    /// 042 M2 models.dev 拉取状态（按钮文案与结果行）
    pub(crate) mj_models_dev_state: MjCatalogState,
    /// 【自动填参】的结果暂存（queried_id, 目录条目）：填参不再落盘，名称/价格
    /// 等字段等【保存】时按它补进模型；`queried_id` 与保存时的模型ID 不一致就
    /// 丢弃（防止把上一个模型的名称/价格串过去）。
    pub(crate) mj_fill_entry: Option<(String, pi_link::model_price::Entry)>,
    /// 模型详情那几个表单变量（mj_id / ctx / max / 能力勾选）当前对应的
    /// section。**唯一用途**：渲染前校验「表单内容 ↔ 当前 section」是否一致，
    /// 不一致就重填一遍——详情页关闭/切换时若有任何一个变量没清干净（值污染），
    /// 会立刻被拉回正确值，而不是把上一个模型的值显示出来。
    pub(crate) mj_form_for: Option<String>,
    /// 模型详情弹窗的**来源 section**（点列表进详情时记下当前右栏）：关弹窗
    /// （✕/点外/ESC）退回此处——内置商模型来自合并列表 / 内置详情（section
    /// = 商 id），自定义商来自 `mj:p:名`；`None` = 从【+模型】草稿进
    /// （退回 `mj:p:名` 的旧行为）。
    pub(crate) mj_return: Option<String>,
    /// 自定义商**仓草稿**登记（2026-11-02 定稿：自定义商详情一律可编辑）：
    /// 点列表进详情时该模型只在清单仓、不在 models.json（未勾选）→ 记下仓
    /// id，详情以「追加槽 + 仓定义预填」的可编辑表单打开；保存只进仓
    ///（已显式勾选才顺手物化），移除删仓条目。`None` = 账本行 / +模型草稿 /
    /// 内置覆盖视图。关闭详情必清。
    pub(crate) mj_draft_store_id: Option<String>,
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
    /// 031-1 计划模式审批弹窗 UI 态（结构/渲染/动作在 crate::mode::dialog
    /// 与 mode::state；状态真值在 runtime record 的 plan_* 投影）
    pub(crate) plan: crate::mode::PlanUi,
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
    /// 034 状态带 [...] 管理菜单：内容 = 申请常驻状态栏的插件键清单，
    /// 勾选 = 上屏，取消勾选 = 写 ext_status_muted 隐藏。(鼠标 x, y)
    pub(crate) status_mgr_menu: Option<(f32, f32)>,
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
    /// 子代理导航数据面（044-0 验收⑥）：会话激活即建（switch_to →
    /// ensure_subagent_panel），1s 常驻任务——纯内存推导（主会话消息 →
    /// 落点映射 + 第三行点阵），零 IO（pi-flash-9eck descriptor 已删）。
    pub(crate) subagent_panel: Option<crate::session::SubagentPanel>,
    /// 040-1 topbar 三横「菜单」浮层：Some = 开着，值为按钮锚点（x, y）。
    /// 渲染挂 render.rs 根（与 psp_overlays 同层），菜单项目前只有
    /// 「Pi扩展市场」。
    pub(crate) market_menu: Option<(f32, f32)>,
    /// 文件右键【扩展菜单】hover 子菜单展开态（tree_menu.rs；子项 hover
    /// 离开或菜单整体关闭时归 false）
    pub(crate) plugin_submenu: bool,
    /// 文件右键主菜单实测高度（px）：扩展子菜单 `max_h` 取它，超长内部滚动
    pub(crate) tree_menu_h: std::rc::Rc<std::cell::Cell<f32>>,
    /// 扩展子菜单滚动位置（跨帧保持，不能用临时 handle）
    pub(crate) tree_sub_scroll: gpui::ScrollHandle,
}

/// 会话池簇（4ea 字段重组）：常驻 runtime 池 + 注意力指针 + 索引缓存
/// + 启动装载的全局态与闸门（010）。
pub(crate) struct PoolUi {
    pub(crate) runtimes:
        std::collections::HashMap<String, gpui::Entity<session::runtime::SessionRuntime>>,
    pub(crate) active_key: String,
    /// 固定段会话 tab 的主会话指针（最近一次激活的**非 observer** 会话，
    /// 含草稿）：新会话/主会话永远占 topbar 第一位（044-0 验收②），子会话
    /// 观察窗进自由区 panel_tabs，不抢固定段。空闲退役只清 runtimes 不清
    /// 这里——固定段照常渲染，点击 activate 走 open_session 秒开重开。
    pub(crate) primary_key: String,
    /// tab 标题快照（key → 标题）：observer 子会话 = 任务名（主会话反查，
    /// 侧栏扫描器不含子会话）；普通会话 =
    /// switch_to / refresh_sessions 时的 session_title_of 快照（空闲退役
    /// 后 runtime 不在池，标题仍有出处）。bind 迁移 / 关闭跟随键变化。
    pub(crate) tab_labels: std::collections::HashMap<String, String>,
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
    /// 统一菜单搜索栏（031-2 万能搜索）：输入实体在菜单首帧惰性创建；
    /// `menu_query` 是其值镜像（on_change 下发），非空 = 万能搜索结果视图
    /// （四类仅按名称子串匹配、分组罗列）
    pub(crate) menu_search: Option<gpui::Entity<crate::TextInput>>,
    pub(crate) menu_query: String,
    /// 万能搜索的文件名过滤记忆化（同 at_filter_cache，键含查询词）
    pub(crate) menu_search_cache: std::cell::RefCell<Option<AtFilterCache>>,
    /// 搜索栏持焦期间发生确认/tab 切换：置位让下一帧焦点仲裁放行 composer
    /// 回收（见 focus_arbitrate 白名单分支）
    pub(crate) menu_refocus: bool,
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
    /// 031-1 计划模式状态 pill 悬停态（input/bar plan_marker_button）：
    /// 悬停时图标 notebook-pen 原位换 x（§5.10 状态驱动）
    pub(crate) plan_marker_hover: bool,
}

/// 壳布局簇（4ea 字段重组）：dock 面板、内容区路由、分隔条拖拽、
/// 导航 flyout 与焦点仲裁锚。
pub(crate) struct LayoutUi {
    pub(crate) dock_panel: DockPanel,
    pub(crate) slp_w: f32,
    pub(crate) panes_hidden: bool,
    pub(crate) slp_drag: Option<(f32, f32)>,
    /// 分隔线响应区悬停（§5.12 分隔线统一规范：hover 与拖拽同为 4px
    /// accent 粗线，线形计算见 `ui::divider`）
    pub(crate) slp_hover: bool,
    /// dock 上下分区（横分隔线）悬停——与 slp_hover 同一机制，两处对称
    pub(crate) split_hover: bool,
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
