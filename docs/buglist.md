# PF Bug 追踪

> 约定：条目按时间倒序；已修复条目只留「根因 / 修法 / 验证」，展开细节见 `docs/progress.md`
> 与 `docs/agents/*.md`。`待复验` 条目 = 当前待跑的手工清单。2026-11-11 全文压缩：过程
> 叙事与返工史删去，需要时查 git 历史。

## 已验证（2026-11-10）——新会话手选模型，等待首轮响应期间胶囊跳回默认模型

- 根因：spawn 带 `--model` 用 `take()` 当下清掉 `pending_model`；pi 冷启动 1-3s 期间首个
  get_state 未到，胶囊标签链（pending → state → 默认）跌进默认再跳回（割裂）。
- 修法：spawn 读 `pending_model.clone()` 不 take；清账挪到首个 get_state 响应臂（state 即
  真值源，`--model` 解析失败回落默认时也不被旧手选遮蔽）。
- 验证：pif-ui 全流程 pill 恒为手选；`pending_model_tests` 3 例 + app 474 / pi-link 278 全绿。

## 待复验（2026-11-06 修复）——子代理 044-0 真机实测 B1-B5（代码 d5ecf742→ac33bdee，真机验收未做）

### 验收①：观察窗挤掉主会话，tab 栏找不回（464e1fa5 → 返工 aa637453）

- 根因：topbar 会话 tab 是固定单槽，open_session 的 switch_to 直接换槽内会话，旧会话无 tab
  可回；标题兜底「新会话」的根子 = 子会话被写进 last-open，启动恢复成子会话（父 catalog 无从解析）。
- 修法：子会话观察窗 = 自由区普通 tab（`PanelTab::Observer`，追加末尾、标题 = 任务名、× 可关、
  可拖拽/右键）；固定段恒一枚 primary 主会话 tab（最近激活的非 observer 会话）；observer 不写
  last-open + boot_target 过滤子会话恢复目标。
- 验证：面板行 → tab 区末尾新增任务名 tab，固定段主会话不动；互切无损；重启不落子会话。

> 取证环境：委派 explorer×4 复现；一次性脚本 `vendor/pi/tmp-repro{,2,3}.mjs`（探针，用完可删）；
> bd：pi-flash-3ghh / -es8m / -y26f / -g9pn / -mtc1。

### B1 行二统计只在 settle 后出现（pi-flash-y26f，d5ecf742）

- 根因：manager.ts watcher 守卫 `state !== "active"`，而运行期全在 settling 内，每轮上报被拦，
  usage 也只在 settle 落盘。
- 修法：守卫改 settled/failed 才拦；watch 通道升级 `ChildProgress{stats,usage}`（usage 随
  message_end 累计，settle 的 result.usage 同口径）。
- 验证：委派 ≥2 轮任务，第 1 轮结束 1s 内行二出轮次/发言/工具/token。

### B2 运行中面板行不可点击打开子会话（pi-flash-3ghh，d1172ea9）

- 根因：descriptor.sessionFile 只在 #settle 回写，in-flight 条目无 sessionFile。
- 修法：prepare 返回即回写 session_file()（accept 失败的 failed descriptor 同样带）。
- 验证：运行中点行 → 只读观察窗且随子会话追加刷新。

### B3 并发满即拒的第 4 个委派完全不可见（pi-flash-g9pn，2e33cc2f）

- 根因：CAPACITY_REACHED 满即拒不排队；被拒调用无 descriptor；PF 把 tool_execution_update
  整体丢弃（WireEvent 空臂），onUpdate 分片到不了 UI。
- 修法：失败路径先发 onUpdate(failed, error_code) 再返回 failure（details snake_case）；PF
  接收 agent 工具 partialResult.details 合入 ToolCall 块；工具描述注容量上限+「等槽位释放
  重派」引导；被拒委派也落 failed descriptor（面板红行 + 内联分码）。
- 验证：并发 3 派 4，第 4 条立即在转录与面板显 [失败]+CAPACITY_REACHED；槽位释放后补派可见。

### B4 面板头部合计时长与行时长矛盾（pi-flash-mtc1，61cc503a）

- 根因：头部用 stale 的 updatedAt−startedAt，行用 live now−startedAt，两套口径。
- 修法：`row_secs` 唯一口径函数（运行中 live / 终态 updatedAt），头部与行同走。
- 验证：运行中合计 ≥ 最大单行且随 tick 增长。

### B5 会话流 compact 卡状态点恒绿（pi-flash-es8m，ac33bdee）

- 根因：render_subagent_card 的 done 条件把「结果未到」也算 done。
- 修法：done = result 到且 stopReason==completed；其余非终态黄 spin；中止/轮次用尽橙点；
  失败红点+分码（B3② 分片即时上卡）。
- 验证：派出即黄 spin，settle 后才变绿/红/橙。

### 信息项（非 bug）

- 内置组只见 2 枚 / explorer 带「覆盖内置」：用户 `~/.pi/agent/agents/explorer.md` 同名自定义
  覆盖内置（by design），删自定义文件即恢复。
- 「看不到 subagent 工作内容」：compact 卡按 044-0 设计不带 prompt/正文；检视 = 面板行点开子会话（B2）。
- 只读观察窗外部追加跟随（~3s）依赖 B2，随本轮一并复验。

## 待复验（2026-11-07 修复）——验收⑥导航第三行/悬浮菜单哑火：descriptor 丢 parentToolCallId（fcf1a069）

- 根因：扩展 `DelegationContext` 收了 `parentToolCallId` 但 `manager.start` 没传、
  `SubagentRequest` 类型上无此字段 → descriptor 落盘即缺，宿主按轮归组无钥匙可用，第三行/菜单
  恒空。PF 侧接线核验完整，锅在扩展。
- 修法：types.ts +字段 → tool.ts 透传 → manager.ts 两处 createDescriptor（含 B3③ 满拒）带上；
  npm test 114 绿（含成功/满拒回归）。
- 老会话不可追溯：修复前落盘的 descriptor 已缺字段，JSONL 只追加——老会话第三行/菜单永不出现，
  属数据现实非 bug。
- 验证：重启 PF → 新派一轮（≥1 个 agent 工具调用）→ 轮卡出第三行 `[bot] ●●●`（完成青/异常红/
  运行 accent），hover 弹 400px 扩展菜单；断言用 `pif-ui snapshot dialogs` 的
  `subagent_panel.turns`。

## 已修（0.4.1 复盘，2026-11-03）——0.4.0 发布包缺子代理扩展

