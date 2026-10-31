# PF Bug 追踪

> 约定：条目按时间倒序；**已修复的只留「根因一行 + 修法一行 + 验证口径」**，展开细节
> 见 `docs/progress.md` 与 `docs/agents/*.md`。标记 `待复验` 的条目才是当前要跑的手工清单。

## 已验证（本轮修复）——新建 Provider 弹窗偶发「读取注册表」几十秒

- 根因：弹窗缓存未命中时起一次性 node dump，脚本尾 `listCredentials()` 走默认
  AuthStorage，撞 `auth.json.lock` 孤儿锁（SDK staleMs 30s）→ 等满 30s。实测干净
  1.78s / 造孤儿锁 30.8s。修一次后缓存落盘、锁被砸开，故「偶发」。
- 修法：DUMP_SCRIPT 给 `ModelRuntime.create` 传只读凭据存储 `ReadOnlyAuthStorage`
  （裸读 auth.json 零锁，dump 只消费 providerId→type 元数据）。
- 验证：30.8s → 2.1s，42 providers / 1599 models / 4 凭据无损。陷阱与升级复核点
  记入 `docs/agents/pi-rpc.md`。

## 已验证（本轮修复）——「🔌 MCP: 1 server enabled」反复出现在 PF 发往微信的消息里

- 根因：pi 冷启动把横幅作为**独立 assistant 消息**发进 RPC 流；泄漏的那个进程跑的是
  横幅过滤入库之前的旧构建，TextEnd 过滤与 flush 行剔除两道防线都不在。
- 修法（第三道总闸）：`RemoteControl::send`（微信出站唯一内容出口）按「整条消息即横幅」
  拦一道——单行 + 含 `mcp:`/`server` + `enabled` 收尾（先剥 ANSI），命中即内部消化 +
  `[wx] 拦截 MCP 横幅出站`；多行聊 MCP 的真回复不误杀。
- 验证：回归测试 `send_gate_swallows_pure_banner_messages`（dump 实抓文案 + 变体 + 反例）。
- ⚠️ 遗留（未定位）：同批 dump 显示同文偶发双发（seq5=6、16=17、19=20），非横幅专属，
  疑似双 flush 或重复喂入，另开任务排查。

## 待复验——跨项目开文件顶掉活跃会话：switch_project 新增「进行中会话保留」

- 根因：`sync_tree_to_file` 走完整 `switch_project`（树定位刚需，正确），但尾部「新项目
  无 last-open 就 `new_session`」把进行中的对话顶出会话标签。（第一版「跨项目文件不切
  项目只开 tab」已回滚：树定位丢失。）
- 修法：无 last-open 时，当前会话若**进行中**（已绑会话文件、非空白草稿）则原地保留，
  只有当前本身是空白草稿才落新草稿。
- 验证：app 398 + pi-link 197 全绿。待手工：聊天中开别的项目文件 → 树定位到新项目、
  会话标签仍是原对话；新项目有 last-open 时照旧恢复；psp ＋ 新建会话不受影响。

## 待复验——微信 Bot 开关/扫码成功后主进程卡死最长 90 秒

- 根因：`Transport::drop` 逐个 join 线程，poll 线程大概率阻塞在 `getupdates` 的 90s
  服务端挂住里（`POLL_TIMEOUT=90s`）；触发路径 = 关开关（`transport = None`）与扫码成功
  后泵检测 token 变化（UI 线程 200ms 一拍）。
- 修法：drop 改「置 stop + `handles.clear()` detach」，UI 不等在途长轮询；游标未 ack →
  下个轮询者重收这批（重复而非丢失）。顺手 `force_restart_if_token_changed` 的 state
  读按 1s 节流。
- 验证：wxprobe 117 / app 383 全绿。待真机：扫码成功瞬间与开关「关」不冻结；关后 90s
  内重开不丢消息。

## 待复验——微信 Bot 出站「受理不投递」：client_id 固定复用被 iLink 去重吞掉

- 根因：`sendmessage` 的 `client_id` 协议要求**每条消息唯一**，PF 用 state 里写死的稳定
  实例 id 长期复用 → iLink 受理（返回新 message_id）但不投递。此前「拿到 message_id =
  出站已通」的结论是错的（它只是受理回执）。
- 修法：`poller::send_text` 内部拼 `{client_id}-{millis}-{seq}`。
- 验证：对照实验固定 client_id 不达 / 唯一 client_id 送达；用户真机确认全部连通；
  wxprobe 115 全绿。

## 待复验——启动卡死数秒：内层 wasm 插件验签把编译放在主线程

- 根因：`spawn_plugin_scan` 只把清单扫描放后台，`PluginInstance::load` + init 在主线程
  `this.update` 闭包跑 cranelift 全量编译（4.6MB 组件 debug 实测 7.2s，且每插件各建
  一个 Engine）。
