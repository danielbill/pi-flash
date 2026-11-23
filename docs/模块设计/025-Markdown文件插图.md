# Markdown 文件插图

对齐 Obsidian 插图功能。

## 1. 目标与非目标

**目标（均已成现状）**

1. **插图落盘**：md 源码态编辑器 Ctrl+V，剪贴板含图片 → 自动存 png 到
   `<md同级>/assets/<md文件名去扩展>/file-<时间戳>.png`，光标处插入
   `![](assets/<md名>/file-<时间戳>.png)`（相对 md 的 `/` 分隔相对路径；
   含空格/括号的路径用 `<>` 包裹，否则 pulldown 在空格处截断）
2. **preview 照常显示**：md 内嵌图片在 preview 渲染页正常出图（§3.3）
3. **文件树归档可见**：assets 目录/图片文件自然出现在文件树（image
   类型图标）

**非目标（现状即定案）**

- 源码态直接渲染真图（依赖 024 块级 widget，未施工，见 §3.2；源码态
  图片引用行按普通文本显示）
- 拖拽图片插入
- 图床上传 / 图片压缩 / 裁剪编辑（落盘即存，png 自压，**不设体积上限**）
- vault 级统一素材库（采用**与 md 文件同级**的 assets，各目录各自归档，
  不上浮到根）
- 文件树后缀徽标（png 就**普通文件名显示**，对齐 Zed 现状）
- 非 md 文件的插图（插图钩子只挂 md 编辑器；非 md 编辑器 Ctrl+V 走原
  文本粘贴路径，不拦截不落盘）

## 2. 现状盘点（能复用的都在）

| 能力 | 落点 |
|---|---|
| 剪贴板图片读取 | gpui `ClipboardItem::entries()` 含 `ClipboardEntry::Image`；聊天附件入口 `session/attachments.rs attach_clipboard_image`（其超限策略 MAX_IMAGE_BYTES 是聊天附件专属，落盘不适用） |
| 编辑器粘贴入口 | vendor `input/state.rs paste`：挂了 `on_image_paste` 钩子且剪贴板含图时优先图片路径（PF-025 缝，升级重放）；钩子返回 None 回落文本粘贴 |
| PNG 编码 | `image 0.25`（features png/jpeg/gif/webp/bmp），转码用 |
| preview 图片渲染 | `markdown/blocks/image.rs render_image`：相对路径按 `IMG_BASE`（预览文件父目录，`markdown/doc.rs`）经 `resolve_relative_image` 解析（md 目录直查 → `assets/` 直层 → `assets/<子目录>/`同名匹配，失败回退原路径）；http/空 url/读不到 → 🖼 alt 占位；本地图 LRU 缓存（32 条 / 64MB 字节预算，驱逐经 `services/asset_evict` 放掉 gpui 位图） |
| wiki 图片 | `markdown/doc/wiki_images.rs`：解析前把 Obsidian `![[file.png]]` 展开为 `![](file.png)`（fence 内不碰，纯 `[[链接]]` 不碰） |
| 文件树类型图标 | `services/file_icons`：png/jpg/webp 等归 `image` 图标 |
| undo | 图片引用串插入走编辑器标准 `replace_text_in_range`，自动入 undo 栈 |

## 3. 功能设计

### 3.1 插图落盘链路（独立不依赖 024）

```
源码态 Ctrl+V
  └→ vendor paste（state.rs，PF-025）：剪贴板含 ClipboardEntry::Image
     且本编辑器挂了钩子？
       ├─ 否 → 原文本粘贴路径（现状不动）
       └─ 是 → app 回调 image_paste_hook（markdown/attachments.rs）
            1. 前置：钩子仅 md 编辑器持有（kind EditorConfig.image_paste，
               仅 MarkdownKind 配 true；挂载点 ensure_file_editor）
            2. 不设体积上限（落盘本地、png 自压；聊天附件的
               MAX_IMAGE_BYTES 不适用此场景）
            3. 格式：png 直存；jpg/gif/webp/bmp 解码转存 png；转不了的
               （svg 等）返回 None = 静默回落文本粘贴
            4. 目标目录：<md同级>/assets/<md文件名去扩展>/（逐级 mkdir）
            5. 命名：file-yyyymmddHHmmssSSS.png（17 位本地时间；同名已
               存在追加 -1/-2 防撞）
            6. 先写盘（失败 None，不留悬空引用也不插入），后由 vendor
               把返回的引用串插入光标处（进 undo 栈）
```

- **路径基准**：md 文件的父目录；strip 失败退化绝对路径（仍可渲染）
- **生效范围**：仅 md 文件的源码态编辑器（preview 是只读渲染页，天然
  不涉）
- **撤销语义**：Ctrl+Z 只撤文本插入，已落盘文件保留（Obsidian 同行为，
  明示不回收）

### 3.2 源码态直接可看（未施工，依赖 024）

- 落地形态（024 块级 widget，`wysiwyg/widget.rs` P3 预留空壳）：图片
  引用行由装饰/widget 产出**行级图片元素**——非光标行渲染内嵌图（按
  行宽等比缩放），光标行还原源码文本（024 reveal 语义一致）；图片块高
  ≠ 行高 → widget 机制负责行高扩展
- 现状：源码态 `![...](...)` 按普通文本显示；看真图走 preview 态（§3.3）

### 3.3 preview 渲染（现状已覆盖）

`render_image` 已覆盖：相对路径解析（含 `assets/<md名>/` 归档结构与
wiki 展开）、失败 🖼 alt 占位、http/远程占位、本地图片缓存。中文/空格
文件名直接按路径读盘（无 URL 解码环节）；引用串含空格/括号由 `<>`
包裹保证 pulldown 解析。

### 3.4 文件树（零开发）

- assets 目录与 png 文件由现有文件树自动呈现（含 image 类型图标）
- 后缀徽标：**不加**——png 普通文件名显示（对齐 Zed 风格现状）

## 4. 模块与接线

```
crates/app/src/editor/markdown/attachments.rs   # 目标目录/命名/转码/引用串生成（纯函数为主）
```

- vendor 缝（薄补丁）：`input/state.rs paste` 剪贴板含图时调用
  `InputState.on_image_paste` 回调（None 或无图回落原文本路径）——
  PF-025 标记，与 IME/装饰同款升级可重放
- 插入串的相对路径由 app 算好交给回调（vendor 不知道 md 路径）；钩子是
  纯函数（不碰 cx/entity），插入/undo 由 vendor paste 完成

## 5. 分期与验收

P0 落盘链路与 preview 渲染均已上线（§3.1/§3.3）；源码态可视（§3.2）
未施工，等 024 块级 widget。落盘链路由 `attachments.rs` 单元测试钉住
（目录规则/17 位命名/防撞/转码/引用串；pif-ui 无剪贴板图片注入，端到
端靠手测）。

## 6. 风险与待定

| 项 | 现状 |
|---|---|
| 超限/格式行为 | 不设上限；转不了的格式静默回落文本粘贴 |
| image crate features | 已带 jpeg/gif/webp/bmp decode（png 直存） |
| 剪贴板格式 | gpui 平台给的 `Image` 格式集合即钩子输入，转不动的返回 None |
| UI 自动化无法注入剪贴板图片 | 链路以单元测试钉住；端到端手测 |
| 撤销不回收文件 | 明示行为（Obsidian 一致） |
| 源码态可视依赖 024 | 024 widget 落地前挂起（§3.1/§3.3 不受影响） |
