# pi-flash 架构契约(ZED 框架 × pi-web 对话层 × 极速启动)

本文档是架构契约。改代码前先对照本文;改完架构相关的代码要同步更新本文。
上位规格:`docs/模块设计/`(模块边界以此为准,本文负责技术落地)。

## 0. 对齐基准

| 层 | 原型 | 方式 |
|---|---|---|
| 框架层(布局/dock/文件树/git 面板/状态栏/顶栏) | **ZED**(`D:\github\zed`,移植基线 commit `4c902c9`) | 剪裁移植源码(渲染/交互照抄,数据层接本地) |
| 会话列表(020) | **ZCode**(`D:\github\---harness-tools---\ZCode` packages/ui) | 按其设计规格实现(排序/交互/布局参数照搬) |
| 对话层(030/031/032) | **pi-web**(行为规格,React 无代码可搬) | 自研,行为 1:1 对齐 |
| 外观体系(006/007) | 架构 ZED,数据 pi-web | ThemeRegistry/ActiveTheme + mist/rose 令牌 + pi-web markdown |
| 通用基座(弹窗/浮层/输入) | gpui-component(vendored 于 `vendor/`,`[patch.crates-io]` 钉版) | Modal/Popover/Notification/InputState |

## 1. 设计原则

| pi-web | pi-flash |
|---|---|
| React 组件 | GPUI `Entity<T> + Render`(Zed 模式) |
| props 向下 | 父持 `Entity<T>`,`child.update(cx, \|c\| c.set_x(..))` |
| callbacks 向上 | 子 `cx.emit(Event)`,父 `cx.subscribe` 分发 |
| hooks | 实体内部异步泵/服务模块 |
| lib 服务层 | 无 UI 的 `services/` 纯函数模块 |
| CSS 令牌 | 主题令牌(见 §5 外观体系) |

## 2. 模块布局(↔ 模块设计文档)

