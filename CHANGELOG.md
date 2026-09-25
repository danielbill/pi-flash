## [0.1.1] - 2026-09-25

### 新增
- **内置终端**（M3）：右侧面板终端标签页，alacritty_terminal + ConPTY；
  拖选复制（Ctrl+C/Ctrl+Shift+C）、粘贴（Ctrl+V）、滚轮回滚、独立重启按钮；
  从文件浏览器工具行「>_」打开
- **模型配置面板**（M4）：底部「模型」打开——已启用模型启停（enabledModels
  白名单，带最后一模型保护）、按 provider 管理 API Key（写入 auth.json，与
  pi 共用）；启停结果实时作用于模型选择器
- **技能 / 插件面板**（M4）：技能列表 + 「对模型可见」开关（SKILL.md 前言）；
  插件列表 / 启停 / 安装 / 移除（通过 vendored pi CLI）；agents 全局设置
  （内置开关、最大并发）
- **工具选择**（M4）：defaultTools 预设（全部/默认/只读/无），新会话生效
- **扩展 UI 协议**（M5）：扩展弹窗（选择/确认/输入）、状态栏条目、右上角
  通知 toast、编辑器文本注入
- **子代理面板**（M5）：四 scope profile 发现与启停、内置子代理管理、
  面板手动运行子会话（状态/中止/输出查看）
- **主题运行时切换**（M6）：四主题（mist/default/dark/rose）即时切换，
  持久化到 settings.json 的 theme 键（与 pi TUI 共用）
- **界面三语**（M6）：简体中文 / 繁體中文 / English，通用页切换并记忆
- **LLM 会话标题**：工具栏「生成标题」，一次性 pi 会话生成，替换启发式截断
- **macOS CI**：tag 推送自动出 .app 包（自带 node + vendored pi，可选签名公证）

### 修复（0.1.0 反馈，buglist 全清）
- 切换项目后会话列表不显示（ListState 未随过滤结果重置）
- 「新建」后无空态：新增 Logo + app/pi 版本号 hero 区（pi-web ChatWindow 对齐）
- 文件浏览器改为侧栏一半高度，且会话/文件分栏可拖动调节
- 输入框点击聚焦有高亮边框与闪烁光标（自绘编辑器补 caret）
- 思考强度改为弹出菜单（auto/low/high/max + 说明，pi-web 菜单对齐）
- 工具预设改为弹出菜单（configured/chat-only/read-only/default/full）
- 「压缩」按钮生效（rpc compact）；「声音提示」开关生效（持久化 + 完成提示音）
- 右侧面板：文件/终端混合多标签、宽度可拖动、可关闭；点文件树在面板内打开
  文件（Source/Preview 切换、大小/行数元信息），替代原预览弹窗
- 顶部「系统/工具」下拉面板实现（经 export_html 提取系统提示词与工具定义）；
  侧栏 🔍 会话文本搜索

### 修复
- node 运行时解析支持 macOS/Linux 自带布局（发布包脱离系统 Node）

### 已知限制
- 子代理由面板手动运行（模型自动派发的 Agent 工具属 pi-web 服务端层，
  不在 RPC 协议面内）
- 系统提示词查看面板暂缺（RPC get_state 未暴露 systemPrompt）
- 终端宽字符（CJK）列对齐受回退字体影响，与 xterm.js 回退行为一致


### 新增
- **内置终端**（M3）：右侧面板终端标签页，alacritty_terminal + ConPTY；
  拖选复制（Ctrl+C/Ctrl+Shift+C）、粘贴（Ctrl+V）、滚轮回滚、独立重启按钮；
  从文件浏览器工具行「>_」打开
- **模型配置面板**（M4）：底部「模型」打开——已启用模型启停（enabledModels
  白名单，带最后一模型保护）、按 provider 管理 API Key（写入 auth.json，与
  pi 共用）；启停结果实时作用于模型选择器
- **技能 / 插件面板**（M4）：技能列表 + 「对模型可见」开关（SKILL.md 前言）；
  插件列表 / 启停 / 安装 / 移除（通过 vendored pi CLI）；agents 全局设置
  （内置开关、最大并发）
- **工具选择**（M4）：defaultTools 预设（全部/默认/只读/无），新会话生效
- **扩展 UI 协议**（M5）：扩展弹窗（选择/确认/输入）、状态栏条目、右上角
  通知 toast、编辑器文本注入
- **子代理面板**（M5）：四 scope profile 发现与启停、内置子代理管理、
  面板手动运行子会话（状态/中止/输出查看）
- **主题运行时切换**（M6）：四主题（mist/default/dark/rose）即时切换，
  持久化到 settings.json 的 theme 键（与 pi TUI 共用）
- **界面三语**（M6）：简体中文 / 繁體中文 / English，通用页切换并记忆
- **LLM 会话标题**：工具栏「生成标题」，一次性 pi 会话生成，替换启发式截断
- **macOS CI**：tag 推送自动出 .app 包（自带 node + vendored pi，可选签名公证）

### 修复
- node 运行时解析支持 macOS/Linux 自带布局（发布包脱离系统 Node）

### 已知限制
- 子代理由面板手动运行（模型自动派发的 Agent 工具属 pi-web 服务端层，
  不在 RPC 协议面内）
- 系统提示词查看面板暂缺（RPC get_state 未暴露 systemPrompt）
- 终端宽字符（CJK）列对齐受回退字体影响，与 xterm.js 回退行为一致


所有对外发布的版本变化记录在此。格式参照 Keep a Changelog；
版本号从 0.1.0 起小版本递增（0.1.1、0.1.2 …），每次发布 +1。

## [0.1.0] - 2026-09-24（内部初版）

### 新增
- 聊天核心：流式对话 / steer / 中断 / 图片发送，Markdown + 语法高亮，
  thinking 折叠，工具卡片（M1）
- 会话工作区：会话列表（过滤/改名/删除）、workspace 记忆与启动恢复、
  分支导航 fork/tree（M2）
- 文件能力：递归文件树、文件预览、git status/diff 面板、长文件名图标修复
  （M3 前半，2b2eeb6）
- 钉版 pi 0.87.1 内置分发 + pi-link RPC 协议层（含协议陷阱 fixture 测试）
