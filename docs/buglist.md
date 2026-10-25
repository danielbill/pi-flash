## 待验证（本轮修复）——跨项目开文件顶掉活跃会话：switch_project 新增「进行中会话保留」规则

用户口径：在 pi-flash 里聊天，快捷面板打开 pi-flash-plugins 的 md 文件，原
会话标签变成「新会话」（2026-12-06）。

- ✅ 根因：`sync_tree_to_file` 对已知其它项目的文件走 `switch_project` 完整
  切项目——这本身是对的（树定位刚需：快捷面板/搜索/消息路径开文件都要
  能在树里定位到文件所在项目）；错在 `switch_project` 尾部：新项目无
  last-open 时无条件 `new_session`——「项目切了」被当成「要新开会话」，
  进行中的对话被空白草稿顶出会话标签。
- ✅ 第一版修法（已回滚）：跨项目文件不切项目只开查看 tab——错：树定位
  没了，快捷面板开文件定位不到项目位置。
- ✅ 定案修法（2026-12-06 用户拍板）：`switch_project` 尾部落**保留规则**
  ——新项目有 last-open → 照旧恢复；没有 → 当前会话若**进行中**（已绑定
  会话文件、非空白草稿）则原地保留：树/侧栏已随新 cwd 重挂，注意力与
  composer 仍连原会话，点会话标签即回原对话，切回原项目/点会话行也能靠
  last-open 归位；只有当前本身是空白草稿才落新草稿——空白换空白无感知，
  且发送目标落到可见的新项目，不重演 2026-10-18「会话落错项目列表隐形」
  （那次正是空白草稿跨项目发消息，新会话落进启动时旧项目的缓存组；本次
  分叉只发生在有真会话挂着时，消息继续落原会话，其项目组列表里有它）。
  顺手：`TreeProjMenu` 结构体注释原停在 10-18 之前的「活跃会话不动」旧
  语义，已改为 switch_project 现行口径。
- ✅ 验证：app 398 + pi-link 197 全绿。**待复验**：聊天中快捷面板开其它
  项目的文件 → 树定位到新项目、会话标签仍是原对话；新项目有 last-open
  时照旧恢复；psp ＋ 新建会话（new_session_in）不受影响。

## 待验证（本轮修复）——微信 Bot 开关/扫码成功后主进程卡死最长 90 秒：Transport drop 在 UI 线程 join 长轮询

用户口径：启动后点「重新获取」二维码或拨启用开关，主进程（整只窗口）卡住。

- ✅ 根因：`Transport::drop` 逐个 `join` poll/send 线程；`stop` 只在长轮询两拍
  之间检查，poll 线程大概率正阻塞在 `getupdates` 的 90s 服务端挂住里
  （`wire.rs POLL_TIMEOUT=90s`）——join 在 UI 线程上最坏等满 90s。触发路径
  ①`set_enabled(false)` → `transport = None`；②扫码成功后泵每拍
  `force_restart_if_token_changed` 检测 token 变化 → `transport = None`
  （`mod.rs:862`，泵在 UI 线程 200ms 一拍）。原注释已自认
  「健康长轮询里最坏等一个 90s 超时」。
- ✅ 修法：drop 改「信号 + 自杀」——置 stop 后 `handles.clear()` detach
  （poll 这一拍收完见 stop 退出 / inbound 断开 break；send ≤100ms 见 stop /
  Disconnected 退出），UI 不再等在途长轮询；游标未 ack，下个轮询者重收这批
  （重复而非丢失，与既有崩溃窗口同方向）。锁释放不受影响（心跳本就 channel 秒退）。
  顺手：`force_restart_if_token_changed` 的 state 读按 1s 节流（原 200ms 一拍
  同步文件读，杀软下抖 UI）。
- ✅ 验证：`cargo test -p wxprobe` 117 全绿；`cargo test -p app --bin pi-flash`
  383 全绿。**待真机复验**：扫码成功瞬间与开关「关」不再冻结；关掉后 90s 内
  重开，新旧渠道交接期间消息不丢（重复可容忍）。

## 待验证（本轮修复）——微信 Bot 出站「受理不投递」：client_id 固定复用被 iLink 去重吞掉

用户口径：消息进了 PF、模型也回复了、`sendmessage` 返回 message_id——手机永远
收不到；CLI 直发探针同样石沉大海（「连这个都没搞定」）。

- ✅ 根因：`sendmessage` 的 `client_id` 在协议里是 **unique_id（每条消息唯一）**
  （epiral/weixin-bot PROTOCOL.md），PF 却用 state 里写死的稳定实例 id，跨消息、
  跨 bot 会话长期复用——iLink 受理（返回新 message_id）但不投递。此前「拿到
  message_id = 出站已通」的验证结论是错的：message_id 只是受理回执。
- ✅ 修法：`poller::send_text` 内部把 client_id 拼成
  `{client_id}-{millis}-{seq}`（毫秒 + 进程内序列号），app send 线程与 CLI
  同走此路径。对照实验钉死：同 ctx、同分钟——固定 client_id 不达，唯一
  client_id 送达（含 from_user_id 照填 bot id 的变体，无需留空）；getconfig
  同 ctx 返回 ret:0 + typing_ticket，排除 token/会话过期。
- ✅ 验证：用户真机确认「已经全部连通」。`cargo test -p wxprobe` 115 全绿。

## 待验证（本轮修复）——启动卡死数秒：内层 wasm 插件验签把 wasmtime 编译放在主线程

用户口径：启动页之后主窗口僵住数秒才活。

- ✅ 根因：`pf_plugin::spawn_plugin_scan` 只把清单扫描放进后台，内层 wasm 插件的
  `PluginInstance::load` + `init` 却在**主线程 `this.update` 闭包**里跑——
  `Component::from_file` 是 cranelift 全量编译（本机 4.6 MB 组件 debug 实测
  **7.2 s**；旧代码每个插件还各建一个 `Engine`）。同闭包末尾才 `plugins_set`，
  状态栏插件 tab 也被一起拖到秒级之后。
- ✅ 修法：拆两段——① 清单扫描后台落位，JS 插件条目立即可见；② wasm 验签等揭幕
  （`pool.booted`）后交 `background_executor` 后台线程，编译/实例化/init 全在
  后台，结果回主线程只写状态栏。配套 `pf-plugin-host`：proof 钩子
  `thread_local` → 进程级 `RwLock`（否则后台线程 init 时插件回调
  `get_host_proof` 取不到材料 → 正常插件被误判拒载）；`Engine` 进程内共享。
- ✅ 第二刀（为什么还要反复编）：wasm 是字节码，`Component::from_file` 每次都做
  cranelift 全量编译，产物只活在进程内存——**第二次启动照样重编**。新增
  `PluginInstance::load_cached`：产物按 wasm sha256 落
  `<数据目录>/cache/pf-plugin/<sha>.cwasm`（原子写、只留最近 8 份），命中 =
  `deserialize_file`（mmap）+ instantiate + init；产物自带 wasmtime 版本/配置
  校验，失配自动重编；安装/启用即后台预热，首次启动也命中。**语义不变——每次
  启动仍真跑 init 验签门**。
- ✅ 验证：`cargo check -p app` 通过；本机 4.6 MB 组件实测——冷 9.24 s(debug) /
  0.62 s(release) → **热 7–10 ms（release 三次，含实例化+init）**，init 单独
  0.1–0.2 ms，产物 1.4 MB。**待真机复验**：启动到可操作无停顿；**验签结论
  一律不展示**（不弹提示、不写状态栏，只进 stderr）。

## 待验证（本轮修复）——微信 Bot 回复石沉大海：出站只认桌面选中态 + 投递静默丢 + 启动横幅泄漏

用户口径：换绑修复后复测，发「刚刚」只回了一句「MCP: 1 server enabled」（不是回复），
再发「？？？？啥意思！」就没反应了。多项目环境下桌面切过会话。

- ✅ 根因（三层，dump + 会话文件 + 源码互证）：① 出站回推门 `pool.rs` 只认
  `is_active`（桌面选中态），入站却固定投 `wx_session`——桌面一切换，模型的
  回复（会话文件里完整躺着）永远进不了微信缓冲，连 ToolCallStart 强制发送点
  都没触发过一次；② 目标会话没打开（重启后 runtime 不存在）时 `send_wx`
  自愈落空即丢、`send_remote_text` 失败只打 stderr——「刚刚」没进任何 pi；
  ③ pi 启动横幅（带 ANSI 色码的轮外文本）从 assistant 通道混进回复缓冲被冲出。
- ✅ 修法：出站门对齐入端口径（`is_active || wx_session` 放行），缓冲加
  `buf_key` 双源防串线，泵的轮末冲刷改看缓冲来源的运行态（防桌面流式文本被
  拍成碎片）；会话文件还在 → `open_session_detached` 后台唤醒（不动桌面注意
  力）再投递，失败必有微信回执（`RemoteSend` 三态，排队不报错）；出站门只在
  发送轮内（`agent_running`）转发，轮外文本不进缓冲。
- ✅ 验证：`cargo test -p app --bin pi-flash` 383 全绿（新增换源冲刷断言）；
  `cargo check -p app` 无新 warning。**待用户复验**：多项目环境微信连发多条，
  每条都收到回复、无横幅碎片；切桌面项目不影响微信线；如再出现异常碎片，
  用 `PI_FLASH_RPC_LOG=1` 复跑抓横幅走的 RPC 事件。

## 待验证（本轮修复）——微信 Bot「渠道运行中、扫码成功」却收不到任何消息

用户口径：面板显示渠道运行中、扫码绑定成功（bot_id = ce6a2ed…），在微信里发
消息毫无反应；且面板同时显示「绑定： bot_id」与「未绑定」（后者是 `/bind`
会话授权，与收发无关，非本次原因）。

- ✅ 根因（三层，dump + 状态文件 + 源码互证）：渠道在扫码**前**被开关拉起，
  轮询线程 spawn 时一次性捕获旧 bot token；用户重扫码（每次注册会让同账号旧
  bot 会话在服务端失效）后 getupdates 全部 `errcode:-14 session timeout`；扫码
  成功后 pump 的 `wants_start && !is_running()` 见渠道活着就跳过，新 token 永远
  装不进去；`register.rs` 成功分支不清 `buf/chat_id/user_id/context_token`，
  state 游标里嵌的还是旧 bot（`2b296a82465@im.bot`），重启也救不回来。三重
  不可见（Err 分支 `let _ = e;`、面板只看 `transport.is_some()`、传输层失败连
  dump 都不落）让它成了「运行中却没反应」的哑弹。
