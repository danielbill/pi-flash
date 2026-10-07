// P2 对拍驱动：跑 ZCode **真实源码**，输出每条 case 的结果（JSONL）。
//
//   node run.ts > golden.jsonl
//
// 唯一改动是 import 说明符（见 prep 脚本），函数体逐字保留 —— 对拍要比的就是它们。
import { readFileSync } from "node:fs";

import {
  extractBotAssistantResponseMessages,
  formatBotAssistantReplyBlocks,
  formatBotPermissionRequestSummary,
  formatBotToolCallReply,
  formatBotToolCallSummaryLine,
  isBotToolCallReplyTerminal,
} from "./replyFormatter.ts";
import { formatStatusTaskLine, formatTaskRunningDuration } from "./statusFormatting.ts";
import { getPermissionRequestPreview } from "./permission-request-preview.ts";
import { getCompactToolCallSummary } from "./tool-call-summary.ts";

type Case = Record<string, any>;

const cases: Case[] = JSON.parse(
  readFileSync(new URL("./cases.json", import.meta.url), "utf8"),
);

function toolCall(c: Case) {
  return {
    toolId: c.id,
    parentToolUseId: null,
    title: c.title ?? undefined,
    kind: c.toolKind ?? undefined,
    input: c.input,
    output: c.output ?? undefined,
    status: c.status ?? undefined,
    error: c.error ?? undefined,
    raw: c.raw ?? undefined,
  };
}

function options(c: Case) {
  return { locale: c.locale, workspacePath: c.workspacePath ?? undefined };
}


const lines: string[] = [];
for (const c of cases) {
  let out: unknown;
  switch (c.kind) {
    case "tool_line":
      out = formatBotToolCallSummaryLine(toolCall(c), options(c));
      break;
    case "tool_reply":
      out = formatBotToolCallReply(toolCall(c), options(c));
      break;
    case "blocks":
      out = formatBotAssistantReplyBlocks(c.blocks, options(c));
      break;
    case "perm_summary":
    case "perm_preview": {
      const req = {
        title: c.title ?? undefined,
        description: c.description,
        kind: c.permissionKind,
        raw: c.raw,
      };
      out =
        c.kind === "perm_summary"
          ? formatBotPermissionRequestSummary(req, options(c))
          : getPermissionRequestPreview(req);
      break;
    }
    case "flush":
      out = extractBotAssistantResponseMessages(c.buffer, c.force);
      break;
    case "terminal":
      out = isBotToolCallReplyTerminal(c.status ?? undefined);
      break;
    case "duration":
      out = formatTaskRunningDuration(c.ms);
      break;
    case "task_line":
      out = formatStatusTaskLine({ title: c.title, taskId: c.taskId }, c.label);
      break;
    case "compact_summary":
      out = getCompactToolCallSummary({
        title: c.title ?? undefined,
        kind: c.toolKind,
        input: c.input,
        output: c.output ?? undefined,
        raw: c.raw ?? undefined,
      });
      break;
    default:
      throw new Error(`未知 case kind: ${c.kind}`);
  }
  lines.push(JSON.stringify({ id: c.id, out }));
}

console.log(lines.join("\n"));
