# pi-flash 进度记录

> 本文件是唯一进度台账（AGENTS.md 只保留铁律与路径）。
> 每轮工作后更新「当前状态」与「里程碑历史」。

## 当前状态（2025-09，提交 816fad6）
## 当前状态（2025-09，会话行对齐 pi-web）

- workspace：`crates/pi-link`（协议层，23 测试）+ `crates/app`（GPUI 界面：assets/markdown/theme + 主渲染，7 markdown 测试）+ `vendor/pi`（钉版 0.87.1）
- **会话行对齐 pi-web SessionItem（本轮）**：54px 固定行高、单行标题 ellipsis、
  运行中 loader spinner（accent 色 14px）、hover 才显示 ✏/🗑 按钮（28px 圆角 7、
  pencil hover 变 accent、trash hover 变红 #ef4444、cx.stop_propagation 防误开行）、
  整行可点恢复会话、非活动会话改名 = open(p,true) 后 get_state 自动弹改名框
- 改名预填对齐 pi-web：session_name || 首条用户消息前 50 字
- 入口修复（816fad6 重写时丢失的接线全部补回）：⚙ 模型弹窗、🖼 attach_images、
  💡 cycle_thinking、生成标题 auto_title、文件树文件行 open_file_preview
- 弹窗统一 ESC 关闭：三处 overlay track_focus(dialog_focus) + on_key_down escape
- 图标：loader.svg 对齐 pi-web spinner path（M21 12a9 9 0 1 1-3.8-7.4, sw 2.8）、新增 trash.svg
- 功能已通：流式聊天/steer/中断/图片发送、Markdown+语法高亮、thinking 折叠(自动+手动)、
  工具卡片(参数+结果)、会话列表/恢复(546 条验证)/新建/删除/改名弹窗/启发式标题/导出 HTML、
  模型切换弹窗+思考级别循环、stats/cost 显示、斜杠菜单(42 命令)/@文件菜单/历史/多行、文件预览
- 测试：30 全绿（pi-link 23 + markdown 7）
## 协议陷阱（实测钉进 fixture）

- 内容块类型是 camelCase `"toolCall"`（message.content 数组）
- 流式 args 起始来自 `partialJson`（message_start 阶段 arguments 为空对象）
- 工具结果以 `role:"toolResult"` 独立消息回灌（需挂接回工具卡片）
- set_model 字段是 **`modelId`**（pi 报 `Model not found: x/undefined` 即此）
- client 不得硬编码 `--no-session`（会压掉 `--session`，恢复为空）
- GPUI：on_mouse_down/listener 漏 `cx.notify()` = 状态变 UI 不动

## 截图索引（tmp/屏幕截图/）

| 文件 | 内容 |
| 图标版-全貌.png | SVG 图标版主界面（当前）|
|---|---|
| 冒烟-hello-gpui.png | GPUI 首窗口（150% DPI）|
| spike-pi桥接.png | spike 端到端首轮对话 |
| 布局改版-全貌.png | pi-web 化布局全貌 |
| 斜杠菜单.png | 42 命令菜单 |
| 模型切换-ok.png | set_model ok 状态栏 |
| 模型弹窗.png | select model 弹窗（过滤+ctx 窗口宽）|
| 用户气泡-mist主题.png | 用户气泡+thinking 卡片 |
| 会话行-hover按钮.png | 单行截断+hover ✏/🗑 |
| 图片选择器.png | 🖼 attach_images 文件选择器 |
| 文件预览.png | Cargo.toml 预览弹窗 |
## 待办

已迁移至 beads 任务跟踪（`bd list` / `bd ready` 查看剩余工作；完成后 `bd close <id>`）。
计划全量见 PORT_PLAN.md。