- ✅ 修法：`register.rs` 扫码成功即 `reset_session_fields` 清旧 bot 路由字段；
  app 泵 QR Done 时 `force_restart_if_token_changed`（运行中渠道 token ≠ state
  现值 → drop 重拉）；`Transport.last_poll_error` 每拍同步面板「最近错误：…」
  + `warn_once` stderr。顺手修两枚次生 bug：出站文本剥 ANSI 色码
  （`poller::strip_ansi`，真机色码直达微信）；`wire.rs` 的 `next_seq`/`prune_dump`
  按 `Path::extension()` 比对 `"res.txt"` 永不命中（实际是 `txt`），Wire 从 001
  起互相覆盖、清理永不执行，改按文件名后缀。
- ✅ 验证：`cargo test -p wxprobe` 115 全绿（新增 reset 清场 / next_seq 编号 /
  prune 保留 / strip_ansi ×3）；`cargo test -p app --bin pi-flash` 382 全绿；
  `cargo test -p pi-link` 185 全绿。出站半程已真机验证（sendmessage 拿到
  message_id）。**待用户复验**：更新后重新扫码一次（清掉磁盘残留旧游标），
  微信发消息应恢复响应。

## 待验证（本轮修复）——固定到任务栏的图标，更新一次就点不开（「找不到 pi-flash」）

用户口径：PF 启动后固定到任务栏，好几次更新之后点固定图标就找不到 PF，只剩
terminal 窗口能启动（npm 薄壳 `pi-flash`）。

- ✅ 根因：任务栏固定 lnk 记录的是**固定那一刻**窗口进程的 exe 绝对路径，而
  081 启动链（薄壳挑最高版本 / 点击重启更新 / 启动交接）让窗口常年运行在
  `payload-<版本>\pi-flash.exe`；下次更新落位后 `cleanup_older` 删掉旧版本
  目录 → pin 目标消失。terminal 能启动是因为薄壳每次重新挑「版本最高的存活
  载荷」。非 PF 独有：凡「按版本装目录 + 无稳定入口快捷方式」的 Windows 应用
  都会踩。
- ✅ 修法（`services/win_pin.rs`，Chrome 同款三件套）：① 开窗前设进程级显式
  AUMID `dev.piflash.app`（该 shell32 出口不在任何导入库符号表里，
  GetProcAddress 运行时绑定）；② 启动时确保开始菜单有同 AUMID 的稳定入口
  lnk → `payload\pi-flash.exe`（081 布局永不改名的路径；explorer 固定运行中
  窗口时优先认领开始菜单里同 AUMID 的现成快捷方式，固定住的就是稳定入口）；
  ③ 启动后台自愈 `User Pinned\TaskBar`：指向版本化载荷目录的 pi-flash pin
  原地改写到稳定入口，老用户的坏 pin 升级后**原地复活**，无需重新固定。
- ✅ 验证：`win_pin::tests::shortcut_roundtrip_with_aumid`（真 COM：写 lnk 读回
  目标一致 + AUMID 属性读回）+ `pin_matching_scope`（只修版本化载荷目录里的
  pin，稳定入口/开发 exe/别人的 lnk 不碰）；updater 16 测全绿。debug 构建与
  非安装布局一律不碰用户的开始菜单/任务栏。

## 待验证（本轮修复）——思考中插话，整屏消息落到屏幕下半部分

用户口径：首条消息发出后已钉顶，agent 思考中再发一条 steer（插话），结果所有
消息落到屏幕下半部分。

- ✅ 根因：上一轮「发言钉顶按是否真打断决定」把打断投递的处置写成**无条件**
  `pager.release()`——钉顶期（锚下内容还短于一屏）退役 = 摘掉那一整屏垫片，
  `layout_items` 的覆盖条件 `pt + below + spacer + pb < H` 立刻成立 → 逻辑位
  被夺 → Bottom 胶水把整块短内容拽到 `屏底 − PAD_BOTTOM`。
  `reload_keeps_pin_or_falls_to_bottom` 的反例（「用户消息掉到屏幕下半部」）
  早就是这个几何，只是当时没把打断投递接上。
- ✅ 修法：打断只退役**跟尾期**的锚点（新 `ChatList::release_if_following`）——
  跟尾期垫片已归零，卸掉不跳，且轮末内容缩水时不会被 `settle_spacer` 翻回钉顶
  把视口拽回上一轮的用户消息；钉顶期保留锚点，插话气泡只是追加在当前屏内内容
  之后，几何原地不动，新回复长过一屏再交还胶水跟尾。
- ✅ 验证：`chat_list::glue_geometry::interrupt_while_pinned_keeps_position`
  （真 gpui 布局：钉顶期插话首条仍钉 `padding.top`、气泡接在其下；跟尾期退役
  不跳）+ `pin_regression_tests::steer_while_pinned_keeps_anchor`（打断不再无条件
  release）；`cargo test -p app` 324 全绿。

## 待验证（本轮修复）——展开文件夹卡几秒（真凶：git status 无 pathspec 扫整仓 270 万条）

外部用户实测报告：`docs/pi-flash-file-tree-freeze-20261019/`（含复现脚本与
微基准）。他的项目是 HOME 遗留 git 仓库（2023 年、无 commit）的子目录，点
文件夹 UI 冻结数秒、git 徽标全部不亮；作者机器（cwd==repo_root）从未复现。

- ✅ 根因（三层叠加）：`git_status_files()` 的 `git status -uall` 无
  pathspec → 子目录项目也扫整仓（junction 递归后 2,702,629 条 / 414MB /
  99.9s）全存 `ws.git_files`；点击文件夹 UI 线程同步 `rebuild_tree()` →
  `flatten()` 为 270 万条建 HashMap + 祖先遍历（C++ 微基准下限 7.554s）→
  冻结，fs watcher 还会在每次写入后重跑整场扫描。附带两个正确性 bug：
  porcelain 路径是仓库根相对、`cwd.join()` 全拼成假路径（徽标失效、34 万
  假祖先目录）；`-z` rename 按 TAB 切永远不命中、old path 被当独立条目。
- ✅ 修法（services/git.rs）：status/numstat 加 `-- .` 钉在 cwd（实测同环境
  99.9s → 0.095s）；`rev-parse --show-toplevel` 定根后把路径重定基到 cwd
  （与 read_dir 同形态，flatten 的 git_map 才命中）；rename/copy 消费第二条
  NUL 记录。行为变化（有意）：文件树/git 面板只显示项目内变更。
- ✅ 验证：新增 status_tests 5 例（临时真仓库：子目录 scoping、徽标命中
  flatten 回归、-z rename、numstat scoping、非 git 空）；app 311+7 全绿、
  pi-link 166 全绿。跟进项入 bd：在途 git 扫描无取消、flatten map 每击重建、
  load_project_files 死代码、read_branch 子目录无分支。
- 备注：测试并发裸 spawn git 撞 0xC0000142（conhost 竞态），复用 git_cmd
  （CREATE_NO_WINDOW）解决，坑已记 engineering.md。

## 待验证（本轮修复）——树/标签切项目后发消息，会话落在旧项目、列表上「消失」

用户口径：打开 PF 起会话查 pin 失忆问题，会话从 pi-flash 列表里消失；观宏知微
组里也没有。实证：会话 01a14bee 完好落在观宏知微组（header cwd=观宏知微），
第三条消息「你是不是混乱了，我在说pf」即症状——agent 在错误目录下跑。

- ✅ 根因（双语义分叉）：项目有两个真相——界面处处显示 `ws.cwd`（树根/psp
  高亮/新会话页标签），composer 发送却跟 `pool.active_key` 的 runtime cwd。
  `sync_tree_to_file` / 树标题下拉切项目时「只改 cwd 不动活跃会话」（注释还
  当优点），启动时落在旧项目的草稿继续接收发送 → 新会话绑到旧项目。四个
  去处全落空 → 列表隐形：当前组 `same_ws` 滤掉它；家组沿用启动时缓存
  （refresh_sessions 只重扫当前 cwd）没它；active_file 兜底只在当前 cwd 扫
  描结果里找；伪标签 FileBound 时按设计退役。
- ✅ 修法（单一语义，用户定案「cwd 就是活跃项目，不存在两个语义」）：
  `sync_tree_to_file` / `tree_proj_menu_pick` 改走完整 `switch_project`
  （恢复该项目上次会话或落新草稿，注意力跟过去），废除「活跃会话不动」；
  内容视图保存/恢复，用户正看的文件/终端不被拽走。曾加过 send 门「cwd 不
  一致则重落草稿」的兑底判断，随后核查全部 ws.cwd 写入点（switch_project/
  open_session/switch_pool_rt/树侧两处）与注意力写入点（三者 + 空闲回收
  跳过 active + 远程控制不切注意力）确认分叉不可达——判断本身即第二语义
  残渣，已删：发送只认活跃会话，零第二判断。
- ✅ 验证：app 300 + pi-link 146 测试全绿。pif-ui 隔离实例双路径断言：
  B 中 `file.open` A 文件 → cwd=A 且活跃 draft 换成 A 的、view=file 保持；
  树标题下拉 down+enter 切 B → cwd=B、活跃 draft=B。

## 待验证（本轮修复）——pin/布局/项目记忆集体失忆（真凶：测试夹具写穿真实配置）

用户口径：升版本后 pin 被清空。实证与升级无关：`~/.pi-flash/workspace.json`
被测试夹具整表替换（只剩 `{pi_work: keep.jsonl, __window:{max}, __tree:
temp\pi-flash-ws-test-*}`——purge 测试与 tree_state 测试夹具的联合会），
06:29:00（agent 跑全量测试窗口正中）刚被冲过一次，文件里留着两代测试进程
pid 指纹（9848/10420）。

- ✅ 根因：`purge_app_paths_in_map` 注释写「纯内核」却在 removed>0 时调
  **全局 `set_memory`**。不带 `PI_FLASH_DIR` 裸跑 `cargo test -p app` 时，
  `purge_app_memory_drops_poisoned_keys_only` 的局部夹具 map 经此整表写进
  真实 workspace.json（冲掉 `__pins`/各项目 `__last`/`__ui`/`__window`），
  后续 `tree_state_default_is_not_stored` 的全局 `save_tree_state` 在夹具
  上 merge——每次裸跑测试都复发。
