# pi-flash 架构(1:1 对齐 pi-web)

本文档是架构契约:模块边界 = pi-web 组件边界,数据流向 = pi-web props/callbacks 流向。
改代码前先对照本文;改完架构相关的代码要同步更新本文。

## 1. 设计原则

pi-web 是三层结构,Rust/GPUI 的对应物:

| pi-web | pi-flash |
|---|---|
| React 组件(`components/*.tsx`) | GPUI `Entity<T> + Render`(Zed 模式) |
| props 向下 | 父持 `Entity<T>`,`child.update(cx, |c| c.set_x(..))` |
| callbacks 向上(`onXxx`) | 子 `cx.emit(Event)`,父 `cx.subscribe` 分发 |
| hooks(`useAgentSession` 等) | 实体内部的异步泵/服务模块 |
| lib 服务层(`lib/*.ts`) | 无 UI 的 `services/` 纯函数模块 |
| CSS 令牌(`globals.css`) | `theme.rs` 令牌(逐组件对照 pi-web 翻译,不手搓) |

## 2. 模块布局(↔ pi-web 组件映射)

```
crates/app/src/
├── main.rs        bootstrap(= app/layout.tsx + page.tsx)
├── shell.rs       AppShell:三栏布局/面板显隐与尺寸/panel tabs/选中会话/
│                  workspace 恢复/modal 路由 (= AppShell.tsx)
├── chat/
│   ├── mod.rs     ChatWindow:PiSession RPC/epoch 泵/事件 reducer/notices
│   │              (= ChatWindow.tsx + hooks/useAgentSession.ts)
│   ├── events.rs  consume_events + on_event 拆分
│   ├── message.rs Msg/render_msg/render_block (= MessageView.tsx)
│   └── stream.rs  流式组装/phase/tps (= lib/streaming-message.ts)
├── composer.rs    Composer:编辑器(IME)/图片附件/history/slash+@ 菜单/
│                  thinking+tools pill/发送-引导-排队 (= ChatInput.tsx)
├── sidebar.rs     SessionSidebar:会话列表/搜索/改名/删除确认/项目框
│                  (= SessionSidebar.tsx)
├── explorer.rs    FileExplorer:文件树 + git 徽标 (= FileExplorer.tsx)
├── panel.rs       右面板:TabBar + FileViewer + 终端宿主
│                  (= TabBar + FileViewer + TerminalPanel)
├── dialogs.rs     模型选择/分支树/项目选择/git diff/改名 modal
├── ext_ui.rs      扩展 widgets/statusbar/dialog/notice
│                  (= ExtensionWidgets + ExtensionStatusBar + ChatWindow 内 dialog)
├── settings/      设置 modal(= SettingsPanel + 各 Config 组件)
│   ├── mod.rs     壳 + Settings 实体 (= SettingsPanel.tsx)
│   ├── ui.rs      ConfigField/Button/Switch 原语 (= SettingsUi.tsx)
│   └── models.rs skills.rs plugins.rs tools.rs subagents.rs general.rs
├── ui/
│   ├── mod.rs     icon/spinner/pill(= 图标层原语)
│   └── text_input.rs TextInput —— 全 app 唯一文本输入组件(见 §4)
├── services/      (= lib/ 纯逻辑)
│   ├── workspace.rs git.rs title.rs branch.rs format.rs
└── theme.rs i18n.rs markdown.rs models_config.rs terminal.rs assets.rs(原样)
```

## 3. 状态所有权(Chat god-object 已按 pi-web 归属拆分)

| 状态 | 属主 | pi-web 依据 |
|---|---|---|
| 编辑器文本/IME/history/图片/pill 菜单 | Composer | ChatInput 自持 state |
| 消息/RPC 会话/流式/stats/branch 树/epoch | ChatWindow | useAgentSession |
| 会话列表/搜索/hover/删除确认 | Sidebar | SessionSidebar 自拉列表 |
| cwd/branch/git/面板尺寸/panel tabs/modal 路由 | AppShell | AppShell 壳状态 |
| settings 面板全部状态 | Settings 实体 | SettingsPanel 自持 |
| 扩展 UI 状态 | ExtUi(ChatWindow 子级) | pi-web 在 ChatWindow 内渲染 |
| 主题/语言 | 全局(theme.rs/i18n.rs) | useTheme/useI18n 外部 store |

## 4. TextInput:全 app 唯一文本输入

pi-web 没有共享输入组件,因为 HTML `<input>` 原生自带焦点/光标/IME,全 app
30+ 输入点零成本。GPUI 没有这个内建物——所以 `ui/text_input.rs` 就是它:

- 每实例独立 `FocusHandle`(点击聚焦、可被 frame 级焦点策略强制聚焦)
- `EntityInputHandler`(utf16 IME 组合段合同,平移自主编辑器已验证实现)
- paint 阶段 `TextInputElement` 注册 `window.handle_input`
- 聚焦时手绘闪烁光标(530ms 泵,仅聚焦实例重绘)
- placeholder / masked(API key)/ numeric(数字校验)/on_change/on_submit/on_escape

**铁律:任何文本输入禁止用 `on_key_down` 字符匹配手搓**(这正是改名框中文
输入坏掉的病根:主编辑器的 IME 修复没有沉淀成组件,5 处对话框输入各自手写
残废实现)。唯一例外:终端(PTY 字节流,非文本框)与主编辑器(多行,
Composer 迁移后同样收敛到 TextInput 基座)。

## 5. 通信规约

- 父→子:方法调用 `composer.update(cx, |c| c.set_draft(..))`
- 子→父:`cx.emit(SidebarEvent::OpenSession(path))`,父 `cx.subscribe` 分发;
  禁止子组件持 `WeakEntity<父>` 散弹式 `weak.update`(回调闭包除外)
- 视图组件不直接碰 PiSession RPC(pi-web:ChatInput/MessageView 无网络请求)

## 6. 守护规则(scripts/check_arch.sh)

1. 单文件 ≤1500 行;render 函数 ≤300 行
2. 禁止 `on_key_down` 里出现 `chars().count() == 1` 之类的字符匹配输入模式
3. cargo build 无新警告;cargo test 全绿

## 7. 迁移记录

- 阶段1(2026-09-25):ui/ + TextInput 落地,9 输入点全部接入(改名/扩展
  input+editor/API key/插件安装/子代理并发/session 搜索/模型过滤为主编辑器
  之外的 8 点;终端按设计例外)。Chat god-object 拆分自阶段3起逐步执行。
