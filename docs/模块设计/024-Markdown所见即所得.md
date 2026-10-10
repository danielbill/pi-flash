# Markdown 所见即所得（Live Preview）

给 md 文件编辑做 Obsidian Live Preview 效果：**一个可编辑视图里，正文即渲染、
语法被隐藏、光标进入哪段哪段语法显现**。不再是"预览态 ↔ 源码态"两块切换。

模块代码：`crates/app/src/editor/markdown/wysiwyg/`（层级定案：editor →
markdown → wysiwyg——WYSIWYG 挂渲染器之下，渲染器整体归 editor 域）+
`vendor/gpui-component/src/input/`
（补丁点，升级需重放，同 IME/剪贴板/screenshot 补丁惯例）。

调研依据：Obsidian Live Preview 实现原理调研（本次会话，2026）——核心结论：
**Markdown 源码永远是文档本体，所见即所得纯粹是视图层装饰变换，从不改写文档**。
CM6 的 lezer 语法树 + Decoration/Widget 架构，对应到我们 = pulldown-cmark +
InputState 补丁（样式 run + 折叠 range + 光标映射）。

## 1. 目标与非目标

**目标（P1 即达成观感主体）**

- md 打开默认进 Live Preview：标题/粗体/斜体/行内代码/链接/列表/引用**所见即所得**，
  `**`、`#`、`` ` ``、`[]()` 等语法字符隐藏（零宽折叠，不留空隙）
- 光标所在行（P2 细化为"光标所在语法元素"）语法重新显现，可直接编辑源码
- eye 按钮保留，切纯源码态（CodeEditor 原样，tree-sitter 高亮）
- 复制/撤销/持久化全部 = 源码，天然正确（文档本体没变过）

**非目标（明确砍掉）**

- 表格所见即所得编辑（Obsidian 也只是接近此体验；我们表格整块折叠为源码编辑）
- 块级 widget 完整复刻（图片/mermaid/公式在编辑区内嵌渲染 → P3，可降级为
  "光标不在该块时折叠占位、进入时展开源码"）
- 阅读模式（纯只读预览态）——现有预览渲染直接复用，不另做态

## 2. 现状与集成点

| 现有 | 位置 | 本模块角色 |
|---|---|---|
| 缓冲区真值（dirty 时预览渲染 buffer 非磁盘） | `editor/view.rs file_editor_body` | **架构铁律已成立**：文档 = rope 源码，装饰只改视图 |
| 双态切换（eye：`doc_blocks` 预览 ↔ CodeEditor 源码） | `editor/view.rs file_editor_body` | 改造对象：`!md_source` 分支由"只读预览"改为 Live Preview |
| Markdown 渲染器（样式规格：字号/字重/混色/行高） | `editor/markdown/mod.rs`（含 `render/` 子模块） | 样式规格唯一来源，`style.rs` 引用而非复制 |
| pulldown-cmark 0.13 | `crates/app/Cargo.toml` | 增量解析 → 行模型（等价 lezer 角色） |
| CodeEditor 底座：rope/光标/undo/IME/搜索 | `vendor/gpui-component/src/input/` | **全部保留**，本模块只加装饰层，不碰编辑内核语义 |

### vendor 补丁点（三个缝，均已实测存在）

| 缝 | 位置 | 现状 | 本模块改法 |
|---|---|---|---|
| 样式 run | `element.rs highlight_lines()` → `Vec<(Range<usize>, HighlightStyle)>` | tree-sitter + diagnostics 合并 | md 模式：改由 `wysiwyg` provider 产出样式 run（关 ts highlighter） |
| 行 shaping | `element.rs layout_lines()` 对 `display_text` 逐行 `shape_line` | 全文按 doc 文本 shaping | md 模式：对**折叠后文本** shaping（见 §6.1） |
| 光标映射 | `element.rs layout_cursor()` 的 `pos_for(offset)` + `movement.rs` 方向键 | doc offset ↔ 屏幕点直映 | md 模式：经 `FoldSet` 换算（见 §6.3） |

关键观察：`layout_lines` 已收 `display_text: &Rope` 与 `state.text` 分离入参
（masked 输入先例），折叠文本有现成入口可探——P0 spike 首要验证项。

## 3. 总体架构

```
rope（InputState.text，文档本体，唯一真相）
  │ 每帧 prepaint，O(可见行)
  ▼
