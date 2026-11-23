整个app的layout 基本参考 ZED实现。

客户区自绘标题栏，body 为左右两栏（`shell/render/body.rs`）：

左：功能面板栏（panel-col）
- 顶：topbar 左段（收放钮，`titlebar::topbar_l`）
- 中：功能面板 dock（015：020-1 整合面板 = 项目会话列表 + 目录文件树；git/插件面板互斥切换）
- 底：底部控制状态栏（018，只在左栏底部，收起态随面板整列隐藏）

右：内容区（content-col）
- 顶：topbar 右段（置顶会话 tab + 自由标签区 + 窗口控制钮，`titlebar::topbar_r`；
  空段挂 `WindowControlArea::Drag`，三个窗口钮注册 Min/Max/Close 命中盒）
- 主：内容区（030 会话界面 / 文件编辑器 / 终端 / 搜索等单例 tab，`content.rs` 状态机）

全窗 overlay 叠层：settings → dialogs → 确认浮层 → psp 悬浮 → picker → toast
（`shell/render/overlay.rs`）。
