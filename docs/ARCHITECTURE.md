# pi-flash 架构契约(v2:ZED 框架 × pi-web 对话层 × 极速启动)

本文档是架构契约。改代码前先对照本文;改完架构相关的代码要同步更新本文。
上位规格:`docs/模块设计/`(000-050,模块边界以此为准,本文负责技术落地)。

## 0. 对齐基准(双轨制)

| 层 | 原型 | 方式 |
|---|---|---|
| 框架层(布局/dock/文件树/git 面板/状态栏/顶栏) | **ZED**(`D:\github\zed`,移植基线 commit `4c902c9`) | 剪裁移植源码(渲染/交互照抄,数据层接本地) |
| 会话列表(020) | **ZCode**(`D:\github\---harness-tools---\ZCode` packages/ui) | 按其设计规格实现(排序/交互/布局参数照搬) |
| 对话层(030/031/032) | **pi-web**(行为规格,React 无代码可搬) | 自研,行为 1:1 对齐 |
| 外观体系(006/007) | 架构 ZED,数据 pi-web | ThemeRegistry/ActiveTheme + mist/rose 令牌 + pi-web markdown |
| 通用基座(弹窗/浮层/输入) | gpui-component 0.2.0(已接通,钉版) | Modal/Popover/Notification/InputState |

## 1. 设计原则(pi-web 三层 → GPUI 对应物,不变)

| pi-web | pi-flash |
|---|---|
| React 组件 | GPUI `Entity<T> + Render`(Zed 模式) |
| props 向下 | 父持 `Entity<T>`,`child.update(cx, \|c\| c.set_x(..))` |
| callbacks 向上 | 子 `cx.emit(Event)`,父 `cx.subscribe` 分发 |
| hooks | 实体内部异步泵/服务模块 |
| lib 服务层 | 无 UI 的 `services/` 纯函数模块 |
| CSS 令牌 | 主题令牌(见 §5 外观体系) |

## 2. 模块布局(目标,↔ 模块设计文档)

```
crates/app/src/
├── main.rs            mod 树 + 根 re-export + main()（~200 行）
├── shell/             应用壳域：chat.rs（Chat 实体本体：8 个状态子结构簇
│                      PoolUi/ComposerUi/LayoutUi/PspUi/WsUi/DlgUi/CfgUi/AuxUi
│                      + 构造器）+ render.rs（渲染根/焦点仲裁）+ types.rs
│                      （跨域壳类型 Dialog/PanelTab/DockPanel/ContentView/
│                      Pill 三件套/FileTab…；单域类型已下沉所属域）
├── startup.rs         启动唯一入口(010)
├── agent_session.rs   无头实体:PiSession/事件泵/epoch/on_event 分发
├── appearance/        主题注册表 + ActiveTheme + 字体(theme.rs,rgba_a 等色助手)
├── content.rs         内容区状态机 chat/term/md(v54)
├── dialogs.rs         弹窗域渲染 + 打开器
├── ext_ui.rs          扩展 UI 面
├── terminal/          内置终端(alacritty+ConPTY):mod(Element/paint/impl Chat)
│                      + palette + snapshot + input_encode(键盘/IME→PTY)
├── i18n.rs assets.rs status_bar.rs titlebar.rs top_panels.rs
├── automation/        pif-ui 自动化服务(snapshot/handlers)
├── editor/            编辑器域:kind FileTypeHandler 注册表 + view + tabs +
│                      markdown/(mod 渲染入口 + highlight syntect + parse
│                      pulldown + links + doc 文档模式 + wysiwyg/ + render/
│                      + tests)
├── function_panel/    左 dock 域:mod(psp 列表渲染 + HoverCard/PspMenu/Tree*
│                      overlay 状态类型) + psp_actions + psp_overlays +
│                      file_tree + file_actions + git_panel
├── remote_control/    微信桥(060;mod/menu/pipeline)
├── services/          无 UI 助手(pi-web lib/ parity)
├── session/           会话域:mod(sessionView 组装/列表) + runtime/(mod 结构+
│                      生命周期+泵、events 事件臂、send 发送路径+rpc 门面、
│                      nav 定位对账、ingest 摄入) + messages/(mod、model 类型
│                      与尾窗解码、cards 工具/压缩卡、turn 行轮渲染、images
│                      解码缓存) + input/(mod Chat 委托层、area 主输入区+
│                      键路由、bar 控件行、ctx_tip 用量面板) + pool + chat_list
│                      + menu + attachments + fork + diff + pickers +
│                      new_session + tools_recipe
├── settings/          设置六页(models/general/plugins/skills/mcp/
│                      custom_models/misc/remote;McProbe/PkgOp 各归其页)
└── ui/                通用件:tokens/button/text_input/composer_input/dropdown/
                       overlay/popup/vlist/psp_scrollbar/tooltip
```

pi-link:sessions.rs 索引化扫描(group 目录定位 + tail-seek 摘要 + (mtime,size)
指纹索引落盘 + `read_tail_messages` 尾窗解析)+ 协议层(不变,只对钉版 pi 负责)。

## 2.5 会话模型(pi-web 多开对齐)

