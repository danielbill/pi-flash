// 031-1 P0 探针：真实 vendored pi RPC + plan_mode.ts。
// A 阶段：进程内 通道/持久化/切换归零；B 阶段：手工会话文件跨进程冷恢复。
// 无模型依赖。用法: node "test/031-计划模式/probe-plan-mode.mjs"
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { randomUUID } from "node:crypto";

const ROOT = resolve(import.meta.dirname, "..", "..");
const CLI = join(
  ROOT,
  "vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js",
);
const EXT = join(ROOT, "crates/app/src/mode/plan_mode.ts");
const work = mkdtempSync(join(tmpdir(), "pf-plan-probe-"));

const results = [];
const check = (name, ok, detail = "") => {
  results.push({ name, ok });
  console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};

function launch(extraArgs) {
  const proc = spawn(
    process.execPath,
    [CLI, "--mode", "rpc", ...extraArgs, "-e", EXT],
    { cwd: work, stdio: ["pipe", "pipe", "pipe"] },
  );
  proc.stderr.on("data", (d) => process.stderr.write(`[pi-err] ${d}`));
  const events = [];
  const responses = new Map();
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
        if (v.type === "response") responses.set(v.id, v);
        else events.push(v);
      } catch {
        /* 非JSON行忽略 */
      }
    }
  });
  let nextId = 0;
  return {
    proc,
    events,
    responses,
    send(obj) {
      const id = `c${++nextId}`;
      proc.stdin.write(JSON.stringify({ ...obj, id }) + "\n");
      return id;
    },
  };
}

const wait = (ms) => new Promise((r) => setTimeout(r, ms));
async function until(name, cond, timeoutMs = 15000) {
  const t0 = Date.now();
  while (Date.now() - t0 < timeoutMs) {
    const r = cond();
    if (r !== undefined && r !== false) return r;
    await wait(80);
  }
  throw new Error(`timeout waiting: ${name}`);
}
const planStatus = (events) =>
  events.filter(
    (e) => e.type === "extension_ui_request" && e.method === "setStatus" && e.statusKey === "pf-plan",
  );
const parseStatus = (e) => {
  try {
    return JSON.parse(e.statusText);
  } catch {
    return null;
  }
};

try {
  // ============ A 阶段：进程内通道与持久化 ============
  const a = launch(["--tools", "read,bash,edit,write,present_plan"]);
  const lastEnabled = () => {
    const s = planStatus(a.events).at(-1);
    return s ? parseStatus(s)?.enabled : undefined;
  };

  await until("startup status", () => planStatus(a.events).length > 0);
  const s0 = parseStatus(planStatus(a.events).at(-1));
  check("startup broadcast v1 enabled=false", s0?.v === 1 && s0?.enabled === false, JSON.stringify(s0));

  const id1 = a.send({ type: "prompt", message: "/plan" });
  await until("/plan response", () => a.responses.get(id1));
  check("/plan disposition handled", a.responses.get(id1)?.data?.disposition === "handled");
  await until("enabled=true", () => lastEnabled() === true);
  check("/plan broadcast enabled=true", true);

  const id2 = a.send({ type: "prompt", message: "/plan off" });
  await until("/plan off response", () => a.responses.get(id2));
  await until("enabled=false", () => lastEnabled() === false);
  check("/plan off broadcast enabled=false", true);

  const id3 = a.send({ type: "prompt", message: "/plan" });
  await until("/plan(2) response", () => a.responses.get(id3));
  await until("re-enabled", () => lastEnabled() === true);
  const id4 = a.send({ type: "get_entries" });
  await until("get_entries", () => a.responses.get(id4));
  const entries = a.responses.get(id4)?.data?.entries ?? [];
  const planEntry = entries.filter((e) => e.type === "custom" && e.customType === "pf-plan-mode").pop();
  check("persist entry enabled=true", planEntry?.data?.enabled === true, JSON.stringify(planEntry?.data));

  const errs = a.events.filter((e) => e.type === "extension_error");
  check("no extension errors", errs.length === 0, JSON.stringify(errs));
  a.proc.kill();
  await wait(300);

  // ============ B 阶段：手工会话文件 → 新进程冷恢复（裁决 1）============
  // 注：无真实消息前 pi 不落盘（探针实测 ENOENT），PF 真实使用中会话文件
  // 必然存在（有对话），所以直接构造合法 v3 会话文件验磁盘恢复路径。
  const sessFile = join(work, "cold.jsonl");
  const now = new Date().toISOString();
  writeFileSync(
    sessFile,
    [
      JSON.stringify({ type: "session", version: 3, id: randomUUID(), timestamp: now, cwd: work }),
      JSON.stringify({
        type: "custom", id: "e1", parentId: null, timestamp: now,
        customType: "pf-plan-mode", data: { enabled: true },
      }),
    ].join("\n") + "\n",
  );
  const b = launch(["--session", sessFile, "--tools", "read,bash,edit,write,present_plan"]);
  await until("cold restore broadcast", () => {
    const s = planStatus(b.events).at(-1);
    return s ? parseStatus(s)?.enabled === true : false;
  });
  check("cold restore from disk enabled=true", true);
  b.proc.kill();
  await wait(300);
} catch (e) {
  check("probe completed", false, String(e));
} finally {
  try {
    rmSync(work, { recursive: true, force: true });
  } catch {}
}

const failed = results.filter((r) => !r.ok);
console.log(`\n${results.length - failed.length}/${results.length} passed`);
process.exit(failed.length ? 1 : 0);
