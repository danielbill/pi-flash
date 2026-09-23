# pi-flash

Zed 级速度的 pi coding agent 桌面端 —— 用 Rust 重写 [pi-web](https://github.com/agegr/pi-web) 的壳。

## 架构

```
┌──────────────────────────┐      ACP / pi RPC (JSONL over stdio)      ┌─────────────────┐
│  pi-flash (Rust + GPUI)  │ ────────────────────────────────────────  │  pi sidecar     │
│  原生 UI / 流式渲染      │   先复用 pi-acp 适配器，后备直连 RPC      │  agent core     │
└──────────────────────────┘                                           └─────────────────┘
```

- **UI**: GPUI（Zed 同款框架，Windows DirectX / macOS Metal 原生渲染）
- **桥接**: pi 子进程 `--mode rpc`，JSONL 协议（见 pi docs/rpc.md）；后期评估直连 ACP client 形态
- **目标平台**: Windows + macOS（macOS 包走 GitHub Actions macos runner）

## 目录

| 路径 | 说明 |
|---|---|
| `hello-gpui/` | GPUI 最小冒烟：crates.io gpui 0.2.2 独立编译、窗口、渲染、DPI |
| `spike/` | RPC 桥接原型：GPUI 聊天窗 ↔ `pi --mode rpc`，流式 text_delta 上屏 |
| `AGENTS.md` | 项目记忆：架构决策、协议要点、已验证结论 |

## 已验证

- [x] Rust 1.90 msvc + gpui 0.2.2（blade/Vulkan）Windows 编译运行，150% DPI 正常
- [x] pi 子进程 spawn、JSONL 双向通信、`message_update/text_delta` 流式渲染
- [x] 多轮对话、工具调用回路（pi 侧 read/bash 结果回灌）
- [ ] Markdown 渲染 / 代码高亮 / tool call 卡片
- [ ] 多会话管理（恢复 / fork / steering）
- [ ] macOS 构建链

## 参考源码

- [zed](https://github.com/zed-industries/zed)（稀疏克隆：gpui、terminal、agent_ui、acp_thread）
- [pi-acp](https://github.com/svkozak/pi-acp)：Zed 里接入 pi 的 ACP 适配器（底层即 `pi --mode rpc`）
