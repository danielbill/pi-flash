# 046 自动任务 v1：用例台账（S5 收口）

总设计 `docs/模块设计/046-自动任务/046-自动任务.md`（§1 UI 唯一事实源 /
§2 开发计划）+ `046-0-自动任务设计需求.md`。切片 S1→S2→S3→S4→S5，提交
0c09f87b / 24f92659 / b6416707 / 4a51f81d。

## 单测矩阵（随码固化，cargo test -p pf-context / -p app）

| 用例 | 落点（测试名） | 断言要点 |
|---|---|---|
| spec 校验矩阵（unit/interval/hour/minute/weekdays/month_day/kind 互斥/tools/thinking/空标题） | pf-context `domain::tasks::tests::spec_validation_rejects_illegal` + `def_title_must_not_be_empty` | 非法值拒写（写库前拒绝） |
| spec roundtrip | pf-context `spec_roundtrip_keeps_fields` | 序列化/解析零漂移 |
| 任务 20 上限（所有状态计入） | pf-context `create_limit_counts_all_states` | 第 21 条拒；停用不豁免；删一补一 |
| 启停/列表/重排 roundtrip | pf-context `enable_list_reschedule_roundtrip` | list_enabled 过滤；reschedule 写指针；None=清空 |
| 整行更新（created_at 不覆盖；非法 spec 拒写） | pf-context `update_def_overwrites_row_keeps_created_at` | |
| run 生命周期（start→settle；防双结算；bind_run 冻结） | pf-context `run_lifecycle_start_then_settle` | 仅 running 态可结算；model/session_path 回填 |
| skip 落账 + 台账排序/截断 | pf-context `record_skip_and_list_runs_ordering` | skipped 一段写齐；started_at 倒序 |
| prune 每任务保 50 | pf-context `prune_runs_keeps_newest_per_def` | 60→50 删最老；幂等；keep 收紧再删 |
| 删除连带删 runs | pf-context `delete_cascades_runs` | 同事务级联；重复删报错 |
| next_run：minute 锚点推进不对齐墙钟/防漂移/时钟回拨 | app `tasks::sched::tests::minute_from_anchor_not_wall_clock` | 11:46 建每 10 分不在 11:50 抢跑 |
| next_run：hourly 只对齐分 | app `hourly_aligns_minute_only` | |
| next_run：daily 跨日回滚 | app `daily_rolls_to_next_day_when_today_passed` | |
| next_run：weekly 周一=1 位次/最近周几 | app `weekly_monday_is_one_and_nearest_weekday_wins` | 2026-01-05 是周一 |
| next_run：monthly 溢出日跳过 | app `monthly_skips_overflow_day` | 1/31 → 3/31（2 月无效跳过） |
| claim：due/宽限内/in-flight 撞车 skip/misfire 超窗 skip+前推 | app `claim_filters_inflight_misfire_and_due` | 5min 宽限边界；busy 落 skipped；misfire 锚点不动前推 next |
| settle：整行结算+锚点保留重算 | app `settle_writes_outcome_and_reschedules_from_anchor` | |
| settle：manual 不污染周期 | app `settle_manual_never_touches_schedule` | next_run/锚点原样 |
| settle：无 running 行（派发失败）周期照常推进 | app `settle_without_running_row_still_advances_schedule` | S2 stub 路径 |
| settle：timeout 细分键映射 | app `settle_timeout_maps_by_kind` | |
| recover：running 残留置 failed(app_exit)+首轮 prune | app `recover_marks_orphans_failed_and_prunes_first_round` | 二轮幂等 |
| 脚本 runner：exit 0/非零/超时强杀/kill 旗标/不支持扩展名 | app `tasks::run::tests::*`（5 例） | CREATE_NO_WINDOW；输出尾进 result_json |

## pif-ui 自动化验收（本目录 ui.md 步骤；flag 关/开两实例）

- `tasks.open` op + `snapshot dialogs` 面的 `tasks` 键：三 tab 结构断言、
  空态、表单字段、flag 关时 tasks 键缺席（按钮/弹窗/source 三处同源）。
- 执行记录见 `ui.md`（回填实际运行输出）。
- 2026-11-22 会话 type 属性（执行记录 9）：任务会话落 pf/session-type
  entry（spawn env + session_type.ts 扩展），psp/恢复/搜索按 kind==User
  过滤（sub 复用 subagent/child 派生）；052 过期删除不区分 type。E2E：
  marker 落盘/psp 隐身/日志行点击开话（修 cwd_for_session 隐身会话盲区）/8 连跑正常；搜索层 hit.kind 单测。
- 2026-11-22 五条拍板回归（执行记录 8）：脚本框只读（点框不开选择器）、
  浏览钮 zh「浏览」/en「Browse」无省略号（6e0f9571）、建任务端到端（名称/
  指令注入 + 项目 picker + 保存 defs=1）、行尾动作钮链（tip + trash →
  确认浮层 → 删除 defs=0）。**发现 bug：行尾 icon 显影失效**（self-group +
  group_hover 同 div，gpui compute_style 先于自身 push——修法 = group 挪
  行容器，详见 ui.md 执行记录 8 与 docs/buglist.md）。

## 人工验收（真机项）

1. 建一个「每 1 分钟」agent 任务（指定任意存在目录）→ 20s 内派发 →
   psp 会话列表出现任务会话、列表行 spinner→结果点、历史行出现；
2. agent 行点击 = 打开该次会话；脚本行点击 = 620 输出查看（含 exit code）；
3. 编辑改周期 → 下一轮按新周期；停用 → 不再触发；启用 → 恢复；
4. 删除运行中任务 → 会话中止、台账连带消失；
5. 模型覆盖生效（会话标签三段链显示覆盖模型）。