- 修法：拆两段——清单扫描后台落位（JS 插件立即可见）；wasm 编译等 `pool.booted` 后交
  `background_executor`，结果回主线程只写状态栏。proof 钩子 `thread_local` → 进程级
  `RwLock`，`Engine` 进程内共享；新增 `load_cached`：产物按 wasm sha256 落
  `<数据目录>/cache/pf-plugin/<sha>.cwasm`（原子写、留最近 8 份），命中即
  `deserialize_file` + instantiate + init，失配自动重编。**每次启动仍真跑 init 验签门。**
- 验证：冷 9.24s(debug) / 0.62s(release) → 热 7–10ms（产物 1.4MB）。待真机：启动到
  可操作无停顿；验签结论一律不展示（只进 stderr）。

## 待复验——微信 Bot 回复石沉大海：出站只认桌面选中态 + 投递静默丢 + 横幅泄漏

- 根因三层：① 出站回推门只认 `is_active`（桌面选中态），入站却固定投 `wx_session` →
  桌面一切换，模型回复（会话文件里完整躺着）永远进不了微信缓冲；② 目标会话没打开
  （重启后 runtime 不存在）时自愈落空即丢；③ pi 启动横幅（带 ANSI）从 assistant 通道
  混进回复缓冲。
- 修法：出站门对齐入端口径（`is_active || wx_session`）+ `buf_key` 双源防串线；轮末冲刷
  改看缓冲来源运行态；`open_session_detached` 后台唤醒（不动桌面注意力）再投递，失败必有
  微信回执（`RemoteSend` 三态，排队不报错）；出站门只在 `agent_running` 轮内转发。
- 验证：app 383 全绿（新增换源冲刷断言）。待用户：多项目连发多条每条都收到回复、无横幅
  碎片；切桌面项目不影响微信线；异常时用 `PI_FLASH_RPC_LOG=1` 抓。

## 待复验——微信 Bot「渠道运行中、扫码成功」却收不到任何消息

- 根因三层：渠道在扫码**前**被拉起 → 轮询线程一次性捕获旧 bot token（重扫码让旧会话在
  服务端失效，getupdates 全部 `errcode:-14`）；pump 见渠道活着就跳过装新 token；
  `register.rs` 成功分支不清 `buf/chat_id/user_id/context_token`，state 游标嵌的还是旧 bot，
  重启也救不回。三重不可见（Err 分支 `let _ = e;`、面板只看 `transport.is_some()`、传输层
  失败不落 dump）。
- 修法：扫码成功即 `reset_session_fields`；QR Done 时 `force_restart_if_token_changed`
  （运行中 token ≠ state 现值则重拉）；`Transport.last_poll_error` 同步面板「最近错误：…」
  + `warn_once` stderr。顺手：出站剥 ANSI（`poller::strip_ansi`）；`wire.rs` 的
  `Path::extension()` 比对 `"res.txt"` 永不命中（实际是 `txt`）→ 改按文件名后缀。
- 验证：wxprobe 115 / app 382 / pi-link 185 全绿。待用户：更新后重新扫码一次（清磁盘残留
  旧游标）应恢复响应。

## 待复验——固定到任务栏的图标，更新一次就点不开

- 根因：pin 的 lnk 记的是**固定那一刻**窗口进程的 exe 绝对路径，而 081 启动链让窗口常年
  运行在 `payload-<版本>\pi-flash.exe`；下次更新落位后 `cleanup_older` 删旧目录 → pin 目标
  消失。（terminal 能启动是因为薄壳每次重挑最高版本载荷。）
- 修法：`services/win_pin.rs` Chrome 三件套——① 开窗前设进程级 AUMID `dev.piflash.app`
  （shell32 出口 GetProcAddress 运行时绑定）；② 确保开始菜单有同 AUMID 的稳定入口 lnk →
  `payload\pi-flash.exe`；③ 启动后台自愈改写在版化载荷目录上的坏 pin。
- 验证：`shortcut_roundtrip_with_aumid`（真 COM）+ `pin_matching_scope`（只碰版本化载荷
  目录的 pin，稳定入口/开发 exe/别人的 lnk 不动）；updater 16 测全绿。

## 待复验——思考中插话，整屏消息落到屏幕下半部分

- 根因：打断投递写成**无条件** `pager.release()`——钉顶期退役 = 摘掉整屏垫片，`layout_items`
  覆盖条件立刻成立 → 逻辑位被夺 → Bottom 胶水把整块短内容拽到 `屏底 − PAD_BOTTOM`。
- 修法：打断只退役**跟尾期**锚点（新 `ChatList::release_if_following`）；钉顶期保留锚点，
  插话气泡追加在当前屏内内容之后，长过一屏再交还胶水。
