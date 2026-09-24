# pi-flash 进度记录

> 本文件是唯一进度台账（AGENTS.md 只保留铁律与路径）。
> 每轮工作后更新「当前状态」与「里程碑历史」。

## 当前状态（2025-09，提交 3fb7bbe）

- workspace：`crates/pi-link`（协议层，23 测试）+ `crates/app`（GPUI 界面，6 markdown 测试）+ `vendor/pi`（钉版 0.87.1）
- 界面：mist 主题（4 套主题逐值翻译自 pi-web globals.css），侧栏/工具栏/输入区/状态栏结构对齐 pi-web
- 功能已通：流式聊天/steer/中断、Markdown+语法高亮、thinking 折叠(自动+手动)、工具卡片(参数+结果)、
  会话列表/恢复(546 条验证)/新建/删除/改名/启发式标题/导出 HTML、模型切换弹窗+思考级别循环、
  stats/cost 显示、斜杠菜单(42 命令)/@文件菜单/历史/Shift+Enter 多行、图片发送(chips+base64)、文件预览弹窗
- 测试：29 全绿（pi-link 23 + markdown 6）

## 里程碑历史

### M0 冒烟（hello-gpui + spike）
- rustc 1.90 msvc + crates.io gpui 0.2.2（自包含，blade/Vulkan）窗口/渲染/字体/DPI 150% OK；首编 3.5min，增量 6s
- spike：GPUI 聊天窗 ↔ `pi --mode rpc` 端到端；要点见下方「协议陷阱」

### M1 聊天核心
- 块化消息（text/thinking/toolCall）、Markdown 渲染（pulldown-cmark→StyledText）+ 语法高亮（syntect, base16-ocean.dark）
- thinking 自动+手动折叠、工具卡片（参数 pretty + 结果区）
- 会话侧栏（list_sessions 扫 `~/.pi/agent/sessions`）、`--session` 恢复 + get_messages 回放（546 条验证）
- steer（流式中 Enter→steer）、get_state 快照（头部模型实时显示）、AgentSettled 刷新
- epoch 守卫：旧 sidecar 事件不污染新会话

### M2 会话工作区（进行中）
- stats（ctx%/cost）头部显示、会话删除（活动保护）、启发式生成标题、导出 HTML
- 模型切换弹窗（过滤 + provider/name/ctx + set_model）、思考级别循环
- 文件浏览器点击 → FilePreview 弹窗（200KB/20k 上限、二进制检测）
- ChatInput：斜杠菜单（42 命令）、@文件菜单（walk_files 深度3/上限400）、历史 ↑↓、Shift+Enter 多行
- 图片发送：🖼 → prompt_for_paths 多选 → base64 chips → prompt/steer images 字段

### 界面改版
- theme.rs 4 主题逐值翻译（mist 默认，PI_FLASH_THEME 可切换）
- 侧栏完整结构（品牌+新建+搜索/项目框/分支框(.git/HEAD)/会话行「X前 · N 条消息」/文件浏览器/底部导航）
- 顶部工具栏 pills（完整历史/生成标题/系统/工具/导出）+ 右侧令牌统计（↑in ↓out $cost ctx%/win）
- 输入区（圆角框+发送按钮+工具行：图片/模型/思考级别/压缩/音频）+ 每消息 usage 脚注 + HH:MM

## 协议陷阱（实测钉进 fixture）

- 内容块类型是 camelCase `"toolCall"`（message.content 数组）
- 流式 args 起始来自 `partialJson`（message_start 阶段 arguments 为空对象）
- 工具结果以 `role:"toolResult"` 独立消息回灌（需挂接回工具卡片）
- set_model 字段是 **`modelId`**（pi 报 `Model not found: x/undefined` 即此）
- client 不得硬编码 `--no-session`（会压掉 `--session`，恢复为空）
- GPUI：on_mouse_down/listener 漏 `cx.notify()` = 状态变 UI 不动

## 截图索引（tmp/屏幕截图/）

| 文件 | 内容 |
|---|---|
| 冒烟-hello-gpui.png | GPUI 首窗口（150% DPI）|
| spike-pi桥接.png | spike 端到端首轮对话 |
| 布局改版-全貌.png | pi-web 化布局全貌 |
| 斜杠菜单.png | 42 命令菜单 |
| 模型切换-ok.png | set_model ok 状态栏 |
| 用户气泡-mist主题.png | 用户气泡+thinking 卡片 |
| 文件预览.png | Cargo.toml 预览弹窗 |

## 待办（对应 PORT_PLAN.md）

- M2 剩余：分支导航（fork/tree，需 get_entries 追踪 entryId）
- M3：文件树交互深化（目录展开）、git status/diff、内置终端
- M4：配置面（模型/插件/技能/系统提示词面板）
- M5：扩展 UI 协议、子代理、原生通知
- M6：i18n、SVG 图标替换 emoji、主题运行时切换、macOS CI、LLM 生成标题（需 pi-ai SDK）