- 根因：f848fdfa 把内置子代理扩展改名 `pi-subagents` → `pf-subagents`（避 pi 市场同名），
  release.sh 随包分发段未同步，`-d` 探测旧名不中，打警告照常出包。
- 修法：release.sh 三处同步（91feed63）；0.4.1 包内 `extensions/pf-subagents/src/index.ts` 在位。
- 0.4.0 已发布资产不可改（RELEASE.md §0.4），受影响用户升级 0.4.1 即得。

## 已修（0.4.0 发版测试，2026-11-03）——微信插件「绑定后重启即哑」：台账挂载路径卡在旧版本目录

- 根因：绑定微信时把 `pi_extensions` 绝对路径写进会话台账，而市场布局含版本目录
  （`plugins/<pkg>/<ver>/pi/index.mjs`）；插件升级清掉旧版本目录后，重启恢复把死路径当 `-e`
  传给 pi，pi 加载失败即退出 → ext 无处安身，微信发消息无响应。
- 修法：`pi_link::session_ext::resolve_sources`（版本层重定位 / 死条目摘除 / 去重，非插件包
  路径原样保留）+ 首次自愈回写；消费点全过闸（SessionRuntime::new / spawn / 
  plugin_attach_pi_exts / has_live_service 抹版本段 / 005 预热判据）。见 90-4 §9。
- 验证：`resolve_mount` 5 例 + `has_live_service_tests` 4 例；真机——台账还原 0.1.4 死路径后
  启动自愈成 0.2.0，pi 存活、`svc_log` 出 boot、`notice_log` 补「已连接」。
- 已知边界（by design）：服务生命期 = 绑定会话进程生命期（005 定案不保活）；重启后需先打开
  绑定会话 ext 才起。打破需「绑定会话后台常驻」策略，待拍板。

## 已修（2026-11-02 晚）——新会话模型列表点行不出 ✓ / pi 死后选不中、发送转圈

- 根因：① 退出臂不清 `agent.session`，SetModel 写进死管道且 `ensure_spawned` 不重 spawn；
  ② ✓ 只读 `state.model`，草稿/死进程只记 `pending_model`。外因 = 下一条（1.5.0 拒载残缺 cost）。
- 修法：退出臂 `agent.session = None`；✓ / pill / snapshot 统一 pending → state → 兜底链；
  新 op `model.pick` + dialogs 面 `model_current_ref` 供断言。
- 验证：`test/042-1/picker-current/run.sh` 13/13；真机点行出 ✓、转圈会话重开可发。

## 已修（2026-11-02 晚）——发送空转 / 模型列表选不中：models.json 残缺 cost 让 pi 整份拒载

- 根因：pi 的 cost 是 `ModelCostSchema` 四键全必填（只有 modelOverrides 是 Partial）；【自动
  填参】只写目录给的键，models.dev 多数条目无缓存价 → 落成 `{input, output}` → pi 对整份
  models.json 报 schema 错丢弃 → 自定义商全消失、`--model` 退出、会话转圈；PF validate 只查
  值是数字不查键，放行了。
- 修法：`models_json::complete_cost`（四键补 0，已有值不动）填参落盘前调；`ensure_model_defaults`
  normalize 补齐；validate 四键必在（最后一道闸）；`models_json::repair` + `mj_repair_models_json`
  启动/设置页对存量自愈（有改动才写）；pi 退出时 stderr 末行挂进 status。
- 验证：单测 `partial_cost_is_completed_and_rejected_until_repaired`；run.sh CASE10 33/33；
  真机重启即自愈、MY-GLM 可选、发送有回复。

## 已修（0.3.2 发版后，2026-11-02）——自定义商勾选的探测模型对话出错 + 详情页只读

- 根因：253ca038 给 `mj_materialize_selection` 加「仓命中 → 原样复制」分支提前返回，没走
  `upsert_provider + save`；探测拉到的模型必在仓里 → 勾选只改内存，models.json 永远没有它 →
  pi 不认识（对话出错）、详情页落只读覆盖视图。验收能过是当时账本模型由旧路径写入，发版后新建
  商必踩。
- 修法：纯变换下沉 `pi_link::models_json::materialize_model`（单一出口，3 单测）；详情页一律
  可编辑——仓有账无的模型以「追加槽 + 仓定义预填」草稿打开，保存只进仓；装载时
  `mj_reconcile_custom_selection` 对账补物化（仅显式勾选，all_enabled 零写盘）；探测解析补
  max_output_tokens/input_modalities/effort + 仓 merge 字段级补缺。
- 验证：`test/042-1/custom-model-materialize/run.sh` 27 断言；CASE11 真端点；真机 myds 自愈、
  对话成功、详情页可编辑。

## 待复验（0.3.2 验收）——设置-模型上组混入已删除的测试商 ds/glm，重启后又消失

- 根因：测试商从 models.json 删除后条目仍留在 `catalog-cache.json`，`merge_models` 并集永不
  淘汰；「不在注册表 ∧ 不在 models.json」落进 `sidebar_providers` 扩展兜底桶 → 上组恒入列；
  `catalog_for` 实时槽优先，所以答复前显示、答复后隐藏（"重启消失"只是时序，缓存里还在）。
- 修法：`Globals.catalog_provisional`（种子期标记）；首个 Models 事件先 `purge_ghosts`（删既
  不在实时答复、也不在 models.json ∪ registry 的条目）再并集，有删除即回写自愈。不动缓存
  格式与归属三分法。
- 验证：单测 `purge_ghosts_drops_only_unbacked_providers`；手工——启动后开一个会话，
  catalog-cache 不含 ds/glm，上组无二者、dds 在分隔线下、freeflow 在上组。

## 结构债务（0.3.2 口径复核放行）——check_arch 行数红线 5 处超线 ✅ 已偿清

- 债务：0.3.1→0.3.2 期间 settings/custom_models.rs（651→2017 行）、settings/models.rs
  （1483→1842）与 pf_plugin 两函数（parse_manifest 384 / render_node_inner 307 /
  mj_model_modal_overlay 315）超当时红线（文件 1500 / fn 300），所有者 2026-11-02 放行发版、
  发后偿还。
- 偿清：2026-11 vh67 收官，基线人工重写归零（FILE×0 + FN×0，commit 407712cf）；现红线已收紧为
  文件 1000 / fn 400（023ba6de）。

## 已验证——新建 Provider 弹窗偶发「读取注册表」几十秒