- 验证：`glue_geometry::interrupt_while_pinned_keeps_position` +
  `pin_regression_tests::steer_while_pinned_keeps_anchor`；app 324 全绿。

## 待复验——展开文件夹卡几秒（真凶：git status 无 pathspec 扫整仓 270 万条）

- 根因三层：`git status -uall` 无 pathspec → 子目录项目也扫整仓（2,702,629 条 / 414MB /
  99.9s）；点击文件夹在 UI 线程同步 `rebuild_tree`（flatten 270 万条下限 7.5s）→ 冻结；
  porcelain 路径是仓库根相对、`cwd.join()` 拼成假路径（徽标失效 + 34 万假祖先目录）；
  `-z` rename 按 TAB 切永不命中。
- 修法（`services/git.rs`）：status/numstat 加 `-- .` 钉 cwd（99.9s → 0.095s）；
  `rev-parse --show-toplevel` 定根后把路径重定基到 cwd；rename/copy 消费第二条 NUL。
  行为变化（有意）：文件树/git 面板只显示项目内变更。
- 验证：status_tests 5 例 + app 311+7 / pi-link 166 全绿。跟进项入 bd（在途扫描无取消、
  flatten map 每击重建、`load_project_files` 死代码、子目录无分支）。
- 备注：测试并发裸 spawn git 撞 0xC0000142，复用 `git_cmd`（CREATE_NO_WINDOW），坑记
  `engineering.md`。

## 待复验——树/标签切项目后发消息，会话落在旧项目、列表上「消失」

- 根因（双语义分叉）：界面处处显示 `ws.cwd`，composer 发送却跟 `pool.active_key` 的
  runtime cwd；切项目「只改 cwd 不动活跃会话」→ 启动时落在旧项目的草稿继续接收发送 →
  新会话绑到旧项目。四个查找去处全落空 → 列表隐形。
- 修法（用户定案「cwd 就是活跃项目，不存在两个语义」）：`sync_tree_to_file` /
  `tree_proj_menu_pick` 改走完整 `switch_project`，废除「活跃会话不动」，内容视图保存/恢复。
  曾加的 send 门「cwd 不一致则重落草稿」核查后确认是第二语义残渣，已删。
- 验证：app 300 + pi-link 146 全绿；pif-ui 双路径断言（B 中开 A 文件 → cwd=A、view=file
  保持；树标题下拉切 B → cwd=B、活跃 draft=B）。

## 待复验——pin/布局/项目记忆集体失忆（真凶：测试夹具写穿真实配置）

- 根因：`purge_app_paths_in_map` 注释写「纯内核」却在 removed>0 时调**全局** `set_memory`；
  不带 `PI_FLASH_DIR` 裸跑 `cargo test -p app` 时局部夹具 map 整表写进真实
  `workspace.json`（冲掉 `__pins`/`__last`/`__ui`/`__window`），后续 `save_tree_state`
  再 merge 一次，每次裸跑都复发。
- 修法：内核真纯化（只改传入 map），`purge_app_install_memory` 在 removed>0 时才 `set_memory`。
- 验证：app 300 全绿（含原 purge 测试）。被冲掉的 pins 无备份（原子写 tmp+rename）救不回。

## 已修复（第二轮，2026-10-20）——「重复发送」复发：停止按钮不撤回已排队消息

- 根因：pi 的 `Abort` 只掐当前 run **不清 followUp/steering 队列**；运行中发的消息按停止后
  留在队列，下一条消息跑完时 agent-loop 在「would stop」处 drain → 陈旧消息被当新轮执行。
  pi 自带 `clear_queue` RPC，PF 从未调用，`queue_update` 事件也被忽略。
- 修法：① pi-link 新增 `Command::ClearQueue`（response 携带被撤回原文）；② `abort_stream`
  abort 后配对发 clear_queue，删乐观气泡 + 摘 `pending_echoes` + 原文回填输入框
  （`SessionEvent::RecallInput`，pi-web parity）；③ 微信 `/停止` 同口径；④ `queue_update`
  拆 typed 事件，状态行显示真实「已排队 N 条」。
- 验证：pi-link 181 全绿；全 workspace cargo check 通过。验收交用户。

## 已修复（第一轮，2026-10-17）——agent 工作时排队发送的内容隔几分钟后「重复发送」

- 根因：RPC `follow_up` 不校验 isStreaming；PF 本地猜 `agent_running`，猜「在跑」而 pi 恰好
  收尾时消息入到**空闲 agent 的队列**，无人消费，直到下一轮 prompt 跑完才被 drain 冲出。
- 修法（pi-web parity）：`Command::Prompt` 增 `streaming_behavior`（`followUp`/`steer`）——
  排队与否由 pi 以自身 isStreaming 原子判定，失败方向安全；`abort_stream` / prompt 被拒不再
  清 `pending_echoes`。笔记归档 `docs/agents/pi-rpc.md`。

