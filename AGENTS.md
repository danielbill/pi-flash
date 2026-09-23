打算用rust重写pi-web，获取一个像ZED一样极速的pi coding agent的桌面端。

pi-web ： D:\github\---ai-tools---\pi-web
zed 参考源码（稀疏浅克隆）: D:\github\zed  → 只取 crates/gpui（UI框架+45个示例）、terminal、agent_ui、agent、acp_thread、extension、docs
pi ACP 接入：pi 本体无 acp 模式，走 pi-acp 适配器（npm svkozak/pi-acp，spawn `pi --mode rpc --no-themes`）；本机已装于 Zed external_agents，Zed settings agent_servers 已配置
pi-flash 架构倾向：做 ACP client（参考 zed crates/acp_thread + agent_ui），复用 pi-acp；直连 pi RPC 作为后备优化
目标平台：Windows + macOS 双端（GPUI 原生支持，macOS 反而是最成熟后端）；macOS 包用 GitHub Actions macos runner 构建，Windows 不做 mac 交叉编译
已验证（2025-09）：hello-gpui 冒烟通过 —— rustc 1.90 msvc + crates.io gpui 0.2.2（自包含，blade/Vulkan 后端），窗口/渲染/字体/DPI 150% 全 OK；首编 ~3.5min，增量 6s；截图 hello-gpui/win4.png
spike 已通（2025-09）：spike/ = GPUI 聊天窗 + pi --mode rpc 子进程桥（JSONL/text_delta 流式/Enter发送/Esc中断），端到端验证 OK（spike/final.png）。要点：user content 是块数组[{type:text,text:..}]；tool 结果以 user-role 消息回灌（后续要过滤）；ListState Bottom 对齐吸底可用；0.2.2 spawn 用 async move |weak, &mut AsyncApp| 闭包