- 根因：弹窗缓存未命中时起一次性 node dump，脚本尾 `listCredentials()` 走默认 AuthStorage，
  撞 `auth.json.lock` 孤儿锁等满 SDK staleMs 30s（干净 1.78s / 有锁 30.8s）；修一次后缓存落盘
  故「偶发」。
- 修法：DUMP_SCRIPT 传只读 `ReadOnlyAuthStorage`（裸读 auth.json 零锁，dump 只消费
  providerId→type 元数据）。
- 验证：30.8s → 2.1s，42 providers / 1599 models / 4 凭据无损；陷阱记 `pi-rpc.md`。

## 已验证——「🔌 MCP: 1 server enabled」反复出现在 PF 发往微信的消息里

- 根因：pi 冷启动把横幅作为独立 assistant 消息发进 RPC 流；泄漏进程跑的是横幅过滤入库之前的
  旧构建（TextEnd 过滤与 flush 行剔除两道防线都不在）。
- 修法（第三道总闸）：`RemoteControl::send`（微信出站唯一内容出口）对「整条消息即横幅」拦一道
  （单行 + 含 mcp:/server + enabled 收尾，先剥 ANSI），命中即内部消化；多行聊 MCP 的真回复不误杀。
- 验证：回归 `send_gate_swallows_pure_banner_messages`（实抓文案 + 变体 + 反例）。
- ⚠️ 遗留（未定位）：同批 dump 显示同文偶发双发（seq 相邻重复），非横幅专属，疑双 flush 或
  重复喂入，另开任务排查。

## 待复验——跨项目开文件顶掉活跃会话：switch_project 新增「进行中会话保留」

- 根因：`sync_tree_to_file` 走完整 switch_project（树定位刚需），尾部「新项目无 last-open 就
  new_session」把进行中对话顶出会话标签。
- 修法：无 last-open 时当前会话进行中（已绑会话文件、非空白草稿）则原地保留，只有空白草稿才落新。
- 验证：app 398 + pi-link 197 全绿。待手工：聊天中开别项目文件 → 树定位新项目、标签仍是原对话；
  新项目有 last-open 照旧恢复；psp ＋ 新建会话不受影响。

## 待复验——微信 Bot 开关/扫码成功后主进程卡死最长 90 秒

- 根因：`Transport::drop` 逐个 join 线程，poll 线程大概率阻塞在 getupdates 的 90s 服务端挂住
  里（POLL_TIMEOUT=90s）；触发 = 关开关与扫码后泵检测 token 变化（UI 线程 200ms 一拍）。
- 修法：drop 改「置 stop + detach」，UI 不等在途长轮询；游标未 ack 下个轮询者重收这批（重复
  而非丢失）；token 变化的 state 读按 1s 节流。
- 验证：wxprobe 117 / app 383 全绿。待真机：扫码瞬间与关开关不冻结；关后 90s 内重开不丢消息。

## 待复验——微信 Bot 出站「受理不投递」：client_id 固定复用被 iLink 去重吞掉

- 根因：`sendmessage` 的 client_id 协议要求每条消息唯一，PF 用 state 里写死的稳定实例 id 长期
  复用 → iLink 受理（返回新 message_id）但不投递；此前「拿到 message_id = 出站已通」是误判，
  那只是受理回执。
- 修法：`poller::send_text` 内部拼 `{client_id}-{millis}-{seq}`。
- 验证：固定 id 不达 / 唯一 id 送达对照实验 + 用户真机确认全部连通；wxprobe 115 全绿。

## 待复验——启动卡死数秒：内层 wasm 插件验签把编译放在主线程

- 根因：`spawn_plugin_scan` 只把清单扫描放后台，`PluginInstance::load` + init 在主线程
  `this.update` 闭包跑 cranelift 全量编译（4.6MB 组件 debug 实测 7.2s，且每插件各建 Engine）。
- 修法：清单扫描后台落位（JS 插件立即可见）；wasm 编译等 `pool.booted` 后交 background_executor，
  结果回主线程只写状态栏；proof 钩子 thread_local → 进程级 RwLock，Engine 进程内共享；新增
  `load_cached`（产物按 wasm sha256 落 cache，原子写留最近 8 份，命中 deserialize+instantiate，
  失配自动重编）。每次启动仍真跑 init 验签门。
- 验证：冷 9.24s(debug) / 0.62s(release) → 热 7–10ms。待真机：启动到可操作无停顿；验签结论
  一律不展示（只进 stderr）。

## 待复验——微信 Bot 回复石沉大海：出站只认桌面选中态 + 投递静默丢 + 横幅泄漏

- 根因三层：① 出站门只认 is_active（桌面选中态），入站固定投 wx_session → 桌面一切换，回复
  永远进不了微信缓冲；② 目标会话没打开（重启后 runtime 不存在）时自愈落空即丢；③ pi 启动
  横幅（带 ANSI）从 assistant 通道混进回复缓冲。
- 修法：出站门对齐入端口径（is_active || wx_session）+ buf_key 双源防串线；轮末冲刷改看缓冲
  来源运行态；`open_session_detached` 后台唤醒（不动桌面注意力）再投递，失败必有微信回执
  （RemoteSend 三态，排队不报错）；出站门只在 agent_running 轮内转发。
- 验证：app 383 全绿（含换源冲刷断言）。待用户：多项目连发每条收到回复、无横幅碎片；切桌面
  项目不影响微信线；异常时 `PI_FLASH_RPC_LOG=1` 抓。

## 待复验——微信 Bot「渠道运行中、扫码成功」却收不到任何消息

- 根因三层：渠道在扫码前被拉起 → 轮询线程一次性捕获旧 bot token（重扫码使旧会话失效，
  getupdates 全 `errcode:-14`）；pump 见渠道活着就跳过装新 token；扫码成功分支不清
  buf/chat_id/user_id/context_token，state 游标嵌旧 bot，重启也救不回。三重不可见（Err 分支
  吞错、面板只看 transport.is_some、传输层失败不落 dump）。
- 修法：扫码成功即 reset_session_fields；QR Done 时 force_restart_if_token_changed（运行中
  token ≠ state 现值则重拉）；`Transport.last_poll_error` 上面板「最近错误」+ warn_once。
  顺手：出站剥 ANSI；wire.rs 扩展名比对 bug（`Path::extension()` 对 `"res.txt"` 永不命中）
  改按文件名后缀。
- 验证：wxprobe 115 / app 382 / pi-link 185 全绿。待用户：更新后重新扫码一次（清磁盘残留旧
  游标）应恢复响应。