## 待复验——切项目后点「＋ 新建会话」直接崩（进程消失）

- 根因两层：① **触发面**：`switch_project` 自抄了一遍换 cwd 的重挂流程，漏刷 **Pin 收藏夹**
  （按项目存，只有 `remount_workspace` 刷）→ `flatten_pins(new_cwd, 旧 pins)` →
  `strip_prefix().unwrap()` panic；panic 落在鼠标事件回调（不可 unwind）→ 进程 abort。
  ② **炸点**：`is_abs_path_ignored` 的 `Some` 分支比 `RepoExclude` 分支少一句归属检查。
- 修法：① `switch_project` 改调 `remount_workspace`（与池命中路径同一条）；② `Some` 分支
  补齐跨 base 交上一层（与本项目内命中置灰语义一致）。
- 验证：`flatten_pins_survives_out_of_project_pin`（两向锁）+ app 296 / pi-link 142 全绿；
  pif-ui 走原崩溃序列零 panic，防御层单跑不炸。
- 定位手法（可复用）：Windows 事件日志异常码 `0xc0000409` + 数据 `7` = panic 撞不可 unwind
  边界（栈溢出是 `0xc00000fd`）；`%LOCALAPPDATA%\CrashDumps` 转储 + 自写 `pdbtool`（pdb
  crate）把栈上 RVA 翻函数名；按栈上残留的 panic 字面量用 debug 构建复现拿行号。

## 待复验——每次启动都从 payload 目录启动（未注册的安装目录常驻项目列表）

- 根因三层：① 交接 spawn 不设 cwd → 子进程继承老 payload 目录；② exe 目录探针只认
  「cwd == exe 目录」，新 exe 在 `payload-<版本>` → 兑底没命中，老 payload 被当用户项目
  采纳 + 经 `set_last_open` 写脏 `__last`（自我延续）；③ `projects.rs::purge_app_paths`
  写了没人调，存量不洗。
- 修法（四处合围）：交接 spawn 在「cwd == 自己 exe 目录」（双击签名）时显式给子进程
  `current_dir`（终端/薄壳场景保持继承）；新纯函数 `workspace::cwd_inside_install_root`
  （cwd 落在安装根内一律按 exe 目录处理，`PI_FLASH_CWD` 显式指定不参与放宽）；
  `startup::boot` 第一步洗注册表；`set_last_open`/`set_last_workspace`/`rebuild_projects`/
  `register_project` 四个写入口拒记安装根内路径。
- 验证：app 295 + pi-link 142 全绿（含 `cwd_inside_install_root_matches_payload_layout_only`、
  `install_root_covers_sibling_payload_dirs`）。待真机：双击老 `payload\pi-flash.exe` 交接后
  落上次工作区、项目列表无 payload 条目。
- 顺带：`cleanup_removes_only_older_payloads` 版本写死 `0.1.11` 当「更新的」必挂 → 改相对
  `current_version()` 推导。

## 待复验——用户消息鼠标穿透（滚轮同帧连动会话列表）

- 根因：v63-3 为修操作栏悬停删掉了用户气泡的 `occlude()`；而 gpui 滚轮派发给**所有命中
  hitbox（含祖先）** → 长用户气泡内滚时气泡与外层会话列表同帧一起滚（左键无穿透通道）。
- 修法：`turn.rs` ububble 挂 `on_scroll_wheel` 边界拦截（滚动前已在对应边界才放行给外层；
  未溢出恒放行）。两个 gpui 坑：offset 域为 `[-max, 0]`；同 div 内置滚动 handler 注册晚于
  自定义 listener 且 bubble 相位逆序 → 判边界须把本次 delta 减回去（EPS 1px）。
- 验证：app 293 全绿。待真机：滚长用户气泡，气泡独滚、列表不动；到边界再滚一档列表接续。

## 待复验——editor 点击可见行，该行瞬移屏幕底部

- 根因（vendor `input/state.rs`）：滚轮路径钳的是 `scroll_size`（含 `empty_bottom_height`
  垫底，允许 overscroll），点击 → `move_to` → `scroll_to` 在光标**已完整可见**时仍无条件把
  offset 重钳回纯内容范围并写 `deferred_scroll_offset` → 停在垫底区的滚动位被拽回内容底。
- 修法：内容钳制收进上下两支越界分支（只作用于「解」）；函数尾改为
  `scroll_offset != scroll_handle.offset()` 才写 deferred + notify——可见时滚动位一字节不动。

## 待复验——editor 打开 minified js 卡到死（单行 94 万字符）

- 根因（vendor，两条都在**每帧**路径）：① `highlight_lines` 按**逻辑行**取整行送
  `SyntaxHighlighter::styles` → 94 万字符行 ~20 万 capture 进 `unique_styles` 二重扫
  （每帧 ~10^10 比较）；② `layout_lines` 对可见行全部软换行段逐段 `shape_line` + 物化整段
  String（~6000 段 / 941KB 每帧）。