- ✅ 修法：内核真纯化（只改传入 map，落盘职责归调用方），
  `purge_app_install_memory` 在 removed>0 时才 `set_memory`。
- ✅ 验证：app 300 测试全绿（含原 purge 测试，改后不再写全局）。
  被冲掉的 pins 无备份（原子写 tmp+rename），救不回；运行中的 PF 内存缓存
  完好，正常退出会把布局键写回大半。

## 已修复（第二轮，2026-10-20）——「重复发送」复发：停止按钮不撤回已排队消息

第一轮修复（37919a1，见下）解决了「入队时机竞态」，但用户复现：agent 工作
中发消息，执行完后又重新执行一遍，好像收到两次（会话 01a1564b，2026-10-19
22:32-01:04，用户口供：中间按过停止、正在修改设计）。

- ✅ 真根因：pi 的 `Abort` 只掐当前 run **不清 followUp/steering 队列**
  （`_handlePostAgentRun` 因 `_agentRunAbortRequested` 直接 return false，
  绝不消费队列）。运行中发的消息经 `prompt+followUp` 入队后按停止 → 消息
  留在队列里；下一条消息跑完时 agent-loop 在「would stop」处 drain 队列，
  **陈旧消息被冲出来当新轮执行** = 「收到两次消息」。pi 自带 `clear_queue`
  RPC（`AgentSession.clearQueue()` 注释即 "restoring to editor when user
  aborts"），PF 从未调用；`queue_update` 事件也被忽略，队列零可见。
- ✅ 修法：① pi-link 新增 `Command::ClearQueue`（wire `{"type":"clear_queue"}`，
  response data 携带被撤回原文 `{steering,followUp}`）；② `abort_stream`
  abort 后配对发 `clear_queue`，response 臂删对应乐观气泡 + 摘
  `pending_echoes` + 文本回填输入框（新 `SessionEvent::RecallInput`，
  pi-web restoring-to-editor parity；后台会话吞进 runtime 草稿）；
  ③ 微信远程 `/停止` 同口径补配对；④ `queue_update` 拆出 typed
  `Event::QueueUpdate`，runtime 持队列镜像，状态行显示真实「已排队 N 条」。
- ✅ 验证：pi-link 181 测试全绿（新增 clear_queue/queue_update wire 测试）；
  全 workspace cargo check 通过。验收交用户（复现路径：运行中发消息 →
  停止 → 确认文本回输入框、气泡移除；再发新消息确认旧消息不再自动执行）。

## 已修复（第一轮，2026-10-17）——agent 工作时排队发送的内容隔几分钟后「重复发送」

用户口径：agent 在跑时排队发了「新theme叫薰衣草」，接着又发了带补充的
第二条；第一条直到下一轮回复结束才被重新发出去（会话尾部多出一条同文
气泡 + 模型又答了一遍）。会话 01a1498d 实证：滞留消息落盘时间
（12:08:08）与下一轮 assistant 终条**同一秒**。

- ✅ 根因：RPC `follow_up` → `session.followUp()` → `_queueUserInput`
  **不校验 isStreaming**，且入队前还要 `await _runInputHandlers`（扩展
  handler 有 latency）。PF 本地猜 `agent_running` 决定发 FollowUp 还是
  Prompt，猜「在跑」而 pi 恰好收尾时，消息入到**空闲 agent 的
  followUpQueue**——无人消费，直到下一轮 `prompt` 跑完时 agent-loop 在
  「would stop」处 drain 队列才被当新轮冲出来 = 重复发送。
- ✅ 修法（pi-web parity，rpc-manager.ts 恒带 streamingBehavior）：
  ① `Command::Prompt` 增加 `streaming_behavior`（wire 键
  `streamingBehavior`），composer/远程/自动化全部发送改走
  `prompt + streamingBehavior:"followUp"（或 steer）`——排队与否由 pi 以
  自身 isStreaming **原子**判定，失败方向安全（空闲→立即开跑，不再滞留）；
  ② `abort_stream` / prompt 被拒**不再清 `pending_echoes`**：pi 的 abort
  只杀当前 run 不清队列，滞留消息回显时还要靠它去重（清了就是同文重复
  气泡）。
- 领域笔记已归档：docs/agents/pi-rpc.md「wire 字段与形态」末条。

## 待验证（本轮修复）——切项目后点项目行「＋ 新建会话」直接崩（进程消失）

用户口径：从文件树切项目到 agentic-design-pattern → 打开一个 md → 点 pi-flash
项目标签栏的新建会话，app 直接崩溃（无报错框，进程瞬间没了）。

- ✅ 定位手法（值得复用）：release 崩得没日志，先看 Windows 事件日志
  `Application Error` → 异常码 `0xc0000409`、异常数据 `7` = Rust panic
  撞上不可 unwind 边界（gpui 鼠标事件回调）后的 fastfail，**不是**栈溢出
  （那是 `0xc00000fd`）；转储在 `%LOCALAPPDATA%\CrashDumps\pi-flash.exe.*.dmp`，
  `payload-x.y.z\pi-flash.exe` 与本地 `target\release\pi-flash.exe`
  **字节一致**（md5 相同）且带 `pi_flash.pdb` → 自写 `pdbtool`（`pdb` crate 读
  全局符号表）把转储栈上的 RVA 翻成函数名，直接看到
  `new_session_in` → `switch_project` → `rebuild_tree` → `flatten_pins`；
  再照 panic 现场留在栈上的字符串字面量（`called \`Result::unwrap()\` on an
  \`Err\` value`）用 debug 构建复现，拿准行号 `services/file_tree.rs:116`。
- ✅ 根因两层：
  1. **触发面（真凶）**：`switch_project` 自己抄了一遍换 cwd 的重挂流程，
     漏掉 **Pin 收藏夹**——那是按项目存的（`pins_for(ws_key)`，只有
     `remount_workspace` 刷）。切项目后 `ws.pinned` 还是上一个项目的绝对
     路径；Pin 视图下 `rebuild_tree` → `flatten_pins(new_cwd, 旧 pins)` →
     `IgnoreStack::is_abs_path_ignored` → `abs_path.strip_prefix(abs_base_path)
     .unwrap()` 当场 panic；panic 发生在鼠标事件回调（非 unwind 边界）→
     `panic_cannot_unwind` → 进程直接 abort。只 pin 过文件、没切过项目的人
     碰不到，所以显得「偶发」。
  2. **炸点本身**：`is_abs_path_ignored` 的 `Some` 分支比 `RepoExclude`
     分支少一句归属检查——后者有 `!abs_path.starts_with(ignore.path()) →
     交上一层`，前者直接 `strip_prefix().unwrap()`。任何「规则来自不包含
     本路径的 base」都会炸（Pin 脏数据、目录被移走的旧条目都能触发）。
- ✅ 修法：① `switch_project` 不再自抄，改调 `remount_workspace`（与
  `open_session` 池命中路径同一条），Pin 清单/退出空 Pin 视图自动跟着换；
  ② `is_abs_path_ignored` 的 `Some` 分支补齐归属检查，跨 base 路径交上一层
  （语义与 `RepoExclude` 分支一致，本项目内的命中照旧置灰）。
- ✅ 验证：新测 `flatten_pins_survives_out_of_project_pin`（外项目绝对路径
  进 Pin + 本项目命中仍置灰两向都锁）；`cargo test -p app` 296 全绿、
  `cargo test -p pi-link` 142 全绿。pif-ui 隔离实例走原始序列复验：
  agentic 里 pin 文件 → 开 Pin 视图 → 切到 pi-flash → `files` 面断言
  `pin_view=false` 且 `pins=[]`；原崩溃序列（切项目 → 开 md → `project.
  new_session_in` / psp ＋）跑通，app.log 零 panic；防御层单跑（留在
  pi-flash 却 pin 了 agentic 文件再开 Pin 视图）出两行不炸。

## 待验证（本轮修复）——每次启动都从 payload 目录启动（未注册的安装目录常驻项目列表）

用户口径：每次启动 pi-flash 都落在 `...node_modules\pi-flash\payload`，这个目录从没
注册过，从项目列表删掉下次启动又回来。

- ✅ 根因（三层叠加，单修哪层都复发）：
  1. **交接 spawn 不设 cwd**（`updater::handoff_if_newer`）：双击老入口
     `payload\pi-flash.exe` → 交接拉起新 exe `payload-0.1.12\pi-flash.exe`，
     子进程继承 cwd = 老 payload 目录。
  2. **exe 目录探针失灵**（`shell/chat.rs`）：探针只认「cwd == exe 目录」；
     新 exe 目录已是 `payload-0.1.12` ≠ 继承的 `payload` → 双击兑底没命中，
     老 payload 目录被当用户项目采纳：`register_project` 入册 + 经
     `set_last_open` 写脏 `__last` → 此后每次启动都恢复成 payload，自我延续。
  3. **存量不洗**：`projects.rs::purge_app_paths` 写了没人调——注册表里的
     payload 脏条目删了下次又写回。
- ✅ 修法（四处合围）：① 交接 spawn 在「cwd == 自己 exe 目录」（双击签名）
  时显式给子进程 `current_dir` = 子 exe 自己的载荷目录，让它同样命中 exe
  目录兑底；终端/薄壳场景 cwd 是用户语义，保持继承。② 新纯函数
  `workspace::cwd_inside_install_root`：exe 目录名为 `payload`/`payload-<版`
  `本>`（npm 薄壳布局）时，cwd 落在安装根（包目录）内一律按 exe 目录启动
  处理，不采纳为项目（堵快捷方式「起始位置」= 包根、手动 cd 进载荷目录
  等残余路径）；`PI_FLASH_CWD` 显式指定不参与放宽。③ `startup::boot` 第
  一步调 `purge_app_paths()` 洗注册表存量（幂等）。④ 写入口堵死：
  `set_last_open`/`set_last_workspace` 拒记安装根内路径，`rebuild_projects`
  不给安装根内 cwd 建常驻项目组，`register_project` 护栏拦新增。
- ✅ 验证：`cargo test -p app` 295 全绿、`cargo test -p pi-link` 142 全绿
  （含新测 `cwd_inside_install_root_matches_payload_layout_only`、
  `install_root_covers_sibling_payload_dirs`）。真机待复验：双击老
  `payload\pi-flash.exe` → 交接后落上次工作区而非 payload；项目列表无
  payload 条目。