## 待复验——固定到任务栏的图标，更新一次就点不开

- 根因：pin 的 lnk 记固定那一刻窗口进程的 exe 绝对路径，而 081 启动链让窗口常年运行在
  `payload-<版本>\pi-flash.exe`；更新落位后 cleanup_older 删旧目录 → pin 目标消失。
- 修法：`services/win_pin.rs` Chrome 三件套——① 开窗前设进程级 AUMID `dev.piflash.app`；
  ② 开始菜单建同 AUMID 稳定入口 lnk → `payload\pi-flash.exe`；③ 启动后台自愈改写在版化载荷
  目录上的坏 pin。
- 验证：`shortcut_roundtrip_with_aumid`（真 COM）+ `pin_matching_scope`（只碰版本化载荷目录
  的 pin，稳定入口/开发 exe/别人的 lnk 不动）；updater 16 测全绿。

## 待复验——思考中插话，整屏消息落到屏幕下半部分

- 根因：打断投递写成无条件 `pager.release()`——钉顶期退役 = 摘掉整屏垫片，`layout_items`
  覆盖条件立刻成立，Bottom 胶水把整块短内容拽到屏底。
- 修法：打断只退役跟尾期锚点（新 `ChatList::release_if_following`）；钉顶期保留锚点，插话追加
  在当前屏内内容之后，长过一屏再交还胶水。
- 验证：`interrupt_while_pinned_keeps_position` + `steer_while_pinned_keeps_anchor`；app 324 全绿。

## 待复验——展开文件夹卡几秒（真凶：git status 无 pathspec 扫整仓 270 万条）

- 根因三层：`git status -uall` 无 pathspec，子目录项目也扫整仓（270 万条 / 414MB / 99.9s）；
  点击文件夹在 UI 线程同步 rebuild_tree（flatten 270 万条下限 7.5s）→ 冻结；porcelain 路径是
  仓库根相对，`cwd.join()` 拼成假路径（徽标失效 + 34 万假祖先目录）；`-z` rename 按 TAB 切
  永不命中。
- 修法（services/git.rs）：status/numstat 加 `-- .` 钉 cwd（99.9s → 0.095s）；rev-parse 定根后
  重定基到 cwd；rename/copy 消费第二条 NUL。行为变化（有意）：文件树/git 面板只显示项目内变更。
- 验证：status_tests 5 例 + app 311+7 / pi-link 166 全绿。跟进项入 bd（在途扫描无取消、flatten
  map 每击重建、load_project_files 死代码、子目录无分支）。
- 备注：测试并发裸 spawn git 撞 0xC0000142，复用 `git_cmd`（CREATE_NO_WINDOW），坑记
  engineering.md。

## 待复验——树/标签切项目后发消息，会话落在旧项目、列表上「消失」

- 根因（双语义分叉）：界面处处显示 ws.cwd，composer 发送却跟 pool.active_key 的 runtime cwd；
  切项目「只改 cwd 不动活跃会话」→ 旧项目草稿继续接收发送 → 新会话绑到旧项目，四个查找去处
  全落空 → 列表隐形。
- 修法（定案「cwd 就是活跃项目，不存在两个语义」）：sync_tree_to_file / tree_proj_menu_pick
  改走完整 switch_project，废除「活跃会话不动」，内容视图保存/恢复；曾加的 send 门（cwd 不一致
  重落草稿）确认是第二语义残渣，已删。
- 验证：app 300 + pi-link 146 全绿；pif-ui 双路径断言（B 中开 A 文件 → cwd=A、view=file 保持；
  树下拉切 B → cwd=B、活跃 draft=B）。

## 待复验——pin/布局/项目记忆集体失忆（真凶：测试夹具写穿真实配置）

- 根因：`purge_app_paths_in_map` 注释写「纯内核」却在 removed>0 时调全局 set_memory；不带
  PI_FLASH_DIR 裸跑 `cargo test -p app` 时夹具 map 整表写进真实 workspace.json（冲掉
  __pins/__last/__ui/__window），后续 save_tree_state 再 merge，每次裸跑复发。
- 修法：内核真纯化（只改传入 map），`purge_app_install_memory` 在 removed>0 时才 set_memory。
- 验证：app 300 全绿。被冲掉的 pins 无备份（原子写 tmp+rename）救不回。

## 已修（2026-10-20）——「重复发送」复发：停止按钮不撤回已排队消息

- 根因：pi 的 Abort 只掐当前 run 不清 followUp/steering 队列；停止后消息留在队列，下一条跑完
  时被 drain 当新轮执行。pi 自带 clear_queue RPC，PF 从未调用，queue_update 事件也被忽略。
- 修法：① pi-link 新增 `Command::ClearQueue`；② abort_stream 后配对发 clear_queue（删乐观气泡
  + 摘 pending_echoes + 原文回填输入框 `SessionEvent::RecallInput`，pi-web parity）；③ 微信
  /停止 同口径；④ queue_update 拆 typed 事件，状态行显示「已排队 N 条」。
- 验证：pi-link 181 全绿；验收交用户。

## 已修（2026-10-17）——agent 工作时排队发送的内容隔几分钟「重复发送」

- 根因：RPC follow_up 不校验 isStreaming；PF 本地猜 agent_running，猜「在跑」而 pi 恰好收尾
  时消息入到空闲 agent 的队列，无人消费，下轮 prompt 跑完才被 drain 冲出。
- 修法（pi-web parity）：`Command::Prompt` 增 `streaming_behavior`（followUp/steer）——排队与
  否由 pi 以自身 isStreaming 原子判定，失败方向安全；笔记归档 `pi-rpc.md`。

## 待复验——切项目后点「＋ 新建会话」直接崩（进程消失）

- 根因：① switch_project 自抄换 cwd 重挂流程，漏刷 Pin 收藏夹（按项目存，只有 remount_workspace
  刷）→ flatten_pins 对旧 pins `strip_prefix().unwrap()` panic，落在鼠标事件回调（不可 unwind）
  → 进程 abort；② is_abs_path_ignored 的 Some 分支比 RepoExclude 分支少一句归属检查。
- 修法：switch_project 改调 remount_workspace（与池命中同一条）；Some 分支补齐跨 base 交上一层。
- 验证：`flatten_pins_survives_out_of_project_pin`（两向锁）+ app 296 / pi-link 142 全绿；pif-ui
  走原崩溃序列零 panic。
