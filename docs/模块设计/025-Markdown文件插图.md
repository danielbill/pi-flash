# Markdown 文件插图

> **需求拍板（原文保留）**
>
> 对齐 Obsidian 插图功能
> 从剪贴板 copy 图片到 markdown 中，在源码状态下直接可看；
> 图片自动保存为 png，保存到文件同级目录 "assets"下 ；
> 每个 markdown 文件单独建一个目录保存文件，png文件用"file-时间戳.png"命名
>
> ![assets目录](image.bmp)（Obsidian 归档结构：`assets/<文章名>/file-*.png`，
> 文件树带 PNG 类型徽标）

## 1. 目标与非目标

**目标**

1. **插图落盘**：源码态编辑器 Ctrl+V，剪贴板含图片 → 自动存 png 到
   `<md同级>/assets/<md文件名去扩展>/file-<时间戳>.png`，光标处插入
   `![](assets/<md名>/file-<时间戳>.png)`（相对 md 的 `/` 分隔相对路径）
2. **源码态直接可看**：源码视图中图片引用行渲染为内嵌图片（光标行还原源码）
3. **preview 照常显示**：md 内嵌图片在 preview 渲染页正常出图（现状已支持，核验）
4. **文件树归档可见**：assets 目录/图片文件自然出现在文件树（现状已支持）

**非目标**

- 拖拽图片插入（Obsidian 有；未拍板，留待后续）
- 图床上传 / 图片压缩 / 裁剪编辑
- vault 级统一素材库（拍板 = **与 md 文件同级**的 assets，各目录各自归档，
  不上浮到根——局部性好，搬运文章时图随文走）

## 2. 现状盘点（能复用的都在）

| 能力 | 现状 | 结论 |
|---|---|---|
| 剪贴板图片读取 | gpui `ClipboardItem::entries()` 含 `ClipboardEntry::Image`；聊天输入已有 `attach_clipboard_image`（actions_panels.rs:536，含格式过滤/超限跳过） | **复用其判定与过滤策略** |
| 编辑器粘贴入口 | vendor `input/state.rs paste` 只取 `clipboard.text()`，图片被 `unwrap_or_default()` 丢弃 | **接缝点**（vendor 补丁 + app 回调） |
| PNG 编码 | `image 0.25`（features 仅 `png`）已在依赖 | 转码用；非 png 解码需补 features |
| preview 图片渲染 | `render_image`（markdown/mod.rs:1577）：相对路径按 `img_base()` 拼接、读盘渲染、失败降级 🖼 占位；`file_editor_body` 已把 md 路径绝对化为基准（view.rs:554） | **已完整**，只核验 |
| 文件树类型图标 | `services/file_icons`：png/jpg/webp 等归 `image` 图标 | 已有；截图的 "PNG" **后缀徽标**没有（可选增强） |
| undo | 文本插入走编辑器标准 `replace_text_in_range` | 自动入 undo 栈 |

## 3. 功能设计

### 3.1 插图落盘链路（P0，独立不依赖 024）

```
源码态 Ctrl+V
  └→ vendor paste 拦截：剪贴板有 ClipboardEntry::Image？
       ├─ 否 → 原文本粘贴路径（现状不动）
       └─ 是 → app 回调 insert_image_from_clipboard(path, cursor…)
            1. 过滤：svg 拒收、超限跳过（复用聊天 attach_clipboard_image 的
               MAX_IMAGE_BYTES/格式策略；超限静默回落文本粘贴还是拒绝——见 §6）
            2. 非 png → image crate 转码 png（features 缺口见 §6）
            3. 目标目录：<md同级>/assets/<md文件名去扩展>/（不存在逐级 mkdir）
            4. 命名：file-yyyymmddHHmmssSSS.png（17 位本地时间，与截图
               file-20260927113156645 同构；同秒多张追加序号防撞）
            5. 先写盘（失败 toast 不插入），后插文本：
               ![ ](assets/<md名>/file-xxx.png) → 光标处（进 undo 栈）
```

