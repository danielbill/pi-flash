# pi-web → pi-flash 复刻计划

> 原则：**pi-web 是产品需求文档**。功能 1:1 复刻、交互对齐，差异只在底层实现（Rust + GPUI 原生，目标极速）。
> 唯一架构偏差：pi-web 把钉版 pi 包内嵌在自己 Node 进程里跑；Rust 嵌不了 JS SDK，等价做法是 **vendor 钉版 pi、spawn 其 cli.js**（见六、「内置 pi」节）。

## 一、架构映射：Web 三层坍缩为单进程

```
pi-web:  浏览器(React) ──HTTP/SSE──> Next.js 服务(lib/*) ──SDK/RPC──> pi
pi-flash: GPUI UI(crates/ui) ──直接调用──> 核心 crates ──JSONL/stdio──> pi sidecar
```

坍缩收益 = 极速来源的一部分：无 HTTP/SSE 序列化、无 DOM、无虚拟 DOM diff、GPU 直绘、原生文件监听。

## 二、模块映射表

| pi-web | pi-flash crate | 参考 zed crate | 说明 |
|---|---|---|---|
| components/AppShell + TabBar | `shell` | ui, title_bar | 主布局、标签页、侧栏停靠 |
| ChatWindow/ChatInput/MessageView/MarkdownBody | `chat` + `markdown` | markdown, agent_ui | 流式对话、Markdown、思考块 |
| ChatMinimap / chat-scroll-position / chat-lazy-load | `chat` | — | 缩略图、滚动锚定、懒加载 |
| AnsiText / TerminalPanel / terminal-tab-state | `terminal` | terminal(alacritty_terminal) | 内置终端、扩展 custom-ui |
| SessionSidebar / SessionSearch / session-catalog | `sessions` | — | 会话工作区（项目/worktree 分组、搜索、改名/导出/删除）|
| BranchNavigator + fork/编辑分支 | `sessions` | — | 两种分支方式 |
| FileExplorer / FileViewer / ImagePreview | `files` | — | 文件浏览、多格式预览、变更自动刷新 |
| git diff/status 路由 | `vcs` | git_ui(参考) | Git 状态与 diff 视图 |
| ModelsConfig/ModelSelector/EnabledModels | `models` | settings_ui(参考) | 模型目录、启停、切换、测试 |
| SettingsPanel/PluginsConfig/SkillsConfig/SystemPromptPanel/ToolDefinitionsPanel | `config` | settings_ui | 配置面（provider 登录/插件/技能/系统提示词/工具选择）|
| ExtensionWidgets/ExtensionStatusBar/custom-ui-terminal | `extensions-ui` | — | pi 扩展 UI 协议（rpc-extension-ui.md）|
| subagent-* | `subagents` | — | 子代理 profiles/运行/控制 |
| useI18n (en/zh-CN/zh-TW) | `i18n` | — | 三语 |
| useTheme + pi themes | `theme` | theme, syntax_theme | 主题（含 pi 原生主题兼容）|
| web-push/browser-notifications | 原生通知 | — | 桌面通知替代 web push |
| provider-usage 路由 | `usage` | — | 用量查询 |

## 三、里程碑（每个可交付、可发布）

- **M1 聊天核心**（先行）：单会话聊天窗（流式/中断/排队/steer）、pi sidecar 管理器（会话↔进程生命周期）、会话列表+恢复、Markdown 渲染、tool call 紧凑卡片、暗色主题、全局快捷键。
  验收：日常用 pi-flash 完成一轮真实编码对话，体感不输 pi-web。
- **M2 会话工作区**：项目/worktree 分组侧栏、搜索、改名/导出/删除、context 占用/花费/压缩显示、两种分支。
- **M3 文件与终端**：文件树、源码/Markdown/图片预览、git status/diff、内置终端、worktree 切换。
- **M4 配置面**：provider/模型/测试、插件包、技能、系统提示词、工具选择、设置持久化。
- **M5 扩展生态**：扩展 UI 协议（widgets/状态栏/dialog）、子代理、原生通知。
- **M6 打磨发布**：i18n 三语、主题兼容 pi 主题、macOS CI 签名包、自动更新。

## 四、刻意推迟

- PWA/移动端布局（桌面形态天然替代；远程访问模式后议）
- Mermaid 图渲染（先输出代码块；后续评估 svg 渲染）
- DOCX/PDF 预览（M3 先做源码/md/图片，二进制格式后置）

## 五、极速预算（复刻之外的差异化指标）

| 指标 | pi-web（浏览器） | pi-flash 目标 |
|---|---|---|
| 冷启动 | 数秒（Next.js + 浏览器） | < 300ms |
| 会话切换 | 网络往返 + 重渲染 | < 16ms（直接读会话文件缓存）|
| 流式刷新 | DOM 更新 | GPU 直绘，掉帧 0 |
| 内存（常驻）| Node + Chromium 百 MB 级 | < 60MB |

## 六、工程约定

### 内置 pi（关键对齐，与 pi-web 同策略）

- pi 每次升级都可能改接口，因此 **pi-flash 内置一个钉版 pi，绝不调用系统安装的 pi、绝不读 PATH**（对齐 pi-web 的 `@earendil-works/pi-coding-agent` 精确钉版、无 `^` 的做法）
- vendor 方式：发布构建时 `npm pack @earendil-works/pi-coding-agent@<PIN>` 解包进 `vendor/pi/` 随应用分发；运行时 spawn `node <app>/vendor/pi/dist/bundle/cli.js --mode rpc`
- 版本号唯一来源：构建配置的 `PI_VENDOR_VERSION`，同时记录在 vendor 目录内
- 升级 = 刻意行为：bump PIN → 跑 `pi-link` 协议符合性测试（对 vendored pi 实测）→ 发布；pi-link 的类型与测试只对钉版负责
- 开发期可用 `PI_FLASH_PI_BIN` 覆盖指向本地 pi 调试；发布形态永远用 vendor
- Node 运行时要求与 pi-web 对齐：>= 22.19（不捆绑 Node，不发明新东西）

### 其他

- workspace 多 crate（shell/chat/sessions/files/terminal/config/pi-link/theme/i18n），`pi-link` 独立封装 pi RPC 协议（可单测、无 UI 依赖）
- 协议层以 vendored pi 的 `docs/rpc.md` + `rpc-types.ts` 为准；每条命令/事件在 `pi-link` 有类型 + 测试
- UI 交互以 pi-web 组件行为为准（含其 ADR 决策：隔离项目命令环境、chat-only 工具选择、子代理开关、模型启停）