- 定位手法（可复用）：Windows 事件日志 0xc0000409 + 数据 7 = panic 撞不可 unwind 边界（栈溢出
  是 0xc00000fd）；`%LOCALAPPDATA%\CrashDumps` 转储 + pdbtool（pdb crate）翻函数名；按栈上
  panic 字面量用 debug 构建复现拿行号。

## 待复验——每次启动都从 payload 目录启动（未注册的安装目录常驻项目列表）

- 根因三层：交接 spawn 不设 cwd → 子进程继承老 payload 目录；exe 目录探针只认「cwd == exe
  目录」，新 exe 在 payload-<版本> 不命中 → 老 payload 被当用户项目 + 经 set_last_open 写脏
  __last（自我延续）；projects.rs::purge_app_paths 写了没人调，存量不洗。
- 修法：交接 spawn 在「cwd == 自己 exe 目录」（双击签名）时显式给 current_dir（终端/薄壳场景
  保持继承）；新纯函数 `workspace::cwd_inside_install_root`（安装根内一律按 exe 目录处理，
  PI_FLASH_CWD 显式指定不参与放宽）；boot 第一步洗注册表；set_last_open 等四个写入口拒记安装
  根内路径。顺带：cleanup 测试版本写死 0.1.11 改相对 current_version() 推导。
- 验证：app 295 + pi-link 142 全绿。待真机：双击老 payload 交接后落上次工作区、项目列表无
  payload 条目。

## 待复验——用户消息鼠标穿透（滚轮同帧连动会话列表）

- 根因：v63-3 为修操作栏悬停删掉用户气泡 occlude()；gpui 滚轮派发给所有命中 hitbox（含祖先）
  → 长用户气泡内滚时气泡与外层会话列表同帧同滚。
- 修法：turn.rs ububble 挂 on_scroll_wheel 边界拦截（滚动前已在对应边界才放行外层；未溢出恒
  放行）。gpui 坑（详见 gpui.md）：offset 域 [-max, 0]；同 div 内滚动 handler 注册晚于自定义
  listener 且 bubble 相位逆序 → 判边界须减回本次 delta（EPS 1px）。
- 验证：app 293 全绿。待真机：滚长用户气泡，气泡独滚、到边界再滚一档列表接续。

## 待复验——editor 点击可见行，该行瞬移屏幕底部

- 根因（vendor input/state.rs）：滚轮路径钳 scroll_size（含垫底允许 overscroll），点击 →
  move_to → scroll_to 在光标已完整可见时仍无条件把 offset 重钳回纯内容范围并写
  deferred_scroll_offset → 停在垫底区的滚动位被拽回内容底。
- 修法：内容钳制收进上下两支越界分支（只作用于「解」）；函数尾只在 scroll_offset 变化才写
  deferred + notify——可见时滚动位一字节不动。

## 待复验——editor 打开 minified js 卡到死（单行 94 万字符）

- 根因（vendor，两条都在每帧路径）：① highlight_lines 按逻辑行取整行送 SyntaxHighlighter::
  styles → 94 万字符 ~20 万 capture 进 unique_styles 二重扫；② layout_lines 对可见行全部软
  换行段逐段 shape_line + 物化整段 String（~6000 段 / 941KB 每帧）。
- 修法（视口字节窗）：prepaint 按段 y 算与视口相交的字节窗（上下各留 2 行），highlight_lines
  逐行查询先与窗求交，layout_lines 窗外段垫等 len 空 ShapedLine（段数/字节 len/y 槽位全保留），
  paint 跳空 text 段。
- 回归（首轮实测抓到）：窗裁剪后样式区间不再从可见文本 0 起连续平铺，消费端按顺序吃 run.len
  不读 range.start → 边界整体错位，开 py 崩 char boundary（纯 ASCII 只是悄悄错位）。修法：窗前/
  窗后各补默认样式 filler 恢复平铺不变式（同时纠正滚动后高亮整体错位的暗病）。顺带钳
  calculate_visible_range 非等高路径初始值（软换行行数当逻辑行号，pagedown 到深处 panic）。
- 验证：稳态 app.info 45-70ms（修复前分钟级假死）；94 万字符行整屏换行 + 高亮正常；pagedown×45
  /方向键/六文件连开全程存活。已知边界：超长行内打字仍整行 rewrap（~0.1-0.5s/键）；首开全量
  wrap + parse 一次 1-2s（debug）。

## 待复验——外部改动不同步：cwd 外文件根本没挂监听

- 根因：attach_fs_watch 只递归 watch cwd——工作区外打开的文件（Obsidian 库、pick_open_files、
  远程 FILE_OPEN）从未被监听。
- 修法（对齐 Zed single-file worktree）：watcher::watch_dir（非递归）+ retain_file_watches 为
  cwd 外打开文件按父目录挂监听（共享同一 fs 信号通道），打开/关闭/切项目后重算；目录级信号统一
  交 check_external_file_changes 按 (mtime,len) 复查。顺带对齐 zed 两语义：vendored
  `InputState::reload_value`（整文换新保留光标偏移与滚动）；markdown 预览块数变化记锚还原，有
  未保存修改时预览取缓冲区文本。
- 验证：app 181 全绿 + pif-ui 隔离实例（外部写 → watch_roots 命中同步；脏缓冲 + 外部写 →
  conflict="Changed" 两按钮；file.reload 后清；预览显本地缓冲；删文件 → Deleted）。待真机：
  Obsidian 改文件自动跟随，源码态光标与阅读滚动保留。

## 待复验——023 fileView：面包屑 `D:\` 拆两段 + 预览/编辑器都不自动回行

- 根因：① Windows 上 Prefix("D:") 与 RootDir 是两个组件 → 两段 crumb，且累积路径固定从
  chat.cwd join → 工作区外文件每段拼成假路径，面包屑兄弟文件菜单指向错目录；② 预览内容段
  flex_span 的自动最小尺寸 = min-content（gpui 在 MinContent 下 wrap_width=None）→ 正文永远
  拿不到可用宽；③ ensure_file_editor 显式 soft_wrap(false)。
- 修法：抽 `content::breadcrumb_segments(cwd, path)`——前缀组件与紧随 RootDir 合成一段，累积
  路径真累积（渲染与自动化快照共用）；内容段 `.min_w(px(0.))`；编辑器 soft_wrap(true)。取舍：
  全部语言开 soft wrap，超长单行慢一点换可读性。
- 验证：app 179 全绿 + 3 条分段单测 + pif-ui 快照与截图（源码/预览都折行）。

## 待复验——新会话页「模型 ∨」弹窗列表空（no models match）

