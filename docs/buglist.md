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
