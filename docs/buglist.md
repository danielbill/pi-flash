## 0.1.0（全部修复于 0.1.1）
- 左侧导航栏问题：
  - ✅ 选中项目后，该项目的对话列表没有显示（已修复）
  - ✅ 新建 按钮后，页面应当刷新如下（已修复，含空态 Logo + 版本号）：
  ![新建页面](image.bmp)
  - ✅ 文件浏览器 窗口默认导航栏一半高度（已修复）

- 中间输入框问题：
  - ✅ 点击输入框不显示闪动光标，没有聚焦效果（已修复）
  - ✅ 点击思考强度，不应该是循环切换，而应该弹出菜单让用户选择（已修复）
  ![思考强度菜单](image_1.bmp)
  - ✅ 点击工具配置没反应，应该弹出工具配置菜单，让用户选择（已修复）
  ![工具配置菜单](image_2.bmp)
  - ✅ 点击 压缩 按钮，没反应（已修复）
  - ✅ 点击 声音提示 按钮，没反应（已修复）

- 左侧面板完全没实现：
  - ✅ 右侧面板宽度可拖动，且提供多标签，可在文件、terminal 之间切换，具体如何实现去查pi-web源码（已修复）
  - 文件浏览器 点击 terminal 按钮后的界面：
  ![打开terminal界面](image_3.bmp)
  - 文件浏览器 点击 文件后的界面：
  ![文件浏览界面](image_4.bmp)

- ✅ 上面命令面板没实现（已修复）：
查看pi-web源码复刻实现

## 待验证（本轮修复）
- ✅ 发送消息后到 agent 首个 token 之间没有等待动画（设计 §13 已设计，实现被回显逻辑掐掉）
  - 根因：`phase_waiting` 同时充当「等待行可见」与「乐观发送待回显」标记，pi 回显 user 消息（瞬时）把它清零 → 等待行一闪即灭
  - 修法：拆出 `pending_echo: Option<String>` 只做回显去重；`phase_waiting` 仅由 assistant `message_start`/`message_update`、`agent_settled`/`agent_end`、`prompt` 失败、Esc 中止清零
  - 顺带补齐设计缺口：等待行上方模型名（as-name）、「正在思考」后 1.4s/4 步循环省略号
  - 验证：`cargo build` 后发一条消息，确认 spark 旋转 + shimmer 一直转，直到 agent 首个 token 出现才消失
- ✅ **中文输入第二个字就崩**（`editor_input.rs:86` `assertion failed: self.is_char_boundary(n)`，退出码 0xc0000409）
  - 根因：IME 组合路径把 **char 下标**当 **字节下标**用 —— `utf16_to_char_offset()` 返回 `chars().enumerate()` 的下标，喂给 `String::replace_range`；ASCII 下 char==byte 所以不暴露，中文一个字 3 字节，第二个组合更新时越界到非字符边界 → panic 带走进程
  - 修法：改 `utf16_to_byte_offset()`（`char_indices` 累计 `len_utf16`）；新增 `safe_replace_range()` 夹取 + 对齐字符边界（平台 range 可能越界/反转）；`text_for_range` 用 `String::from_utf16_lossy`（原来逐 u16 `char::from_u32` 会把代理对拆成 U+FFFD）；两个写入方法补 `cx.notify()`
  - 遗留：chat composer 是全项目唯一手写的输入框（其余输入框都走 `ui::TextInput` = gpui-component `InputState`，自带 Windows IME）。根治方案 = composer 也换 `TextInput`；手写版只有「光标在末尾 + 退格」能力，无选区/方向键/复制粘贴
  - 验证：中文输入法连续输入（含中英混输）不再崩；拼音组合串与候选上屏正常
- ✅ 等待动画下方的 shimmer 骨架条 → **已按用户要求删除**（不再实现）
  - 该条是设计 §13 的 `.shimmer`（260×10 圆角 + 渐变扫过），本项目判定为视觉噪音：已有「正在思考…」文案，骨架条不承载信息
  - 同步删除：`session/mod.rs` 的 `shimmer_bar()` 函数 + phase row 的 child；设计 HTML 的 `.shimmer` CSS / `@keyframes slide` / `<div class="shimmer">`；设计说明 §13 已标注作废
  - 保留：模型名 + 旋转 spark +「正在思考…」（省略号循环）
