# 023 编辑器「替换全部」即崩：scroll_to 可见行索引 usize 下溢（bead pi-flash-2pyq）

## 背景

用户报障（2026-11-20）：`世界背景.md`（CJK 长段 md，软换行）在编辑器内置搜索
替换条做「替换全部」时 app 闪退。**0.5.2 版本不崩，当前代码崩**。

crash log 全同栈：

```
43: core::panicking::panic_const::panic_const_sub_overflow
44: gpui_component::input::state::InputState::scroll_to   (state.rs:2053)
49: gpui_component::input::search replace_all             (search.rs:442)
… window_procedure panic in a function that cannot unwind → abort
```

## 根因

`SearchPanel::replace_all`（vendor `input/search.rs`）在
`replace_text_in_range_silent(0..len, 全文)` 之后调 `state.scroll_to(0, cx)`。
此时 `last_layout` **还是上一帧的**（`visible_range` 停在旧滚动位），而
`offset_to_display_point` 走的是**新** wrap 快照——文档被滚到中部时
`visible_range.start > 0`，目标行 0 在视口上方：

```rust
last_layout.lines.get(disp_row as usize - last_layout.visible_range.start)
//                         ^^^^^^^^^ 0 - N → usize 下溢 → panic unwind 出 FFI 边界
```

裸减法由 dqjd S2（6808ea6d）显示行收口引入——pre-S2 版本是
`row.saturating_sub(...)`，S2 重写时守卫丢失。

**为什么 0.5.2 不崩**：0.5.2 同时带着 wrap_map gap 填充 bug（0e019415 已修），
替换后 wrap 树是歪的，offset 0 被映射到错误的高行、恰好 ≥ visible_range.start，
裸减法被下游 bug 掩护；0e019415 修对了 wrap 树，这颗潜伏雷反而露出来。

## 修法（对齐 zed，不打手搓补丁）

zed `scroll/autoscroll.rs` `autoscroll_horizontally`（L427-431）索引可见
layouts 切片前先做范围守卫：`head.row() >= start_row && head.row() < start_row
+ layouts.len()`，守卫不过直接跳过水平调整。PF 的数据形状是
「lines Vec + visible_range.start 偏移」，等价谓词 = `checked_sub + get`：

```rust
if let Some(line) = disp_row
    .checked_sub(last_layout.visible_range.start as u32)
    .and_then(|vis_ix| last_layout.lines.get(vis_ix as usize))
```

目标行在视口上方时水平调整本就无意义（该行不可见），vertical 分支的闭式解
不依赖 lines 索引，照常把视口滚到目标——语义与 zed 守卫一致。

**判据记忆点**：`last_layout` 是上一帧的，新快照映射出的显示行与旧
`visible_range` 不可比——凡「目标行可能不在可见行」的索引一律
`checked_sub + get`，禁裸减。同类受害入口：`search.rs` 363/371（命中跳转）、
417（replace_next）、454（replace_all）、`movement.rs:50`（move_to 光标滚动）
——全部收在 scroll_to 一个入口上，守卫一处全覆盖。

## 复现（pif-ui 全链，run.sh 可重跑）

前置：`cargo build`（产物 target/debug/{pi-flash,pif-ui}.exe）。

1. 生成 40 节 CJK 长段 md（每节带 `ORIGIN-0xx` ASCII token）；
2. 隔离实例 `file.open` → `input.focus editor` → `pagedown`×5（视口滚到
   214 行后，visible_range.start > 0）→ `ctrl-h` 开替换面板；
3. 搜索框键入 `origin`（53 命中）→ 点替换框键入 `mark` → 点「全部替换」；

### A/B 实录（2026-11-20）

| 版本 | 结果 |
|---|---|
| 未修复（stash 后重建） | 进程死，`crash-*.log` 帧全同用户实录（panic_const_sub_overflow @ scroll_to） |
| 修复后重建 | 存活；53 处全替换盘上验证（mark-0xx ×53 / ORIGIN ×0）；视图滚回文档顶（行号 1 起） |

## 人工验收

1. 打开任意长 md，滚到中部，ctrl-h → 替换全部 → 不崩、回顶、内容全换；
2. 命中跳转（prev/next）跨视口滚动不崩；
3. 「替换一处」在命中行于视口上方时（滚动态）不崩。
