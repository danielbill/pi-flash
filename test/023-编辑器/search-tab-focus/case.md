# 023 fv-search Tab 焦点对齐 zed：查询框→替换框→编辑器 环转（bead pi-flash-8rzn）

## 背景

用户报障（2026-11-20）：搜索替换条填完查询词后按 Tab，预期落到替换栏继续填
写，实际焦点落到编辑区。

上游 gpui-component 的 `on_action_tab` 是**故意**一步甩编辑器（Tab=离开面板
回正文），与用户肌肉记忆相悖。zed `crates/search/src/buffer_search.rs` 的已
验证语义是字段间环转：

- `tab`/`backtab` → `cycle_field(Next/Prev)`（L1661-1690）：焦点环 =
  查询框 → 替换框（replace 开时）→ 可搜索项（编辑器本体），取模轮转 +
  `cx.stop_propagation()`；
- 截获方式 = **`capture_action`**（L462-463）：面板在捕获阶段（祖先先于焦
  点元素）截获编辑器既有动作——zed 是 `editor::Tab`/`editor::Backtab`，PF
  对应物 = InputState CONTEXT 现成绑定 `tab→IndentInline`、
  `shift-tab→OutdentInline`（input/state.rs L145/146）。焦点在面板内时编
  辑器收不到（不误触 indent/outdent）；焦点落编辑器后面板退出焦点链，两键
  恢复原语义——**无需新增键位绑定或 action**；
- 另对齐 `toggle_replace`（L1749-1761）：ctrl-h 展开替换行时焦点**自动落
  替换框**（收起回查询框）——zed 用户靠这一步续填，不必 Tab。

## 改动（vendor/gpui-component/src/input/search.rs，升级必须重套）

1. `on_action_tab` 改 `cycle_field(true)` + 新增 `on_action_backtab`
   → `cycle_field(false)`；`cycle_field` 逐行对齐 zed L1668-1690；
2. render 面板 `.on_action(on_action_tab)` → **`.capture_action` 两行**
   （tab + backtab），zed L462-463 同款；
3. `show(replace=true)` 焦点落替换框（zed toggle_replace 语义）。

## pif-ui 验证实录（2026-11-20，inst 口径见 run.sh）

| 步骤 | 断言 | 结果 |
|---|---|---|
| ctrl-h 展开替换行 | 直接打字 `mark` 落**替换框**、查询框空 | ✓ |
| 点查询框打 `origin` → **Tab** → 打 `x` | 替换框 `markx`、查询框 `origin` 不动 | ✓ |
| **shift-tab** → 打 `q` | 查询框收到 `q`、替换框 `markx` 不动 | ✓ |
| 第三环（替换框→Tab→编辑器） | 代码同构（同一 cycle_field），未单测 | — |

vendor 102 / app 529 全绿。

## 人工验收

1. ctrl-f 打查询词 → 点替换模式钮（或 ctrl-h）→ Tab 应落替换框；
2. 替换框 Tab 落编辑器（可缩进），shift-tab 回替换框；
3. 编辑器内 Tab/shift-tab 缩进/反缩进不受影响（面板关闭态）。
