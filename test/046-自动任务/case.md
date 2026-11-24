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
| 台账只含真实执行（历史 skipped 过滤）+ 排序/截断 | pf-context `list_runs_filters_legacy_skips_and_orders` | started_at 倒序；last_run 同口径 |
| prune 每任务保 50 | pf-context `prune_runs_keeps_newest_per_def` | 60→50 删最老；幂等；keep 收紧再删 |
| 删除连带删 runs | pf-context `delete_cascades_runs` | 同事务级联；重复删报错 |
| next_run：minute 锚点推进不对齐墙钟/防漂移/时钟回拨 | app `tasks::sched::tests::minute_from_anchor_not_wall_clock` | 11:46 建每 10 分不在 11:50 抢跑 |
| next_run：hourly 只对齐分 | app `hourly_aligns_minute_only` | |
| next_run：daily 跨日回滚 | app `daily_rolls_to_next_day_when_today_passed` | |
| next_run：weekly 周一=1 位次/最近周几 | app `weekly_monday_is_one_and_nearest_weekday_wins` | 2026-01-05 是周一 |
| next_run：monthly 溢出日跳过 | app `monthly_skips_overflow_day` | 1/31 → 3/31（2 月无效跳过） |
| claim：due/宽限内/in-flight 接力保持到期/misfire 超窗静默丢弃 | app `claim_filters_inflight_misfire_and_due` | 5min 宽限边界；busy/late 零落账；misfire 锚点不动前推 next |
| 运行中接力：跨格零落账、结算后补跑 | app `in_flight_overlap_relays_after_settle` | 重叠触发保持到期；结算后认领 scheduled_at=重叠格；指针再前推 |
| 接力上界：超长运行拖过宽限 → 静默丢弃 | app `relay_exceeding_grace_drops_silently` | 不追债不排队爆炸；零落账只留执行行 |
| 运行期 tick 零刷屏（刷屏事故回归） | app `running_task_ticks_do_not_spam_ledger` | 运行期多 tick 台账恒 1 行 |
| settle：整行结算不碰指针 | app `settle_writes_outcome_and_never_touches_pointer` | 指针归 claim 认领即消费 |
| settle：manual 不污染周期 | app `settle_manual_never_touches_schedule` | next_run/锚点原样 |
| settle：无 running 行（派发前置失败）空转 | app `settle_without_running_row_is_noop` | 指针已由 claim 前推 |
| settle：timeout 细分键映射 | app `settle_timeout_maps_by_kind` | |
| recover：running 残留置 failed(app_exit)+首轮 prune | app `recover_marks_orphans_failed_and_prunes_first_round` | 二轮幂等 |
| 脚本 runner：exit 0/非零/超时强杀/kill 旗标/不支持扩展名 | app `tasks::run::tests::*`（5 例） | CREATE_NO_WINDOW；输出尾进 result_json |
| spec 扩展清单：显式清单/空表零漂移 + 存量 spec（无 `extensions` 键）= None | pf-context `agent_extensions_roundtrip_and_legacy_default` | None = 沿用全局启用集（pi-flash-jt44） |
| spec 扩展清单：空串条目写库前拒 | pf-context `agent_extensions_reject_blank_entry` | 拒写后库空；合法清单落库读回一致 |
| 表单快照 → spec 扩展清单三态零漂移 | app `tasks::form_spec_tests::build_spec_carries_extensions_verbatim` | None / 显式 / 空表互不串味，均过 validate |
| plug 计数：Some = 清单长度（空表 [0]）；None = 全局启用集条数 | app `tasks::form_spec_tests::ext_count_reads_list_or_global_enabled` | 禁用条目/空 source 不计 |
| 勾选集 → 源清单：宇宙序去重 + 展示名排序；未勾/不在宇宙的不落 | app `session::pi_extension_picker::picked_sources_tests::*`（2 例） | 会话区/任务页两宿主共用 |
| 派发扩展清单：任务清单原样 + 落台账 draft 键；None = 全局集；两种都清 draft 键残留 | app `tasks::run::tests::dispatch_ext_sources_pins_task_list_and_clears_stale_draft` | 上一实例同名草稿清单不串味；任务配方覆写残留不合并 |
| 全局启用集内存版/计数版与盘读同口径；`entry_source` 借用版同值 | pi-link `skills::tests::display_source_strips_npm_and_enabled_sources_filter_disabled` | 非法形状条目同值 |

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

## 046-T 可交互任务页签（2026-11-24 拍板：永不降级只读）

点任务日志行名 = 开该会话的**普通可交互 tab**（可看可聊可介入 steering），
原只读观察窗口径作废（子代理观察窗不受影响）。派发照旧后台跑不自动开页；
关闭页签不杀进程（runtime 留池按退役窗清账）。

| 项 | 落点 |
|---|---|
| 页签打开/重建/关闭 | `session::pool::open::open_task_session` / `build_task_rt` / `close_task_tab`（唯一入口：tasks/view.rs 日志行点击） |
| 三态激活分流（不抢固定段） | `session::pool::runtime_link::switch_to`（task_tab 分支：在 panel_tabs 找 Observer\|Task 双变体激活，主指针不动） |
| 草稿/last-open 护栏 | switch_to `!r.task_owned` 守卫（任务页输入不落用户草稿） |
| rekey 迁移 | `pool::session_ops` on_file_bound 迁移循环扩 Observer\|Task 双匹配 |
| tab 激活/批量关/摘牌 | `editor::tabs::activate_content_tab`（池缺失重建）/ `tabs::close`（Task 仅摘牌）/ psp_actions 项目删除双匹配 |
| 退役窗协商 | `retire_due(…, task_tab_open)`：页签开 ⇒ 通用窗 600s+阻塞件保护；页签关 ⇒ 任务窗 30s+跳保护 |
| 徽标 | titlebar/tabs `task_tab_badge`：运行中橙点 > 红!（外部错误）> 黄点（中断）> 青蓝点（完成未读），读 psp 三集 |
| 快照 | automation/snapshot：tabs `kind=task` / open_tabs `task_tab` |

单测：`session::retire::tests::task_tab_open_upgrades_to_general_window`
（页签开 ⇒ 599s 不退 / 601s+新鲜阻塞件拦 / 601s 无阻塞件退）。

人工验收（真机项）：①点运行中任务日志行 → tab 开、可发消息介入（steer）
且不杀进程；②关闭页签任务继续跑、到期正常退役结算；③退役后点 tab 秒开
重开为可交互（非只读）；④任务 tab 打开期间固定段（用户会话）不被顶替；
⑤任务页输入切走不串用户草稿；⑥徽标四态颜色正确；⑦重开/持久化后任务
tab 不复活（不进 persist）。

## 人工验收（真机项）

1. 建一个「每 1 分钟」agent 任务（指定任意存在目录）→ 20s 内派发 →
   psp 会话列表出现任务会话、列表行 spinner→结果点、历史行出现；
2. agent 行点击 = 打开该次会话；脚本行点击 = 620 输出查看（含 exit code）；
3. 编辑改周期 → 下一轮按新周期；停用 → 不再触发；启用 → 恢复；
4. 删除运行中任务 → 会话中止、台账连带消失；
5. 模型覆盖生效（会话标签三段链显示覆盖模型）。
