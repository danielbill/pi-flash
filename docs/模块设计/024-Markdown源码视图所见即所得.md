# Markdown 源码视图所见即所得

> **需求定位（拍板，勿再偏）**：所见即所得 = **markdown 源码视图的感官**——
> 用户看到的是"带感官的源码"：光标不在的行，语法字符（`**`、`#`、反引号…）
> 隐藏、正文按渲染规格着色；光标进入哪行，哪行还原源码可直接编辑。
>
> **两条铁律（前车之鉴，违反即错）**：
>
> 1. **preview 零改动**：`editor/view.rs file_editor_body` 的 `is_md && !md_source`
>    分支（`doc_blocks` 只读渲染页）是既有 preview 代码，**一行不改**。
>    024 曾把该分支替换为可编辑视图、架空 preview——已全部撤回
>    （git `5a7a7fe` 实现 → `30a4a4d` 撤回），不许重演。
> 2. **默认态**：初版拍板"不变（进 preview）"；**2026-10-10 用户二改拍板：
>    md 默认打开 = 源码态**（WYSIWYG 感官完整后便于日常测试），eye 一键
>    切 preview 渲染页——两者均完好，非事故勿撤回。

## 1. 目标与非目标

**目标**

- eye 切到源码态（`md_source=true`）时，CodeEditor 视图**自带所见即所得感官**：
  - 非光标行：`**粗体**`、`# `、`` ` ``、`*斜体*`、`[文字](url)` 的语法字符隐藏
    （零宽折叠，不留空隙），文本按渲染器同款规格着色/字重
  - 光标行：整行还原源码（reveal），直接编辑
  - 复制/撤销/持久化 = 源码原文，天然正确
- 源码视图的既有能力不丢：行号、tree-sitter 高亮（折叠让位、reveal 行外
  由 md 样式接管）、Ctrl+F 搜索替换、行尾/软换行行为

**非目标**

- **不动 preview**（铁律 1）
- 不动默认态机制（铁律 2：现拍板默认源码态，eye 双态切换不变）
- 表格 WYSIWYG 编辑、frontmatter、嵌入笔记、阅读模式（原 024 砍掉项沿用）
- 不新增第三种视图态——eye 仍是二态：preview 渲染页 ↔ 源码视图

## 2. 现状与禁改区

| 现状 | 位置 | 024 态度 |
|---|---|---|
| preview 渲染页（`doc_blocks` 只读 + ListState 虚拟化） | `editor/view.rs file_editor_body` 的 `is_md && !md_source` 分支 | **禁改区** |
| eye 双态切换（`ft.md_source`） | `file_nav_bar` + `FileTab.md_source`（默认 true，024 二改拍板：默认源码态） | **双态机制禁改**（默认值按铁律 2） |
| 源码态 CodeEditor（行号/ts/搜索，`.code_editor()` 创建） | `ensure_file_editor` + `file_editor_body` 通用分支 | **改造对象**（只挂装饰，不改结构） |
| 渲染器样式规格（strong=700+88% accent 混色…） | `editor/markdown/mod.rs highlight()` | 引用不复制 |
| pulldown-cmark 0.13 | `crates/app/Cargo.toml` | 源码态装饰的解析器 |

**唯一接线点**：`ensure_file_editor` 创建 md 编辑器时挂装饰 provider
（`InputState::set_decorations`）；provider 在源码视图渲染帧生效——
preview 分支根本不渲染这个编辑器，天然零影响。

## 3. 架构（沿用已实测验证的部分）

原 024 的技术底座经 P0 spike 实现并全链路实测（折叠/reveal/光标映射/undo
往返，`cargo test` 452 绿、pif-ui 截图验证），**架构本身没有错，错在接线**：

```
rope（InputState.text，文档本体，唯一真相）
  │ 每帧 prepaint，O(可见行)，纯函数现算（不落缓存状态、不反写 rope）
  ▼
pulldown-cmark（into_offset_iter）
  ▼
行内 span + 语法标记 folds（doc 字节坐标，逐字节验证才折叠，保守容错）
  ▼
FoldSet（折叠集：doc↔vis 双向映射 + atomic 跳表）—— 映射单点收口
  ├→ layout_lines：折叠后文本 shaping（隐藏 = 零宽）
  ├→ highlight_lines：md 样式 run 替代 ts run（可见区全覆盖分区）
  └→ layout_cursor / movement：光标 x、上下键列经 FoldSet 换算