```
crates/app/src/
├── main.rs            mod 树 + 根 re-export + main()(崩溃落盘 hook、更新交接、
│                      字体/键位/gpui-component 初始化、建窗、退出序列)
├── shell/             应用壳域:chat.rs(Chat 实体本体 + 工作区簇 WsUi)+ chat/
│                      (ui_fields 其余 7 簇界面状态、boot 构造器、focus 焦点仲裁)
│                      + render/(渲染根:body/focus/overlay/pill)+ types.rs
│                      (跨域壳类型 Dialog/PanelTab/DockPanel/ContentView/
│                      Pill 三件套/FileTab…;单域类型已下沉所属域)
├── startup.rs startup/  启动唯一入口(globals/pumps/restore)(010)
├── agent_session.rs   无头实体:PiSession/事件泵/epoch/on_event 分发
├── content.rs         内容区状态机 chat/term/md
├── appearance/        主题注册表 + ActiveTheme + 字体槽位(theme.rs,rgba_a 等色助手)
├── i18n.rs i18n/ i18n_audit.rs   三语(zh-CN 为源;表在 i18n/table/ 按域分表)+ 审计
├── assets.rs status_bar.rs titlebar.rs titlebar/ top_panels.rs top_panels/
│                      图标编入 / 状态栏 / topbar / 系统提示词·工具定义面板
├── automation/        pif-ui 自动化服务(mod=listener+泵 / snapshot / handlers /
│                      screenshot)
├── dialogs/           弹窗域:mod(dialog_shell+分发+小弹窗) + provider_picker
│                      + project_picker + ask_card(整卷问卷卡) + ext_ui(扩展阻塞件)
├── terminal/          内置终端(alacritty+ConPTY):mod(常量/类型/spawn/tests)
│                      + element(Element/paint) + host(impl Chat) + palette
│                      + snapshot + input_encode(键盘/IME→PTY)
├── editor/            编辑器域:kind(FileTypeHandler 注册表)+ view + tabs +
│                      markdown/(mod 渲染入口+类型 + code_block + blocks +
│                      highlight syntect + parse pulldown + links + doc 文档模式 +
│                      wysiwyg/ + render/ + cache 解析缓存)
├── function_panel/    左 dock 域:mod(psp view+dock 容器+overlay 状态类型) +
│                      psp_*(行模型/项目卡/菜单/拖拽/悬浮卡)+ project_row +
│                      file_tree.rs(三视图+行渲染)+ file_tree/(lists/rows/
│                      selection/tree_state/tree_action_bar)+ file_actions* +
│                      git_panel/(changes/history)+ 菜单原语上移 ui/menu
├── search/            全局搜索域(083 Zed ProjectSearch 复刻)：actions/engine/
│                      nav/panel/query/replace/results/sessions
├── mode/              计划模式(031)：dialog/state + plan_mode.ts
├── market/            扩展市场页 + 三市场分段外壳(040-1/090-4):
│                      pf_market/ + pf_installed/ + pi_native_market/
├── pf_plugin/         PF 插件宿主 UI(090-4):manifest/(扫描/校验)+ panel/
│                      (组件树渲染)+ host_actions/ + host_tabs/ + lifecycle +
│                      svc(执行域宿主侧)+ wasm_hooks
├── tasks/             自动任务(046):store/view/form/popup/run/sched
├── services/          无 UI 助手(pi-web lib/ parity)：updater/(081:version/
│                      fetch/download/stage/payload/stable_entry)+ workspace/
│                      (store/ws_state/app_settings)+ models_config +
│                      custom_models_store/mj_doc_cache/pf_state/pfctx_ops(001)/
│                      pi_extension_updates(082)/scheduler(定时调度,046)
│                      + git/format/… 原平铺件
├── session/           会话域:mod(sessionView 组装/列表)+ runtime/(mod 结构+
│                      生命周期+泵、events 事件臂、send 发送路径+rpc 门面、
│                      nav 定位对账、ingest 摄入)+ messages/(mod、model 类型
│                      与尾窗解码、tool_card/compaction/diff_view、user_row
│                      用户行+块分发、turn 轮装配、images 解码缓存、textview
│                      消息划选)+ input/(mod Chat 委托层、area 主输入区+
│                      键路由、bar 控件行、ctx_tip 用量面板)+ pool + chat_list
│                      + footer_bar/fn_bar(034) + nav_panel(033) +
│                      subagent_panel(044) + actions_bar + menu+menu_rows+
│                      menu_views + attachments + fork + diff + pickers
│                      (model_picker/mcp_picker/pi_extension_picker/picker_shell)
│                      + new_session + template + tools_recipe
├── settings/          设置十页(general/models/custom_models/agents/skills/
│                      pi_extensions/mcp/shortcuts/misc/about；McProbe/PkgOp 各归其页)
└── ui/ ui/kit/        通用件:tokens/button/text_input/composer_input/dropdown/
                       overlay/popup/vlist/psp_scrollbar/tooltip/menu;
                       kit = 页面级组件库(badge/checkbox/row/slider/stepper/
                       switch/toggle)
```

pi-link:协议层(只对钉版 pi 负责)+ `sessions/` 会话发现(索引化扫描:group 目录
定位 + tail-seek 摘要 + (mtime,size) 指纹索引落盘;`read.rs` 的
`read_tail_messages` 尾窗解析)。文件级清单见 AGENTS.md「File Map」。

workspace 共 4 crate:`pi-link` + `app` + `pf-context`(001 会话上下文基座:
设置快照单写者/原子写/watcher、ext-context.json 投影、SQLite WAL 域库;
agent 设置面经 pi-link `pfctx_manager.rs` + `extensions/pfctx-manager`)+
`pf-plugin-host`(090-4 wasm 插件宿主:WIT + 宿主运行时 + `plugin-test`/
`qjsc`/`pf-precompile` 三个 bin)。

## 2.5 会话模型

