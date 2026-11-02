# 042 模型菜单：新会话点行出 ✓ / pi 启动即死后仍可选可发

## 背景（2026-11-02 真机，发版前）

pi 1.5.0（`5f754ed0`）对 models.json 校验变严：残缺 cost 整份拒载 → 商消失 →
`--model my-glm/...` 启动即 `Error: Model not found` 退出。这是外因（0619cb24 已修
文件层）。它第一次把 PF 两处老缺口推到台面：

1. **退出臂不清死进程句柄**（`session/runtime/mod.rs` 退出臂）：`agent.session`
   仍 Some → `select_model` 把 SetModel 写进死管道（✓ 永远不动、pill 不动），
   `ensure_spawned` 认为活着不重 spawn（发送转圈）。
2. **菜单 ✓ 只读 `state.model`**（`model_picker.rs::view`）：草稿 / 死进程时
   `select_model` 只记 `pending_model`，✓ 不读它——新会话点任何行都不出 ✓。

修法：退出臂 `agent.session = None`（与 `shutdown_process` 同口径）；✓ 与 pill /
snapshot `draft_model` 统一为 **pending → state → 兜底**（pending 只在无活进程时
写入，是用户最新意图；活进程下恒空）。

## 自动化：`run.sh`（离线、确定性）

隔离 `PI_FLASH_DIR` + `PI_CODING_AGENT_DIR`；settings.json 不带 packages（免 pi 首启
npm 装扩展）；星标指向**不存在的商** `nope/ghost` 制造启动即死（已知商下的未知
模型 id pi 只警告合成，不退出）。新 op `model.pick {provider,id}` = 行点击同链
（↑/↓/Enter 是 GPUI action，合成按键到不了卡片）；断言读 dialogs 面
`model_current_ref`（✓ 行）与 session 面 `draft_model`（pill）。

```bash
cargo build
bash test/042-1/picker-current/run.sh
```

| 用例 | 断言 |
|---|---|
| CASE1 | 发送 → pi 以 `--model nope/ghost` 退出 → status 含 `Model … not found`、无 streaming；死会话里 `model.pick` 生效（pill 变、✓ 在该行）；再发送 → 重 spawn `--model myds/deepseek-v4-pro` → `get_state` 回包 model 就位；rpc 日志两次 spawn 参数如上 |
| CASE2 | 新草稿：未选无 ✓；点行即收菜单、pill = 所选；重开 ✓ 在所选行；再点另一行 ✓ 跟着走 |

## 人工验收

新会话点模型行 → 行前出 ✓、pill 同步；之前转圈的会话（nihao / ????）重开后可发送。