pulldown-cmark（into_offset_iter，revision 缓存）
  ▼
LineModel[] ──── 行级：heading/quote/list 样式 + 行内 span（doc offset 范围）
  ▼                    + folds（该行要隐藏的语法 range，已按 reveal 决策过滤）
FoldSet（折叠集，doc↔vis 双向映射 + atomic 跳表）
  ├→ layout_lines：折叠后文本 shaping（隐藏=零宽）
  ├→ highlight_lines：span → HighlightStyle run（合并前一缝）
  └→ layout_cursor/movement：pos_for / 方向键 / 点击反算经 FoldSet
```

**装饰是每帧从 rope 现算的纯函数**（与现有 `highlight_lines` 同生命周期），
不落缓存状态、不反向写 rope——编辑/undo/IME/搜索全部走原有路径零改动。
reveal 因此是免费的：selection 变了 paint 自然拿到新 cursor，无需额外 notify。

## 4. 模块划分

```
crates/app/src/editor/             # 编辑器模块域（024 重构：fileView 视图已
├── mod.rs                         #  自 content.rs 拆入 view.rs；file_cache/打开
├── view.rs                        #  编排状态仍在 Chat，后置收拢）
└── markdown/                      # Markdown 渲染器（聊天正文 + md 预览）
    ├── mod.rs     渲染器主体（原 markdown.rs）+ pub mod wysiwyg
    ├── render/    html / math / mermaid 富渲染组件
    └── wysiwyg/                   # Markdown 所见即所得（本设计主体）
        ├── mod.rs     装配：MdLiveProvider、md 模式开关（view.rs 调）
        ├── parse.rs   pulldown → Vec<LineModel>（容错：未闭合标记不折叠，保守露源码）
        ├── style.rs   MdStyle → HighlightStyle（规格引用同域渲染器，不复制常量）
        ├── fold.rs    FoldSet：doc↔vis 映射、atomic 跳过、reveal 决策（纯函数，主力测试区）
        └── widget.rs  P3 预留：块级折叠/占位（图/表/mermaid/公式）
```

vendor 侧（实际落地，升级需重放；每处 ~20-50 行）：

- `decorations.rs`（**新文件**）：`DecorationProvider` trait + `Decorations`
  帧结果 + doc↔vis 映射核心——**trait 必须定义在 vendor**（app 依赖
  vendor，反向不可能；原文写反已修正），app 的 `wysiwyg::MdLiveProvider`
  实现它；映射单点收口，element/movement 就地同用
- `state.rs`：`decorations` 字段 + `set/has_decorations`、`show_line_number`
  getter + `LastLayout.decorations`（帧结果，paint 后保留供鼠标反算 P2）
- `element.rs`：prepaint 调 provider（优先于 ts）、display 折叠分支、
  `layout_lines` 折叠单段 shaping、`layout_cursor` pos_for 经映射、
  longest_line 取折叠行
- `movement.rs`：preferred_column 取折叠列、move_vertical 结果经
  `vis_to_doc` 换回 doc（左右/词的 `next_atomic` 跳过 = P2）
- `input/mod.rs`：导出 decorations + `LineType` re-export；**`mode.rs` 未动**
  （复用 CodeEditor，md 态 provider 优先绕过 ts highlighter）

`editor/view.rs file_editor_body`：`is_md && !md_source` 分支从只读预览改为
Live Preview 的 `TextInput`（关行号、开 provider）；原 `doc_blocks` 预览路径
保留给 P3 的块占位与未来阅读态。

## 5. 核心数据结构（草案）

```rust
/// 一行的装饰模型（parse.rs 产出，doc offset 为坐标）
pub struct LineModel {
    pub line: u32,
    pub line_style: Option<LineStyle>,      // Heading{level} / Quote / List{indent, marker}
    pub spans: Vec<(Range<usize>, MdStyle)>, // 行内：strong/em/code/link...
    pub folds: Vec<Range<usize>>,            // 本行折叠（隐藏）range，已按 reveal 过滤
}