- ✅ 顺带：`cleanup_removes_only_older_payloads` 陈旧测试修复——版本写死
  `0.1.11` 当「更新的」，crate 版本涨过 0.1.11 后必挂；改为相对
  `current_version()` 推导（older = 0.0.9，newer = `9<cur>`）。

## 待验证（本轮修复）——用户消息鼠标穿透（滚轮同帧连动会话列表）

用户口径：会话里用户消息会鼠标穿透。

- ✅ 根因：v63-3 为修操作栏悬停显影删掉了用户气泡的 `occlude()`（它挡祖
  先 hover，必须删）；但 gpui 的滚轮派发给**所有命中 hitbox（含祖先）**
  （buglist 设置页「滚列表页面同帧跟滚」同源），长用户气泡（>300px 内滚）
  上滚轮时气泡与外层会话列表**同帧一起滚**——感知即「滚用户消息穿透滚
  列表」。左键点击无穿透通道（祖先链上无可感知的左键 handler；TextView
  起选有 is_hovered 守卫）。
- ✅ 修法（`turn.rs` ububble 挂 `on_scroll_wheel` 边界拦截）：滚动前已在
  对应边界才放行给外层列表（原生嵌套滚动语义：内滚到底，下一滚才带动外
  层）；未溢出（max_offset=0）恒放行。两个 gpui 语义坑：① 滚动 offset
  存储域为 **[-max, 0]**（0=顶）；② 同 div 内置滚动 handler 注册晚于自
  定义 listener，bubble 相位逆序派发 → listener 先收、内置后滚，
  `offset()` 里已含本次 delta，判边界须减回去取滚动前值（EPS 1px 容差）。
- ✅ 验证：`cargo test -p app` 293 全绿；真机待复验：滚长用户气泡，气泡
  独滚、列表不动；气泡滚到顶/底后再滚一档，列表接续。

## 待验证（本轮修复）——editor 点击可见行，该行瞬移屏幕底部（滚动位被拽回内容底）

用户口径：把文件最后一行滚到屏幕中间（下方留空），鼠标点击该行，该行跳到
屏幕底部。（021 md 源码编辑器实测；任何 code editor 场景皆然）

- ✅ 根因（vendor gpui-component `input/state.rs`，两个钳制范围不一致 +
  scroll_to 无条件落账）：
  1. 滚轮路径 `update_scroll_offset` 钳的是 `scroll_size`（内容高 +
     `empty_bottom_height` 半视口垫底）——允许 overscroll，末行能合法停进
     视口中部（滚动位比纯内容底更深）。
  2. 点击 → `move_to` → `scroll_to`：光标行明明完整可见（上下两支越界判据
     都不命中），但函数尾部**无条件**把 offset 重钳进纯内容范围
     `[-(wrapper.len()×行高-H), 0]` 并写 `deferred_scroll_offset`——停在
     垫底区的滚动位被拽回内容底（视口上收半视口-行高），点击行瞬移屏幕
     底部。
  3. 2026-10-15 鬼畜振荡修复笔记里「滚轮含垫底与本处纯内容钳制并存不冲突」
     的判断不成立——冲突就出在「光标可见也落账」这一步。
- ✅ 修法：内容钳制收进上下两支越界分支（只作用于「解」）；函数尾改为
  `scroll_offset != scroll_handle.offset()` 才写 deferred + notify——光标
  可见时滚动位一字节不动，用户停好的 overscroll 位保留。分支解的钳制行为
  与 10-15 修复完全一致（解不越内容底、不裁顶），composer 振荡修复不受
  影响。

## 待验证（本轮修复）——editor 打开 minified js 卡到死（单行 94 万字符）

用户口径：打开 `素材-2026柴油裂解价差-echarts.min.js`（1,030,855 字节、
45 行、末行 **941,277 字符**、次长行 86,887 字符）编辑器假死。（该文件
就是 echarts 库本体，minified——「引用外部 echarts」与本 bug 无关。）

- ✅ 根因（vendor gpui-component，两条都在**每帧**路径上）：
  1. 高亮：`highlight_lines` 按**逻辑行**取整行字节范围送
     `SyntaxHighlighter::styles`——94 万字符行 `match_styles` 产出 ~20 万条
     capture，再进 `unique_styles` 的 O(区间×样式) 二重扫（每帧 ~10^10 次
     比较）→ 分钟级/帧 = 假死；86,887 字符的第 35 行首屏就落在
     visible_range 里，打开即 ~10^8/帧 的卡。
  2. shaping：`layout_lines` 对可见逻辑行的**全部软换行段**逐段
     `shape_line` + 整段可见行物化成一个 String——94 万字符行 ~6000 段
     /941KB 每帧重付。
- ✅ 修法（**视口字节窗**，对齐 2026-10-12 注入高亮修复的思路）：
  prepaint 按行高几何定位每条软换行段的 y，算出与视口相交的字节窗
  （上下各留 2 行余量，方向键/选区端点取 x 用）；`highlight_lines` 逐行
  查询范围先与窗求交；`layout_lines` 窗外段垫等 len 的空 `ShapedLine`
  （段数/字节 len/y 槽位全保留，光标/点击/选区定位按 len 累加，x 取 0
  只发生在屏外行）；`closest_index_for_*`/`index_for_position` 字节累加
  改用覆写后的 `len()`；`LineLayout::paint` 跳空 text 段。
- ✅ 首轮修复的回归（用户实测抓到）：开 js 不卡了，接着开含中文注释的
  py 文件崩（`direct_write.rs shape_text: byte index 67 is not a char
  boundary`，0xc0000409）。根因：视口窗裁剪后 `highlight_lines` 返回的
  样式区间不再**从 visible 文本 0 起连续平铺**，而消费端
  （layout_lines → runs_for_range → shape_text）按顺序吃 `run.len`、
  不读 `range.start`——缺口让所有 run 边界相对文本整体错位；纯 ASCII
  的 min.js 错位不炸（高亮悄悄错位，肉眼难察），py 的中文字符被从字节
  中间切开直接 panic。修法：窗前/窗后各补一段默认样式 filler，恢复
  平铺不变式（`combine_highlights` 是边界扫描线、输入有缺口输出就丢段，
  filler 必须自己补）。此修复同时纠正了滚动后高亮整体错位的暗病。
- ✅ 顺带修一个被掩住的 panic：`calculate_visible_range` 非等高路径
  `visible_range` 初始值用了 `text_wrapper.len()`（**软换行行数**），
  与循环赋值的逻辑行号混用——滚到文件底部 break 不触发时 end 跑到几千，
  旧代码靠 `slice_lines` 越界钳制掩住，改逐行迭代后 pagedown 到深处直接
  panic（`line should exists in text_wrapper`）。已钳到 `lines.len()`。
- ✅ 验证（pif-ui 隔离实例实测，debug 构建）：打开后稳态 `app.info`
  45-70ms（修复前分钟级假死）；pagedown×45 到文件底 + pageup 折返，
  94 万字符行整屏软换行 + 高亮渲染正常；大行内方向键移动正常；普通
  rust 文件无回归；`cargo test -p app`(4) + `cargo test -p pi-link`(134+3)
  全绿。回归场景复测（filler 补丁后）：gen.py（182 行含中文注释）开文件
  /pagedown×12/pageup×12/方向键/连续开 js+html+md+csv+py 六文件 全程
  存活，CJK 注释与 Python 高亮渲染正确；js 深处高亮锚定正确。
  已知边界：在超长行内打字仍整行 rewrap（~0.1-0.5s/键），打开
  （全量 wrap + 首次 tree-sitter parse）一次 ~1-2s（debug 口径）。

## 待验证（本轮修复）——外部改动不同步：cwd 外文件根本没挂监听

用户口径：在 Obsidian 里改了文件，pf 编辑器窗口不同步（预览/源码都不变）。
要求变化检测对齐 zed。

- ✅ 根因：`attach_fs_watch` 只递归 watch `self.cwd`——**cwd 外打开的文件
  （Obsidian 库、pick_open_files、远程 FILE_OPEN）从未被监听**，notify 没
  事件 → `check_external_file_changes`（disk_sig 复查 + 自动重载/标冲突）
  一次都不会被触发。实测：开工作区外文件后外部写入，`files.watch_roots`
  为空、`ext_probe.runs` 不涨、`content_len/editor_len` 不变（完全无感）；
  同一动作在 cwd 内文件上正常（hits=1，自动重载）。
- ✅ 修法（对齐 Zed 的监听面）：Zed 对不在任何 worktree 里的文件建
  **single-file worktree**（只监听该文件所在目录）；这里等价实现：
  `services::watcher::watch_dir`（非递归）+ `Chat::retain_file_watches`——
  为每个「不在 cwd 下」的打开文件按父目录挂一条监听（共享同一条 fs 信号
  通道），打开/关闭/切项目后重算，不再被需要的目录卸掉（句柄 drop = 停
  监听）。目录级信号不区分具体文件（事件类型/原子替换/临时文件都不必猜），
  统一交给 `check_external_file_changes` 按 (mtime,len) 复查。
- ✅ 顺带对齐 zed 的两个「重载语义」缺口（同一个用户场景直接撞上）：
  1. `set_value` 把光标扔到文末 + 滚动归零，外部每存一次就跳一次屏；
     新增 `InputState::reload_value`（vendored gpui-component）：整文换新但
     保留光标字节偏移与滚动像素（越界钳制）——Zed 的外部 reload 靠 diff
     保留 anchor，这里是位置近似的轻量版。
  2. markdown 预览不跳顶且读的是**缓冲区**：块数变化（外部改动/编辑动了
     段落结构）时先把 `list.logical_scroll_top()` 锚点记下、`reset` 后
     `scroll_to` 还原；有未保存修改时预览取编辑器文本而非磁盘真值（Zed
     的 preview 渲染 buffer，不是文件）——冲突挂着时预览不再显示磁盘旧文。
- ✅ 验证（`cargo test -p app` 181 全绿 + pif-ui 事务式，全在隔离实例里
  对**工作区外**文件跑）：
  - 开文件 → `files.watch_roots` = 其所在目录；外部写入 → `ext_probe.hits`
    1、`content_len/editor_len` 同步跟上（截图 `tmp/ui-w3/synced.png`）。
  - 缓冲区脏 + 外部写入 → `conflict = "Changed"`（本地缓冲保留，banner
    两个按钮可见，截图 `tmp/ui-watch2/conflict.png`）；`file.reload`（新
    op，与横幅「重新加载」同一方法）→ conflict 清、内容 = 磁盘
    （截图 `tmp/ui-w4/after-reload.png`）。
  - 预览态 + 未保存修改：预览显示本地缓冲区文本
    （`tmp/ui-w4/preview-live-buffer.png`）。
  - 外部删文件 → `conflict = "Deleted"`；文件重新出现 → 自动重载回同步。