**每个会话 = 一个常驻 SessionRuntime 实体**(session/runtime/):独立 pi 进程
(AgentSession)+ 全量消息 + 投影状态 + **独立 inputPanel(草稿/附件/history/
思考强度/工具集/模型)** + 自有事件泵。创建即常驻,删除才销毁;
**几十个 agent 并发工作,互不依赖用户注视**——切换 = 用户注意力转移
(`chat.pool.active_key` 改指针 + 编辑器镜像 flush/load,毫秒级、零 IO、零进程
操作);侧栏转圈 = 每行读自己 runtime 的 agent_running(并发监控面板)。
Chat 是壳:池(runtimes map)+ 注意力指针 + 侧栏/git/文件/终端/设置。
工具集无活改 RPC:档位与插件集是 spawn 参数配方(`session/tools_recipe.rs`),
变更即重绑进程——`full` 档发 `--tools` 名单,`full+` 档不发 `--tools`、
由扩展在 `session_start` 调 `setActiveTools`。模型/思考强度走活改 RPC。
空闲退役(session/retire.rs 模块,参数内定不进 app_settings):非 active 空闲
超窗**连 runtime 实体一起退役**——杀进程,transcript(消息含工具结果全文、
system_msgs 含整份系统提示词)随实体释放;双窗:通用 600s,定时任务会话 30s
(结算后内存无消费者,即弃)。回开走 open_session 的 tail 秒开路径。未应答的
阻塞型扩展 UI 请求有保护窗(对齐通用窗;任务会话无输入通道,跳过),窗内拦退役。

## 3. 状态所有权

Chat 是壳实体,**直属字段 = 8 个状态子结构簇 + 两个直属域字段**:`pool`(会话池/
指针/缓存/启动闸门)、`composer`(输入面板 031)、`layout`(dock/路由/分隔条/
flyout/焦点)、`psp`(项目/会话列表/搜索/通知点)、`ws`(文件树/编辑器/终端/
git/cwd)、`dlg`(弹窗/悬浮层/右键菜单/toast/设置句柄)、`cfg`(设置页配置状态)、
`aux`(扩展 UI/更新/市场/系统提示词弹窗);直属域字段 `plugins`
(pf_plugin::PluginUi)与 `tasks`(046 自动任务状态面)。跨域字段访问在代码里
可见可查:`chat.ws.tree_rows` vs `chat.dlg.tree_menu` 一眼可辨归属。

| 状态 | 属主 |
|---|---|
| RPC 会话/事件泵/epoch | AgentSession（无头）+ `session/runtime/`（泵在 runtime/pump.rs） |
| 消息数据/折叠/流式状态 | SessionRuntime 字段（`session/runtime/`）；**渲染**是
  `session/messages/`（model/user_row/tool_card/compaction/diff_view/turn/images
  各件，无状态自由函数）；
  列表组装在 session/mod.rs |
| 编辑器草稿/附件/历史（**双宿主镜像**） | Chat.composer（UI 侧）+ SessionRuntime
  （会话侧）各一份；镜像协议两个写入点：切会话 `session/pool/runtime_link.rs`
  `switch_to`（flush/load）、发送前 `input/mod.rs with_active_editor`
  （Chat→Runtime 刷草稿）。改镜像协议只动这两个方法 |
| 会话列表渲染/搜索弹窗 | Chat.psp（状态）+ function_panel（psp 渲染）+ dialogs/（搜索弹窗） |
| cwd/branch/git/文件树/终端/编辑器 tab | Chat.ws |
| 弹窗/右键菜单/toast/设置面板句柄 | Chat.dlg |
| 设置页配置状态（mc_*/provider registry/mcp） | Chat.cfg；面板实体渲染在 settings/ |
| 窗口 bounds/布局状态 | 状态文件(services/workspace,shell 恢复) |
| 扩展 UI/自动更新/市场/系统提示词弹窗 | Chat.aux |
| 自动任务（任务/运行状态面） | Chat.tasks（业务在 `tasks/`） |
| PF 插件（清单注册表 + 懒加载运行时 + 面板行缓存） | Chat.plugins（`pf_plugin/`） |
| 主题/语言/字体 | 全局(appearance/i18n) |

**RPC 发送纪律**：`Command` 直发只允许出现在
`session/runtime/` 域内与 agent_session.rs。域外一律走 `SessionRuntime::rpc()`
门面（无进程返回 false）。新命令接入只改 runtime 域。

## 4. 启动(010)

