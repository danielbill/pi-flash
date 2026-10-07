# P2 对拍 harness

跑 **ZCode 真实 TS 源码**产出黄金结果，再与 Rust 实现逐字节比对。

```bash
# 1) 重新生成拷贝（ZCode 仓库在 D:\github\---harness-tools---\ZCode）
python prep_parity.py          # 已删除；脚本见下方「重新生成」一节

# 2) 跑 TS 原文产出黄金文件
node parity/run.ts > parity/golden.jsonl

# 3) Rust 侧比对（不一致即非零退出）
cargo run -p wxprobe -- parity
```

## 文件来源与许可

`*.ts`（除 `run.ts` / `shared.ts` / `stub.ts`）是 ZCode v3.14.3 源码的**逐字节拷贝**，
ZCode 采用 **Apache-2.0**（见其根 `LICENSE`）。此处拷贝仅用于兼容性对拍，
**唯一改动是 import 说明符**（`@zcode/shared` → `./shared.ts` 等），函数体未动 ——
要比的正是它们。原项目：`https://github.com/zai-org/ZCode`。

| 文件 | ZCode 原路径 |
|---|---|
| `tool-call-summary.ts` | `packages/shared/src/tool-call-summary.ts` |
| `permission-request-preview.ts` | `packages/shared/src/permission-request-preview.ts` |
| `messages.ts` | `packages/services/src/bots/messages.ts` |
| `replyFormatter.ts` | `packages/services/src/bots/replyFormatter.ts` |
| `commandParser.ts` | `packages/services/src/bots/commandParser.ts` |
| `statusFormatting.ts` | `packages/services/src/bots/statusFormatting.ts` |

`shared.ts` / `stub.ts` 是本仓库自写的垫片（`@zcode/shared` 与
`../session/taskChangeSummary.js` 的最小替代），`run.ts` 是对拍驱动。

## 重新生成拷贝

原 `prep_parity.py` 做三件事，改回 ZCode 版本或升级基线时重跑一次：

1. 把上表 6 个文件拷进本目录，用正则改写 import 说明符；
2. 写 `package.json`（`"type": "module"`，让 Node 走 ESM）；
3. 写 `shared.ts`（`@zcode/shared` 垫片）与 `stub.ts`（`buildPerTurnChangeSummaries` 垫片）。

## 运行要求

**Node ≥ 24**（依赖其原生 type-stripping，直接 `.ts` 跑，不需要装依赖），
`pnpm install` 一次都不用。

## 覆盖范围

42 条 case 覆盖 P2 界面层的全部导出面：`formatBotToolCallSummaryLine`、
`formatBotToolCallReply`、`formatBotAssistantReplyBlocks`、
`formatBotPermissionRequestSummary`、`getPermissionRequestPreview`、
`extractBotAssistantResponseMessages`、`isBotToolCallReplyTerminal`、
`formatTaskRunningDuration`、`formatStatusTaskLine`、
`getCompactToolCallSummary`，zh-CN / en-US 双语。

**未覆盖**：`formatStatusLine` / `formatStatusStateValue` —— 它们是
`createBotsService` 的内部闭包，未导出，无法从外部调用（由 `format/status.rs`
的单测按源码语义固化）。

## 加 case

改 `cases.json` 后：

```bash
node parity/run.ts > parity/golden.jsonl     # 刷新黄金
cargo run -p wxprobe -- parity --update      # 用 Rust 侧重写一遍做交叉校验
cargo run -p wxprobe -- parity               # 应输出 PARITY OK
```
