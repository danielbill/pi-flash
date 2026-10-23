# market 域（040-1 Pi 扩展市场）陷阱

改动会碰到的文件：`crates/app/src/market/**`（页状态机 + 渲染 + 启动预热）、
`crates/pi-link/src/market/**`（npm 搜索 / 清单补全 / 磁盘快照）、
`crates/app/src/shell/types.rs`（`PanelTab::PiExtMarket` / `ContentView::PiExtMarket`）。
设计稿 `docs/UI设计/Pi扩展市场-原型.html`，规格 `docs/模块设计/040-1-Pi的扩展市场.md`。

## 入口与开关

- **入口走 dev-flags `pi_ext_market`，默认关 = 发版不可见**（2026-10-20 用户定夺：
  页面问题没清完，本版不发）。注册在 pi-link `dev_flags::KNOWN`；env
  `PI_FLASH_PI_EXT_MARKET=1/0` 最高优先；启动 stderr 打 `pi_ext_market=off/on`。
- 关态三处闸门，**新增入口必须一起想**：`titlebar::topbar_l` 不渲染「菜单」三横钮
  / `open_pi_ext_market()` 直接 return（覆盖 pif-ui 自动化 `pi-ext-market`，
  `automation/handlers.rs`）/ `spawn_pi_ext_market_prefetch` 直接 return（不为看不见的
  页面偷跑网络）。**菜单钮与菜单项的上线取舍 2026-10-23 用户否决过一版改动，别再动**
  （只该碰市场页本体）。
- `open_pi_ext_market` 是**唯一**打开路径（菜单项、自动化、将来的快捷键都打这里）；它
  自己 push `PanelTab::PiExtMarket` + `set_content_view` + 建输入框 + 首装载。
  自动化开页走 `content.view` 的 `pi-ext-market` / `pi-native-market` /
  `pf-market` 三个键（都打各自 open 口）。

## 页状态机（`aux.market.pi_ext`）

- 行状态（未装 / 有新版 / 已最新）**渲染期派生**，不落状态；开关态从
  `cfg.mc_pkgs_global` 现查。
- `gen_ix` = 请求代际：换词/翻页/刷新都 ++，回包不符即丢。**不要用「loading 就 return」
  做单飞**——那会让当前轮的 loading 永不置位（按钮全灰、下一页点不动，已踩）。
- `stale` = 当前内容待整体替换：第 1 页重搜时**必须置**，否则 append 的「满 50 跳过」
  会把新结果全部丢掉（「第二次起搜索结果不变」的根因）。
- `pager()` 是翻页口径唯一来源：已装页签 = 本地全量 → 恒 `(1, 1)`；若还用全局 total
  折算，点「下一页」会跳到不存在的页。
- `loading` 一个标志被搜索/翻页/重试共用 → 摘要行那个转圈就是它的唯一指示器；
  列表空 + loading = 骨架屏（6 行，尺寸与真卡一致，不跳位）。

## 磁盘快照（唯一的落盘物）

- `HomeSnapshot` → `<PF数据目录>/pi-ext-market-cache.json`（`paths::pi_ext_market_cache_file`）。
- **只当「默认首页」用**：只有 `query` 为空的快照才上屏、才允许落盘。带搜索词的快照
  一律不认（重开应用还能看见上次的词 = 用户明确禁止的「留存」，2026-10-20 踩过）。
- 打开页面时无条件清 `query` + 输入框内容（`set_value("")`），会话内切走再回来也是干净首页。
- 启动预热只在「已有快照 && 快照 >1h」时跑一次，**只写磁盘不碰 UI 状态**；没有快照 =
  从没人用过市场页 = 零请求。

## 搜索框

- 打字**不**搜（npm 是远端海量数据，逐键打过去是灾难）；**回车**是唯一触发
  （`TextInput::set_on_submit`）。页面上的「搜索」按钮 2026-10-20 已删。
- **`on_submit` 回调里禁止回头 `input.read(cx)`**：回调就是 TextInput 自己在 update 里
  发出的，重复租 → panic「cannot read … while it is already being updated」（曾表现为
  「回车搜索程序崩掉」）。它把提交值传进来了，直接用。
- 页面没有「搜索按钮」之后，`pi_ext_search_now()`（读实体再搜）已一并删除，别再长回来。

## 布局踩点

- **090-4 PF 插件市场页（v3）只对齐 pi 页的 layout**：复用 `pi_native_market::design`
  的 `Metrics/sp/px18`，外壳/分段骨架/双列网格/行距/页码条同源。**无「最近发布」段、
  无工具栏**（不出输入框·搜索钮·状态/排序下拉·重置·字段标签行，用户 2026-10-23 定夺）；
  **首段自带 `pt(shell_pad)`**（pi 页那份上内距挂在「最近发布」段上，删段后必须
  自己补，否则贴顶边）；卡行段自带 `pt(card_pad_body)`（原由工具栏容器给）。
  **卡片内部是 PF v1 定稿，不许改成 pi 卡版式**（72 预览槽 / 名称 16 semibold /
  描述 / meta 行含安装钮）。色板走 theme 语义 token（映射表在 pf_market.rs 文件头），
  不共享 pi 的深色字面量。
