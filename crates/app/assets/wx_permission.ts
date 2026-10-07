import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

/**
 * 微信远程控制的**逐次审批**扩展（060 §4.1 审批桥，档 2 硬前置）。
 *
 * pi 默认**不**逐次询问工具调用（`security.md`），所以没有这个扩展，
 * pi 根本不会发 `extension_ui_request`，微信那头永远收不到审批。
 *
 * 加载：app 随包分发，spawn 时 `-e <绝对路径>`（与 `full_activate.ts` 同机制，
 * 见 `tools_recipe::wx_permission_script_path`）。
 *
 * **自门控**：读 `wxprobe-state.json` 的 `bot_token` ——
 *   * 没扫码 → 整个 handler 不注册，**对既有用户零行为变化**；
 *   * 扫过码 → 走 pi 标准 `ctx.ui.confirm`，请求同时出现在桌面弹窗与微信
 *     （§4.1 双端并行，谁先应答算谁的，pi 只 resolve 一次）。
 *
 * 门控放在扩展内部而不是 spawn 参数上，是为了「扫码即生效、reset 即失效」
 * 单一事实源（P4 的设置页开关也复用同一个 token 字段）。
 */

/** `~/.pi-flash/wxprobe-state.json`（或 `PI_FLASH_DIR` 覆盖）里有没有 token。 */
function hasBotToken(): boolean {
  try {
    const dir = process.env.PI_FLASH_DIR ?? join(homedir(), ".pi-flash");
    const raw = readFileSync(join(dir, "wxprobe-state.json"), "utf8");
    const token = JSON.parse(raw)?.bot_token;
    return typeof token === "string" && token.trim().length > 0;
  } catch {
    // 没文件 / 读失败 / JSON 坏 —— 一律视为未启用
    return false;
  }
}

/**
 * 要不要问 —— 用 pi 文档给的 Codex 对齐写法：
 * 标了 `readOnlyHint` 的只读工具不问；标了
 * `destructiveHint:false && openWorldHint:false` 的也不问；其余都问。
 */
function needsApproval(pi: ExtensionAPI, toolName: string): boolean {
  const hints = pi.getAllTools().find((t) => t.name === toolName)?.annotations;
  if (hints?.readOnlyHint === true) return false;
  return (
    hints?.destructiveHint === true ||
    (!hints?.readOnlyHint &&
      ((hints?.destructiveHint ?? true) || (hints?.openWorldHint ?? true)))
  );
}

/** 给审批消息带上的参数摘要 —— 长 JSON 截断，别把微信一条消息撑爆。 */
function summarize(input: unknown): string {
  if (input === null || input === undefined) return "";
  let text: string;
  try {
    text = typeof input === "string" ? input : JSON.stringify(input);
  } catch {
    return "";
  }
  text = text.replace(/\s+/g, " ").trim();
  return text.length > 300 ? `${text.slice(0, 300)}…` : text;
}

export default function (pi: ExtensionAPI) {
  // 没扫码 = 不接管审批（见文件头「自门控」）
  if (!hasBotToken()) return;

  pi.on("tool_call", async (event, ctx) => {
    if (!needsApproval(pi, event.toolName)) return;
    const detail = summarize(event.input);
    const ok = await ctx.ui.confirm(
      `允许执行 ${event.toolName}？`,
      detail || event.toolName,
    );
    if (!ok) {
      return { block: true, reason: `${event.toolName} was not approved` };
    }
  });
}