/// 折叠集：全文 folds 排序去重；所有光标/映射行为的唯一换算器
pub struct FoldSet { folds: Vec<Range<usize>> /* doc offsets */ }
impl FoldSet {
    pub fn doc_to_vis(&self, off: usize) -> usize;   // 光标绘制、wrap 计算
    pub fn vis_to_doc(&self, vis: usize) -> usize;   // 点击反算、双击选词
    pub fn next_atomic(&self, off: usize, dir: i8) -> usize; // 方向键跳过隐藏段
    pub fn reveal_at(&self, cursor: Range<usize>) -> Self;   // 光标进入的元素退出 folds
}

/// vendor 缝上的契约（element.rs prepaint 时调用）
pub trait DecorationProvider {
    fn decorate(&self, text: &Rope, visible: Range<usize>,
                cursor: Range<usize>) -> Option<Decorations>; // None = 走原 ts 路径
}
pub struct Decorations {
    pub display: Option<Rope>,          // 折叠后可见区文本；None = 不折叠原样
    pub styles: Vec<(Range<usize>, HighlightStyle)>, // 折叠坐标系
    pub folds: FoldSet,                 // 供 layout_cursor/movement 换算
}
```

## 6. 关键机制

### 6.1 隐藏折叠（最难，P0 spike 对象）

- `**`、`#` 等语法字符不绘制且**零宽**：对折叠后文本 shaping，折叠 range 在
  doc↔display 之间由 `FoldSet` 换算。
- **风险点（spike 必须先杀掉的）**：`text_wrapper` 的软换行结构按 doc 文本算。
  P1 定案：**md Live Preview 关软换行**（文件编辑器默认不折行），wrap 与折叠
  正交化；软换行 + 折叠列入 P2（折叠后文本重建 wrap 表）。
- GPUI 无 contenteditable，反而是优势：折叠映射全在自己手里，没有浏览器
  selection API 和 DOM 的黑箱。

### 6.2 reveal 决策

- P1 粗粒度：光标所在**行** folds 全部 reveal（Obsidian 早期行为，逻辑一行）。
- P2 细粒度：光标所在的**行内元素**（光标 offset 落在 span 内或紧邻）reveal
  该元素 folds，其余仍藏；选区跨元素则全 reveal。
- 实现 = `FoldSet::reveal_at(cursor)`，每帧 prepaint 纯函数计算，无状态。

### 6.3 光标与点击（P2 主战场）

- 方向键/词跳动：`next_atomic` 跳过 folds，光标永不停在看不见的 `*` 中间
- 点击反算：屏幕 x → 折叠坐标系 offset → `vis_to_doc` → doc offset
- 双击选词、拖选跨隐藏段：选区始终存 doc 坐标，绘制时换算——选中高亮盖过
  折叠段时该段强制显示（与 Obsidian 一致：选到了就看得见）
- 已知边界（CM6 同款，接受不做完美）：atomic range 两端点仍是合法停靠位

### 6.4 编辑语义决策表

