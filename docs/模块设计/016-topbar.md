topbar UI设计

topbar 分左右两段（`titlebar.rs`），整条高 40（`HEIGHT`，两段同高，窗口控制
钮高度跟随）。整条是客户区自绘标题栏：空段挂 `WindowControlArea::Drag` 命中
盒，三个窗口钮注册 Min/Max/Close 命中盒（zed platform_title_bar parity）。

左段（`topbar_l`，在 panel-col 内）
对应 015 功能面板，放置收放钮（panel-left icon，贴左 `align::TOGGLE_ML`，
图标线与 psp 列表对齐）+ 三横「菜单」钮（紧贴收放钮右侧，恒显；浮层 =
Pi 原生扩展市场 / PF 插件市场 / 插件 `contributes.topbar_menu` 贡献条目，
渲染挂 render 根），空段中段为窗口拖拽区。手机遥控钮已隐藏（060 管线保留）。

右段（`topbar_r`，在 content-col 内）
最小 最大 关闭 窗口控制按钮（关闭钮红底白字豁免，min/max 走 HoverIcon 动效）。
设置按钮放到 018 底部控制状态栏。面板收起态（panes_hidden）收放钮跳到右段
起点（竖线镜像位）。

中段 标签栏（`titlebar/tabs.rs tab_strip`，flex_1 到最小化钮左侧）
单 tab 最大宽度 300px（激活 tab；`tab_item.rs`）。
两部分组成
1、当前会话标签
永远置左侧第一位，没有关闭按钮；距条左缘 10px；前置 bot-message-square
icon（12px，muted）；标题 = 主会话快照（≤30 字截断），无主会话时显示「新
会话」。会话 tab 为固定段、不参与滚动，不接 on_drag（禁止拖拽），仍 pointer；
右键弹标签菜单（无 Close、Close Left 恒灰）；点击回会话视图。
无 ⋯ 菜单。终端/会话目录入口在会话 tab 下方的常驻功能栏
（`session::session_fn_bar`，36px：栏首项目名 + 会话目录 + 打开终端）；
系统提示词/会话工具集两钮居上下文用量面板（ctx_tip）。

2、自由标签区
向右一直延展至 右段窗口钮左侧 20px（flex_1 + min_w_0 + 横向滑动；右端 20px
固定为窗口拖拽区）。标签打开切换规则同文件标签（023：脏点/冲突标记在 tab
行尾）。terminal 打开也在这里。自由 tab 种类 = `PanelTab`：Term / File /
Changelog / PiNativeMarket / PfMarket / Tasks / Search（083）/
SessionSearch（084）/ Plugin（090 插件页签）/ Observer（044 子代理观察窗）。
激活 tab Obsidian 卡片融底，条上不画横线。


实施要点：
- 无「+」按钮（打开文件…/新建文件入口与 Dialog::NewFile 弹窗链均不设）。
- 自由标签区独立横向滑动；会话 tab 不随自由标签滚动（超多标签时它必须恒定
  可见）。
- 会话 tab / 自由 tab 拖拽换位 = zed pane.rs 原文移植：
  - **元素 id 每 tab 唯一**（`Tab::new(ix)` 同款：文件 = `ctab-file`、其余 =
    `ctab-term`，配下标）。所有 tab 共用同一个字符串 id 时 gpui 的
    element_state 是同一条（pending_mouse_down / drag_listener 共用），在
    任意 tab 上起手拖拿到的都是**最后一个** tab 的载荷——幽灵黏住最后一个
    tab（黏连 bug 根因）。
  - on_drag 发起（载荷 `TabDrag { ix, key, label, icon, path, active, strike }`）：
    幽灵 = 被拖 tab 本体（`TabDragGhost`，`DraggedTab::render` 同款，复用
    tab_body 外观：icon + 标题 + 激活态配色 + shadow），不另造小签。幽灵
    icon 取值同文件树拖拽幽灵（`TreeDragGhost` 的两态口径）：文件 tab →
    `file_icons::get_icon`（`ui::icon_path`）+ 文件类型，终端/更新日志等 →
    tab 自身的命名图标；**tab 本体仍按 023 不画 icon**（只有幽灵带）。
  - drag_over 逐字：`bg(drop_target_background).border_0()` + 源在目标左
    → 目标左缘 2px、源在目标右 → 右缘 2px（原地只留底不画边；底色
    `t.bg_hover`、边 `t.accent`）。**方向与直觉相反**：目标在源右侧时源是
    插到目标**后**面，所以亮的是目标右缘。
  - on_drop 落位：按 TabKey 现查 from（防中途列表漂移）；`to` 越界按 zed
    `insertion_index.min(items.len())` 夹到末尾；active_panel_tab 随移平移。
  - 标签区右侧余量 = `render_tab_bar_drop_target` 同款（min_w 24 +
    flex_grow + 空 child 的 zed HACK），拖过时底色高亮，落 = 移到末尾。
  - tab 激活挂 on_click（mouse-up）而非 on_mouse_down：起手拖拽会清
    clicked_state，拖一个未激活 tab 不会把它顶成激活态（zed `Tab::on_click`
    同式）。
  - **悬停/拖拽光标 = 抓取手型**（不是一指禅 pointer）：可拖拽的自由
    tab `.cursor(CursorStyle::OpenHand)`，拖拽中由 tabs_host 的
    `on_drag_move::<TabDrag>` 覆写 ClosedHand——与 020 项目行拖拽同一
    口径（Windows 无掌形系统光标，OpenHand/ClosedHand 由 vendored gpui
    自绘 SVG）。
  - **激活跟随滚动**（zed `update_active_tab` 同式）：自由标签区挂
    `track_scroll(&chat.layout.tabs_scroll)`，`activate_panel_tab` 末尾一句
    `scroll_to_item(ix)`（最小滚动，已可见不动）——保证 Ctrl+←/→
    切到屏外 tab 时可见可点。响应只挂激活路径，不挂渲染
    路径（否则用户手滚走一帧就被拽回）。
    已知 4px 瑕疵：tab 的 `ml(4px)` 是 margin，不入 child bounds，走到首
    tab 时它会贴紧会话 tab（zed 用 position 内 padding 不复现）。
  - app-body 根容器挂全局兜底 on_drop：gpui 落点无注册目标时 active_drag
    残留（ghost 不消失、新拖被挡），根 no-op listener 兼清道夫，真 drop
    在深层已 stop_propagation。
- Ctrl+←/→ 切 tab（`ViewLeft`/`ViewRight`，body.rs 分流）：模态/编辑器搜索
  面板/文件编辑器本体持焦时退回词移动，其余焦点切 tab；切 tab 不自动聚焦
  内容区（焦点收回 content-main 锚点）。
