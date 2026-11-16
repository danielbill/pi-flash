# 021 文件树多选 pif-ui 自动化验收记录（2026-11-16）

前置：`cargo build`；隔离实例 `PI_FLASH_DIR=tmp/ui-test-multisel` +
`PI_FLASH_AUTOMATION=auto`；fixture 项目 `tmp/ui-test-multisel/proj/`
（a.txt / b.txt / c.txt / sub/d.txt / sub/deep/e.txt）。

展平行序（0 基）：0=sub · 1=deep · 2=d.txt · 3=a.txt · 4=b.txt · 5=c.txt。

## 断言面

- `snapshot files`：`tree_selected`（活动条目）/ `tree_marked`（标记集，
  插入序）/ `rows[].selected`（动作对象集成员高亮位）——本任务新增。
- `snapshot dialogs`：`tree_confirm_del` 由单路径升级为路径数组。
- op：`files.select {path, modifiers?}`（新）——与行 on_click 同一方法
  `tree_click_select`，修饰分支同 UI；`input.mouse` 新增 `modifiers` 参数
  （真实事件链备用）；`file.delete` 走 `tree_right_select` +
  `confirm_tree_delete_from_slot`（右键菜单同链路）。

## 执行记录（files.select + snapshot，11 断言 fail=0）

```
✓ 1 裸点 a.txt       → tree_selected=a.txt，tree_marked=[]，file_tabs 开 a.txt
✓ 2 ctrl+点 b.txt    → tree_marked=[b.txt]（active=b.txt）
✓ 3 ctrl+点 c.txt    → tree_marked=[b.txt, c.txt]
✓ 4 ctrl+点 b.txt    → tree_marked=[c.txt]（再点切出）
✓ 5 裸点 a.txt       → tree_marked=[]（裸点清空），开 tab
✓ 6 shift+点 c.txt   → tree_marked=[a,b,c]（锚=a.txt 正向区间）
✓ 7 ctrl+点 d.txt    → tree_marked=[a,b,c,d]（区间外成员并入）
✓ 8 shift+点 a.txt   → tree_marked=[a,b,c,d]（锚=c.txt 反向区间，并集不清 d）
✓ 9 snapshot 面      → rows[].selected 与 tree_marked 一致、tree_confirm_del 数组形制
```

组删除链路（file.delete 走 from_slot 合并规则，2026-11-16 实测）：

```
✓ 10 标记 sub+b.txt+d.txt → tree_marked=[sub, b.txt, sub/d.txt]
✓ 11 file.delete {sub} → dialogs.tree_confirm_del=[sub, b.txt]
      （d.txt 被 disjoint 剪掉——标记祖先连带整棵，子孙不入确认卡）；
      file.delete_confirm → tree_deleting 归零，树行刷新后仅剩 a.txt/c.txt
      （sub、b.txt 已进回收站，d.txt 随 sub 子树连带）
```

## 用户真机验收（2026-11-16，同一隔离实例内亲手操作）

- 组移动（多选拖拽到目录）✓
- 组删除（确认卡 → 回收站）✓
  （fixture 里 a/b/c.txt 即被该轮真机组操作消耗，第 10/11 条断言前已重建）

RESULT: fail=0。