| 场景 | 行为 | 期 |
|---|---|---|
| 在折叠边界 Backspace | P1：整段 fold run 直接删除；P2：先 reveal 再删（Obsidian 行为） | P1/P2 |
| 列表行回车 | 续行符自动补 `- ` / `1. `（markdown 输入习惯） | P2 |
| 复制/剪切 | 复制 doc 文本 = 源码，**零处理** | 天然 |
| undo/redo | rope 原生，**零处理** | 天然 |
| 语法未闭合（`**abc`） | 不折叠不加样式，保守显示源码 | P1 |
| 代码块/fenced | 围栏 ``` 折叠隐藏，块内保持源码着色（不做内嵌渲染） | P1 |
| 图片/表格/公式块 | 光标不在 → 折叠占位；进入 → 展开源码 | P3 |

### 6.5 与 tree-sitter 高亮的关系

md 模式下关 ts highlighter（源码态 eye 切换时恢复），行内样式全部来自
`style.rs`；两者不叠加，避免 run 合并的坐标系纠缠。

## 7. 分期与验收

| 期 | 内容 | 验收 |
|---|---|---|
| **P0 spike**（先杀风险） | 三缝打样：折叠文本过 `layout_lines`、`pos_for` 经 FoldSet 换算、光标行 reveal 一个元素 | ✅ **已完成**（2026-10-10）：pif-ui 实测 `**粗体**` 隐藏、光标行显现、光标对齐、编辑/undo 往返、eye 双态切换全通；新增单测 28 个 |
| **P1 效果主体** | parse/style/fold 全量：标题/粗体/斜体/行内码/链接/列表/引用/围栏折叠；行级 reveal；关行号；eye 双态保留 | pif-ui 快照：渲染态断言无 `**`；光标行断言有 `**`；复制粘贴往返 = 原文 |
| **P2 光标打磨** | atomic 跳词、点击反算、选区强制 reveal、元素级 reveal、Backspace 边界、列表续行、软换行 | pif-ui 合成按键序列断言光标 offset 序列；fold 映射 property test |
| **P3 块 widget** | 图/表/公式折叠占位 + 进入展开（`widget.rs`，复用 `doc_blocks` 渲染） | 手测 + 快照 |

不设全量 Obsidian parity 里程碑——表格编辑、frontmatter、嵌入笔记明确砍掉
（§1 非目标），后续按需单独立 bead。

## 8. 性能预算

- decorate 每帧 O(可见行)：pulldown 全文解析典型笔记 <1ms，先全文解析 +
  revision 缓存（edit 时失效）；超大文件（>1MB）降级为可见窗口解析
- 折叠/映射是纯 Vec 扫描，无分配热点
- 预算内不变项：非 md 文件、源码态 = provider 为 None，零开销走原路径

## 9. 测试

- **单元（editor/markdown/wysiwyg/fold.rs 为主，纯函数）**：`doc_to_vis∘vis_to_doc` 往返、
  单调性 property test；parse fixtures（嵌套/未闭合/转义/中英混排/长行）
- **UI（pif-ui 自动化链路）**：三态快照（live preview / 光标行 / 源码）+
  合成按键后光标位置断言；复制往返断言
- **vendor 补丁**：升级 gpui-component 时按 IME/剪贴板惯例重放，补丁面
  §2 三缝 + movement，越薄越安全

## 10. 风险与开放问题

| 风险 | 应对 |
|---|---|
| 折叠文本与 `text_wrapper` wrap 结构纠缠 | P1 关软换行，正交化；P2 再解 |
| `pos_for` 换算漏一处 = 光标错位（最难测） | 映射收口在 FoldSet 单点；property test + pif-ui 按键序列断言 |
| pulldown offset 对分隔符范围不精确（`**a**b` 类） | parse.rs 保守策略：范围存疑 → 不折叠；fixtures 兜底 |
| vendor 补丁随 gpui-component 升级丢失 | 补丁面薄 + 三缝集中在 element/state/movement；升级 checklist 追加 |
| IME 组合输入跨折叠段 | 组合期间强制 reveal 光标处 folds（P2）；spike 观察 |

**开放问题（原 P0 待定，已有结论）**：revision 缓存键——**P0/P1 不做缓存**，
provider 每帧全文解析现算（纯函数、无状态，reveal 因此免费）；典型笔记
<1ms 达标，若实测热再按可见区文本 hash 或 edit 计数加（§8 预案不变）。

**P0 实测发现的已知缺口**（不阻塞，P1/P2 处理）：

| 缺口 | 影响 | 期 |
|---|---|---|
| 强调内嵌异种定界符（`**_a_**`）外层可能漏折 | 标记露源码（样式仍在），不崩溃不改文档 | P1 |
| reveal 行样式保留（`**粗体文字**` 显源码但文字仍加粗） | 与 Obsidian 行级 reveal 略异 | P1 调 |
| 选区/搜索高亮 quad 未经折叠映射 | 折叠行上选区盒偏移 | P2 |
| 左右键 `next_atomic` 已实现未接线 | 光标可停在隐藏段（行级 reveal 下仍可见，无损） | P2 |
| 引用块内围栏/标题不感知；setext 标题不折 | 保守漏折 | P1 |
| provider 每帧 `to_string()` 全文拷贝 | 大文件（>1MB）开销 | P1 窗口化 |