**每个会话 = 一个常驻 SessionRuntime 实体**(session/runtime.rs):独立 pi 进程
(AgentSession)+ 全量消息 + 投影状态 + **独立 inputPanel(草稿/附件/history/
思考强度/工具集/模型)** + 自有事件泵与 title 泵。创建即常驻,删除才销毁;
**几十个 agent 并发工作,互不依赖用户注视**——切换 = 用户注意力转移
(Chat.active_key 改指针 + 编辑器镜像 flush/load,毫秒级、零 IO、零进程操作);
侧栏转圈 = 每行读自己 runtime 的 agent_running(并发监控面板)。
Chat 是壳:池(runtimes map)+ 注意力指针 + 侧栏/git/文件/终端/设置。
工具集无活改 RPC(pi-web 走进程内 SDK)→ 会话带 --tools 等参数重绑进程;
模型/思考强度走活改 RPC。空闲回收:非 active 且空闲 10 分钟只杀进程、
消息保留。

## 3. 状态所有权

Chat 是壳实体，**直属字段只有 8 个子结构**：`pool`（会话池/
指针/缓存/启动闸门）、`composer`（输入面板 031）、`layout`（dock/路由/分隔条/
flyout/焦点）、`psp`（项目/会话列表/搜索/通知点）、`ws`（文件树/编辑器/终端/
git/cwd）、`dlg`（弹窗/悬浮层/右键菜单/toast/设置句柄）、`cfg`（设置页配置状态）、
`aux`（扩展 UI/更新/远程/系统提示词弹窗）。跨域字段访问在代码里可见可查：
`chat.ws.tree_rows` vs `chat.dlg.tree_menu` 一眼可辨归属。

| 状态 | 属主 |
|---|---|
| RPC 会话/事件泵/epoch | AgentSession（无头）+ `session/runtime/`（泵在 runtime/mod.rs） |
| 消息数据/折叠/流式状态 | SessionRuntime 字段（`session/runtime/`）；**渲染**是
  `session/messages/`（model/cards/turn/images 四件，无状态自由函数）；
  列表组装在 session/mod.rs |
| 编辑器草稿/附件/历史（**双宿主镜像**） | Chat.composer（UI 侧）+ SessionRuntime
  （会话侧）各一份；镜像协议两个写入点：切会话 `pool.rs switch_to`（flush/load）、
  发送前 `input/mod.rs with_active_editor`（Chat→Runtime 刷草稿）。改镜像协议
  只动这两个方法 |
| 会话列表渲染/搜索弹窗 | Chat.psp（状态）+ function_panel（psp 渲染）+ dialogs.rs（搜索弹窗） |
| cwd/branch/git/文件树/终端/编辑器 tab | Chat.ws |
| 弹窗/右键菜单/toast/设置面板句柄 | Chat.dlg |
| 设置页配置状态（mc_*/provider registry/mcp） | Chat.cfg；面板实体渲染在 settings/ |
| 窗口 bounds/布局状态 | 状态文件(services/workspace,shell 恢复) |
| 扩展 UI/自动更新/远程桥 | Chat.aux |
| 主题/语言/字体 | 全局(appearance/i18n) |

**RPC 发送纪律**：`Command` 直发只允许出现在
`session/runtime/` 域内与 agent_session.rs；域外一律走 `SessionRuntime::rpc()`
门面（无进程返回 false）。新命令接入只改 runtime 域。

## 4. 启动(pi-web 启动清单 × zed 机制)

四层清单(pi-web 核实):恢复层(主题/语言/布局/上次 workspace/每项目上次会话,
同步一次读)→ 会话列表层(索引化扫描)→ 会话内容+运行时层(磁盘直读渲染,
pi 单次 spawn,新会话懒 spawn)→ 后台杂项(git/模型,不阻塞)。

zed 机制:骨架先行(首帧零数据 IO)、同步小读+异步节流写、目录树后台增量扫描、
不存树展开状态(用最后活动文件 + auto-reveal 替代)、无总闸门渐进填充。

状态机:`Restore(<5ms,一次读全部状态文件)→ FirstFrame(<50ms,壳骨架+欢迎页)
→ BackgroundFill(会话列表/文件树/git 并行;pi 单次 spawn 带 --session,
newSession 懒到首条 prompt)→ SessionReady(磁盘直读渲染上次对话,pi ready 后
RPC 对账)→ 页面流转`。

**预算(验收线):首帧 <50ms;会话列表 <150ms;上次对话消息上屏 <300ms
(与 node 无关);pi 就绪 <1s 且不阻塞任何 UI。** 实测基线:热索引扫描
49 会话 5-9ms/0 文件扫描。

