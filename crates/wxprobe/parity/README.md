# 对拍 harness（P2 + P3）

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

唯一例外是 `bot-service.ts`：它是**按行号从 `botsService.ts` 抽取的片段**
（原文那些函数是模块私有、不 `export` 就调不到），除 import 说明符外还给顶层声明
加了 `export` 前缀。**去前缀后与原文逐行零差异**（抽取脚本自检过）。

| 文件 | ZCode 原路径 |
|---|---|
| `tool-call-summary.ts` | `packages/shared/src/tool-call-summary.ts` |
| `permission-request-preview.ts` | `packages/shared/src/permission-request-preview.ts` |
| `messages.ts` | `packages/services/src/bots/messages.ts` |
| `replyFormatter.ts` | `packages/services/src/bots/replyFormatter.ts` |
| `commandParser.ts` | `packages/services/src/bots/commandParser.ts` |
| `statusFormatting.ts` | `packages/services/src/bots/statusFormatting.ts` |
| `bot-service.ts` | `packages/services/src/bots/botsService.ts` **:492-633**（片段） |

`shared.ts` / `stub.ts` 是本仓库自写的垫片（`@zcode/shared` 与
`../session/taskChangeSummary.js` 的最小替代），`run.ts` 是对拍驱动。

## 重新生成拷贝

原 `prep_parity.py` 做三件事，改回 ZCode 版本或升级基线时重跑一次：

1. 把上表 6 个文件拷进本目录，用正则改写 import 说明符；
2. 从 `botsService.ts` 按行号 `492-633` 抽出 `bot-service.ts`：改 import 说明符，
   并给顶层声明加 `export`（自检：去掉前缀后与原文逐行零差异）；
3. 写 `package.json`（`"type": "module"`，让 Node 走 ESM）；
4. 写 `shared.ts`（`@zcode/shared` 垫片）与 `stub.ts`（`buildPerTurnChangeSummaries` 垫片）。

## 运行要求

**Node ≥ 24**（依赖其原生 type-stripping，直接 `.ts` 跑，不需要装依赖），
`pnpm install` 一次都不用。

## 覆盖范围

**71 条 case**，zh-CN / en-US 双语，覆盖：

* **P2**：`formatBotToolCallSummaryLine`、`formatBotToolCallReply`、
  `formatBotAssistantReplyBlocks`、`formatBotPermissionRequestSummary`、
  `getPermissionRequestPreview`、`extractBotAssistantResponseMessages`、
  `isBotToolCallReplyTerminal`、`formatTaskRunningDuration`、
  `formatStatusTaskLine`、`getCompactToolCallSummary`
* **P3**：`formatSelectionFallback`（微信纯文本编号菜单）、
  `getBotPermissionOptionDisplayKind`、`sortBotPermissionOptions`、
  `formatBotPermissionOptionLabel`、`formatBotPermissionOptionDescription`、
  `isBotPermissionRejectOption`，以及 `permission.respond` 分支的
  `Number.parseInt(..., 10) - 1` 下标解析（越界 / NaN / 溢出 / radix-10 的 `0x`）

**未覆盖**：
* `formatStatusLine` / `formatStatusStateValue` —— `createBotsService` 的内部闭包，
  未导出，外部调不到（由 `format/status.rs` 单测按源码语义固化）
* `permission.respond` 的**持久化语义**（ACK 成功才写 `handledAt`）—— 属状态机而非
  字符串，由 pipeline 侧实现并测

## 加 case

改 `cases.json` 后：

```bash
node parity/run.ts > parity/golden.jsonl     # 刷新黄金
cargo run -p wxprobe -- parity --update      # 用 Rust 侧重写一遍做交叉校验
cargo run -p wxprobe -- parity               # 应输出 PARITY OK
```