- ⏳ 真机待复验：Obsidian 里改文件，pf 预览/源码应自动跟随（源码态光标与
  阅读滚动位置保留）；正在输入（脏缓冲）时改文件 → 顶部横幅提示，点
  「重新加载」变磁盘版、点「保留我的版本」继续用本地版。

## 待验证（本轮修复）——023 fileView：面包屑 `D:\` 拆两段 + 预览/编辑器都不自动回行

用户口径（编辑器窗口截图）：①面包屑把 `D:\` 拆成 `D:` 与 `\` 两段；②不论
markdown 预览还是源码编辑，长行都不自动回行（右缘溢出被裁）。参考 zed 修。

- ✅ 根因 ①（面包屑）：`file_nav_bar` 用 `Path::components()` 逐组件出 crumb，
  Windows 上 `Prefix("D:")` 与 `RootDir` 是**两个**组件 → 两个 crumb。且分段累积
  路径 `acc` 固定从 `chat.cwd` 起 join，工作区外文件（截图里 `D:\my_obsidian\…`
  ≠ cwd）每段都拼成 `cwd\D:\…`，**面包屑目录段的兄弟文件菜单指向错目录**。
- ✅ 根因 ②（预览不回行）：`render_doc_item_inner` 的内容段是 flex 项
  `flex_span(92., None)`（basis 0 + grow 92 + shrink 0，min_w 未设）。flex 项的
  **自动最小尺寸 = min-content**，而 gpui 文本在 `AvailableSpace::MinContent` 下
  `wrap_width = None`（`elements/text.rs` 实测路径）→ min-content = **整段单行宽**，
  于是 92% 段被撑到整段宽、正文永远拿不到可用宽（一行到底）。
- ✅ 根因 ③（编辑器不回行）：`ensure_file_editor` 显式 `.soft_wrap(false)`（当初为
  vendored gpui-component 的「等高行」快路径：行 y = row×行高 闭式解）。
- ✅ 修法（对齐 zed：预览块 `min_w_0`、编辑器 soft wrap 可开）：
  1. 面包屑分段抽成 `content::breadcrumb_segments(cwd, path) -> Vec<(文本, 段绝对路径)>`：
     前缀组件与紧随的 `RootDir` **合成一段**（`D:\` / `\\server\share\`），累积路径
     从空 PathBuf 起按组件真累积（相对 cwd 时起点仍是 cwd）；渲染与自动化快照共用。
  2. 内容段补 `.min_w(px(0.))` → 段宽 = 字面 92/100 容器宽，StyledText 得 definite
     宽自然折行（zed markdown 里同样到处 `min_w_0()`）。
  3. 文件编辑器 `.soft_wrap(true)`：vendored text_wrapper 的折行是**增量**的（只重算
     改动行范围），未折行时 `is_uniform()` 仍走等高快路径，折行后走线性可见区间。
- ✅ 验证（`cargo test -p app`，179 全绿）+ pif-ui 事务式对拍（隔离实例，直接开
  截图那个工作区外文件）：
  - `snapshot files --only crumbs` = `["D:\\", "my_obsidian", "gitee_vault", …]`
    （修复前是 `["D:", "\\", "my_obsidian", …]`）；新增 3 条单测锁分段与累积路径
    （相对/盘符+根合并/POSIX 根）。
  - `exec file.view_mode --arg mode=source|preview` + `shot`：源码态折行、预览态
    段落折行都可见（`tmp/ui-test-bc/source2.png`、`tmp/ui-test-bc/preview.png`）。
  - 自动化口：files 面新增 `crumbs`（渲染同一个 `breadcrumb_segments`，防两边漂移）。
- ⏳ 真机待复验：打开工作区外文件看面包屑是 `D:\ › my_obsidian › …`（点目录段
  列出的同级文件正确）；md 预览与源码态长行都回行、折行后光标/Ctrl+F 命中正常。
- 📌 取舍：编辑器**全部语言**开 soft wrap（不只 md）——用户口径「预览和编辑状态
  都要回行」；超长单行文件（如压缩过的 js）折行会走线性可见区间路径，比之前的
  等高快路径慢一点，换来的是不回行就不成立的可读性。

## 待验证（本轮修复）——新会话页「模型 ∨」弹窗列表空（no models match）

用户口径：第一次进（启动那个项目）「模型 ∨」有列表；**换一个目录**（切项目后落到
新会话页）再点「模型 ∨」，列表空了；而按钮上明明写着默认模型名。

- ✅ 根因：`Chat::filtered_models`（`actions_rename.rs`，v57 加 ↑↓/Enter 导航时
  抽出来的渲染+键盘同源函数）**直读 `models_by_cwd`，`.unwrap_or(&[])` 没有回落**。
  而 010-启动 那轮把目录口径改成「进程答案优先、无则回落到启动装载的全局清单
  （磁盘 `models.json` + `models-store.json` ∪ 自有缓存 `catalog-cache.json`）」，
  新的共享入口是 `Chat::catalog_for`——设置页、胶囊标签、`new_session_default`、
  微信 `/模型` 菜单**全都改走了它，只有这个弹窗漏了**（`catalog_for` 与
  `filtered_models` 是两次提交：1cc7309 在前、773e423 在后）。
- ✅ 触发面 = **该 cwd 没有活进程答过 `get_available_models`**。实测两条路：
  (a) 启动那个 runtime 由 `startup::spawn_initial_attach` **首帧即拉进程**（所以
  「第一次进有列表」）；(b) 切项目（`switch_project` → `new_session`）建的是
  **惰性无进程草稿**，而 `ensure_models_requested` 对无进程草稿只借「同 cwd 的活
  runtime」——新目录一个都没有，于是什么都不发（设计如此：选模型不许凭空拉进程）
  → `models_by_cwd` 无此 cwd 条目 → 列表空。胶囊标签走 `catalog_for` 有回落，所以
  出现「按钮有名、列表全空」的错位。
- ✅ 修法：`filtered_models` 改走 `self.catalog_for(&cwd)`（cwd 取活跃 runtime 的
  cwd），与其余入口同源；白名单/过滤逻辑不动（`compute_state` 对空 enabled 已有
  「全放行」回落，不是本轮原因）。
- ✅ 验证（pif-ui 事务式，两种构建同一条序列对拍）：隔离实例 →
  `project.switch` 到一个空目录 → `snapshot session.has_process == false`（复现态）
  → `exec model.picker_open` → `snapshot dialogs.model_rows`：
  **根因版 0（列表空）/ 修复版 5**（本机 `enabledModels` 白名单命中 5 条）。
  截图核对：弹窗列出 deepseek/DeepSeek Flash、glm/GLM 5.3 ×2、freeflow ×2。
- 📌 顺带补断言口（此前这个弹窗**没有任何自动化入口**，所以漏测）：
  `model.picker_open`（新 op，与「模型 ∨」点击路径同构：先 `ensure_models_requested`
  再开窗）+ dialogs 面新增 `model_rows`（弹窗可见行数）。复验命令：
  `pif-ui exec model.picker_open && pif-ui snapshot dialogs --only model_rows`。
- ⏳ 真机待复验：换目录后点「模型 ∨」列表有货、能选、选完胶囊跟着变；再切回原目录
  仍是进程答案那份目录（不能被全局清单盖掉）。

## 待验证（本轮修复）——文件树 / 设置·可用模型列表「滚不动」

用户口径：左边文件树与设置·模型·「可用模型」两处鼠标滚轮毫无反应（截图里模型
列表停在 Claude Fable 5 起、第一个开关右侧露出半截滚动条 thumb）。

- ✅ 根因：`ui::vlist` **每帧 `UniformListScrollHandle::new()`**。gpui 对这个
  句柄的契约是「存在视图里、每帧传给 uniform_list」（`uniform_list.rs` 里
  `UniformListScrollHandle` 的文档原话）——滚动位就挂在该句柄内部的
  `Rc<RefCell<Point>>`（`base_handle.offset`）上，由 div interactivity 的滚轮
  监听器写入、uniform_list 每帧 prepaint 再从同一句柄读回来做可见范围计算。
  每帧新建 = 滚轮写进「上一帧那个已经被扔掉的句柄」→ 下一帧以 0 重画：列表
  纹丝不动（滚动条 thumb 也永远钉在顶端，因为 offset 恒 0）。三个调用点同源
  全中：文件树（Fill）、可用模型列表（Capped 360）、字体弹层（Fill）。
- ✅ 修法：句柄表收进 vlist——`scroll_handle(id)`（thread_local
  `HashMap<&'static str, UniformListScrollHandle>`，按 id 复用）。本 app 单窗口
  （`main.rs` 只 `open_window` 一次）+ gpui 渲染单线程，等价于「句柄挂在视图
  上」，不必让三个调用点各穿一根句柄（字体弹层的渲染闭包拿不到 Chat）。约束：
  同一 id 同屏只出现一次（现三个 id 互不相同）。
- ✅ 验证：`cargo test -p app` **115 全绿**，新增 `ui::vlist` 两条——句柄按 id
  复用（同 id 同一 Rc / 不同 id 不串）、**滚轮 → 下一帧仍按新偏移构建行**的真
  布局锁（模拟 ScrollWheelEvent 后断言本帧构建的行号集合 = 视口移到了第 3 行）。
  反证有效：把 `scroll_handle(id)` 改回每帧 `new()`，新测试立刻红
  （`滚轮偏移必须写进 vlist 复用的句柄: left 0px`）。
- ⏳ 真机待复验：文件树滚轮上下滚到任意位置不弹回；设置·模型·可用模型列表滚轮
  滚动 + 右缘 thumb 跟随（可拖拽、轨道翻页）；顺带看字体下拉列表。
- ⚠️ 附带说明（不是本轮改出来的）：gpui 的滚轮派发给**所有命中的可滚动
  hitbox**（含祖先），所以设置页里滚列表时页面自身也会同时滚（pi-web 的原生
  嵌套滚动是先内后外）。要「列表滚到底才带动页面」需要给 vlist 加滚轮
  拦截（stop_propagation + 到底才放行），等真机手感反馈再定。

## 待验证（本轮修复）——操作栏「复制」图标不显示