- 修法：**视口字节窗**——prepaint 按段 y 算与视口相交的字节窗（上下各留 2 行余量），
  `highlight_lines` 逐行查询先与窗求交，`layout_lines` 窗外段垫等 len 的空 `ShapedLine`
  （段数/字节 len/y 槽位全保留），`LineLayout::paint` 跳空 text 段。
- 首轮回归（用户实测抓到）：开 py 崩 `shape_text: byte index 67 is not a char boundary`
  （0xc0000409）。根因：窗裁剪后样式区间不再从可见文本 0 起连续平铺，消费端按顺序吃
  `run.len` 不读 `range.start` → 边界整体错位（纯 ASCII 只是悄悄错位）。修法：窗前/窗后各补
  一段默认样式 filler 恢复平铺不变式（同时纠正滚动后高亮整体错位的暗病）。
- 顺带：`calculate_visible_range` 非等高路径初始值用软换行行数当逻辑行号，pagedown 到深处
  panic，已钳到 `lines.len()`。
- 验证：稳态 `app.info` 45-70ms（修复前分钟级假死）；94 万字符行整屏换行 + 高亮正常；
  pagedown×45/pageup、方向键、连续开 js+html+md+csv+py 六文件全程存活。已知边界：超长行内
  打字仍整行 rewrap（~0.1-0.5s/键），首开全量 wrap + 首次 parse 一次 1-2s（debug 口径）。

## 待复验——外部改动不同步：cwd 外文件根本没挂监听

- 根因：`attach_fs_watch` 只递归 watch `self.cwd`——工作区外打开的文件（Obsidian 库、
  pick_open_files、远程 FILE_OPEN）从未被监听 → `check_external_file_changes` 一次都不触发。
- 修法（对齐 Zed 的 single-file worktree）：`watcher::watch_dir`（非递归）+ `retain_file_watches`
  为每个「不在 cwd 下」的打开文件按父目录挂一条监听（共享同一 fs 信号通道），打开/关闭/切项目
  后重算；目录级信号统一交 `check_external_file_changes` 按 (mtime,len) 复查。
- 顺带对齐 zed 两个重载语义：① 新增 `InputState::reload_value`（vendored）——整文换新但保留
  光标字节偏移与滚动像素；② markdown 预览块数变化时记锚点 `scroll_to` 还原，且有未保存修改时
  预览取缓冲区文本（Zed preview 渲染 buffer 而非文件）。
- 验证：app 181 全绿 + pif-ui 隔离实例（外部写 → watch_roots 命中并同步；脏缓冲 + 外部写 →
  `conflict="Changed"` + 两按钮；`file.reload` 后清；预览显示本地缓冲区；删文件 → Deleted）。
  待真机：Obsidian 改文件 pf 自动跟随，源码态光标与阅读滚动保留。

## 待复验——023 fileView：面包屑 `D:\` 拆两段 + 预览/编辑器都不自动回行

- 根因三个：① `Path::components()` 在 Windows 上 `Prefix("D:")` 与 `RootDir` 是两个组件 →
  两段 crumb，且 `acc` 固定从 `chat.cwd` join → 工作区外文件每段拼成 `cwd\D:\…`，面包屑
  兄弟文件菜单指向错目录；② 预览内容段 `flex_span(92., None)` 的自动最小尺寸 = min-content
  （gpui 在 `MinContent` 下 `wrap_width = None`）= 整段单行宽 → 正文永远拿不到可用宽；
  ③ `ensure_file_editor` 显式 `.soft_wrap(false)`。
- 修法：① 抽 `content::breadcrumb_segments(cwd, path)`——前缀组件与紧随 `RootDir` 合成一段，
  累积路径从空 PathBuf 真累积（相对 cwd 时起点仍是 cwd），渲染与自动化快照共用；② 内容段补
  `.min_w(px(0.))`；③ 编辑器 `.soft_wrap(true)`（vendored text_wrapper 折行是增量的，未折行仍
  走等高快路径）。
- 验证：app 179 全绿 + 3 条分段单测 + pif-ui 快照（`crumbs` = `["D:\\","my_obsidian",…]`）与
  截图（源码态/预览态都折行）。取舍：**全部语言**开 soft wrap，超长单行慢一点换可读性。

## 待复验——新会话页「模型 ∨」弹窗列表空（no models match）