- **工具栏不出字段标签行（两页都已删，2026-10-23）**：设计稿里控件的 `<span
  class="visually-hidden">Filter packages by name…</span>` 三枚标签，原意是
  sr-only，但 `.visually-hidden` 定义在 `@layer utilities` 里、而
  `.action-bar-field` / `label.stack` 的长相规则是 **unlayered**（unlayered 优先于
  任何 layer）→ 浏览器里它们真的可见，成为控件上方那排「注释」。已在
  `pi-packages.html` 删掉三个 span、`controls.rs` 删掉 `field_label()`、
  `design.rs` 删掉 `FIELD_GAP`（2026-10-24 清死码 warning）；
  **别照旧设计稿把它加回来**。
- **vlist 行高 = card_h + SP4，卡片自己带 `mt(SP4)`**：所以「摘要行 ↔ 列表」想得到 N，
  `SUMMARY_PB` 必须写 `N − SP4`（现在是 `10 − 8`）。直接写 N 会凭空多 8。
- `card_metrics` 全部**派生**（head/desc/bottom → card_h → row_h），禁止写死字面量：
  `bottom_h()` 必须跟着底行按钮档走（页面按钮统一 MD 档 → `btn::MD_H`），否则换档后
  flex 会把描述第三行压成半行。
- 工具栏右侧分段控件与摘要行「全部更新」**共用列宽 `side_col_w(window)`**：两处右缘都贴
  `PAGE_RIGHT`，等宽才能左右缘同时对齐。宽度用 `window.text_system().layout_line` 实测
  标签（SEMIBOLD，因为选中段加粗）+ 内距，**禁写死**——英文 "Not installed" 比中文长一倍。
  这也是 `page()` 吃 `&mut Window` 的唯一理由。
- 滚动条压卡片问题只能靠**行内 `mr(ROW_PAD_R = SP7)`** 解决；容器 `pr` 会把 wrapper 连同
  滚动条一起左移，照样压。
- `ui::button` 只有**纯文字**形态（无图标、不能自定义宽度）：要图标/计数徽标/`w_full`
  就降一层用 `ui::button_base()` 自己挂 children 与 `on_mouse_down`。
- 页内分段控件（未安装/已安装）是**自绘**：`settings::widgets::switch_base` 是 MD 28×16
  的开关、不是分段控件，且 Settings 模块受保护不能改。拇指 = 白底 + `border×60%` 细边 +
  `shadow_sm`，未选中段 hover 洗底——两边都没有可见反差时整条读起来是静态标签（用户原话：
  「根本看不出来是 switch」）。

## 与设计稿 / 规范的三方漂移（未销账，改前先定夺）

- **Primary 语义**：设计稿 = accent 洗底 + accent 字；规范 §5.1 = accent 实底 + 白字；
  实现全部 `Secondary`（「安装」现在和「重试」同款，丢掉 CTA 层级）。
- 图标档：规范按钮内 `ICON_SM`(14)；实现排序 chevron / 垃圾桶图标是 12。
- 排序菜单：设计 `px8/gap8`，规范 §5.6 `px10/py7/gap8`，实现 `px8/gap6`；容器还缺 `p4`
  （hover 底色贴容器边）。
- 垃圾桶：设计是 ghost（hairline 边 + 正文色），实现 `danger 35%` 边 + 6% 红底 + 红图标，
  而旁边注释写着「市场页不要红色按钮」——注释与代码自相矛盾。
- disabled 透明度：规范 §8 = 0.5；设计 `.btn` = 0.45、`.pager` = 0.38；实现 0.5（ui::button）
  + 0.38（翻页钮）。
- `docs/模块设计/040-1` 还没跟上实现：文档写「左侧搜索框，右侧【未安装】【已安装】
  【全部更新(n)】三按钮」，实际已是「排序下拉最左 → 搜索框 → ……(flex) → 未安装/已安装」，
  「全部更新」移到摘要行右端。
- i18n `market.search` 随搜索按钮一起变死键（审计只报告不判失败）。

## 数据面（pi-link `market::pi_ext_market`）

- `search(query, from, size)` = npm 搜索（`search_url` 是纯函数，fixture 测试钉着）；
  `fill_kinds` 并发补包清单（8 条一批 ≈1.0–1.6s）；`HomeSnapshot` 见上。
- 常量：`PAGE_SIZE` 50、`MAX_PAGES` 20（也是会话内存天花板）、`SNAPSHOT_MAX` 50。
- **内存只留当前页**（翻页先清空）；`kinds` 表（name → 资源类型）只活会话内，翻回旧页
  命中即 0 网络。
- 单测：pi-link 侧在 `market/pi_ext_market.rs` 的 `#[cfg(test)]`（搜索 fixture）；
  app 侧已拆目录模块 `market/pi_ext_market/`（mod.rs 状态机 + page 骨架 +
  `thousands`；card.rs 卡片及子件；pager.rs `page_window` / `pager` / `pager_nav`
  及翻页单测），跑
  `cargo test -p app --bin pi-flash market`（本机 vendored gpui-component 的 dev 配置会
  先报无关错误，见 engineering.md）。