`main()` 顺序:崩溃落盘 hook → 更新交接(`handoff_if_newer`:安装根下已有
更高版本的完整载荷就让位给它)→ `startup::boot()`(建 `~/.pi-flash/`、旧文件
迁移、recents 种子、042-1 迁移;必须早于任何读盘者)→ 字体/键位/
gpui-component 初始化 → 建窗(`Chat::new` 内挂启动泵与恢复)→ 自动化服务
(显式开启才有)。

**启动页与揭幕**:先画启动页;揭幕闸门 = pi 进程已附着(或恢复已定案)且
启动页已展示 ≥500ms,3s 超时兜底(`MIN_SPLASH`/`SPLASH_TIMEOUT`)。揭幕落点
即启动恢复的分流:有上次会话 → 会话界面,无 → 新会话页。应用永远从无项目态
出发,cwd 只来自恢复的最后会话或显式动作(切项目/`PI_FLASH_CWD`)。

四层清单(pi-web 核实):恢复层(主题/语言/布局/上次 workspace/每项目上次会话,
同步一次读)→ 会话列表层(索引化扫描)→ 会话内容+运行时层(磁盘直读渲染,
pi 随恢复路径 spawn,新会话懒 spawn)→ 后台杂项(git/模型,不阻塞)。

zed 机制:骨架先行(首帧零数据 IO)、同步小读+异步节流写、目录树后台增量扫描、
每项目一份树展开集合并持久化(`__tree`)、无总闸门渐进填充。

**全局态一次装载**(`startup/globals.rs`):模型清单/命令/插件/默认项/项目
上下文启动时从硬盘装进内存,渲染路径只读内存(不扫盘、不发 RPC);会话进程
答到的 RPC 只做覆盖并回写 `catalog-cache.json`。

**预算(验收线):首帧 <50ms;会话列表 <150ms;上次对话消息上屏 <300ms
(与 node 无关);pi 就绪 <1s 且不阻塞任何 UI。** 打点:`PI_FLASH_PERF=1` 时
`[perf]` 输出启动阶段耗时(时刻基准 = `main` 起点)。

**尾窗直读与预载**:pi 本体无窗口——SessionManager.open 全量解析文件(大文件
秒级);pi-web 的 50 条尾窗是其 API route 自截断,并非 pi 的能力。PF 打开会话
先用 `read_tail_messages` 物理尾窗上屏(初窗 256KB/100 条;可渲染条目不足 10 条
则窗口倍增,2MB 封顶),get_messages 投影快照到达后 clear+替换对账(权威投影
替换预渲染)。启动时后台预载最近活动会话里除当前会话外前 `TAIL_PRELOAD`
(10)条的尾窗进 `session_tail_cache`(常量,非设置项),open_session 先查缓存
命中秒出;缓存满 20 条即清。已知偏差:物理尾窗可能含废弃分支条目,由 RPC
对账校正。

## 5. 外观体系(006/007)

- **主题**:ZED 架构(ThemeRegistry + ActiveTheme + 主题族 light/dark)。
  内置 10 套:浅色 mist(雾青)/kitchen/latte/rose(蔷薇,pi-web 令牌转换)/
  one-light;深色 one-dark/nord-dark/pine/night-owl/noctis。主题切换经 `sync_gpui_tokens`
  重跑 gpui-component token 映射;主题名只存 `app-settings.json`。
- **syntax**:zed 主题 syntax 色 → syntect 色彩方案映射(markdown +
  syntect 保留;不引 zed markdown,markdown preview 对齐 pi-web)。
- **icon theme**:ZED 架构(注册表/文件图标集解析,服务 021 文件树),
  内置内容 = pi-web 图标。
- **字体三档**:session font(≈zed UI font)/ panel font / markdown preview
  font + 字号(`app-settings.json`;字号钳制 10–30)。
- **配置三层分界**:pi `settings.json`(模型/凭证/工具,归 pi)≠
  `~/.pi-flash/app-settings.json`(PF 自有设置:主题/语言/提示音/字体/加载
  窗口/恢复开关等,section 化见 001-1)≠ `~/.pi-flash/workspace.json`
  (运行时状态:`__last` 每项目上次会话、`__window`、`__ui` 布局、`__tabs`、
  `__tree`、`__pins`)。后两者原子写(tmp+rename)+ 内容不变跳过 +
  进程内单次读缓存。
