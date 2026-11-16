# 021 文件树多选 v1：用例台账（对齐 ZED project_panel 选择模型）

设计依据：Zed `crates/project_panel/src/project_panel.rs` 选择模型直接挪植
（用户拍板：不自己造轮子）——`selection`（活动条目）+ `marked_entries`
（标记集）双轨；PF 落位 `ws.tree_selected`（既有 082 字段，语义升级为活动
条目）+ `ws.tree_marked`（新）。纯函数 + Chat 入口在
`function_panel/file_tree/selection.rs`。

## 范围拍板（2026-11-16 用户答问）

- 多选 = **鼠标** shift+点（范围）/ ctrl+点（切换）；不做键盘扩选（树面板
  不可聚焦是既有架构约定，键盘导航单独立项再做）。
- 作用于整组的操作：**删除 + 复制路径 + 拖拽批量移动**（Zed 同款挪植；
  树剪切粘贴 PF 没有，不在范围）。
- @提及/资源管理器/终端/Pin/浏览器打开/执行：保持单条（活动条目/菜单槽）。

## 交互语义（Zed 挪植对照表）

| 手势 | Zed 落点 | PF 实现 |
|---|---|---|
| 裸点目录 | 清标记 + 翻面 | `tree_click_select` 裸点分支（原 082 语义） |
| 裸点文件 | 打开 → 重标记单条 | 同上 + `open_file_tab` |
| ctrl+点 | 进出 marked_entries，不开文件/不翻面 | `Modifiers::secondary()` 分支 |
| shift+点 | 锚=活动条目，展平行区间**并集**扩选 | `range_union`，不清旧标记 |
| 右键未标记行 | 折叠标记集再开菜单 | `tree_right_select`（Zed 6620 注释同款理由） |
| 动作对象 | `effective_entries` 合并规则 | `effective_sel`：无标记→单活动；唯一标记不含活动→单活动；否则=标记集 |
| 删除取数 | `disjoint_effective_entries_excluding_roots` | `confirm_tree_delete_from_slot`：合并 → `disjoint` 剪子孙 → 剔根 |
| 拖拽整组 | `DraggedSelection::items()` | `TreeDrag { path, is_dir, marked }` + `drag_items`：拖标记成员→整组逐条落点判定，否则单条 |
| 复制路径多选 | `\n` 拼接 | `tree_copy_path(Vec)` join `\n` |
| 改名折叠 | workspace 级 handler 清标记 | `open_file_rename` 清 `tree_marked` |
| Tab 跟随 | active 移动不清标记 | `sync_tree_to_file` 只动 `tree_selected` |
| 切项目 | — | `tree_proj_menu_pick` 清 selected + marked |
| 移动/改名后 | 条目 id 天然存活 | `remap_moved_entry` 逐条 remap marked（PF 路径键） |
| 外部删除 | worktree 事件同步 | `rebuild_tree` 收口：marked 剪掉不在展平行的行 |

## 单测矩阵（随码固化，`cargo test -p app`）

| 用例 | 落点 | 断言要点 |
|---|---|---|
| 合并规则全分支 | `selection::tests::effective_sel_follows_zed_merge_rules` | 无标记→[active]；唯一标记不含 active→[active]；唯一标记=active→标记集；多标记→标记集；全空→空 |
| shift 范围双向 | `range_union_covers_anchor_to_clicked_both_directions` | 正向/反向区间元素全、点自己=单元素 |
| 锚点失联退化 | `range_union_degrades_to_clicked_when_anchor_missing` | 锚/目标不在展平行 → 只点选目标 |
| 删除剪子孙 | `disjoint_prunes_descendants_keeps_order` | 标记祖先连带整棵剪子孙、平级保留、原序不变、空集安全 |
| 拖拽整组解析 | `file_tree_drag::tests::drag_items_group_when_marked_else_single` | 拖标记成员→整组（含 is_dir）；非成员→单条；空标记→单条 |
| 三语审计（新键） | i18n_audit（存量套件） | `tree.delete_confirm_multi(_dirty)` 三语齐 |

## pif-ui 自动化验收（见 ui.md 执行记录，2026-11-16）

- 断言面：`snapshot files` 新增 `tree_selected` / `tree_marked` /
  `rows[].selected`；`snapshot dialogs` 的 `tree_confirm_del` 升级为数组。
- op：`files.select`（新，与行 on_click 同一方法 `tree_click_select`，可选
  `modifiers` 数组走修饰分支）；`input.mouse` 支持 `modifiers` 参数（真实
  事件链冒烟用）；`file.delete` 升级走 `tree_right_select` +
  `confirm_tree_delete_from_slot`（与右键菜单「删除」完全同链路）。
- 断言 8 条：裸点选中开 tab / ctrl 进出×3 / 裸点清空 / shift 正向范围 /
  shift 并集保留区间外成员 / 右键未标记折叠（含反向扩选后）。

## 用户真机验收（2026-11-16，口头回报）

- **组移动（多选拖拽）成功** ✓
- **组删除（多选删除）成功** ✓

## 人工验收清单（真机项）

- [ ] 复制路径多选：标记 2 条 → 右键「复制路径」→ 粘贴出两行
- [ ] 删除确认卡 N≥2 文案：「删除选中的 N 项？…」（含脏文件时丢更改提示）
- [ ] 删除标记目录 + 其子孙同在标记集：确认卡条数 = 剪枝后（子孙不算）
- [ ] 右键未标记行开菜单后「删除」只删该行（不波及看不见的标记集）
- [ ] 标记后点编辑器 tab 切走再回树：标记保留（active 跟随 tab）
- [ ] 标记后改其中一条名（F2）：标记折叠，改名行正常
- [ ] 拖标记组到列表空白处 = 整组移到项目根
- [ ] Pin 视图/只看变更视图点击：裸点清标记（无多选手势，行为不劣化）
