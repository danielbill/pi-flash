// 031-1 P5 探针：present_plan 声明随计划态收放（工具只在 plan mode 下提供）。
// 机制：--tools 点名只保证**注册**（白名单分支会无条件激活所有点名工具，
// defaultActive:false 救不了），所以声明态由扩展 setActiveTools 主动收放。
// 本探针验证四条集成路径 + 底层 API 在命令 handler 里可用（appendEntry
// 在 RPC 命令 handler unbound 的前科必须排除）：
//   A 启动态（plan off）声明里无 present_plan（session_start sync）
//   B setActiveTools 加/摘在命令 handler 里生效
//   C /plan off 后声明被摘（plan_mode /plan handler OFF 路径 sync）
//   D /plan on 后声明被加回（ON 路径 sync）
//   F 手工会话文件冷恢复（enabled=true）→ 声明保留
// 无模型依赖。用法: node "test/031-计划模式/probe-plan-tool-exposure.mjs"
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
const PROBE_EXT = join(ROOT, "test/031-计划模式/probe-tools-ext.mjs");
const work = mkdtempSync(join(tmpdir(), "pf-plan-exposure-"));

const results = [];
const check = (name, ok, detail = "") => {
	results.push({ name, ok });
	console.log(`${ok ? "PASS" : "FAIL"}  ${name}${detail ? ` — ${detail}` : ""}`);
};

function launch(extraArgs) {
	const proc = spawn(
		process.execPath,
		[CLI, "--mode", "rpc", ...extraArgs, "-e", EXT, "-e", PROBE_EXT],
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
const statusEvents = (events, key) =>
	events.filter(
		(e) => e.type === "extension_ui_request" && e.method === "setStatus" && e.statusKey === key,
	);
const parseStatus = (e) => {
	try {
		return JSON.parse(e.statusText);
	} catch {
		return null;
	}
};
const lastPlan = (events) => {
	const s = statusEvents(events, "pf-plan").at(-1);
	return s ? parseStatus(s) : undefined;
};
const lastProbe = (events) => {
	const s = statusEvents(events, "pf-probe").at(-1);
	return s ? parseStatus(s) : undefined;
};
const extErrors = (events) => events.filter((e) => e.type === "extension_error");

try {
	// ============ A-E 阶段：活进程四条路径 ============
	const a = launch(["--tools", "read,bash,edit,write,present_plan"]);
	await until("startup pf-plan broadcast", () => lastPlan(a.events) !== undefined);

	// A：启动态（plan off）声明里无 present_plan
	let id = a.send({ type: "prompt", message: "/probe-tools" });
	await until("/probe-tools response", () => a.responses.get(id));
	check("/probe-tools disposition handled", a.responses.get(id)?.data?.disposition === "handled");
	await until("pf-probe broadcast", () => lastProbe(a.events) !== undefined, 8000);
	const startup = lastProbe(a.events);
	check("getActiveTools 可用（命令 handler）", Array.isArray(startup), JSON.stringify(startup));
	check(
		"基线 sanity：read/bash 在声明集",
		Array.isArray(startup) && startup.includes("read") && startup.includes("bash"),
		JSON.stringify(startup),
	);
	check(
		"A 启动态声明无 present_plan（session_start sync）",
		Array.isArray(startup) && !startup.includes("present_plan"),
		JSON.stringify(startup),
	);

	// B：命令 handler 里 setActiveTools 加/摘生效
	id = a.send({ type: "prompt", message: "/probe-add" });
	await until("/probe-add response", () => a.responses.get(id));
	await until("B 加回 present_plan", () => {
		const p = lastProbe(a.events);
		return Array.isArray(p) && p.includes("present_plan");
	});
	check("B setActiveTools 命令 handler 内生效（加）", true);

	// C：/plan off（状态本就 off，no-op 翻转）→ plan_mode 自身 sync 摘除
	id = a.send({ type: "prompt", message: "/plan off" });
	await until("/plan off response", () => a.responses.get(id));
	await until("C pf-plan enabled=false", () => lastPlan(a.events)?.enabled === false);
	id = a.send({ type: "prompt", message: "/probe-tools" });
	await until("C /probe-tools response", () => a.responses.get(id));
	await until("C 声明摘除 present_plan", () => {
		const p = lastProbe(a.events);
		return Array.isArray(p) && !p.includes("present_plan");
	});
	check("C /plan off 路径 sync（OFF）", true);

	// D：/plan on → plan_mode 自身 sync 加回
	id = a.send({ type: "prompt", message: "/plan on" });
	await until("/plan on response", () => a.responses.get(id));
	await until("D pf-plan enabled=true", () => lastPlan(a.events)?.enabled === true);
	id = a.send({ type: "prompt", message: "/probe-tools" });
	await until("D /probe-tools response", () => a.responses.get(id));
	await until("D 声明加回 present_plan", () => {
		const p = lastProbe(a.events);
		return Array.isArray(p) && p.includes("present_plan");
	});
	check("D /plan on 路径 sync（ON）", true);

	const errsA = extErrors(a.events);
	check("活进程无扩展错误", errsA.length === 0, JSON.stringify(errsA));
	a.proc.kill();
	await wait(300);

	// ============ F 阶段：冷恢复（enabled=true）→ 声明保留 ============
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
	await until("F 冷恢复 enabled=true", () => lastPlan(b.events)?.enabled === true);
	const idF = b.send({ type: "prompt", message: "/probe-tools" });
	await until("F /probe-tools response", () => b.responses.get(idF));
	await until("F 声明保留 present_plan", () => {
		const p = lastProbe(b.events);
		return Array.isArray(p) && p.includes("present_plan");
	});
	check("F 冷恢复（计划态）声明保留", true);
	const errsB = extErrors(b.events);
	check("冷恢复无扩展错误", errsB.length === 0, JSON.stringify(errsB));
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
