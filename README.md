# pi-flash

Zed 级速度的 pi coding agent 桌面端 —— 用 Rust 重写 [pi-web](https://github.com/agegr/pi-web) 的壳。

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
| `hello-gpui/`、`spike/` | 历史冒烟与原型，保留作参考 |
| `AGENTS.md` | 项目记忆：架构决策、协议要点、已验证结论 |

## 已验证

- [x] Rust 1.90 msvc + gpui 0.2.2（blade/Vulkan）Windows 编译运行，150% DPI 正常
- [x] pi 子进程 spawn、JSONL 双向通信、`message_update/text_delta` 流式渲染
- [x] 多轮对话、工具调用回路（pi 侧 read/bash 结果回灌）
- [x] **M1 骨架落地**：workspace + pi-link 协议层（测试绿）+ app 聊天核心（截图 m1.png）
- [ ] Markdown 渲染 / 代码高亮 / tool call 卡片
- [ ] 多会话管理（恢复 / fork / steering）
- [ ] macOS 构建链

## 参考源码

- [zed](https://github.com/zed-industries/zed)（稀疏克隆：gpui、terminal、agent_ui、acp_thread）
- [pi-acp](https://github.com/svkozak/pi-acp)：Zed 里接入 pi 的 ACP 适配器（底层即 `pi --mode rpc`）
