#!/usr/bin/env node
// 001-PFContext 自动验收驱动（pfctx-manager）。
//
// 跑法（本目录）：node run.mjs
//       （仓库根）  node test/001-PFContext/run.mjs
//
// 覆盖（映射见 pfctx-manager/case.md）：
//   A 工具面（目录校验/恒拒项/注册形态）  → node --test extensions/pfctx-manager
//   B 挂载链守卫（静态漂移检查）          → 源码 grep 式断言
//   E S7.1 重载差值 / agent section        → cargo test -p pf-context（手动：
//                                            cargo test -p pf-context，此处只提示）
//
// D 端到端（agent 真调用）走 live probe（需已配置模型，#[ignore] 门控）：
//   cargo test -p pi-link --test live_pfctx_manager_probe -- --ignored --nocapture

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, "../..");
const extRoot = resolve(repo, "extensions/pfctx-manager");

let failed = false;

console.log(`# 001-PFContext 自动验收（pfctx-manager）  (node ${process.version})`);
console.log(`# 扩展目录: ${extRoot}\n`);

// ---- A：工具面 node --test ----
{
	const r = spawnSync(process.execPath, ["--test", join("test", "*.test.ts")], {
		cwd: extRoot,
		shell: true,
		encoding: "utf8",
	});
	const out = `${r.stdout ?? ""}\n${r.stderr ?? ""}`;
	const pass = Number(/(?:ℹ|#) pass (\d+)/.exec(out)?.[1] ?? 0);
	const fail = Number(/(?:ℹ|#) fail (\d+)/.exec(out)?.[1] ?? 0);
	console.log(`[A 工具面] node --test: pass=${pass} fail=${fail}`);
	if (r.status !== 0 || fail > 0) {
		failed = true;
		const names = [...out.matchAll(/✖\s+(.+)/g)].map((m) => m[1].trim()).slice(0, 20);
		if (names.length) console.log("  失败用例:\n    " + names.join("\n    "));
	}
}

// ---- B：挂载链守卫（源码漂移检查；细节断言在 cargo test，见 case.md B1/B2）----
const guards = [
	{
		name: "扩展入口存在",
		file: extRoot,
		check: () => existsSync(join(extRoot, "src", "index.ts")),
	},
	{
		name: "pi-link 挂载分支存在（client.rs）",
		file: join(repo, "crates/pi-link/src/client.rs"),
		check: (text) => text.includes("pfctx_manager::mount_target"),
	},
	{
		name: "spawn allowlist 点名（spawn.rs）",
		file: join(repo, "crates/app/src/session/runtime/spawn.rs"),
		check: (text) => text.includes("pfctx_manager") && text.includes("agent_self_debug"),
	},
	{
		name: "宿主条目消费存在（on_event.rs）",
		file: join(repo, "crates/app/src/session/runtime/events/on_event.rs"),
		check: (text) => text.includes('"pfctx-manager-op"') && text.includes("pfctx_ops::push"),
	},
	{
		name: "泵执行点存在（pumps.rs）",
		file: join(repo, "crates/app/src/startup/pumps.rs"),
		check: (text) => text.includes("pfctx_ops::execute_pending"),
	},
	{
		name: "扩展条目类型与宿主一致（index.ts）",
		file: join(extRoot, "src/index.ts"),
		check: (text) => text.includes('"pfctx-manager-op"'),
	},
];

for (const g of guards) {
	let ok = false;
	try {
		const text = g.file.endsWith("index.ts") || existsSync(g.file)
			? (g.file.endsWith(String(extRoot)) ? "dir" : readFileSync(g.file, "utf8"))
			: "";
		ok = g.file.endsWith(String(extRoot)) ? g.check() : g.check(text);
	} catch (e) {
		ok = false;
	}
	console.log(`[B 挂载链] ${g.name}: ${ok ? "pass" : "FAIL"}`);
	if (!ok) failed = true;
}

console.log(
	`\n[Rust 侧]（本驱动不代跑，随常规测试走）:`,
	`\n  cargo test -p pf-context   # agent section / S7.1 重载差值（case.md E）`,
	`\n  cargo test -p app pfctx    # C 桥（gpui 实体上下文）`,
	`\n  cargo test -p pi-link pfctx # B1/B2`,
	`\n[D 端到端]（需已配置模型）: cargo test -p pi-link --test live_pfctx_manager_probe -- --ignored --nocapture`,
);

console.log(`\n${failed ? "验收 FAIL" : "验收 PASS"}  （用例-断言映射见 pfctx-manager/case.md；D2/D3 为真机项）`);
process.exit(failed ? 1 : 0);
