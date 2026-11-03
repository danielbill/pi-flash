// issue #7 wire 级复现：spawn vendored pi --mode rpc，发 prompt "/usage"，
// 记录 6s 内 stdout 全部事件，验证「扩展命令吃掉 prompt 后 wire 上还有什么」。
import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const CLI = resolve("vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js");
const EXT = resolve("test/issue7-ext-command/ext-usage.ts");

const cwd = mkdtempSync(join(tmpdir(), "issue7-"));
mkdirSync(join(cwd, ".pi"), { recursive: true });

const child = spawn(process.execPath, [CLI, "--mode", "rpc", "-e", EXT], {
	cwd,
	stdio: ["pipe", "pipe", "pipe"],
});

let buf = "";
const events = [];
child.stdout.on("data", (d) => {
	buf += d;
	let ix;
	while ((ix = buf.indexOf("\n")) !== -1) {
		const line = buf.slice(0, ix);
		buf = buf.slice(ix + 1);
		if (!line.trim()) continue;
		try {
			events.push(JSON.parse(line));
		} catch {
			events.push({ type: "<unparseable>" });
		}
	}
});
let stderrTail = "";
child.stderr.on("data", (d) => {
	stderrTail += d;
	if (stderrTail.length > 8000) stderrTail = stderrTail.slice(-8000);
});

const send = (obj) => child.stdin.write(JSON.stringify(obj) + "\n");

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

await sleep(1500); // boot + extension bind
send({ type: "get_commands", id: "c1" });
await sleep(500);
send({ type: "prompt", message: "/usage", id: "p1" });
await sleep(2500);
send({ type: "prompt", message: "/usageboom", id: "p2" });
await sleep(2500);

child.kill();
await sleep(300);

const names = events.map((e) =>
	e.type === "response" ? `response:${e.command}(success=${e.success},disposition=${e.data?.disposition ?? "—"})` : e.type,
);
console.log("== wire 事件序列 ==");
for (const n of names) console.log(" ", n);

const agentEvents = names.filter((n) =>
	["agent_start", "agent_end", "agent_settled", "message_start", "message_update", "message_end"].some((k) =>
		n.startsWith(k),
	),
);
const uiNotifies = events.filter((e) => e.type === "extension_ui_request").map((e) => e.method);
console.log("\n== 结论 ==");
console.log("extension_ui_request 方法:", uiNotifies.join(", ") || "(无)");
console.log("agent/消息事件:", agentEvents.length ? agentEvents.join(", ") : "(无——PF 的 phase_waiting 无人清)");
if (stderrTail.trim()) console.log("\n== stderr 尾部 ==\n" + stderrTail.trim().split("\n").slice(-6).join("\n"));

rmSync(cwd, { recursive: true, force: true });