用户口径：操作栏里「编辑」「新分支」图标都在，只有「复制」前面空着。

- ✅ 根因：`ui::icon(name)` = `gpui::svg().path("icons/{name}.svg")`，资产靠
  `assets.rs` 里手写的 `assets!()` 列表 + `include_str!` 编进二进制。
  `copy.svg` 文件在磁盘上，但**没写进那个列表** → `Assets::load` 返回 `None` →
  gpui 对取不到的 svg **静默画空白**（不报错、不 warn）。用户行与 agent 轮两处
  复制图标同源，所以一起消失；其余图标都登记了，故正常。
- ✅ 修法：`assets!()` 补 `"icons/copy.svg"`；新增
  `assets::tests::every_icon_file_and_call_site_is_registered` 双向锁（磁盘 svg
  必须全登记 + 源码每个 `icon("x")` 必须有资产）。旧测试只遍历宏列表，漏登的
  文件它看不见——bug 正是从这个盲区漏出去的。
- ✅ 验证：`cargo test -p app` 91 全绿；临时删掉登记行测试立刻报出
  `["copy.svg"]`（反证有效）。真机待复验：两处操作栏「复制」前出现双矩形图标。

（细节见 `docs/progress.md` 的「v63-4」。）

## 待验证（本轮修复）——用户消息操作栏悬停不显影

用户口径：鼠标停在**用户消息**上看不到底部操作栏（复制/编辑/新分支 + 时间），
agent 回复的轮块却正常；同一个行为不该有两套写法。

- ✅ 根因：用户气泡挂了 `div().occlude()`（v57 加的，注释写「禁止鼠标透传到下层
  消息」）。gpui `Window::hit_test` 从后往前收集 hitbox，一旦命中
  `HitboxBehavior::BlockMouse` 就 `break` —— 比它**先插入**的 hitbox（该气泡的
  **全部祖先**，含挂行级 `on_hover` 的 `msgrow-*`）连 `ids` 都进不去，
  `hitbox.is_hovered()` 恒 false。于是鼠标停在气泡上时行级 on_hover 永不触发 →
  `Chat.bar_hover` 不置位 → 操作栏 `opacity` 停在 0。agent 轮块没有遮挡后代，
  所以同一个功能在两处表现不同。连带副作用：`ids` 被截断也让滚轮在气泡上失效
  （外层会话列表收不到 scroll），气泡内滚到底后不能再滚会话。
- ✅ 修法（统一实现）：删掉气泡的 `occlude()`；把两段内联 `on_hover` 合并成
  `messages::bar_hover_wired(el, weak, ix)` 唯一接线，状态也从
  `user_bar_hover` / `turn_bar_hover` 两字段并为 `Chat.bar_hover: Option<usize>`
  （用户行 = msg_ix，agent 轮 = 轮首 msg_ix，角色不同故永不撞车）。
- ✅ 验证：`cargo test -p app` 90 全绿（新增 `messages::bar_hover_hit_test`
  正反两侧——不遮挡必须触发行悬停、遮挡必须被吞，锁住上面那条 gpui 语义）。
  真机待复验：鼠标停在用户气泡任意位置（含气泡内文字、长气泡滚动区）操作栏都显影。

（细节见 `docs/progress.md` 的「v63-3」。）

## 待验证（本轮修复）——发言钉顶 / 回复占满整屏

用户口径：每次发言，用户消息刷新到屏幕顶部；把整屏留给 agent 回复。

- ✅ agent 输出顶出屏幕、到输出结束都不自动上滚：快流下一帧 2~4 个事件，
  `sync` 第 5 步重测尾部把「承载整轮内容的轮首条目」打回 Unmeasured，同帧第二个
  事件结算就量到 0 → 钉顶/跟尾判定每事件翻转 → 视图被钉顶压住不跟尾。修法：第 5 步
  避开轮首条目（它本就每帧被布局重测）。另按口径把下边距 135→150：跟尾时内容末条
  停在「输入面板上沿 + 20px」。
- ✅ agent 内容顶到屏顶、用户消息消失（每次消息都「顶上去」）：钉顶靠精算垫片
  （内容区高 − 锚下内容）撑着，但 gpui 在「锚下填不满视口」时会丢掉逻辑位改成
  贴底胶水，而胶水用**缓存条目高度**定位——内容在两帧之间变矮（轮末折叠、等待行/
  思考块收起、下一条消息开始，尤其模型派发后的静默期无 pi 事件）时，垫片偏大 →
  覆盖成立 → 逻辑位被夺 → 胶水把起点算得更高 → 锚点不在绘制范围里。修法：垫片在
  钉顶期改成**常数（一整屏内容区高）**，覆盖条件恒不成立，锚点位置与测量精度解耦。
- ✅ 流式五步走错：pi-flash 在第②步就给出折叠的「工作详情」（思考框看不见、
  徽章不显示），随后翻页乱。pi-web 的 `isLiveTail` 是**运行中这一轮平铺渲染**
  （每消息模型名行 + thinking/tool/text 就地展开），折叠行只在轮末整形才出现。
- ✅ 内容形状一变（轮末折叠/手动展开/chips 出现）贴底胶水就把锚点摆错位置，
  只能等下一条 pi 事件自愈：`take_frame_sync` 增加每帧垫片自检。
- ✅ 流式期间没有「模型名 + ↓token估算 + t/s 徽章」行、也看不到思考框：
  消息区判「工作中」误用过期的 `get_state.is_streaming` 快照（一轮内无人重拉 ⇒
  恒 false），已改用事件驱动 `agent_running`（与 composer 同源）。
- ✅ 发送后消息不在屏顶（或干脆整片白）：垫片公式漏减列表上下内边距
  （22 + 135 = 157px）——gpui 贴底胶水把末条底边钉在 `屏底 − padding.bottom`，
  少减这两个内边距，锚点会停在屏顶之上 135px，可见区只剩空白垫片。
- ✅ 回复流式增长时消息一路往屏顶外漂：垫片结算用的 `bounds_for_item` 对
  「逻辑位之前」的条目直接返回 None，而胶水态逻辑位恒为 item_count ⇒ 垫片只能
  结算一次，之后全靠一帧前旧高度。
- ✅ 回复结束后消息从屏顶跳到屏底：轮末退役了锚点（卸垫片），Bottom 对齐把
  短内容拽回屏底；pi-web 只让垫片收敛、滚动位保持。

（本轮修复细节与验证见 `docs/progress.md` 的「v59 滚屏三修」一节。）

## 0.1.0（全部修复于 0.1.1）
- 左侧导航栏问题：
  - ✅ 选中项目后，该项目的对话列表没有显示（已修复）
  - ✅ 新建 按钮后，页面应当刷新如下（已修复，含空态 Logo + 版本号）：
  ![新建页面](image.bmp)
  - ✅ 文件浏览器 窗口默认导航栏一半高度（已修复）

- 中间输入框问题：
  - ✅ 点击输入框不显示闪动光标，没有聚焦效果（已修复）
  - ✅ 点击思考强度，不应该是循环切换，而应该弹出菜单让用户选择（已修复）
  ![思考强度菜单](image_1.bmp)
  - ✅ 点击工具配置没反应，应该弹出工具配置菜单，让用户选择（已修复）
  ![工具配置菜单](image_2.bmp)
  - ✅ 点击 压缩 按钮，没反应（已修复）
  - ✅ 点击 声音提示 按钮，没反应（已修复）

- 左侧面板完全没实现：
  - ✅ 右侧面板宽度可拖动，且提供多标签，可在文件、terminal 之间切换，具体如何实现去查pi-web源码（已修复）
  - 文件浏览器 点击 terminal 按钮后的界面：
  ![打开terminal界面](image_3.bmp)
  - 文件浏览器 点击 文件后的界面：
  ![文件浏览界面](image_4.bmp)

- ✅ 上面命令面板没实现（已修复）：
查看pi-web源码复刻实现

## 待验证（本轮修复）
- ✅ 发送消息后到 agent 首个 token 之间没有等待动画（设计 §13 已设计，实现被回显逻辑掐掉）
  - 根因：`phase_waiting` 同时充当「等待行可见」与「乐观发送待回显」标记，pi 回显 user 消息（瞬时）把它清零 → 等待行一闪即灭
  - 修法：拆出 `pending_echo: Option<String>` 只做回显去重；`phase_waiting` 仅由 assistant `message_start`/`message_update`、`agent_settled`/`agent_end`、`prompt` 失败、Esc 中止清零
  - 顺带补齐设计缺口：等待行上方模型名（as-name）、「正在思考」后 1.4s/4 步循环省略号
  - 验证：`cargo build` 后发一条消息，确认 spark 旋转 + shimmer 一直转，直到 agent 首个 token 出现才消失
- ✅ **中文输入第二个字就崩**（`editor_input.rs:86` `assertion failed: self.is_char_boundary(n)`，退出码 0xc0000409）
  - 根因：IME 组合路径把 **char 下标**当 **字节下标**用 —— `utf16_to_char_offset()` 返回 `chars().enumerate()` 的下标，喂给 `String::replace_range`；ASCII 下 char==byte 所以不暴露，中文一个字 3 字节，第二个组合更新时越界到非字符边界 → panic 带走进程
  - 修法：改 `utf16_to_byte_offset()`（`char_indices` 累计 `len_utf16`）；新增 `safe_replace_range()` 夹取 + 对齐字符边界（平台 range 可能越界/反转）；`text_for_range` 用 `String::from_utf16_lossy`（原来逐 u16 `char::from_u32` 会把代理对拆成 U+FFFD）；两个写入方法补 `cx.notify()`
  - 遗留：chat composer 是全项目唯一手写的输入框（其余输入框都走 `ui::TextInput` = gpui-component `InputState`，自带 Windows IME）。根治方案 = composer 也换 `TextInput`；手写版只有「光标在末尾 + 退格」能力，无选区/方向键/复制粘贴
  - 验证：中文输入法连续输入（含中英混输）不再崩；拼音组合串与候选上屏正常
- ✅ 等待动画下方的 shimmer 骨架条 → **已按用户要求删除**（不再实现）
  - 该条是设计 §13 的 `.shimmer`（260×10 圆角 + 渐变扫过），本项目判定为视觉噪音：已有「正在思考…」文案，骨架条不承载信息
  - 同步删除：`session/mod.rs` 的 `shimmer_bar()` 函数 + phase row 的 child；设计 HTML 的 `.shimmer` CSS / `@keyframes slide` / `<div class="shimmer">`；设计说明 §13 已标注作废
  - 保留：模型名 + 旋转 spark +「正在思考…」（省略号循环）

