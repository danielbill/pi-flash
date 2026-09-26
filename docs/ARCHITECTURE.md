# pi-flash 架构契约(v2:ZED 框架 × pi-web 对话层 × 极速启动)

本文档是架构契约。改代码前先对照本文;改完架构相关的代码要同步更新本文。
上位规格:`docs/模块设计/`(005-032,模块边界以此为准,本文负责技术落地)。

## 0. 对齐基准(双轨制)

| 层 | 原型 | 方式 |
|---|---|---|
| 框架层(布局/dock/文件树/git 面板/状态栏/顶栏) | **ZED**(`D:\github\zed`,移植基线 commit `4c902c9`,2026-09-23) | 剪裁移植源码(渲染/交互照抄,数据层接本地) |
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
├── main.rs            bootstrap(<300 行)
├── startup.rs         启动编排:恢复层/骨架先行/后台填充管线(§4)
├── appearance/        主题注册表 + ActiveTheme + icon theme + 字体(§5)
├── shell.rs           AppShell:布局组装/页面路由(welcome|newSession|session)/
│                      dock 位置/焦点仲裁 (= 005)
├── titlebar.rs        顶栏:logo + settings + 窗口控制三按钮 (= 005 上段;
│                      源:zed platform_title_bar/platform_windows.rs 直搬+stub)
├── function_panel/    ZED 式 dock(= 015;源:zed workspace/dock.rs 剪裁)
│   ├── mod.rs         容器:视图互斥切换 + 左右 dock 位置(018 状态栏驱动)
│   ├── sessions.rs    projectSessionList(= 020;设计规格:ZCode)
│   ├── file_tree.rs   dirTreeView(= 021;源:zed project_panel 骨架移植)
│   ├── git_panel.rs   gitPanel(= 022;源:zed git_panel 行渲染,简化版:
│   │                  Changes|History 双列表 + commit/push,无 diff 视图)
│   └── (terminal)     终端宿主(第 4 视图;alacritty_terminal 不变)
├── status_bar.rs      statusControlBar(= 018;源:zed status_bar.rs StatusItemView)
├── pages/             welcome(= 011)+ new_session(= 012,复用 inputPanel)
├── session/
│   ├── mod.rs         sessionView 容器(= 030)
│   ├── messages.rs    sessionMessagePanel:render_msg/render_block/流式(= 032)
│   └── input.rs       inputPanel 专用 Composer 实体:编辑器基座+工具栏
│                      (发送/停止/steer/排队+模型/思考/skill/权限/工具/压缩/
│                       上下文/图片 pill)+补全菜单(= 031)
├── agent_session.rs   无头实体:PiSession/事件泵/epoch/on_event 分发;
│                      单次 spawn + 磁盘直读消息(pi ready 前 RPC 对账)
├── zed_ui/            vendored zed ui 原语(Icon/Button/ListItem/ContextMenu/
│                      Tooltip/IndentGuides/Scrollbar/DiffStat 等,剪裁版)
├── dialogs.rs         ModelSelect/BranchTree/ProjectSelect/GitDiff/文件预览弹窗
├── ext_ui.rs          扩展 widgets/statusbar/dialog/notice(自 settings/tools.rs 归位)
├── session_search.rs  搜索弹窗 + 结果页(= 013,细节后置)
├── settings/          设置 modal(= SettingsPanel;模型/技能/插件/工具/子代理/通用)
├── ui/                text_input.rs(全 app 唯一文本输入,§6)+ 图标原语
├── services/          workspace(状态+app_settings)/git/title/branch/format
└── theme.rs i18n.rs markdown.rs models_config.rs terminal.rs assets.rs
```

pi-link:sessions.rs 索引化扫描(group 目录定位 + tail-seek 摘要 + (mtime,size)
指纹索引落盘 + `read_tail_messages` 尾窗解析)+ 协议层(不变,只对钉版 pi 负责)。

## 3. 状态所有权

| 状态 | 属主 |
|---|---|
| RPC 会话/事件泵/epoch/消息数据 | AgentSession(无头) |
| 消息渲染/折叠/流式状态 | session/messages |
| 编辑器草稿/附件/pill 菜单/history | session/input(Composer 实体) |
| 会话列表/搜索/改名/删除确认/分钟 ticker | function_panel/sessions |
| cwd/branch/git 状态/页面路由/dock 位置 | shell |
| 窗口 bounds/布局状态 | 状态文件(services/workspace,shell 恢复) |
| settings 面板全部状态 | Settings 实体(自持) |
| 扩展 UI 状态 | ExtUi |
| 主题/语言/字体 | 全局(appearance/i18n) |

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
49 会话 5-9ms/0 文件扫描(阶段 A,原 200-400ms×2 全量读)。

## 5. 外观体系(006/007)

- **主题**:ZED 架构(ThemeRegistry + ActiveTheme + 主题族 light/dark)。
  内置 7 套:浅色 mist(雾青)/rose(蔷薇,pi-web 令牌转换)+ One Light/
  nord light/ayu light(拷 zed 主题 JSON);深色 One Dark/nord dark。
  主题切换需重跑 gpui-component token 映射。
- **syntax**:zed 主题 syntax 色 → syntect 色彩方案映射(markdown.rs +
  syntect 保留;006 定案不引 zed markdown,markdown preview 对齐 pi-web)。
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
剪贴板/IME 由其提供;e60f842 起)。**铁律:任何文本输入禁止用 `on_key_down`
字符匹配手搓。** 例外:终端(PTY 字节流)与主编辑器(多行,EditorInputElement
+ EntityInputHandler,随 Composer 迁移,收敛到 InputState 基座是既定方向,
迁移时须实测中文 IME)。

## 7. 通信规约

- 父→子:方法调用;子→父:`cx.emit` + `subscribe`;
  禁止子组件持 `WeakEntity<父>` 散弹式 update(回调闭包除外)
- 视图组件不直接碰 PiSession RPC(pi-web:ChatInput/MessageView 无网络请求)

## 8. 守护规则(scripts/check_arch.sh,阶段 F 落地)

1. 单文件 ≤1500 行;render 函数 ≤300 行
2. 禁止 `on_key_down` 字符匹配输入模式
3. `cargo build` 无新警告;`cargo test` 全绿
4. 新 UI 字符串走 `tr()`

## 9. 迁移记录

- 阶段 1(2026-09-25,52c5675):ui/ + TextInput 落地,9 输入点接入。
- 阶段 2(9bcf25b):services/ 层拆出,main.rs 9881→9055。
- 阶段 3a(c0d8f55):settings/ 模块树,→6180。
- 阶段 3b(848b96b):SettingsPanel 独立实体;主线程栈 16MB。
- e60f842:TextInput 换 gpui-component InputState 基座;窗口根包 Root。
- 3b8ae6f:docs/模块设计/ 新设计(005-032)入库,驱动本轮重构。
- **阶段 A(93c29e9,2026-09-26)**:pi-link sessions 索引化扫描(group 定位/
  tail-seek/指纹索引/尾窗解析,热扫描 5-9ms)+ app_settings.json/状态文件
  三层配置(原子写/单次读缓存/__window/__dock schema)。
- **阶段 B(2026-09-26)**:撤 settings 双 BISECT 短路(render_settings 空返回 +
  open_settings reload-only),按 3b 架构重接 SettingsPanel;修焦点仲裁缺失
  settings 分支(Esc 关闭弹窗,原 Esc 误触会话中止);6 tab UI 实测通过;
  修 015/020 文档;本文重写为 v2 契约。
- **阶段 C(a34af22,2026-09-26)**:render() 3260→1291 行,main.rs 6173→3533。
  新边界模块(session/messages+input、dialogs、ext_ui、function_panel/mod+file_tree)
  以 settings 式自由函数承接原内联区块,零行为变更;警告 20→19(余量均为
  BISECT 时代死代码,阶段 D/E 清)。
- **阶段 D(f09b60e,2026-09-26)**:005 壳骨架落地——无边框窗口 + 自绘顶栏
  (WindowControlArea 拖拽/三按钮,zed platform_title_bar 机制)、底部
  statusControlBar(018,四视图互斥 + 右键换边)、functionPanel dock、
  welcome 页面态 + 后台填充(会话列表不再阻塞首帧,骨架先行)、窗口
  bounds/最大化恢复;右面板拆除(终端宿主暂挂 dock Terminal 视图,阶段 E
  归位);appearance/ 注册表(mist/rose/one-dark;One Light/nord/ayu 待
  zed 仓库可达补数据,真实源:fallback_themes.rs + assets/themes JSON)+
  切换重跑 gpui-component 映射 + 字体槽位 + icon theme 骨架;设置通用页
  主题选择器接新链路。default/dark 主题按 006 退役。
- **阶段 E(32c985d + 45e4404 + 51e9984,2026-09-26,已完成)**:
  ① dock 收尾——git 面板简化版(Changes|History + 行点击暂存 + commit/push,
  push 走后台执行器;services/git.rs 增 stage/unstage/commit/push/log)、
  终端宿主归位 function_panel::terminal_view、文件预览弹窗化(PanelTab::File
  删除);② **AgentSession 无头实体**(Chat 持 Entity<AgentSession>,session/
  epoch 进实体,spawn 走 entity update,refresh_state 取 &App);③ **单次
  spawn**(启动恢复决策前移,消灭白起再杀的 node 进程);④ **磁盘直读**
  (启动恢复与 open_session 先用会话文件尾窗上屏,get_messages 快照后到
  对账——已实测:resuming 状态下消息已可见);⑤ sessionView 组装迁
  session::main_column(main.rs 3172→2740);⑥ 主题选择器接 registry
  (雾青/蔷薇/One Dark 显示名)。
  **E 范围裁定**:InputPanel/SessionsPanel/FunctionPanel 的完整
  Entity+EventEmitter 化移入 G+ 按需升级——模块边界已由自由函数视图落位,
  粗粒度重绘对单窗口应用足够;IME/编辑器迁移涉及最高回归风险区,须在
  031 细化要求到位后单独成战役。
- 待做:阶段 F(check_arch.sh 守护脚本)→ G+(功能细化,等详细要求;
  含 InputPanel/SessionsPanel/FunctionPanel 实体升级、外观字体设置 tab、
  One Light/nord/ayu 主题数据)。
  待补交互实测:主题切换/dock 换边/git 面板操作/磁盘直读上屏速度。
