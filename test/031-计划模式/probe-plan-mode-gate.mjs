// 031-1 P0 闸门探针（需可用模型，探针自选 get_available_models 末项）：
// /plan 后 write 被打回（核心断言）、read 直通与 plan-off 恢复（弱模型可能
// 不肯调工具，正式断言留 P4）。用法: node "test/031-计划模式/probe-plan-mode-gate.mjs"
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const ROOT = resolve(import.meta.dirname, "..", "..");
const CLI = join(
  ROOT,
  "vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js",
);
const EXT = join(ROOT, "crates/app/src/mode/plan_mode.ts");
const work = mkdtempSync(join(tmpdir(), "pf-plan-gate-"));
writeFileSync(join(work, "sample.txt"), "hello-from-probe\n");

const proc = spawn(
  process.execPath,
  [CLI, "--mode", "rpc", "--tools", "read,bash,edit,write,present_plan", "-e", EXT],
  { cwd: work, stdio: ["pipe", "pipe", "pipe"] },
);
proc.stderr.on("data", (d) => process.stderr.write(`[pi-err] ${d}`));

const toolResults = new Map(); // toolCallId -> {isError, text}
const toolStarts = []; // {toolName, id}
const typeCounts = {};
const responses = new Map();
const assistantEnds = [];
let buf = "";
proc.stdout.on("data", (d) => {
  buf += d.toString();
  let idx;
  while ((idx = buf.indexOf("\n")) >= 0) {
    const line = buf.slice(0, idx).trim();
    buf = buf.slice(idx + 1);
    if (!line) continue;
    try {
      const v = JSON.parse(line);
      typeCounts[v.type] = (typeCounts[v.type] ?? 0) + 1;
      if (v.type === "response") responses.set(v.id, v);
      else if (v.type === "tool_execution_start") {
        toolStarts.push({ toolName: v.toolName, id: v.toolCallId });
      } else if (v.type === "tool_execution_end") {
        const text = (v.result?.content ?? [])
          .filter((c) => c.type === "text")
          .map((c) => c.text)
          .join("\n");
        toolResults.set(v.toolCallId, { toolName: v.toolName, isError: !!v.isError, text });
      } else if (v.type === "message_end" && v.message?.role === "assistant") {
        const text = (v.message.content ?? [])
          .filter((c) => c.type === "text")
          .map((c) => c.text)
          .join("\n");
        assistantEnds.push(text);
      }
    } catch {
      /* ignore */
    }
  }
});

let nextId = 0;
const send = (obj) => {
  const id = `c${++nextId}`;
  proc.stdin.write(JSON.stringify({ ...obj, id }) + "\n");
  return id;
};
const wait = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(name, cond, timeoutMs = 300000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const r = cond();
    if (r !== undefined && r !== false) return r;
    await wait(150);
  }
  throw new Error(`timeout: ${name}`);
}

/** 等 agent 空闲：连续 2.5s 无新工具结果视为本轮结束。 */
async function settleTurn() {
  const before = toolResults.size;
  await wait(2500);
  while (toolResults.size !== before) {
    await wait(2500);
  }
}

const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};

const findResult = (toolName, nth = 0) => {
  const ids = toolStarts.filter((s) => s.toolName === toolName).map((s) => s.id);
  const r = toolResults.get(ids[nth]);
  return r;
};

try {
  // 默认模型 pattern 可能失配（PF 平时显式传 --model）：取可用列表后显式 set_model
  const idm = send({ type: "get_available_models" });
  await until("models list", () => responses.get(idm));
  const models = responses.get(idm)?.data?.models ?? [];
  if (!models.length) throw new Error("no available models configured");
  const pick = models.at(-1);
  console.log(`[debug] using model ${pick.provider}/${pick.id ?? pick.modelId}`);
  const ids = send({ type: "set_model", provider: pick.provider, modelId: pick.id ?? pick.modelId });
  await until("set_model", () => responses.get(ids));

  send({ type: "prompt", message: "/plan" });
  await wait(1500);

  // 1) 计划态：write 必须被打回
  send({
    type: "prompt",
    message:
      "Use the write tool to create a file named probe-blocked.txt with content 'hi'. Call the tool now.",
  });
  await until("write attempt", () => toolStarts.some((s) => s.toolName === "write"));
  await settleTurn();
  const wr = findResult("write");
  check(
    "plan mode blocks write (isError + reason)",
    wr?.isError === true && wr.text.includes("plan mode"),
    wr?.text.slice(0, 120),
  );

  // 2) 计划态：read 直通
  send({
    type: "prompt",
    message:
      "Your next action MUST be a tool call: call the read tool with path \"sample.txt\". Do not write any reply text, do not call present_plan. Just call read now.",
  });
  await until("read attempt", () => toolStarts.some((s) => s.toolName === "read"));
  await settleTurn();
  const rd = findResult("read");
  check("plan mode allows read (no error)", rd?.isError === false, rd?.text.slice(0, 120));

  // 3) /plan off：写恢复
  send({ type: "prompt", message: "/plan off" });
  await wait(1500);
  send({
    type: "prompt",
    message:
      "Use the write tool to create a file named probe-ok.txt with content 'done'. Call the tool now.",
  });
  await until("write(2) attempt", () => toolStarts.filter((s) => s.toolName === "write").length >= 2);
  await settleTurn();
  const r2 = findResult("write", 1);
  const onDisk = existsSync(join(work, "probe-ok.txt"));
  check("plan off restores write (result ok + file on disk)", r2?.isError === false && onDisk, `disk=${onDisk}`);
} catch (e) {
  check("gate probe completed", false, String(e));
  console.log(`[debug] toolStarts=${JSON.stringify(toolStarts)}`);
  console.log(`[debug] typeCounts=${JSON.stringify(typeCounts)}`);
  console.log(`[debug] responses=${JSON.stringify([...responses.entries()].map(([k, v]) => ({ id: k, cmd: v.command, data: v.data })))}`);
  console.log(`[debug] assistantEnds=${JSON.stringify(assistantEnds.map((t) => t.slice(0, 200)))}`);
} finally {
  proc.kill();
  await wait(300);
  try {
    rmSync(work, { recursive: true, force: true });
  } catch {}
}

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed`);
process.exit(failed.length ? 1 : 0);
