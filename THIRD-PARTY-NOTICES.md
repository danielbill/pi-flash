# Third-Party Notices

pi-flash 集成/参考了以下第三方软件。各部分版权归其各自权利人所有，
许可证条款见对应章节；与本项目自身的 GPL-3.0-or-later 许可证并列生效。

## vendored（随仓库/随应用分发）

### gpui — Apache License 2.0

- 位置：`vendor/gpui`
- 来源：https://github.com/zed-industries/zed （`crates/gpui`），主页 https://gpui.rs
- 版权：Copyright Zed Industries, Inc.
- 许可证全文见 `vendor/gpui/LICENSE-APACHE`（随上游分发）

### gpui-component — Apache License 2.0

- 位置：`vendor/gpui-component`
- 来源：https://github.com/longbridge/gpui-component
- 版权：Copyright Longbridge
- 许可证全文见 `vendor/gpui-component/LICENSE-APACHE`（随上游分发）

### pi-coding-agent（内置 pi sidecar）— MIT License

- 位置：`vendor/pi`（运行时 `npm ci` 拉取 `@earendil-works/pi-coding-agent`，随应用包分发）
- 来源：https://github.com/earendil-works/pi
- 版权：Copyright (c) Mario Zechner
- 许可证：MIT（随 npm 包分发，见其 `package.json` 的 `license` 字段）

## 派生代码（本仓库 GPL-3.0-or-later 覆盖）

### Zed — GPL-3.0-or-later

- 来源：https://github.com/zed-industries/zed
- 版权：Copyright Zed Industries, Inc.
- 派生/移植文件（以源码注释为准）：
  - `crates/app/src/ui/psp_scrollbar.rs` — 移植自 `crates/ui/src/components/scrollbar.rs`
  - `crates/app/src/theme.rs` — One Dark/One Light/Nord/Ayu 主题移植自 `crates/theme`
  - `crates/app/src/terminal.rs` — 终端模式参考 `crates/terminal`
  - `crates/app/src/automation/screenshot.rs` — 走 gpui `Window::render_to_image()`
  - `crates/app/src/dialogs.rs`、`crates/app/src/function_panel/mod.rs` — 滚动条样式同源

## 参考（无代码复制）

### pi-web — MIT License

- 来源：https://github.com/agegr/pi-web
- 版权：Copyright (c) 2026 agegr
- 本项目的产品规格/交互对齐以其为原型；代码为 Rust/GPUI 独立实现，
  未复制其源码。许可证：MIT（见上游仓库 `LICENSE`）。