## 待验证（2026-10-05 「新分支」按钮修复）
- ✅ **用户消息下的「新分支」点了没反应**
  - 根因①：pi-link `parse_line` 用 serde_json 默认解析（递归上限 128），
    而 `get_tree` 按 children 嵌套整条会话（85 条消息 → 树深 95 → 整行被
    丢弃、无事件）→ 用户消息拿不到 entry id → 按钮不挂 handler
  - 根因②：长会话（实测 2970 条 entry）上 **pi 自己** 的 `get_tree` 就
    `Maximum call stack size exceeded`，客户端再准也拿不到树
  - 根因③：pi fork 是进程内 rebind 到新分支文件，旧实现没跟着换
    `self.file`/pool key → 分支生效了但侧栏还高亮父会话
  - 修法：`json::parse_value`（unbounded_depth）+ reader 线程 16MB 栈 +
    非 UTF-8 行不再 break 流；新增 `get_entries` 扁平链回溯算锚点
    （pi-web sliceActiveBranch 同源）；`follow_session_file` 跟随重绑定；
    按钮补 pi-web forking 态（创建中…/主题色/禁用/栏常显）
  - 验证：① 打开一条 85+ 条消息的老会话，hover 用户消息 → 「新分支」可点，
    ② 点击后状态栏出现 forked，侧栏高亮切到新分支会话，标题改为新分支内容，
    ③ 在新分支里发一条消息正常流转；④ 长会话（1000+ 轮）里「新分支」同样可用
- ✅ **「新分支」入口位置反了 + 克隆出来的会话重名**（用户定案两改）
  - ① 入口从用户消息栏移到 **agent 轮操作栏**：`fork` 只能 before，分支点 =
    该用户消息的 parentId（这条用户消息不带过去），点自己却从它之前开始；
    正确语义 = 从 agent 回复处 clone，保留到本轮为止、其后丢弃。尾部轮
    （无下一条用户消息）走 rpc `clone`（整段复制）
  - ② 分支自动改名：原名截取 20 字（超长加 "…"，按字截断），fork 前捕获
    原 title，落地后 `set_session_name` 写到新分支文件上
    （初版为「前 15 字 + "2"」，用户连做两次 clone 后反馈后缀叠成 `…22` 很蠢 → 去掉后缀）
  - 验证：agent 回复 hover 出现「新分支」；点击后新分支侧栏名字为
    「<原名前15字>2」；分支内容停在点击的那轮回复之后
- ✅ **等待回复时稍一滚就跳底，之后滚轮彻底失灵（会话区滚屏）**
  - 根因：滚轮回调把任何"非贴底"滚动都判成用户主动离底（钉顶期逻辑位恒
    为 Some，滚一格就中）→ 退役锚点、摘掉整屏垫片 → 内容高 < 视口，Bottom
    贴底胶水把整块内容拽到屏底；砸底后滚动余量归零（内容短于视口 ⇒ 最大
    滚动位就是尾部位），滚轮从此无响应，想回钉顶只能再发一条消息。叠加
    vendor `list()` 滚轮累加器缺陷（同帧 N 事件把累计 delta 整包重放 N 次，
    流式全窗重建拖长帧时事件积压 → 一滚跳一大截）
  - 修法：滚轮改 `free_scroll` 软状态（chat_list.rs）——只暂停 sync 第 6 步
    重钉与 settle 跟尾翻转拽底，锚点与垫片保留：微滚平滑滑动、滚回贴底
    （几何上≡钉顶位）自动恢复钉顶、读历史不被流式拽走；`release` 真退役
    只留给发送失败/fork/整表重读。vendor `elements/list.rs` 滚轮改只应用
    本事件 delta + 真实行高（div 同款；升级 vendor 需重套）。有意偏离
    pi-web promptAnchorActive（原"勿当 bug 修"口径作废）
  - 验证：短会话发消息 → 钉顶后轻滚一格，内容平滑上移不跳底；滚回底部
    恢复钉顶视图；上滚读历史时流式继续、视口不被拽走；长回复照常转跟尾；
    `cargo test -p app` chat_list 单测 + glue_geometry 几何锁全绿

## 待验证（本轮修复）——subagent M4–M6 全量落地后的验证清单（新对话从这里接手）

背景：subagent 四里程碑已完成并推送（M4 字段面 5bd517e→67dd15b 之间共 7 个
提交；M5 Settings-子代理页 b41839a；M6 子代理面板 98011ef；warning 清理
88ed6f3；内置 profile 落进运行时 67dd15b）。黄金仓已同步到 67dd15b，测试
全绿（pi-link 165 / app 304 / 扩展 85）。**尚未做 UI 实测验收**（045-3
验收清单未跑），用户刚 cargo run 起新版，准备实测时开新对话。

### 本轮修过的坑（验证时注意回归）

1. **agent-workspace.sh 克隆 origin 指错**（e15658b）：脚本用本地黄金仓
   路径当克隆源，克隆的 origin 就指向本地路径，推当前分支被
   receive.denyCurrentBranch 拒。已改为克隆后 remote set-url 重指黄金仓的
   origin（GitHub）。现存克隆 pi-flash-m4 的 origin 已手工修正。
2. **Settings-子代理页内置态输入框可敲字**（b41839a 内）：TextInput 无
   禁用 API，原型 `.frm.ro { pointer-events:none }` 的等价实现 = 只读态
   渲染静态文本（快照传值），非置灰可聚焦输入框。
3. **parse_iso_secs 丢时区偏移**（98011ef 内，pi-link/subagent_catalog.rs）：
   原实现 split 掉 `±HH:MM` 当没看见，已正经折算。
4. **半行保护**（同上）：面板流式扫父会话 JSONL 时，尾部未写完的行不推进
   offset，扩展 append 中途被读到不丢 descriptor。
5. **内置 explorer 曾三处断裂**（67dd15b，最重要）：
   - 扩展运行时无内置常量 → `profile:"explorer"` 直接 PROFILE_INVALID；
   - 工具描述没传 allowedProfiles → 模型不知道有任何 profile 可点名；
   - 不点名 = 裸委派（resolveComposition 返回 {model}，全继承委派者）。
   现已修：BUILTIN_PROFILE_DOCS 进 profile.ts（内容与 pi-link/agents.rs
   `builtin_agents` 逐字一致，**两处各存一份，改动必须同步**）；
   loadProfile 文件未命中回落内置；注册工具时清单拼进描述（同步扫盘，
   新建 profile 下次会话可见——PF 每会话独立 spawn pi）。
   **语义（2026-10-19 拍板）**：不点名 = 裸委派兜底（全继承），不设默认
   profile；委派清单进工具描述让模型知道委派谁。

### 验证路径（045-3 清单的 UI 部分）

- Settings → 子代理（导航第 5 项 user-round 图标）：新建/保存/删除自定义、
  内置只读态、复制为自定义、损坏态红条+打开文件修复（手写坏 md 测）
- 对话里「用 explorer 帮我调研 xxx」→ 子代理卡 + 面板行出现
- 会话操作栏「子代理面板」按钮 → 浮层（LIST_W=400 宽、锚操作栏正下方）、
  运行中徽标、整行点击开子会话 tab、外点/ESC/✕ 关
- 徽标陈旧性（2026-10-19 拍板：关面板只是隐藏，常驻轮询）——已改：打开期
  1s / 收起期 5s 常驻轮询，会话激活即建缓存，徽标从切换起就准
- maxTurns 软限实测：profile 设 50/指定 N，委派超限任务看 steer 收尾与
  stopReason=max_turns 卡片「轮次用尽」
- maxloop 委派参数：CLI 或对话里传 maxloop=3 验证优先级

### 遗留

- pif-ui 自动化断言未写（045-1 §7 收尾项）

## 本轮追加（2026-10-19 第二批）——内置 general-purpose + 委派清单 + 常驻轮询

考证结论：dsh（deepseek-harness）**无出厂内置子代理**（subagent 包是纯委派
框架，preset 全靠部署 bundle 提供），按用户指示对齐 ZCode 两内置 → explorer
（即 Explore 位）+ 新增 **general-purpose**。同轮落地用户两拍板：①委派清单
带描述进工具描述；②面板关着 = 隐藏，常驻轮询。

### 改动

- 内置 general-purpose：`extensions/pi-subagents/src/profile.ts`
  BUILTIN_PROFILE_DOCS + `crates/pi-link/src/agents.rs` builtin_agents
  两侧逐字同步（agents.rs 新增 `builtins_match_extension_docs` 对账测试）；
  不写 `tools:` = 继承全部工具，maxTurns 0 = 无软限，与 ZCode「全部工具」位对齐
- 委派清单带摘要：profile.ts `listProfileIdsSync` 升级为 `listProfilesSync`
  （返回 id+description+tools，同名自定义覆盖内置摘要，损坏文件降级裸 id）；
  tool.ts 描述渲染 `- id: description (tools: 档位)` 逐行清单
- 面板常驻轮询（subagent_panel.rs）：`if !open 退场` 改为开 1s / 关 5s 常驻
  （`polling` 守卫防重复 spawn）；poll 内比对 rt file 自愈换会话（归零重扫、
  open 保持）；pool.rs `switch_to` 末尾挂 `ensure_subagent_panel`（open=false
  空结构起步，首轮 poll 全量扫，不拖慢切换热路径）——徽标不再依赖先开一次面板

### 本轮验证项（叠加在上一节验证路径上）

- Settings 子代理页左列表出现两个内置（explorer、general-purpose），都可
  只读查看 + 复制为自定义；general-purpose 详情工具档显示「继承」
- 工具描述实测（PI_FLASH_RPC_LOG 或让模型复述）：`agent` 工具描述含
  「- explorer: 只读探索 · 调研与定位 (tools: read-only)」「- general-purpose:
  通用子代理 … (tools: inherit)」两行
- 对话点名「用 general-purpose 帮我 xxx」→ 正常委派、子会话有全部工具
  （可写文件/跑命令）；自定义 `general-purpose.md` 同名覆盖内置（两处语义一致）
- 徽标：会话 A 委派后台任务 → 切到会话 B 再切回（或停在 B 看操作栏），
  徽标 ≤5s 内出现/更新；浮层关着徽标照样走