- 根因：`Chat::filtered_models` **直读 `models_by_cwd`，`.unwrap_or(&[])` 无回落**，而目录
  口径早已改成「进程答案优先、回落启动装载的全局清单」的 `Chat::catalog_for`（设置页、胶囊、
  `new_session_default`、微信 `/模型` 都改走了它，只有这个弹窗漏了）。触发面 = 该 cwd 没有
  活进程答过 `get_available_models`——切项目建的是惰性无进程草稿，`ensure_models_requested`
  借不到同 cwd 活 runtime 就什么都不发（设计如此）→ 列表空而按钮有名。
- 修法：改走 `self.catalog_for(&cwd)` 与其余入口同源。
- 验证：pif-ui 对拍——根因版 0 行 / 修复版 5 行。顺带补自动化口 `model.picker_open` +
  dialogs 面 `model_rows`（此前该弹窗零自动化入口，故漏测）。

## 待复验——文件树 / 设置·可用模型列表「滚不动」

- 根因：`ui::vlist` **每帧 `UniformListScrollHandle::new()`**。gpui 契约是句柄存在视图里、
  每帧传给 `uniform_list`；滚动位就在该句柄内部 `Rc<RefCell<Point>>` 上——每帧新建 = 滚轮写进
  已丢弃的句柄 → 下一帧以 0 重画（滚动条 thumb 也永远钉顶端）。三个调用点（文件树 / 可用模型
  / 字体弹层）同源全中。
- 修法：句柄表收进 vlist——`scroll_handle(id)`（thread_local `HashMap<&'static str, …>` 按 id
  复用）。本 app 单窗口 + 单线程渲染，等价于「挂在视图上」。约束：同一 id 同屏只出现一次。
- 验证：app 115 全绿，含「滚轮 → 下一帧仍按新偏移构建行」的真布局锁；改回 `new()` 立刻红。
- 附带（非本轮）：gpui 滚轮派发给所有命中可滚动 hitbox（含祖先），设置页滚列表时页面同滚；
  要「内滚到底才带动外层」需给 vlist 加拦截，等手感反馈再定。

## 待复验——操作栏「复制」图标不显示

- 根因：`copy.svg` 在磁盘上但**没写进 `assets!()` 列表** → `Assets::load` 返回 None → gpui
  对取不到的 svg 静默画空白（不报错不 warn）。用户行与 agent 轮两处同源一起消失。
- 修法：补登记 + 新增 `assets::tests::every_icon_file_and_call_site_is_registered` 双向锁
  （磁盘 svg 全登记 + 源码每个 `icon("x")` 有资产）；旧测试只遍历宏列表正是漏网盲区。

## 待复验——用户消息操作栏悬停不显影

- 根因：用户气泡挂 `div().occlude()`（BlockMouse）→ gpui `hit_test` 命中即 break，比它先插入
  的**全部祖先** hitbox（含挂行级 `on_hover` 的 `msgrow-*`）连 `ids` 都进不去 →
  `is_hovered()` 恒 false。连带副作用：滚轮在气泡上失效。
- 修法（统一实现）：删 `occlude()`；两段内联 `on_hover` 合并成
  `messages::bar_hover_wired(el, weak, ix)` 唯一接线；字段并为 `Chat.bar_hover: Option<usize>`
  （用户行 = msg_ix，agent 轮 = 轮首 msg_ix，永不撞车）。
- 验证：app 90 全绿（含 `bar_hover_hit_test` 正反两侧锁——不遮挡必须触发、遮挡必须被吞）。

## 待复验——发言钉顶 / 回复占满整屏（一批滚屏修复）

- 输出顶出屏幕不跟尾：快流下 sync 第 5 步把轮首条目打回 Unmeasured，每事件翻转钉顶/跟尾
  判定 → 第 5 步避开轮首条目；下边距 135→150。
- 锚点被胶水夺位（用户消息消失）：钉顶垫片在「锚下填不满视口」时被 gpui 丢掉逻辑位改贴底，
  而胶水用缓存条目高度定位 → 垫片偏大即夺位。修法：钉顶期垫片改成**常数（一整屏内容区高）**，
  覆盖条件恒不成立。
- 流式第②步就折叠：改事件驱动 `agent_running` 判「工作中」（原用过期的 `get_state.is_streaming`
  快照，一轮内无人重拉恒 false）。
- 内容形状一变锚点错位：`take_frame_sync` 增每帧垫片自检。
- 发送后不在屏顶/整片白：垫片公式漏减列表上下内边距（22 + 135 = 157）。
- 流式增长往屏顶漂：`bounds_for_item` 对逻辑位之前条目返回 None → 垫片只结算一次。
- 轮末从屏顶跳屏底：轮末不退锚点，只让垫片收敛（pi-web 同）。
- 细节见 `docs/progress.md`「v59 滚屏三修」。

## 0.1.0（全部修复于 0.1.1）

- 左侧导航栏：选中项目后对话列表不显示；新建页空态（Logo + 版本号）；文件浏览器窗口默认
  导航栏半高。