- **多语言(007)**:`i18n.rs` 现行模式(语义键 + `i18n/table/` 分表);
  新模块字符串一律 `tr()` 收口;`i18n_audit` 单测守键存在/空译文挂账/死键
  三道防线。

## 6. TextInput:全 app 唯一文本输入基座

所有文本输入都是 gpui-component `InputState` 的门面(真实选区/光标/
剪贴板/IME 由其提供):单行 `ui/text_input/`(`TextInput`)、多行 composer
`ui/composer_input.rs`(`ComposerInput`)、文件编辑器 `editor/view.rs`
(直接建 `InputState`)。**铁律:任何文本输入禁止用 `on_key_down`
字符匹配手搓。**唯一例外:终端(PTY 字节流)。composer 的 ↑/↓/Tab/Ctrl-V/
Ctrl-C 由 `main.rs` 在同上下文("Input")注册的覆盖绑定截获,交给 composer
的 `on_action`。

## 7. 通信规约

- 父→子:方法调用;子→父:`cx.emit` + `subscribe`;
  禁止子组件持 `WeakEntity<父>` 散弹式 update(回调闭包除外)
- 视图组件不直接碰 PiSession RPC(pi-web:ChatInput/MessageView 无网络请求)

## 7.5 UI 自动化服务

- **目的**:agent 调试不抢真实屏幕/鼠标——数据快照代替肉眼,方法直调代替鼠标点击。
- **协议**:`pi_link::automation`(NDJSON over 127.0.0.1;hello→auth→auth_ok→
  Request/Response;method 常量表 = op 清单唯一事实源;协议测试在 pi-link)。
- **服务**:`crates/app/src/automation/`(mod=accept 线程 + 每连接读写线程 +
  `cx.spawn` 主线程泵 + catch_unwind;snapshot=字段→JSON 只读;handlers=直调
  Chat 方法;screenshot=窗口渲染回读)。op 在主线程 catch_unwind 内执行,
  自动化 panic 不许带崩 app。
- **开关**:`PI_FLASH_AUTOMATION=<port|auto|1>`,缺省关;token 走
  `<配置目录>/automation/<pid>.json`(启动时探活清理死文件,CLI `clean` 同规)。
- **CLI**:`pi-link` 的 `pif-ui` bin(list/info/snapshot/exec/keys/type/shot/wait/clean)。
- **截图**:`ui.screenshot` 走 vendored gpui 的 `Window::render_to_image()`
  (D3D11 staging 纹理回读,移植自 zed main):进程内、不抢屏、窗口被遮挡也能截。
  截的是最近一次绘制的帧——`exec` 改状态后先 `wait` 再 `shot`。
- **边界**:文本输入一律直调 setter(gpui-component 文本走 IME/替换路径,
  KeyDown 逐字不可靠)。

## 8. 守护规则(scripts/check_arch.sh)

`scripts/check_arch.sh` 五项,非零退出 = 违规:

1. 单文件**非测试行数** ≤1000(`#[cfg(test)]` 块与文件式测试模块不计,
   测试体量不惩罚生产代码)
2. 顶层视图函数 ≤400 行(仅扫 crates/app;视图函数在本仓均为顶层自由函数)
   —— 1/2 的超线存量冻结在 `scripts/arch-lines-baseline.txt`:只拦新增超线与
   存量增长;基线只许人工 `--update-baseline` 重写,agent 禁刷
3. 禁止 `on_key_down` 字符匹配输入模式
4. 页面禁手搓控件(`scripts/check_ui_raw.py`:交互链里的裸几何字面量;存量走
   `scripts/ui-raw-baseline.txt`;新页面用 `ui/kit/*` + `ui/tokens`)
5. `cargo build` 无新警告;`cargo test` 全绿

另:新 UI 字符串走 `tr()`(由 `i18n_audit` 单测守)。
守卫挂进发版 checklist(RELEASE.md §2)——发版前必跑 `scripts/check_arch.sh`。