- perf_probe 三测需 `tmp/perf/perf-80k.txt` 基准文件（gitignore，新克隆要
  从黄金仓拷），缺文件报 NotFound 属环境而非代码

## 本轮追加（2026-10-19 第三批）——面板弃浮层改停靠 + 选不了模型修复

用户实测两个问题，同一根因：`subagent_panel_element` 只判面板存在、没判
`open`（注释写了「没开 → None」但代码没实现）。M6 一直没 UI 实测所以没暴露；
第二批加「会话激活即建缓存」后浮层 + 全屏外点拦截层常驻——点 composer 模型
下拉全被外点关闭吃掉（「无法选择模型」），面板本身也永远关不上（✕/外点只置
open=false，浮层照渲染）。

### 改动

- **删浮层**（用户拍板：不要悬浮窗）：面板改停靠内容区上半——挂 chat-wrap 顶、
  `h(relative(0.5))`、保底 160px，消息与 composer 让位下半；无遮挡/外点/ESC，
  ✕ 或再点操作栏按钮关。subagent_panel_element 签名去掉 content_h 参数
- **open 门**：element 开头 `if !panel.open { return None }`——缓存常驻后这道门
  不可省（教训记入 044-1 §1.3）
- 044-1 §定案/§1.1/§1.3 同步二改记录

### 验证项

- 点「子代理面板」→ 面板占内容区上半、消息区压到下半，再点/✕ 收起
- 面板开着时 composer 模型下拉/思考档/输入全部正常可点（原 bug）
- 切会话/新会话页（012）面板不残留；徽标照常 ≤5s 更新

## 本轮追加（2026-10-19 第四批）——dsh 鼓励委派对齐 + 面板回浮层

（上一节「第三批」的停靠式面板已被本批三改取代：面板回浮层、不下推消息区；
第三批的 open 门修复仍然有效。）

### 1. dsh 三件套对照结果（用户问「都设置了吗」）

| dsh 原有 | PF 现状 | 本轮动作 |
|---|---|---|
| 系统提示词注入（tool-subagent 末尾 section："Start independent delegations … together in one assistant message and continue useful work while they run."） | ❌ 没有（只有 promptSnippet 一行 + namespace.instructions） | 补：pi 原生通道 `promptGuidelines`（注册即进系统提示词 Rules 节），dsh 原文照搬（工具名换 agent）+ PF 追加一条「自包含子任务主动派」 |
| `isConcurrencySafe: true` | ❌ 没有 | 补：pi 等价物 `executionMode: "parallel"`（同帧多委派真并行） |
| 容量池（slot 占用至清理完成 / 满了拒绝不排队） | ✅ 早有：capacity.ts 两条规则移植，默认 8，`PI_SUBAGENTS_CAPACITY`/`PI_SUBAGENTS_MAX_DEPTH` env 可调 | 无动作 |

已知差异：dsh 还有「深度到顶提示」（systemPrompt context 注入 "remaining
levels 0, do not call"），PF 未做——超深委派有 DEPTH_LIMIT 错误反馈兜底，
默认 maxDepth=1 且子代理拿不到 agent 工具，影响面小。

### 2. 面板回浮层（三改）

用户实测停靠版（二改）把消息区下推，不要。回浮层：absolute 挂 chat-wrap
顶左（操作栏正下方）、宽 400（LIST_W）、最大高内容区上半、阴影圆角卡；
**保留二改教训不挂全屏外点拦截**（composer 模型下拉不受影响），✕/再点按钮
关。渲染序放 chat-wrap 末位（与 composer 重叠时面板在上）。044-1 定案块记三改。

### 验证项

- 新会话直接说「调研一下 xxx」（不点名 subagent）→ 模型主动委派（提示词生效）
- 一句话派多个任务 → 多条 agent 调用同帧并行（面板出现多行「运行中」）
- 面板打开：消息区**不被下推**（浮层盖在消息上）、composer/模型下拉可正常点
- ✕/再点按钮关面板；行点击开子会话

## 本轮追加（2026-10-19 第五批）——主动委派失败的根因与修复（已无头实测验证）

第四批注入后用户实测仍不主动派。取证：翻用户实测会话的 transcript——
**两条 guideline 都已进系统提示词 rules 节**（管线通），模型看到的委派
信息只有：常驻工具清单一行机械说明（"- agent: Delegate a task…"）+
两条淹没在 12 条规则里的弱 bullet + 工具描述（纯机制无政策）。对照
ZCode 同模型会自动派：差异在 **ZCode 的 task 工具描述本身是政策文档**
（when to use / keep the conclusion not the file dumps / don't also run
it yourself）。

### 修复（第五批）

- tool.ts 委派描述追加 "When to delegate" 政策段：任务匹配 profile、
  或需要横扫大量文件/目录（调研、对比实现、定位实现、多步验证）且只要
  结论不要文件转储 → 委派并继续主线、**不要自己重跑委派过的检索**；
  单一已知文件/符号直接动手
- promptSnippet（常驻工具清单那行的文案）从机械说明改为带政策：
  "Delegate self-contained research or multi-step tasks … while it runs"
- index.ts 第四批缩进打歪的注册段顺手扶正

### 无头实测（vendor pi CLI -p，真模型真调用）

提示「对比一下 pi-flash 的 subagent 实现和 pi-web 的」→ 模型**同帧并行
派出两条 agent 调用**，均点名 explorer（"pi-flash subagent 调研" /
"pi-web subagent 调研"），prompt 为结构化调研提纲——即 ZCode 行为。
已过：扩展 91 测试（新增政策回归断言）。

## 本轮追加（2026-10-19 第六批）——面板行 pi-web Agents 化 + tokens 落盘

用户给 pi-web Agents 面板截图定稿：面板只画子代理状态。行从单行 30px 改
两行版式：第一行 = 标题 + 状态图标文案（⟳ 运行中 / ✓ 完成 / ⊘ 已中止 /
⌛ 轮次用尽 / ✕ 失败）；第二行 = `profile · 多久前`（刚刚/N秒前/N分钟前/
N小时前）+ `⏱时长 · Nk tokens`。头部加聚合：`N 个子代理 | 合计 ⏱总时长 ·
总tokens · N 个运行中`（运行中 >0 显 accent）。行排序 = 运行中在前、其余
按开始时间新→旧。整行点击开子会话保留，不画 pi-web 的「主 Agent」行。

### 数据链路（tokens 列是新数据）

- descriptor.ts：加可选 `usage`（不动版本号，老 descriptor 缺字段 → 面板
  显示 "—"）
- manager.ts `#settle`：终态把 `result.usage` 折进 descriptor 落盘
- subagent_catalog.rs：CatalogRow 加 `usage_total`（优先 `total`，缺了
  input+output+cacheRead+cacheWrite 求和，全零/缺失 → None）
- i18n：subagent.n_agents / total / n_running / just_now / ago_secs /
  ago_mins / ago_hours 七键

### 验证项

- 委派后开面板：两行版式、运行中排前、头部合计随运行跳动；完成后 tokens
  列出现（新委派的才有，重启前的老子代理 tokens 为 "—"）
- 行点击开子会话；ESC 外点不关（浮层无拦截，✕/按钮关）

## 本轮追加（2026-10-19 第七批）——subagent 功能总开关（发布前拍板）

用户拍板：subagent 未过 UI 实测就要发版 → 做开发旗标总开关，**发布包即
关态**，不做 UI 开关、不动 app-settings.json。机制：

- 旗标文件 `<PF数据目录>/dev-flags.json`：`{"subagents": true}` 才开。
  debug = `./pi-flash-dev/`（gitignore，开发机各自建；本机已建），
  release = `~/.pi-flash/`（用户机器无此文件 = 恒关）
- env 覆盖最高优先：`PI_FLASH_SUBAGENTS=1` 强开 / `=0` 强关
- app 侧 OnceLock 缓存（`workspace::subagents_enabled`），改动**重启生效**
- 同源：pi-link `subagents::dev_enabled()`

### 关态生效面（7 处消费点）

①client.rs 不注入 `-e pi-subagents`（agent 工具消失）；②full 档 `--tools`
不点名 agent；③设置导航隐藏子代理页（TAB_REMOTE 先例同款，渲染分支保留）；
④操作栏「子代理面板」按钮整颗隐藏；⑤会话激活不建面板缓存不起轮询（零 IO）；
⑥token_probe 无涉（本就不引）；⑦徽标随按钮隐藏。

### 验证项

- 默认（删/改 dev-flags.json 后重启）：无按钮、无设置页、对话里无 agent 工具
- `{"subagents": true}` 重启：全部恢复
- `PI_FLASH_SUBAGENTS=0` 压过旗标文件

## 本轮追加（2026-10-19 第八批）——dev-flags 配置化（机制泛化）

用户拍板：开关走**配置文件**不走删文件；此后其他重型模块的发版排除统一
走 dev-flags.json。子代理 = 首个配置化模块，机制泛化落地：

- 新 pi-link 模块 `dev_flags.rs`：`KNOWN` 键注册表（现仅 "subagents"）、
  `enabled(key)`（env `PI_FLASH_<键大写>` 覆盖 > 文件键值 > false）、
  `from_file`/`summary()`；解析换 `config::read_json`（**dev-flags.json 支持
  // 注释/尾逗号**，缺文件=空对象，坏文件 fail-closed 全关）
- app `workspace::dev_flag_enabled(key)`（OnceLock 快照 KNOWN 全键，渲染
  安全，重启生效）；`subagents_enabled()` 变薄包装，5 个调用点零改动
- `subagents::ext_path()` 的 env 短路移除（政策归 dev_flags，定位归定位）
- `startup::boot()` 启动打 `[startup] dev-flags: subagents=off` 行 +
  未知键 typo 提示
- 本机 pi-flash-dev/dev-flags.json 呈配置态：`{ "subagents": false }` 带注释头

### 后续模块接入（约定）

KNOWN 加键 → 消费点调 `workspace::dev_flag_enabled("键")` → env 覆盖自动
获得。候选：remote_control（有 TAB_REMOTE 隐藏先例 + 独立 wxprobe crate）、
automation（现 env 默认关，迁移只是换钥匙）、terminal（alacritty 重依赖）、
updater；markdown 不建议（与内容区三态耦合）。

### 验证项

- cargo run 启动 stderr 出现 `[startup] dev-flags: subagents=off`
- false 关态三处隐藏齐全；改 true 重启全恢复；`PI_FLASH_SUBAGENTS=0` 压制