- 根因：`Chat::filtered_models` 直读 models_by_cwd（unwrap_or(&[]) 无回落），目录口径早已改成
  「进程答案优先、回落全局清单」的 `Chat::catalog_for`，只此弹窗漏改；触发 = 该 cwd 无活进程
  答过 get_available_models（切项目建惰性无进程草稿，借不到 runtime 就什么都不发，设计如此）。
- 修法：改走 catalog_for 与其余入口同源。顺带补自动化口 `model.picker_open` + dialogs 面
  `model_rows`（此前零自动化入口故漏测）。
- 验证：pif-ui 对拍——根因版 0 行 / 修复版 5 行。

## 待复验——文件树 / 设置·可用模型列表「滚不动」

- 根因：`ui::vlist` 每帧 `UniformListScrollHandle::new()`。gpui 契约是句柄存在视图里、滚动位
  在句柄内部——每帧新建 = 滚轮写进已丢弃句柄 → 下一帧以 0 重画（thumb 永远钉顶端）。三个
  调用点同源全中。
- 修法：句柄表收进 vlist——`scroll_handle(id)` 按 id 复用（thread_local HashMap；本 app 单窗口
  单线程渲染，等价「挂在视图上」）。约束：同一 id 同屏只出现一次。
- 验证：app 115 全绿（含「滚轮 → 下一帧仍按新偏移构建行」真布局锁；改回 new() 立刻红）。
- 附带：设置页滚列表时页面同滚（gpui 滚轮派发祖先 hitbox），要「内滚到底才带动外层」需给 vlist
  加拦截，等手感反馈再定。

## 待复验——操作栏「复制」图标不显示

- 根因：copy.svg 在磁盘但没写进 `assets!()` 列表 → Assets::load 返回 None → gpui 对取不到的
  svg 静默画空白（不报错不 warn）。用户行与 agent 轮两处同源。
- 修法：补登记 + `assets::tests::every_icon_file_and_call_site_is_registered` 双向锁（磁盘 svg
  全登记 + 源码每个 icon("x") 有资产；旧测试只遍历宏列表正是漏网盲区）。

## 待复验——用户消息操作栏悬停不显影

- 根因：用户气泡挂 occlude() → gpui hit_test 命中即 break，比它先插入的全部祖先 hitbox（含挂
  行级 on_hover 的 msgrow）连 ids 都进不去 → is_hovered 恒 false；连带滚轮在气泡上失效。
- 修法：删 occlude()；两段内联 on_hover 合并 `messages::bar_hover_wired` 唯一接线；字段并
  `Chat.bar_hover`（用户行 = msg_ix，agent 轮 = 轮首 msg_ix，永不撞车）。
- 验证：app 90 全绿（bar_hover_hit_test 正反锁——不遮挡必须触发、遮挡必须被吞）。

## 待复验——发言钉顶 / 回复占满整屏（一批滚屏修复）

- 顶出屏幕不跟尾：快流下 sync 第 5 步把轮首条目打回 Unmeasured，每事件翻转钉顶/跟尾判定 →
  第 5 步避开轮首条目；下边距 135→150。
- 锚点被胶水夺位（用户消息消失）：钉顶垫片在「锚下填不满视口」时被 gpui 丢逻辑位改贴底 →
  垫片改常数（一整屏内容区高），覆盖条件恒不成立。
- 流式第②步就折叠：改事件驱动 agent_running 判「工作中」（原 get_state.is_streaming 过期快照，
  一轮内无人重拉恒 false）。
- 内容形状一变锚点错位：take_frame_sync 增每帧垫片自检。
- 发送后不在屏顶/整片白：垫片公式漏减列表上下内边距（22+135=157）。
- 流式增长往屏顶漂：bounds_for_item 对逻辑位之前条目返回 None → 垫片只结算一次。
- 轮末从屏顶跳屏底：轮末不退锚点，只让垫片收敛（pi-web 同）。
- 细节见 progress.md「v59 滚屏三修」。

## 0.1.0（全部修复于 0.1.1）

导航选中项目后对话列表不显示；新建页空态；文件浏览器默认导航栏半高；输入框不聚焦/思考强度
误循环切换/工具配置、压缩、声音按钮无反应；右侧面板宽度可拖 + 多标签（pi-web parity）；
顶部命令面板未实现。

## 其它已修复（本轮小项）

- 首个 token 前无等待动画：phase_waiting 兼任回显去重标记被 pi 回显清零 → 拆 `pending_echo`
  只做去重，phase_waiting 仅由 assistant 事件/agent_settled/prompt 失败/Esc 清零；补模型名 +
  「正在思考…」循环；shimmer 骨架条按用户要求删除（设计 §13 作废）。
- 中文输入第二个字崩（0xc0000409）：IME 组合路径把 char 下标当字节下标 → 改
  `utf16_to_byte_offset` + `safe_replace_range` 夹取对齐 + from_utf16_lossy。遗留：composer 是
  全项目唯一手写输入框（无选区/方向键/粘贴），根治 = 换 `ui::TextInput`。

## 2026-10-05「新分支」按钮

- 点了没反应：serde_json 默认递归上限 128 丢嵌套整行 → `json::parse_value` unbounded + 16MB
  栈 reader 线程；长会话 pi 自身 get_tree 爆栈 → 新增 get_entries 扁平链回溯算锚点（pi-web
  同源）；fork 进程内 rebind 未跟随 → follow_session_file 换 file/pool key。
- 入口反 + 克隆重名：入口从用户消息栏移到 agent 轮操作栏（fork 只能 before，分支点 = 该轮之后）；
  分支名截原名前 20 字。
- 等待回复时滚一下跳底、之后滚轮失灵：任何非贴底滚动被判用户主动离底 → 退役锚点摘垫片 → 内容
  短于视口后余量归零。修法：free_scroll 软状态（只暂停重钉与跟尾翻转，锚点/垫片保留，滚回贴底
  自动恢复）+ vendor elements/list.rs 滚轮只应用本事件 delta 与真实行高（**升级 vendor 需重套**）。

## subagent 建设期备忘（2026-10-19 各批合并，非 bug）

- 内置 profile = explorer + general-purpose（dsh 无出厂内置，对齐 ZCode；不写 tools = 继承全部，
  maxTurns 0 = 无软限）；profile.ts 与 `pi-link/agents.rs` 两侧逐字同步
  （`builtins_match_extension_docs` 对账）——**`BUILTIN_PROFILE_DOCS` 双份必须同步**。
