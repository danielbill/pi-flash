# pi-flash

<p align="left"><img src="crates/app/assets/icon/pi-flash-256.png" width="88" alt="pi-flash logo"></p>
极速 pi coding agent 桌面端 —— 用 Rust 重写 [pi-web](https://github.com/agegr/pi-web) 的壳。

## 架构

```
┌──────────────────────────┐      ACP / pi RPC (JSONL over stdio)      ┌─────────────────┐
│  pi-flash (Rust + GPUI)  │ ────────────────────────────────────────  │  pi sidecar     │
│  原生 UI / 流式渲染      │   vendor 钉版 pi，spawn 其 cli.js       │  agent core     │
└──────────────────────────┘                                           └─────────────────┘
```

- **UI**: GPUI（Zed 同款框架，Windows DirectX / macOS Metal 原生渲染）
- **桥接**: 内置钉版 pi（vendor 随应用分发），spawn `node vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js --mode rpc`；绝不读 PATH（对齐 pi-web 的版本锁步策略）
- **目标平台**: Windows + macOS（macOS 包走 GitHub Actions macos runner）

## 目录

| 路径 | 说明 |
|---|---|
| `crates/pi-link` | 钉版 pi 的 RPC 协议层（vendor 解析、JSONL、类型化命令/事件、10 个符合性测试）|
| `crates/app` | GPUI 主程序（M1：聊天核心，流式/多轮/中断）|
| `vendor/pi` | 内置钉版 pi 0.87.1（package.json+lockfile 入库；`npm ci` 生成 node_modules，不入库）|
| `AGENTS.md` | 项目记忆：架构决策、协议要点、已验证结论 |

## 许可证

本项目以 **GPL-3.0-or-later** 许可证发布（见 [LICENSE](LICENSE)），继承 Zed
（crates/ui、theme、terminal 等派生代码）的 copyleft 语义：你可以自由使用、
修改、分发本软件（含商用），但任何基于本软件的衍生作品**必须**以
GPL-3.0-or-later 开源。不接受未经 Copyright Holder 同意的闭源分发。

第三方组件的版权与许可证声明见 [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)。

本仓库不接受 Pull Request（已在仓库设置中禁用）。

