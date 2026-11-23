# Markdown 源码视图所见即所得

> **需求定位**：所见即所得 = **markdown 源码视图的感官**——
> 用户看到的是"带感官的源码"：非光标行的语法字符（`**`、`#`、反引号…）
> 隐藏、正文按渲染规格着色；光标触碰哪段，哪段还原源码可直接编辑。
>
> **两条铁律**：
>
> 1. **preview 零改动**：md 预览渲染页（`MarkdownKind::render_body` 的
>    preview 分支，`markdown/doc.rs render_preview_body` + `doc_blocks`
>    只读虚拟化）是既有 preview 代码，**一行不改**。
> 2. **默认态**：md 默认打开 = 源码态（`FileTab.md_source` 默认 true），
>    eye 一键切 preview 渲染页。WYSIWYG 装饰另受界面设置
>    `chat.md_live_preview` 控制（默认关，见 §2）——开关关 =
>    普通源码编辑器（章节折叠 §8 仍可用）。

## 1. 目标与非目标

**目标**

- 设置开启 Live Preview（`chat.md_live_preview`）且源码态时，CodeEditor
  视图**自带所见即所得感官**：
  - 非光标行：`**粗体**`、`# `、`` ` ``、`*斜体*`、`[文字](url)` 的语法
    字符隐藏（零宽折叠，不留空隙），文本按渲染器同款规格着色/字重；
    标题行字重 + 行级字号上浮（`Decorations.line_scale`）；围栏标记行
    整段折叠
  - 光标/选区触碰的语法容器 reveal（容器级：整对标记同进退），直接编辑
  - 复制/撤销/持久化 = 源码原文，天然正确
- 源码视图的既有能力不丢：行号、tree-sitter 高亮（折叠让位、reveal 行外
  由 md 样式接管）、Ctrl+F 搜索替换、软换行（恒开，与折叠自由组合）、
  章节折叠（stock 路径，不依赖本开关，见 §8）

**非目标**

- **不动 preview**（铁律 1）
- 不动默认态机制（铁律 2：默认源码态，eye 二态切换不变；点预览正文 =
  回源码态，Obsidian 同型）
- 表格 WYSIWYG 编辑、frontmatter、嵌入笔记、阅读模式
- 不新增第三种视图态——eye 仍是二态：preview 渲染页 ↔ 源码视图
- 块级 widget（图片/表格/公式源码态真渲染）：P3 预留（`widget.rs` 空壳；
  025 §3.2 源码态图片可视依赖它）

## 2. 现状与禁改区

| 现状 | 位置 | 024 态度 |
|---|---|---|
| preview 渲染页（`doc_blocks` 只读 + ListState 块级虚拟化） | `editor/markdown/doc.rs render_preview_body`（`MarkdownKind::render_body` preview 分支） | **禁改区** |
| eye 双态切换（`FileTab.md_source`，默认 true = 默认源码态） | `view/bar.rs file_nav_bar` + `shell/types.rs` | **双态机制禁改**（默认值按铁律 2） |
| Live Preview 开关 `chat.md_live_preview`（界面页，**默认关**） | `services/workspace/app_settings.rs`（pf-context hub Chat 段） | 开关语义禁改：关 = stock 编辑器 |
| 装饰挂载（`InputState::set_decorations`，幂等对账） | `MarkdownKind::render_body` → `markdown::doc::sync_live_decorations`（开 = `md_source && md_live_preview()`） | **唯一接线点** |
| 源码态 CodeEditor（行号/ts/搜索，`.code_editor()` 创建） | `view.rs ensure_file_editor` + `render_editor_surface` | 底座（只挂装饰，不改结构） |
| 渲染器样式规格（strong=700+88% accent 混色…） | `editor/markdown/runs.rs highlight()` | 引用不复制 |
| pulldown-cmark 0.13 | `crates/app/Cargo.toml` | 源码态装饰的解析器（与渲染器同 Options） |

**装饰数据契约（缝 1 坐标约定）**：`Decorations` 里 `styles` = **绝对
vis 坐标**（从 doc 0 起，滚动+折叠叠加不错位）、`folds`/`section_folds`
= doc 字节区间、`display` = 折叠后文本（行结构须与 fold 链一致）；样式
分区必须产出**按序、全覆盖、互不重叠**的 run 序列（`wysiwyg/style.rs`）。
provider 是每帧纯函数：不落缓存状态、不反写 rope。

preview 分支根本不渲染这个编辑器，装饰挂上与否对 preview 天然零影响。

## 3. 架构

```
rope（InputState.text，文档本体，唯一真相）
  │ 每帧 prepaint，O(可见行)，纯函数现算（不落缓存状态、不反写 rope）
  ▼