- 委派语义（2026-10-19 拍板）：不点名 = 裸委派兜底（全继承），不设默认 profile；委派清单
  （id+description+tools）进工具描述，另带「When to delegate」政策段（对照 ZCode：任务匹配
  profile、或需横扫大量文件只要结论 → 委派并继续主线、不自己重跑委派过的检索；单一已知文件
  直接动手）；executionMode parallel 同帧真并行；容量池默认 8（DEPTH_LIMIT 错误反馈兜底，
  「深度到顶提示」未做）。
- 面板：行两行版式（标题+状态 / profile·多久前 + ⏱时长·tokens），头部聚合、运行中排前；整行
  点击开子会话；descriptor.usage 可选落盘（老数据显示 "—"）；常驻轮询开 1s / 关 5s（polling
  守卫防重复 spawn，poll 内比对 rt file 自愈换会话）。
- subagent 总开关：`<PF数据目录>/dev-flags.json` 的 `{"subagents": true}` 才开，env
  `PI_FLASH_SUBAGENTS=1/0` 最高优先，改动重启生效；关态 7 处生效点（不注入 -e / --tools 不点名
  agent / 设置页隐藏 / 按钮隐藏 / 不建面板缓存零 IO 等）。
- dev-flags 机制泛化：pi-link `dev_flags.rs` KNOWN 键注册表 + enabled(key)（env > 文件 > false，
  坏文件 fail-closed）；app `dev_flag_enabled`（OnceLock 快照，渲染安全）；接入约定 = KNOWN 加键
  + 消费点调 dev_flag_enabled。
- 回归注意（M4-M6 坑）：agent-workspace.sh 克隆 remote 指错（set-url 重指）；Settings 内置态
  TextInput 无禁用 API → 只读态渲染静态文本；parse_iso_secs 时区折算；面板流式扫父会话 JSONL
  半行保护；perf_probe 需 `tmp/perf/perf-80k.txt`（gitignore，新克隆从黄金仓拷，缺文件报
  NotFound 属环境非代码）。遗留：pif-ui 自动化断言未写。

## pif-secret.exe 未随包发布，秘钥保存 dev 自嗨（2026-11-01 修）

- 现象：auth.json 的 `!cmd` 标记指向 dev 绝对路径（pi 无 keyring 能力，042-1 退役 env 注入后
  `!cmd` 是唯一桥），release.sh 组装清单漏复制 exe → 用户机上取 key 全灭，PF 不可用。
- 修法：release.sh 补 `cp target/release/pif-secret.exe dist/pi-flash/`（缺源硬失败），包内与
  pi-flash.exe 同目录（绿色包根 / npm payload / payload-<版本> 自动带上）；RELEASE.md §3.1/§5、
  080 §0 同步；用例 test/042-1/pif-secret-packaging/。dev 路径条目是本机 debug 构建正常产物，
  无需迁移。

## pif-secret 绝对路径绑定版本化载荷目录，升级即崩（0.4.0→0.4.1 实录，2026-11-04 修）

