# 001-PFContext · pfctx-manager（agent 动态自管理设置工具）

对象：`extensions/pfctx-manager`（工具 `pfctx_manager`）+ 宿主执行链
（`services/pfctx_ops.rs` → 设置页同链）+ 开关（hub `agent.self_debug`）。
设计：`docs/模块设计/001-PFContext/001-4-PFContext内部工具化.md`。

## A 工具面（无 UI，纯扩展校验）

### A1 目录校验：合法 op 全过

- 步骤：合法 op 跨段各一（capacity=8 / send_mode=steer / sound=false /
  default_model="p/m" 与 null / 字体对象）。
- 期望：0 错误。
- 自动：`extensions/pfctx-manager/test/manager.test.ts::accepts well-formed ops`

### A2 目录外拒绝

- 步骤：`no_such.param`。
- 期望：1 错误，提示「目录外 + 先 query」。
- 自动：`…::rejects out-of-catalog keys with guidance`

### A3 恒拒项带原因

- 步骤：`subagents.max_depth=2`、`appearance.icon_theme`。
- 期望：各 1 错误（恒 1 / 无消费者）。
- 自动：`…::rejects permanently-denied params with reasons`

### A4 值域/枚举/类型闸

- 步骤：capacity=1/11、auto_clean_days=-1、send_mode="bogus"、sound="on"、
  capacity="6"（字符串）、default_model="no-slash"。
- 期望：逐条 1 错误；边界值 10 通过。
- 自动：`…::enforces numeric ranges…` / `…::enforces enums and types`

### A5 注册形态

- 期望：恰好注册 1 个工具，名 `pfctx_manager`。
- 自动：`…::registers exactly one tool named pfctx_manager`

## B 挂载链（无 UI，Rust）

### B1 dev 布局定位随仓扩展

- 期望：`pi_link::pfctx_manager::ext_path()` 定位到
  `extensions/pfctx-manager/src/index.ts`。
- 自动：`crates/pi-link/src/pfctx_manager.rs::dev_layout_resolves_bundled_extension`

### B2 宿主意愿硬闸

- 期望：`mount_target(false)` = None（自我调试关 = 绝不挂）。
- 自动：`…::host_switch_is_the_only_gate` + `…::rel_matches_extension_layout`

## C 宿主桥（无 UI，gpui 实体上下文）

### C1 op 条目入队

- 步骤：构造 `entry_appended` WireEvent（customType=pfctx-manager-op，双 op）。
- 期望：队列 FIFO 收 2 条，session 归属正确，drain 即清空。
- 自动：`crates/app/src/session/runtime/events/on_event.rs::pfctx_manager_op_entry_feeds_queue`

### C2 他人条目不串台

- 步骤：customType=subagent-settled 条目。
- 期望：op 队列保持空。
- 自动：`…::other_custom_entries_do_not_feed_queue`

### C3 执行面同链（hub 写路径）

- 说明：dispatcher 各 op 与设置页调用点同函数，hub.set→落盘→投影链由
  pf-context 50 项单测覆盖（S1-S8）；dispatcher 自身 = 薄 match，拒绝臂
  与 A2-A4 同目录。app 级运行验证并入 D（真链路）。

## D 端到端：agent 真调用（无 UI；需已配置模型，`#[ignore]` 门控）

### D1 agent 调用 set → 条目广播

- 前置：模型已配置（pi 正常可用）；驱动/探针 spawn pi，挂 pfctx-manager
  扩展 + `--tools pfctx_manager`。
- 步骤：发 prompt 指示 agent 调 `pfctx_manager`（set capacity=8）。
- 期望：事件流出现 `pfctx_manager` 工具调用 + `entry_appended`
  （customType=pfctx-manager-op，ops 含 capacity=8）。
- 自动：`cargo test -p pi-link --test live_pfctx_manager_probe -- --ignored --nocapture`
- 实测注记（2026-11-09，4.5s 过）：三个环境坑——①机器默认模型免费档配额耗尽
  （429），探针显式 `--model deepseek-flash`（env `PFCTX_PROBE_MODEL` 可覆盖）；
  ②RPC 裸 spawn 必须**先握手**（GetState/GetAvailableModels/GetCommands）再
  prompt，否则受理后零事件；③pi 工具结果契约 `content` 必须是块数组（纯字符串
  在 pi 归一化时 `content.some` 崩）——曾因此工具结果报错，已修。

### D2 宿主执行落盘（真机，配合运行中的 PF）

- 步骤：D1 同款操作在真实 PF 会话内。
- 期望：设置页数值 2s 内变；`pfctx.log` 出现 `[agent]` 与 `[write]` 行；
  `ext-context.json` 的 subagents 更新。
- 自动：真机时 `tail pfctx.log` 核对（PF 运行中无法起第二实例断言）。

### D3 开关注入/移除

- 步骤：设置-更多【自我调试】关 → 空闲会话重绑。
- 期望：重绑后工具列表无 pfctx_manager；再开 → 回归。
- 自动：真机项（UI 开关 + 重绑观察）；开关存储/默认值由
  `pf-context::s8_agent_section_defaults_on_and_persists` 锁定。

## E S7.1 重载差值（无 UI）

- 步骤：外部改 app-settings.json capacity 5→8 → check_external_change。
- 期望：pfctx.log 出现 `[reload] subagents: …5… → …8…`。
- 自动：`crates/pf-context/src/settings/tests.rs::s8_reload_diff_logged_with_values`