pulldown-cmark（into_offset_iter，与渲染器同 Options）
  ▼
容器模型（parse.rs）：行内 span + 语法标记 folds + 行级字号 + 标题表
  （doc 字节坐标，逐字节验证才折叠，保守容错；fold 不含换行）
  ▼
vendor 变换链（fold→tab→wrap）：deco 折叠 = 空占位符 Transform，
  与软换行自由组合（无互斥）
  ├→ 渲染：折叠行按折叠文本 shaping（隐藏 = 零宽）；md 样式 run 替代
  │   ts run（绝对 vis 坐标全覆盖分区 + 占位符灰显）
  ├→ 光标/移动/点击：offset↔display 换算收口 offset_to_display_point；
  │   左右键遇折叠段原子跳（next_atomic）
  └→ 章节（§8）：provider 声明表 + vendor fold 快照回读
```

- **铁律**：文档 = rope 源码；装饰只改视图，从不改写文档——复制/undo/
  IME/持久化零处理天然正确
- reveal = 每帧纯函数（光标/选区触碰的容器 folds 退出折叠集），selection
  变了自然生效；粒度见 §6.2
- 折行恒开（kind.rs `EditorConfig`）：装饰折叠进变换链后与软换行自由
  组合，挂/摘装饰只切换 provider（收口在 `sync_live_decorations` 一处），
  stock 路径不为 WYSIWYG 让步

### vendor 缝（薄补丁，升级重放）

| 缝 | 位置 | 现状 |
|---|---|---|
| 装饰类型/映射核心 | `input/decorations.rs`（新文件） | `DecorationProvider` trait + `Decorations`（folds/section_folds/section_decl/line_scale/display/styles）+ doc↔vis 映射单点（trait 必须定义在 vendor，app 实现） |
| deco 折叠链化 | `input/fold_map.rs` | provider 折叠集 = 空占位符 Transform 进变换链，prepaint 每帧对账（不一致即重建，摘装饰清残留） |
| 渲染/命中 | `input/element.rs` | runs 直查 + 折叠占位符灰显 + gutter 箭头/hitbox + 搜索高亮 `quad_span` |
| 光标/移动 | `element.rs` + `movement.rs` | 显示换算收口 `offset_to_display_point`；`next_atomic` 原子跳（doc 空间） |
| 挂载 | `state.rs` | `decorations` 字段 + `set_decorations` + 章节点击 toggle |
| 软换行链层 | `input/wrap_map.rs` | WrapMap（zed 移植）——折叠+折行组合的载体 |

升级重放清单统一见 `docs/agents/gpui.md`「vendor 本地改动」。

## 4. 模块划分

```
crates/app/src/editor/
├── view.rs / kind.rs         # 接线点（MarkdownKind::render_body → sync_live_decorations）
└── markdown/
    ├── mod.rs / runs.rs      # 渲染器样式规格（highlight()，preview 与装饰共用）
    ├── doc.rs                # render_preview_body（preview 禁改区）+ sync_live_decorations
    └── wysiwyg/              # 源码视图装饰（024 主体）
        ├── mod.rs     MdLiveProvider（DecorationProvider 实现，每帧纯函数）
        ├── parse.rs   pulldown → span/容器/行级字号/标题表（容错：存疑不折）
        ├── style.rs   样式 → HighlightStyle 分区器（可见区全覆盖）
        ├── fold.rs    折叠合并/reveal/映射入口 + 章节区间（主力测试区）
        ├── widget.rs  P3 预留空壳（块级 widget）
        └── parse/     parse 单元测试
