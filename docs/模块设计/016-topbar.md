topbar UI设计

topbar分左中右三段

左段 
对应 015 功能面板 ， 放置2个icon， 左侧展开收起按钮， 右侧手机遥控 ， 确保左右padding一致

右段
最小 最大 关闭 窗口控制按钮
设置按钮 放到 018 底部控制状态栏

中段 标签栏 最大标签宽度 300px
两部分组成
1、当前会话标签
永远置左侧第一位，没有关闭按钮 
改动：会话icon = bot-message-square；标签距条左缘 10px

2、自由标签区
向右一直延展至 右段
标签打开切换规则同文件标签
terminal打开也在这里


实施要点：
- 设置钮落 018 状态栏右槽（46×36）；收起态状态栏随面板隐藏
  = 设置无入口，不加补偿入口。
- ⋯ 菜单（打开终端 / 此会话系统提示词 / 此会话加载工具）保留在会话标签
  卡片内右缘，仅会话态显示。
- 无「+」按钮（打开文件…/新建文件入口与 Dialog::NewFile 弹窗链均不设）。
- 中段不画底线（Obsidian 式：激活 tab 卡片融进下方内容区，标签规则同
  文件标签口径）。
- 自由标签区伸展到最小化钮左侧 20px（flex_1 + min_w_0，无 75% 限宽，
  Zed parity）+ 横向滑动；右端 20px 固定为窗口拖拽区。
- 会话 tab 为固定段、不参与滚动，且不接 on_drag（禁止拖拽）。
  自由 tab 拖拽换位 = zed pane.rs 原文移植：
  - **元素 id 每 tab 唯一**（`Tab::new(ix)` 同款 `("ctab-file", ix)`）。
    所有 tab 共用同一个字符串 id 时 gpui 的 element_state 是同一条
    （pending_mouse_down / drag_listener 共用），在任意 tab 上起手拖拿到
    的都是**最后一个** tab 的载荷——幽灵黏住最后一个 tab（黏连 bug 根因）。
  - on_drag 发起：幽灵 = 被拖 tab 本体（`DraggedTab::render` 同款，复用
    tab_body 外观：icon + 标题 + 激活态配色 + shadow），不另造小签。幽灵
    icon 取值同文件树拖拽幽灵（`TreeDragGhost` 的两态口径）：文件 tab →
    `file_icons::get_icon`（`ui::icon_path`）+ 文件类型，终端/更新日志 →
    tab 自身的命名图标；**tab 本体仍按 023 不画 icon**（只有幽灵带）。
  - drag_over 逐字：`bg(drop_target_background).border_0()` + 源在目标左
    → 目标左缘 2px、源在目标右 → 右缘 2px（原地只留底不画边；高亮色走
    `t.bg_hover` / `t.accent`，PF 无色板 token）。**方向与直觉相反**：目标
    在源右侧时源是插到目标**后**面，所以亮的是目标右缘。
  - on_drop 落位：按 TabKey 现查 from（防中途列表漂移）；`to` 越界按 zed
    `insertion_index.min(items.len())` 夹到末尾；active_panel_tab 随移平移
    （shifted_active 穷举单测）。
  - 标签区右侧余量 = `render_tab_bar_drop_target` 同款（min_w 24 +
    flex_grow + 空 child 的 zed HACK），拖过时底色高亮，落 = 移到末尾。
  - tab 激活挂 on_click（mouse-up）而非 on_mouse_down：起手拖拽会清
    clicked_state，拖一个未激活 tab 不会把它顶成激活态（zed `Tab::on_click`
    同式）。
  - **悬停/拖拽光标 = 抓取手型**（不是一指禅 pointer）：可拖拽的自由
    tab `.cursor(CursorStyle::OpenHand)`，拖拽中由 tabs_host 的
    `on_drag_move::<TabDrag>` 覆写 ClosedHand——与 020 项目行拖拽同一
    口径（Windows 无掌形系统光标，OpenHand/ClosedHand 由 vendored gpui
    自绘 SVG）。会话 tab 不可拖，仍 pointer。
  - **激活跟随滚动**（zed `update_active_tab` 同式）：自由
    标签区挂 `track_scroll(&chat.layout.tabs_scroll)`，`activate_panel_tab`
    末尾一句 `scroll_to_item(ix)`（最小滚动，已可见不动）——保证 Ctrl+←/→
    切到屏外 tab 时可见可点。响应只挂激活路径，不挂渲染
    路径（否则用户手滚走一帧就被拽回）；拖拽换位落位后同样滚到被拖 tab。
    已知 4px 瑕疙：tab 的 `ml(4px)` 是 margin，不入 child bounds，走到首
    tab 时它会贴紧会话 tab（zed 用 position 内 padding 不复现）。
  - app-body 根容器挂全局兑底 on_drop：gpui 落点无注册目标时 active_drag
    残留（ghost 不消失、新拖被挡），根 no-op listener 兼清道夫，真 drop
    在深层已 stop_propagation。
- 会话标签前置 bot-message-square icon（15px，muted），标签贴标签栏左缘
  10px；会话 tab 为固定段、不随自由标签滚动。


## 微信通话 tab（自由标签区）

- `PanelTab::Wx` + `TabKey::Wx`：微信 tab 就是普通自由 tab，**右键菜单
  （Close / Others / Left / Right / All）、拖拽换位、中键关闭、批量关闭
  全套直接继承**，无特例代码路径。
- 内容归属判据：`PanelTab::owns_view(to, wx_line)`——微信 tab 同时拥有
  `ContentView::Wx`（管理页）与 `ContentView::Chat`（微信线会话聊天），
  激活高亮/关闭回退不走 `content_view == tab.view()` 的枚举相等（微信
  会话在类型上是 Chat，枚举相等永远认不出它）。
- 开关真身唯一：`wx_tab_open()` = `panel_tabs` 里有没有 `PanelTab::Wx`。
  服务泵门控、自动化 `wx_tab_closed`、存档 `wx_open` 全从它派生。
- 关闭 = 「标签与内容一体」：摘 tab → 收专属模态（二维码/新建会话）→
  收前台（`wx_leave_front`）。服务与影子会话不动。
- Ctrl+←/→：微信 tab 按它在 `panel_tabs` 的真下标参与邻位切换；往左回到
  会话 tab 时若微信线在前台，走 `wx_leave_front` 真正离开（微信会话也是
  Chat 视图，只 `set_content_view(Chat)` 会原地不动且无 tab 高亮）。
- 中键关闭统一给所有自由 tab。
- 存档仍只存「开/关」一条腿（`wx_open`），微信 tab 在 `tabs` 数组里按
  瞬态页跳过——老档格式不变，重启后回到标签带尾部。