- 根因：上一修让 exe 进 `payload-<版本>\`，但 auth.json 标记写该目录绝对路径 → 升级后
  cleanup_older 清旧目录 → pi `!` 命令取 key 全灭 → 全部 provider「未配置」。路径随版本化载荷
  目录必然漂移，写死任何安装树内路径都逃不掉。
- 修法（051 扩展，治本 = 标记去路径化）：pif-secret 落用户级稳定目录 `%LOCALAPPDATA%\pi-flash\bin`
  并加入 HKCU Path（判重追加 / 保 REG_EXPAND_SZ / WM_SETTINGCHANGE 广播 / 本进程同步，全幂等）；
  标记改写零路径裸命令 `!pif-secret get <provider>`（pi 的 `!` 整串交 bash -c/execSync，走 PATH
  解析）。启动最前（抢先升级 cleanup）+ 更新落位后 + 保存 key 三处 ensure_installed；存量路径
  形态标记（含用户手改的）启动自动迁裸形态，oauth/用户明文/用户引用零触碰，不可读拒写；PATH
  模式失败回落老绝对路径行为。用例 test/042-1/marker-pathless/。

## agent 流式时整 app 卡死、切快捷面板冻结（v0.4.3 实录，2026-11-06 修）

- 根因：非死锁，主线程饱和——三个增量条件同期成立：①插件钩子机制（每 notify broadcast tick +
  tick_session「notified==0 重试」+ 徽标/fn_bar 渲染路径同步插件调用 + 面板首拍 reset）；
  ②用户装 quick-panel（wasm 插件）后扇出有了真实对象；③快模型（116 t/s）把 delta 间隔压到
  8.6ms。
- 修复（pi-flash-yftj）：泵批处理 30ms（2000 唤醒→13）+ notify 33ms 节流 + markdown 流式尾
  150ms 解析门 / est 250ms 门 + 扇出就绪检查禁隐式首装 + tick_session 派发即记账（派发风暴
  87→2）+ wasm 一律后台装载（根除产物失效 UI 线程重编 / compile_lock 等锁）。
- 台架 test/030-流式卡死/（S1 极限洪水可交互 + S2 批处理零丢失），实测对比见 case.md。

## subagent 错误集合
1、并发数量上限和下限应该写入内部配置，

## settings 界面错误集合
1、鼠标点击穿透，点击配置项时，会点击到会话区的链接从而报错
2、

## 重启后会话 tab 仍显示「新会话」（2026-11-07 修，复发两次）

- 根因：tab 标题取值顺序快照 `pool.tab_labels` 优先于活值 `session_title_of`，兜底词「新会话」
  三路共用；启动恢复链（spawn_boot_restore → open_session → switch_to）在 psp 分组/会话清单
  未建时把兜底词写进快照成毒值，之后不发消息永不触发 ListDirty → refresh_sessions，毒值钉死
  tab。「改了好几遍仍复发」= 每遍只堵了一条写入路。
- 修复（三处叠加根治）：① 毒源——switch_to 不再把兜底词入快照，真值取不到留空（与 sync 的
  「兜底词永不入快照」同规）；② 启动治愈——restore 分组建好后立刻 sync_primary_tab_label，
  不等 ListDirty；③ 素材缺口——psp_title_of 加 pool.sessions 兜底。标题整形收拢
  `tab_title` 三路同源同形。

## 问卷卡末题回车确认即崩（2026-11-08 修）

- 根因：on_submit 回调在 ask_input 自己的 update 里发起（gpui.md 双租节），末题直连
  ask_confirm → `ask_input.read(cx)` 撞 double_lease_panic，panic 落 window_procedure
  （cannot unwind）→ 进程消失。前几题不崩 = 焦点不在框上（ask_switch_tab 同 read/set_value
  同族隐患）。
- 修复：boot.rs on_submit 接线改 `cx.defer`——回调里只调度，ask_enter 整段推迟到本轮 flush
  尾部（栈上实体已归还）。同族已排除：ext_input 的 ext_respond 用回调参数，不摸实体。

## 输入卡顿：repeat() 动画满帧率整窗重绘烧满一核（2026-11-09 修，待构建验收）

- 根因：打字每键付「整窗每帧渲染 13-18ms(debug)」的钱，卡不卡取决于屏上有没有别的帧源在抢
  ——repeat() 动画不限帧，屏上有 spinner 时用户实例单核 102%（~66fps × 15ms ≈ 100%）；无
  spinner 隔离实例仅 2.3%。「subagent 后开始卡」= agent 运行窗口变长，spinner/点阵常驻变长。
  机制见 gpui.md「repeat() 动画不限帧」节。
- 修复：vendored gpui `Animation` 新增 `.with_fps(n)`（min_frame_interval + 每窗口共享 pending
  续帧）；四个 repeat() 站点全部上限（spark/spinner=24、working_dots=15、nav 计时=2）。预期
  spinner 常驻期单核 102% → ~35%(debug) / ~6%(release)。
- 遗留：①「pump 批处理耗时 699ms」单批卡顿（大会话后 4s 零帧同期出现）待查；② user_row
  loader 900ms oneshot 转一圈就停——已修（11-10）改走 `ui::spinner` + with_fps(24)；③
  PI_FLASH_SUBAGENTS=0 设了但启动日志 subagents=on（dev_flags env 解析待查）；④ debug 每帧
  成本回退已证实（基线 e145a3d8 空闲帧 avg 10.5ms → HEAD 18.5ms，+76%）——动画限帧之外每帧
  本身的增量（拆分/spark 时代引入）待另行拆解定位。

## 问卷卡输入框内 Enter 一次连跳两题（2026-11-09 修，待构建验收）

- 根因：gpui-component 的 InputState 对 Enter/数字键不停传播（vendor 仅 otp_input
  stop_propagation），冒泡到问卷卡浮层 on_key_down → 同帧两条路径各调一次 ask_enter
  （① 输入框 on_submit→defer；② 浮层监听器同步）→ Q1 直落 Q3。同族隐患：自定义答案里打
  数字会顺带选中第 N 个选项（同一冒泡路径）。
- 修复：`render_ask_card` 的 keys 闭包先查 `ask_input.focus_handle_in(cx).is_focused(window)`，
  输入框持股焦点时整卡键盘处理让位（Enter 归输入框，数字只作正文）。

## 问卷卡答完三题后又弹「坏掉的窗口」（2026-11-09 修，待构建验收）

- 现象：整卷答完、卡收走后又冒 legacy 扩展弹窗（题面 + 选项 + "Type your answer:"）。ext-ui.log
  实锤：这一卷发了 4 个阻塞请求（select → input → select → input），题只有 3 道；前 3 个按下标
  正确应答，第 4 个到时 next_request 已到卷尾 → 重解析 title mismatch (idx=0) → 落 legacy。
- 根因：路由假设「一题一请求、按序自增」，与 rpiv-ask-user-question 的 rpc-fallback 实际时序
  不符——多选走 ui.input、单选走 ui.select，自定义答案还追一条 ui.input，追问数不与题数对齐。
- 修复：请求 ↔ 题目改按 title 含题面匹配（`AskCard::match_question`），不按下标/条数；卷标识 =
  tool_call id（新 id = 新卷，旧卡先 cancelled 再作废）；已作答题的请求静默应答，未作答才挂起
  弹卡；确认后卡不销毁（open=false、pending=None），专为接住最后那条追问；`AskCard::finished()`
  （全答完 + 无挂起）让自动化快照 `dialogs.ask` 维持旧语义。新增 `ask: reply id=… q=… value=…`
  留痕，追问再现时可直接对账。

## 子代理：点第一个委派卡打开窗口错误，挤掉主会话 tab（2026-11-10 修，pi-flash-iqkg）

- 根因①（架构）：子代理卡点击明知是子会话却走通用 open_session，在打开层靠文件嗅探反推身份、
  判否即按普通会话开 → 抢占固定段第一位、污染侧栏/last-open。嗅探函数还有确定性 bug 放大触发
  面：头部 64KB 窗口整段 UTF-8 解码，窗口末字节切在多字节字符中间即整体失败 → 误判普通会话
  （真机实锤：某子代理文件 65536 字节正落汉字内部，每次点必挤占）。委派后立刻点击（pi 懒持久化，
  首条消息前文件不存在）同病。
- 修法：新增 `Chat::open_subagent_session_window` 作观察窗唯一入口（子代理卡点击 / 观察窗 tab
  激活 / 会话搜索跳转直达）——身份由调用意图携带，不反推；open_session 只剩防呆闸（marker 命中
  即改道观察窗，只拦自动化 op 等未知来源，防两个 pi 同抢一份转录）；持久化指针守卫收拢
  `startup::restorable_session`（启动恢复 + switch_project 共用，堵历史污染 last-open 的姊妹雷）；
  嗅探改逐行字节解析（`is_subagent_head`，截断行自然跳过）带回归测试；SearchHit 携带 subagent
  标记供搜索跳转路由。
- 验证：pi-link 278 / app 474 全绿（含多字节截断回归）。真机口径：逐个点 4 个委派卡 → 全部开
  自由区末尾任务名 tab，固定段主会话恒在；会话查询命中子会话 → 跳只读观察窗；重启不恢复成子会话。

## 子代理：每次启动都出现已删除的 explorer（2026-11-10 修，pi-flash-7zdl）

- 根因：96bb7e82（11-06 profile 目录搬家）实现时顺手加的「启动迁移」非设计项——每次启动把旧
  目录 `~/.pi/agent/agents/*.md` copy 进 `<PF数据目录>/agents/`，只凭「新目录有无同名文件」判断
  迁过，旧文件又永不清除：新目录删掉的条目下次启动被当「还没迁」拷回来。
- 修法：整段删除迁移（legacy_agents_dir / migrate_legacy_agents / migrate_agents_from +
  startup/pumps.rs 启动调用 + 2 条迁移测试），不改造成带标记的一次性迁移；旧目录从此不读不迁。
- 验收：盘上被拷回的 explorer 删一次 → 重启不再出现。