```

- **铁律**：文档 = rope 源码；装饰只改视图，从不改写文档——复制/undo/IME/
  持久化零处理天然正确
- reveal = 每帧纯函数（光标行 folds 退出折叠集），selection 变了自然生效
- 源码态关软换行（wrap 与折叠正交，避免 text_wrapper 结构纠缠）

### vendor 缝（薄补丁，升级重放；实测过的实现见 git `5a7a7fe`）

| 缝 | 位置 | 改法 |
|---|---|---|
| 装饰类型/映射核心 | `input/decorations.rs`（新文件） | `DecorationProvider` trait + `Decorations` 帧结果 + doc↔vis 映射（trait 必须定义在 vendor，app 实现） |
| 样式/显示文本 | `element.rs prepaint` | provider 优先于 ts；display 用折叠文本 |
| 行 shaping | `element.rs layout_lines` | 折叠行按折叠长度单段 shaping |
| 光标/上下键 | `element.rs layout_cursor`、`movement.rs` | doc↔vis 换算（防错位） |
| 挂载 | `state.rs` | `decorations` 字段 + `set_decorations` |

## 4. 模块划分

```
crates/app/src/editor/
├── mod.rs / view.rs          # view 零改动（preview 禁改区在内）
└── markdown/
    ├── mod.rs / render/      # 渲染器（preview 与聊天正文用，零改动）
    └── wysiwyg/              # 源码视图装饰（024 主体）
        ├── mod.rs     MdLiveProvider（DecorationProvider 实现，每帧纯函数）
        ├── parse.rs   pulldown → 行内 span + 标记 folds（容错：存疑不折）
        ├── style.rs   样式 → HighlightStyle 分区器（可见区全覆盖）
        ├── fold.rs    折叠集操作 + 映射委托（主力测试区）
        └── widget.rs  P3 预留
```

## 5. 分期与验收

| 期 | 内容 | 验收 |
|---|---|---|
| **P0**（已验证，可从 git `5a7a7fe` 取回） | 三缝打样：折叠文本过 `layout_lines`、`pos_for` 经 FoldSet、光标行 reveal | ✅ 实测通过（P0 spike 全链路 + 28 单测） |
| **P1 接线纠偏**（✅ 2026-10-10 完成） | 按本文 §2 接线：装饰挂 `ensure_file_editor`（`sync_md_live_state`：`md_source=true` 挂 provider）；**preview 分支零改动**；行号保留；md 关软换行。P1 扩容：标题行级字号上浮（`Decorations.line_scale`，行高 uniform 内 cap 1.7）+ 字重；链接 `[text](url)` 折叠+着色；围栏标记行折叠 | ✅ 三条红线全绿：`git diff` 不含 preview 分支；`md_source` 默认 false（当时；后用户二改拍板默认源码态）；preview 往返快照**逐字节一致**；源码态标题/链接/折叠/样式 pif-ui README 截图验证 |
| **P2 光标与交互打磨** | 1. **元素级 reveal**：光标落在哪段哪段显现（现为光标行整行还原，Obsidian 为段级）
2. **atomic 跳词接线**：left/right 遇折叠段原子跳过（`next_atomic` 已实现未接线，现会走进隐藏字符）
3. **点击反算**：鼠标点折叠区 → doc 偏移（`vis_to_doc` 已备）
4. **选区强制 reveal**：选区跨折叠段时显示原文
5. **列表/引用样式**：`- ` 标记着色缩进、`> ` 引用竖条
6. **行级行高缝**：标题真正独立行高（h1 两倍行高）——需突破 vendor uniform line_height 假设（scroll/cursor/selection 全链路），大工程高价值
7. parse 覆盖：删除线 `~~`、公式 `$…$` | pif-ui 合成按键断言光标 offset 序列；fold 性质测试 |
| **P3 块级 widget** | 1. widget 机制：源码态块渲染（可变行高内容）
2. **图片行**：`![]()` 显示真图（025 源码态可视等此项）
3. **表格块**：源码态渲染对齐表格
4. 代码块灰底圆角块样式；5. 公式块 `$$…$$` | 手测 + 快照 |
| **P2+ 性能** | 大文件窗口化 parse（>1MB 只解析可见区±余量）；edit 计数器缓存（P0 拍板无缓存，实测慢再上） | 大文件滚动帧耗基准 |

**P1 验收红线（防复发）**：
1. `git show --stat` 不得包含 `file_editor_body` preview 分支的删除/替换
2. `FileTab.md_source` 默认值必须为 `false`
3. 打开 md 的首帧快照与 023 基线一致

## 6. 测试

- **单元**（`wysiwyg/fold.rs` 为主）：映射往返/单调性质测试、parse fixtures
  （嵌套/未闭合/转义/中英混排/跨行边界）、分区器全覆盖不变式
  （P0 已沉淀 28 个，随 `5a7a7fe` 可整体取回）
- **UI（pif-ui）**：双态快照——preview 态与 023 基线比对（零 diff）；
  源码态折叠行无 `**`、光标行有 `**`、复制往返 = 原文

## 7. 风险

| 风险 | 应对 |
|---|---|
| 再次误伤 preview | §5 三条验收红线；改动评审先看 diff 范围 |
| 折叠与 wrap 结构纠缠 | 源码态关软换行（正交化） |
| `pos_for` 漏一处 = 光标错位 | 映射收口 FoldSet 单点 + 性质测试 + pif-ui 按键断言 |
| vendor 补丁随升级丢失 | 补丁面集中在 decorations/state/element/movement，升级 checklist 重放 |
| 每帧全文解析/拷贝开销 | 典型笔记 <1ms；>1MB 再窗口化（P2 视实测） |
