# 铁律

- **内置钉版 pi**：对齐 pi-web（精确钉 `@earendil-works/pi-coding-agent` 无 `^`），vendor 进应用分发，运行时 spawn `node <app>/vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js --mode rpc`。**绝不读 PATH/系统 pi**；升级 = bump `vendor/pi/package.json` + VERSION + 跑 pi-link 符合性测试。开发期 `PI_FLASH_PI_BIN` 可覆盖。
- **产品 = 1:1 复刻 pi-web**（布局/功能），Rust+GPUI 只做"更快的壳"；不自创产品。布局做法：逐组件读 pi-web 源码（globals.css 令牌 / panel-layout.ts / 组件结构）→ 令牌级翻译，不手搓近似。
- **目标平台**：Windows + macOS（macOS 包走 GitHub Actions runner，不做交叉编译）。

# 路径

- pi-web 源码（产品需求文档）：`D:\github\---ai-tools---\pi-web`
- zed 参考源码（稀疏浅克隆）：`D:\github\zed` → gpui、markdown、theme、ui、terminal、agent_ui、acp_thread 等
- vendored pi：`vendor/pi`（package.json+lock 入库；node_modules 走 `npm ci` 不入库）
- 进度台账：`progress.md`；复刻计划：`PORT_PLAN.md`
- 截图：`tmp/屏幕截图/`（gitignore）

# 架构

- workspace：`crates/pi-link`（钉版 pi RPC 协议层，测试在此）+ `crates/app`（GPUI 界面）
- 主题：`theme.rs`（mist 默认；`PI_FLASH_THEME` 可切 default/dark/rose）
- gpui 0.2.2（crates.io，自包含）；0.2.2 要点：`cx.spawn(async move |weak, &mut AsyncApp|)`、prompt_for_paths 返回 oneshot Receiver、HighlightStyle 无 builder（字段赋值）

# GPUI/Rust 陷阱

- 交互 handler（on_mouse_down/on_key_down/listener）漏 `cx.notify()` = 状态变 UI 不动
- 方法链中间插入语句后漏/多 `),` → 结构性编译错误；大补丁用脚本文件，勿用 bash heredoc（会静默截断）
- `gen` 是 edition 2024 保留字
- PrintWindow 对 GPU 窗口有偏移伪影（截屏用 CopyFromScreen + DPIAware）

# pi 协议陷阱（详见 progress.md）

- 内容块类型 camelCase `toolCall`；流式 args 起始走 `partialJson`；工具结果 `role:"toolResult"` 回灌
- set_model 字段是 `modelId`；client 不得硬编码 `--no-session`