```

## 5. 实施现状

装饰体系主体已落地，细节以 §1/§3/§6 为准。两条不变量仍由测试钉住：
preview 渲染页分支零改动（双态快照比对零 diff）；`FileTab.md_source`
默认 true（源码态）。未落地：块级 widget（P3 空壳）、parse 的删除线
`~~`/公式 `$…$` 覆盖（样式枚举已在，装饰 parse 不产出）、大文件窗口化
parse（现每帧全文解析，典型笔记 <1ms）。

## 6. 测试

- **单元**（`wysiwyg/fold.rs` 为主）：doc↔vis 映射往返/单调性质测试、
  章节区间 fixtures、parse fixtures（嵌套/未闭合/转义/中英混排/跨行
  边界）、样式分区器全覆盖不变式
- **UI（pif-ui）**：双态快照——preview 态与基线比对（零 diff）；源码态
  折叠行无 `**`、光标容器有 `**`、复制往返 = 原文

### 6.2 reveal 粒度

容器级 reveal：光标/选区与语法容器内容**相交或接触** → 该容器全部定界
folds 显现（`**粗体**` 整对标记同进退，不会左显右不显；空选区落在段内/
两端停靠位同样触发；选区跨折叠段强制可见原文）。章节折叠不参与 reveal，
只随 gutter 箭头/占位符点击 toggle。落点 `wysiwyg/fold.rs
reveal_containers`（`wysiwyg/mod.rs` 消费），由 fold.rs 性质测试与
pif-ui 按键断言钉住。

### 6.4 容错策略

语法标记只在**逐字节验证相邻**后才折叠——范围存疑 → 不折叠（保守露
源码），绝不猜；任何 fold 都不得含 `\n`（折叠只在行内，行数必须与原文
严格一致，否则行模型错乱）。落点 `wysiwyg/parse.rs`，由 parse fixtures
钉住。已知保守缺口（样式在、标记露源码，不崩溃不改文档）：强调内嵌另
一种定界符（`**_a_**`）时外层定界符可能漏折；引用块内的围栏/标题不做
感知。

## 7. 风险

| 风险 | 应对 |
|---|---|
| 误伤 preview | 主体分发收口 `MarkdownKind::render_body`；preview 分支禁改，双态快照比对钉住 |
| 折叠与软换行结构纠缠 | 已解：deco 折叠进 vendor 变换链自由组合；勿再写分支换算（`docs/agents/gpui.md`） |
| 光标错位 | 映射收口 `Decorations::map_doc_to_vis_with`/`map_vis_to_doc_with` 单点 + fold.rs 性质测试 + pif-ui 按键断言 |
| vendor 补丁随升级丢失 | 补丁面集中 `input/{decorations,fold_map,element,movement,state,wrap_map}.rs`，重放清单见 `docs/agents/gpui.md` |

## 8. 章节折叠

**需求边界**：md 标题小节可折叠，**仅此一项**（无 sticky headers、无
右键菜单项、无键位）；级别 **1..=5**，`######` 起不再折叠（样式/字号/
prefix 隐藏不受影响，parse `scan_headings` 仍认 1..=6）。

**主战场 = 正常编辑模式（stock 源码态）**——不依赖 Live Preview 开关，
vendor 折叠层 markdown crease 规则：

```
vendor fold_map.rs md_section_scan ─ fence 感知章节扫描（ATX `#{1,6}`+
                                     缩进≤3、```/~~~ 同字符闭合；###### 进表
                                     闭合上级但自身不折）；crease = 标题行尾 →
                                     章节最后非空行行尾（不含换行——占位符后
                                     保留行断，对齐 zed crease）；纯空行段不产
InputState.md_sections 缓存       ─ 键 = fold 快照版本，每帧刷新（编辑后
                                     O(doc) 重扫一次，命中 O(1)）
fold_map_fold_at markdown 分支    ─ 按缓存 crease 折叠（FoldPlaceholder::
                                     default = "⋯"，用户态语义——非 deco，
                                     摘装饰/对账不清）；「仅此一项」：md 不走
                                     缩进启发式
element.rs 箭头/hitbox、state.rs 点击 ─ stock md foldable 走章节表
```

**wysiwyg 态（Live Preview 开启时）**：provider 声明章节表（`wysiwyg/
fold.rs section_ranges`，与 vendor 扫描同规）——契约小版本：`SectionFold`、
`Decorations::section_folds/section_decl`、`decorate` 回读 `folded_sections`、
统一映射 `map_*_with`、`rebuild` 带占位文本。折叠态存 vendor fold 快照
（anchor 随编辑平移；provider 保持纯函数）；被生效章节吞掉的行内 fold
不产出（链上已并入章节段占位符）。章节占位符灰显（`text_muted`）。