- 中间输入框：点击不出闪动光标/无聚焦；思考强度应弹菜单而非循环切换；工具配置点击无反应；
  压缩按钮无反应；声音提示按钮无反应。
- 左侧面板：右侧面板宽度可拖 + 多标签（文件/terminal），复刻 pi-web；terminal 与文件浏览界面。
- 顶部命令面板未实现（复刻 pi-web）。

## 其它已修复（本轮小项）

- 发送后到首个 token 无等待动画：`phase_waiting` 兼任回显去重标记，pi 回显用户消息把它清零 →
  拆出 `pending_echo` 只做去重；`phase_waiting` 仅由 assistant 事件 / `agent_settled` /
  prompt 失败 / Esc 中止清零。顺带补模型名 +「正在思考…」省略号循环。shimmer 骨架条按用户
  要求**删除**（设计 §13 作废）。
- 中文输入第二个字崩（`editor_input.rs:86`，退出码 0xc0000409）：IME 组合路径把 **char 下标**
  当 **字节下标** 用 → 改 `utf16_to_byte_offset`（`char_indices` 累计 `len_utf16`）+ 新增
  `safe_replace_range` 夹取对齐 + `text_for_range` 用 `String::from_utf16_lossy`。
  遗留：composer 是全项目唯一手写输入框（无选区/方向键/粘贴），根治 = 换 `ui::TextInput`。

## 2026-10-05「新分支」按钮

- 点了没反应：① `parse_line` 用 serde_json 默认递归上限 128，`get_tree` 嵌套整行被丢 →
  `json::parse_value` unbounded + 16MB 栈 reader 线程；② 长会话（2970 entry）pi 自己的
  `get_tree` 就爆栈 → 新增 `get_entries` 扁平链回溯算锚点（pi-web sliceActiveBranch 同源）；
  ③ fork 是进程内 rebind 未跟随 → `follow_session_file` 换 `self.file`/pool key。
- 入口反 + 克隆重名：入口从用户消息栏移到 **agent 轮操作栏**（fork 只能 before，分支点 =
  该轮之后）；分支名截原名前 20 字（「前 15 字 + 2」的后缀叠成 `…22` 已去掉）。
- 等待回复时滚一下就跳底、之后滚轮彻底失灵：滚轮把任何非贴底滚动判成用户主动离底 → 退役
  锚点摘垫片 → 内容短于视口后滚动余量归零。修法：`free_scroll` 软状态（只暂停重钉与跟尾
  翻转，锚点/垫片保留；滚回贴底自动恢复）+ vendor `elements/list.rs` 滚轮只应用本事件 delta
  与真实行高（**升级 vendor 需重套**）。有意偏离 pi-web 的 promptAnchorActive。

## subagent M4–M6 验收清单

- 本轮修过的坑（回归注意）：`agent-workspace.sh` 克隆 origin 指错（改 remote set-url 重指）；
  Settings 子代理页内置态 `TextInput` 无禁用 API → 只读态渲染静态文本；`parse_iso_secs` 丢
  时区偏移（已折算）；面板流式扫父会话 JSONL 半行保护；**内置 explorer 曾三处断裂**（扩展
  运行时无内置常量 / 工具描述没传 allowedProfiles / 不点名=裸委派），已修且
  `BUILTIN_PROFILE_DOCS` 与 `pi-link/agents.rs` 各存一份**必须同步**（有对账测试）。
  语义（2026-10-19 拍板）：不点名 = 裸委派兜底（全继承），不设默认 profile；委派清单进工具描述。
- 待跑 UI 清单：Settings→子代理（新建/保存/删除、内置只读、复制为自定义、损坏态红条+打开
  文件）；对话里点名委派 → 子代理卡 + 面板行；操作栏面板浮层（宽 400、锚操作栏下方、行点击
  开子会话、外点/ESC/✕ 关）；徽标开 1s / 关 5s 常驻轮询；maxTurns 软限与 maxloop 参数优先级。
- 遗留：pif-ui 自动化断言未写。

## 第二批（2026-10-19）：内置 general-purpose + 委派清单 + 常驻轮询

- 考证：dsh 无出厂内置子代理；按用户指示对齐 ZCode 两内置 → explorer + 新增
  **general-purpose**（不写 `tools:` = 继承全部工具，maxTurns 0 = 无软限）。profile.ts 与
  `pi-link/agents.rs` 两侧逐字同步（`builtins_match_extension_docs` 对账）。
- 委派清单带描述：`listProfilesSync` 返回 id+description+tools，工具描述逐行
  `- id: description (tools: 档位)`。
- 面板常驻轮询：开 1s / 关 5s（`polling` 守卫防重复 spawn），poll 内比对 rt file 自愈换会话；
  `pool.rs::switch_to` 末尾挂 `ensure_subagent_panel`（徽标不再依赖先开一次面板）。