- **路径基准**：md 文件的父目录（绝对化逻辑与 preview 基准 view.rs:554 同源）
- **粘贴语境**：只在源码态编辑器聚焦时可达（preview 是只读禁改区，天然不涉）
- **撤销语义**：Ctrl+Z 只撤文本插入，已落盘文件保留（Obsidian 同行为，明示不回收）

### 3.2 源码态直接可看（P2，**强依赖 024**）

- 依赖 024《Markdown源码视图所见即所得》的源码视图装饰机制：图片引用行由
  `DecorationProvider` 产出**行级图片元素**——非光标行渲染内嵌图（按行宽
  等比缩放），光标行还原源码文本（024 reveal 语义一致）
- 高度问题：图片块高 ≠ 行高 → 024 的块级 widget（P3 `widget.rs`）负责行高
  扩展；**024 未按新设计实施前，本项不可施工**
- 可先行的降级：024 P1 样式把 `![...](...)` 着色为链接色，一眼可辨（非真图）
- 点击图片选中/跳转源码：P2 细化

### 3.3 preview 渲染核验（P1）

现状 `render_image` 已覆盖：相对路径拼接、失败占位、远程占位。核验清单：

- [ ] 中文/空格文件名的本地路径读取（无 URL 解码问题则勾销）
- [ ] `assets/**` 缺图时占位样式（失败态已有 🖼 alt）
- [ ] 大图（几 MB）加载是否阻塞渲染帧（必要时解码挪线程）

### 3.4 文件树（P0 零开发 + P3 可选）

- assets 目录与 png 文件由现有文件树自动呈现（含 image 类型图标）——零开发
- **后缀徽标**（截图右侧 "PNG" 粗体标签，Obsidian 风格）：pi-flash 现状为
  Zed 风格（不显后缀）——**待拍板**是否加；若加，落 `function_panel/file_tree.rs`
  行尾徽标列（git 徽标同列位）

## 4. 模块与接线

```
crates/app/src/editor/markdown/
├── attachments.rs   # P0 新增：目标目录/命名/转码/插入串生成（纯函数为主，可测）
```

- vendor 缝（薄补丁）：`input/state.rs paste` 剪贴板含图时调用可插拔钩子
  （`InputState.on_image_paste` 回调，None 或无图回落原文本路径）——与
  IME/装饰同款升级可重放
- 插入串的相对路径由 app 算好交给回调（vendor 不知道 md 路径）

## 5. 分期与验收

| 期 | 内容 | 验收 |
|---|---|---|
| **P0 插图落盘** | paste 图片拦截 + 目录/命名/转码 + 插入 | 单元：目录规则/17 位命名/防撞/转码/相对路径；手测 Ctrl+V（pif-ui 无剪贴板图片注入，见 §6） |
| **P1 preview 核验** | §3.3 清单 | pif-ui 预置 assets 图片 → preview 快照出图 |
| **P2 源码态可视** | §3.2（依赖 024） | 024 源码态快照：图片行内嵌、光标行还原 |
| **P3 文件树徽标** | §3.4 后缀徽标（待拍板） | 文件树快照 PNG 徽标 |

## 6. 风险与待拍板

| 项 | 说明 |
|---|---|
| **待拍板：超限/不支持格式行为** | 静默跳过（聊天现状） vs toast 提示？设计倾向静默回落文本粘贴 |
| **待拍板：文件树 PNG 后缀徽标** | 对齐 Obsidian 截图 vs 保持 Zed 风格 |
| image crate features | 仅 `png`；剪贴板非 png（jpg/gif/webp）需补对应 decode features（转码为 png 拍板） |
| 剪贴板格式实测 | gpui Windows 平台读到的 Image format 集合需实测探明（可能直接给 png） |
| UI 自动化无法注入剪贴板图片 | P0 验收 = 单元测试 + 手测；pif-ui 可留 `exec clipboard.set_image` 扩展位 |
| 撤销不回收文件 | 明示行为（Obsidian 一致），避免被当 bug 报 |
| P2 依赖 024 | 024 未按新设计实施前，源码态可视挂起（P0/P1 不受影响） |