**加载深度与预加载**:pi 本体无窗口——
SessionManager.open 全量解析文件(82MB ~1.1s),get_messages 返回
compaction 感知投影全量(压缩后 = 摘要检查点 + 压缩点之后的消息,
活跃投影通常远小于原始消息数);pi-web 的 50 条尾窗是其 API route
自截断(tail=50 cap 1000,toolResult 不占额,raw cap=max(200,tail×6)),
并非来自 pi。pi-web 的"切项目秒出"= 页面内存 LRU(8 会话/32MB)+
SM 指纹缓存(12 个/256MB)+ 尾窗响应,无跨项目预加载。
pi-flash 策略:read_tail_messages 物理尾窗(256KB/100 条)天然快于
pi-web 冷路径;启动后台预加载**跨项目最近 N 个会话**的尾窗进内存
(`preload_sessions`,app_settings.json,缺省 10,0=关),open_session
先查 `session_tail_cache` 命中秒出;get_messages 投影快照到达后
clear+替换对账(权威投影替换预渲染,修复叠加翻倍)。已知偏差:
物理尾窗可能含废弃分支条目,由 RPC 对账校正;分支感知尾窗留 G+。

## 5. 外观体系(006/007)

- **主题**:ZED 架构(ThemeRegistry + ActiveTheme + 主题族 light/dark)。
  内置 7 套:浅色 mist(雾青)/rose(蔷薇,pi-web 令牌转换)+ One Light/
  nord light/ayu light(拷 zed 主题 JSON);深色 One Dark/nord dark。
  主题切换需重跑 gpui-component token 映射。
- **syntax**:zed 主题 syntax 色 → syntect 色彩方案映射(markdown.rs +
  syntect 保留;不引 zed markdown,markdown preview 对齐 pi-web)。
- **icon theme**:ZED 架构(注册表/文件图标集解析,服务 021 文件树),
  内置内容 = pi-web 图标。
- **字体三档**:session font(≈zed UI font)/ panel font / markdown preview
  font + 字号(app_settings.json)。
- **配置三层分界**:pi `settings.json`(模型/凭证/工具,归 pi)≠
  `~/.pi/agent/pi-flash-app-settings.json`(主题/图标/字体/lang/sound)≠
  `pi-flash-workspace.json`(运行时状态:每项目上次会话/__window/__dock 布局)。
  后两者原子写(tmp+rename)+ 内容不变跳过 + 进程内单次读缓存。
- **多语言(007)**:i18n.rs 现行模式;新模块字符串一律 `tr()` 收口。

## 6. TextInput:全 app 唯一文本输入

`ui/text_input.rs` = gpui-component InputState 的 facade(真实选区/光标/
剪贴板/IME 由其提供)。**铁律:任何文本输入禁止用 `on_key_down`
字符匹配手搓。** 例外:终端(PTY 字节流)与主编辑器(多行,EditorInputElement
+ EntityInputHandler,随 Composer 迁移,收敛到 InputState 基座是既定方向,
迁移时须实测中文 IME)。

## 7. 通信规约

- 父→子:方法调用;子→父:`cx.emit` + `subscribe`;
  禁止子组件持 `WeakEntity<父>` 散弹式 update(回调闭包除外)
- 视图组件不直接碰 PiSession RPC(pi-web:ChatInput/MessageView 无网络请求)

## 7.5 UI 自动化服务

- **目的**:agent 调试不抢真实屏幕/鼠标——数据快照代替截图,方法直调代替鼠标点击。
- **协议**:`pi_link::automation`(NDJSON over 127.0.0.1;hello→auth→auth_ok→
  Request/Response;method 常量表 = op 清单唯一事实源;协议测试在 pi-link)。
- **服务**:`crates/app/src/automation/`(mod=listener+unbounded channel+`cx.spawn`
  泵+catch_unwind;snapshot=字段→JSON 只读;handlers=直调 Chat 方法)。op 在主线程
  catch_unwind 内执行,自动化 panic 不许带崩 app。
- **开关**:`PI_FLASH_AUTOMATION=<port|auto|1>`,缺省关;token 走
  `<配置目录>/automation/<pid>.json`(启动时探活清理死文件,CLI `list` 同规)。
- **CLI**:`pi-link` 的 `pif-ui` bin(list/info/snapshot/exec/keys/type/wait)。
- **边界**:文本输入一律直调 setter(gpui-component 文本走 IME/替换路径,KeyDown
  逐字不可靠);无头截图 gpui 0.2.2 无回读 API 不做,纯视觉问题留给人工;升级 gpui
  时可 backport Zed 的 AccessKit a11y 树 + capture_screenshot 补上。

## 8. 守护规则(scripts/check_arch.sh)

1. 单文件**非测试行数** ≤1500（`#[cfg(test)]` 块与文件式测试模块不计，
   测试体量不惩罚生产代码）；顶层视图函数 ≤300 行
   （仅扫 crates/app；视图函数在本仓均为顶层自由函数）
2. 禁止 `on_key_down` 字符匹配输入模式
3. `cargo build` 无新警告;`cargo test` 全绿
4. 新 UI 字符串走 `tr()`

守卫挂进发版 checklist（RELEASE.md §2）——发版前必跑 `scripts/check_arch.sh`。

## 9. 待办

- G+(功能细化,等详细要求;含 InputPanel/SessionsPanel/FunctionPanel
  实体升级、外观字体设置 tab、032 气泡控制栏、013 会话搜索)。
- 待补交互实测:dock 换边/git 面板操作/池多会话并发场景。