- 验证项：设置页出现两个内置且可只读/复制；工具描述含两行清单；点名委派子会话有全部工具；
  自定义同名覆盖内置；徽标 ≤5s 更新；perf_probe 需 `tmp/perf/perf-80k.txt`（gitignore，新
  克隆从黄金仓拷，缺文件报 NotFound 属环境而非代码）。


## 第四批（2026-10-19）：dsh 鼓励委派对齐 + 面板回浮层

- 对齐 dsh 三件套：① `promptGuidelines` 注入系统提示词 Rules 节（dsh 原文照搬 + PF 追加
  「自包含子任务主动派」）；② `executionMode: "parallel"`（同帧多委派真并行）；③ 容量池早已
  有（`capacity.ts` 两条规则，默认 8）。已知差异：dsh 的「深度到顶提示」未做，有
  DEPTH_LIMIT 错误反馈兜底。
- 面板回浮层：absolute 挂 chat-wrap 顶左（操作栏正下方）、宽 400、最大高内容区上半、阴影圆角；
  **不挂全屏外点拦截**（避免三批的 composer bug），✕/再点按钮关。
- 验证项：不点名也主动委派；一句话多委派同帧并行；面板打开不下推消息区、composer 可点。

## 第五批（2026-10-19）：主动委派失败的根因与修复

- 取证：两条 guideline 确已进 rules 节（管线通），但模型可见的委派信息只有机械一行工具清单 +
  淹没在 12 条规则里的弱 bullet；对照 ZCode 差异在 **task 工具描述本身是政策文档**。
- 修复：tool.ts 委派描述追加「When to delegate」政策段（任务匹配 profile，或需横扫大量文件
  且只要结论 → 委派并继续主线、**不自己重跑委派过的检索**；单一已知文件直接动手）；
  `promptSnippet` 改带政策文案；index.ts 注册段缩进扶正。
- 验证：无头实测（vendor pi CLI `-p` 真模型）→ 模型同帧并行派出两条 explorer、prompt 为
  结构化调研提纲；扩展 91 测试全绿（含政策回归断言）。

## 第六批（2026-10-19）：面板行 pi-web Agents 化 + tokens 落盘

- 行改两行版式：第一行标题 + 状态（⟳ 运行中 / ✓ 完成 / ⊘ 已中止 / ⌛ 轮次用尽 / ✕ 失败）；
  第二行 `profile · 多久前` + `⏱时长 · Nk tokens`。头部聚合 `N 个子代理 | 合计 ⏱总时长 ·
  总tokens · N 个运行中`；排序运行中在前。整行点击开子会话，不画「主 Agent」行。
- 数据链：descriptor 加可选 `usage`（不动版本号，老 descriptor 显示 "—"）→ manager `#settle`
  折进落盘 → `CatalogRow.usage_total`（优先 `total`，缺则四项求和）→ i18n 七键。
- 验证：新委派的才有 tokens；行点击开子会话；浮层无拦截，✕/按钮关。

## 第七批（2026-10-19）：subagent 功能总开关（发布前拍板）

- `<PF数据目录>/dev-flags.json` 的 `{"subagents": true}` 才开；debug 落 `./pi-flash-dev/`，
  release 落 `~/.pi-flash/`（用户机无此文件 = 恒关）；env `PI_FLASH_SUBAGENTS=1/0` 最高优先；
  app 侧 OnceLock 缓存，改动重启生效。
- 关态 7 处生效点：不注入 `-e pi-subagents`（agent 工具消失）、full 档 `--tools` 不点名 agent、
  设置导航隐藏子代理页、操作栏按钮隐藏、会话激活不建面板缓存不起轮询（零 IO）、徽标随按钮
  隐藏、token_probe 无涉。
- 验证：默认无按钮/无设置页/无 agent 工具；`{"subagents": true}` 重启全恢复；
  `PI_FLASH_SUBAGENTS=0` 压过旗标文件。

## 第八批（2026-10-19）：dev-flags 配置化（机制泛化）

- 新 pi-link `dev_flags.rs`：`KNOWN` 键注册表、`enabled(key)`（env `PI_FLASH_<键大写>` >
  文件键值 > false）、`config::read_json` 解析（支持 `//` 注释/尾逗号，缺文件=空对象，坏文件
  fail-closed）；app `workspace::dev_flag_enabled(key)`（OnceLock 快照，渲染安全）；
  `startup::boot` 打 `[startup] dev-flags:` 状态行 + 未知键 typo 提示。
- 接入约定：KNOWN 加键 → 消费点调 `dev_flag_enabled("键")` → env 覆盖自动获得。候选：
  remote_control、automation、terminal、updater；markdown 不建议（与内容区三态耦合）。
- 验证：启动 stderr 出现状态行；false 关态三处隐藏齐全；改 true 重启全恢复。
