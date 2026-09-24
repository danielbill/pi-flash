打算用rust重写pi-web，获取一个像ZED一样极速的pi coding agent的桌面端。

pi-web ： D:\github\---ai-tools---\pi-web
zed 参考源码（稀疏浅克隆）: D:\github\zed  → 只取 crates/gpui（UI框架+45个示例）、terminal、agent_ui、agent、acp_thread、extension、docs
pi ACP 接入：pi 本体无 acp 模式，走 pi-acp 适配器（npm svkozak/pi-acp，spawn `pi --mode rpc --no-themes`）；本机已装于 Zed external_agents，Zed settings agent_servers 已配置
产品策略（定案）：1:1 复刻 pi-web，不自创产品；Rust+GPUI 只做“更快的壳”。详见 PORT_PLAN.md（模块映射/M1-M6 里程碑/极速预算）
关键对齐：内置钉版 pi（对齐 pi-web 精确钉 @earendil-works/pi-coding-agent 无^），vendor 进应用分发，spawn `node <app>/vendor/pi/dist/bundle/cli.js --mode rpc`，绝不读 PATH/系统 pi；升级=bump PIN+协议符合性测试。此前“ACP client 复用 pi-acp”方案作废（pi-acp 反而读系统 pi，不稳）
目标平台：Windows + macOS 双端（GPUI 原生支持，macOS 反而是最成熟后端）；macOS 包用 GitHub Actions macos runner 构建，Windows 不做 mac 交叉编译
已验证（2025-09）：hello-gpui 冒烟通过 —— rustc 1.90 msvc + crates.io gpui 0.2.2（自包含，blade/Vulkan 后端），窗口/渲染/字体/DPI 150% 全 OK；首编 ~3.5min，增量 6s；截图 hello-gpui/win4.png
ChatInput 深水区完成（73d2607）：斜杠菜单（get_commands 实测 42 命令、↑↓/Tab/Enter/Esc 键盘流+鼠标、选中高亮）、@文件菜单（walk_files 深度3上限400，跳过 .git/node_modules/target）、prompt 历史（↑↓）、Shift+Enter 换行。教训：heredoc 大补丁会静默失败→改用补丁脚本文件；GPUI 交互 handler 漏 cx.notify() 则状态变 UI 不动。界面大改版（29a1c63）：theme.rs 4 主题逐值翻译（mist 默认）+ 侧栏完整结构（新建/搜索/项目框/分支框/会话行时间·条数/文件浏览器/底部导航）+ 顶部工具栏 pills + 令牌统计 + 输入框工具行 + 每消息 usage 脚注（Usage 解析自 wire，chrono 时间戳）。用户气泡/列表/工具卡全部主题化。方法定案：逐组件读 pi-web 源码→令牌级翻译（不可直接粘贴 React/Tailwind）。待办：SVG 图标、ChatInput 多行/历史/斜杠菜单、生成标题、模型切换下拉、分支导航。此前 M1 记录：steer（流式中 Enter=steer，rpc steer 命令）、get_state 快照（SessionState/ModelInfo，头部实时显示 provider/model）、AgentSettled 后自动刷新状态。同前：markdown 渲染（pulldown-cmark→StyledText）、块化消息（text/thinking/toolCall 卡片带 args+result）、会话侧栏（list_sessions 扫 ~/.pi/agent/sessions）、--session 恢复 + get_messages 回放（546 条历史验证 m1h.png）。协议实测修正：内容块类型 camelCase 'toolCall'、args 起始走 partialJson、tool 结果 role='toolResult'、client 不得硬编码 --no-session（会压掉 --session）。31 测试绿。下一步：M1 收尾（follow_up/steer、模型显示/切换、消息内跳转）→ M2
spike 已通（2025-09）：spike/ = GPUI 聊天窗 + pi --mode rpc 子进程桥，端到端 OK（spike/final.png）。要点：user content 是块数组[{type:text,text:..}]；tool 结果以 user-role 消息回灌（后续要过滤）；ListState Bottom 对齐吸底可用；0.2.2 spawn 用 async move |weak, &mut AsyncApp| 闭包
