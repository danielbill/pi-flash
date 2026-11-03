// 诊断探针：记录关键时点的工具集到 mcp-probe.log（每进程追加一行）。
// 挂载顺序必须排在 full_activate.ts 之后（模拟 PF custom 档：full_activate 最后）。
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { appendFileSync } from "node:fs";

const LOG = process.env.MCP_PROBE_LOG ?? "mcp-probe.log";

function log(tag: string, pi: ExtensionAPI) {
  let active: string[] = [];
  let all: string[] = [];
  try {
    active = pi.getActiveTools?.() ?? [];
  } catch (e) {
    active = [`ERR:${String(e)}`];
  }
  try {
    all = (pi.getAllTools?.() ?? []).map((t: { name: string }) => t.name);
  } catch (e) {
    all = [`ERR:${String(e)}`];
  }
  const line = `${new Date().toISOString()} [${tag}] pid=${process.pid}\n  active(${active.length})= ${JSON.stringify(active)}\n  all(${all.length})= ${JSON.stringify(all)}\n`;
  try {
    appendFileSync(LOG, line);
  } catch (e) {
    console.error(`[probe] log write failed: ${String(e)}`);
  }
  console.error(`[probe] ${tag} active=${JSON.stringify(active)}`);
}

export default function (pi: ExtensionAPI) {
  // after full_activate 的 session_start 已跑完（同事件按挂载序派发，probe 最后注册）
  pi.on("session_start", () => log("session_start(post-full_activate)", pi));
  // 轮次真正发模型前的最终 active 集 —— 「挂没挂上」的裁决点
  pi.on("before_agent_start", () => log("before_agent_start", pi));
}
