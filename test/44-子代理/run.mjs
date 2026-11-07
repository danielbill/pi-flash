#!/usr/bin/env node
// 44-子代理 自动验收驱动。
//
// 跑法（本目录）：node run.mjs        → 全部用例 + 宿主模型守卫
//       （仓库根）  node test/44-子代理/run.mjs
//
// 覆盖：
//   A 接入控制（并发准入/深度/结算）   test/44-子代理/接入控制/case.md
//   B 并发上限（配置驱动）             test/44-子代理/并发上限/case.md
//   C maxloop / 轮次预算               test/44-子代理/maxloop/case.md
//   D 自定义 subagent（profile）       test/44-子代理/profile/case.md
//   E 生命周期与工具面                 test/44-子代理/生命周期与工具面/case.md
//
// 人工项（不在此跑，见各 case.md）：
//   B7 外部并发（模型服务端限流）、D11 地区限制冒烟。

import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync, existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const extRoot = resolve(here, "../../extensions/pf-subagents");

// ---- 宿主模型守卫（A12）：子会话不得装配 pf-subagents ----
//
// PARITY.md P0 的结论"depth≥2 不可达"依赖这个前提。三个来源：
//   1. PF 数据目录 agents/（pi-flash-dev 或 ~/.pi-flash）
//   2. ~/.pi/agent/agents/
//   3. 当前项目 .pi/agents/
// 任一 profile 的 frontmatter 引用 pf-subagents 即 FAIL（同时提示重开对齐）。

function collectProfileDirs() {
	const dirs = [];
	const pfDev = resolve(here, "../../pi-flash-dev/agents");
	if (existsSync(pfDev)) dirs.push(pfDev);
	const home = process.env.USERPROFILE ?? process.env.HOME ?? "";
	if (home) {
		const pfHome = join(home, ".pi-flash", "agents");
		const piHome = join(home, ".pi", "agent", "agents");
		if (existsSync(pfHome)) dirs.push(pfHome);
		if (existsSync(piHome)) dirs.push(piHome);
	}
	const proj = resolve(here, "../../.pi/agents");
	if (existsSync(proj)) dirs.push(proj);
	return dirs;
}

function hostModelGuard() {
	const problems = [];
	for (const dir of collectProfileDirs()) {
		for (const name of readdirSync(dir)) {
			if (!name.endsWith(".md")) continue;
			const text = readFileSync(join(dir, name), "utf8");
			const fm = /^---\r?\n([\s\S]*?)\r?\n---/.exec(text)?.[1] ?? "";
			const inExtensions = /^\s*extensions\s*:/m.test(fm);
			if (!inExtensions) continue;
			// frontmatter 里 extensions 列表出现 pf-subagents（路径或包名都算）
			const block = fm.slice(fm.indexOf("extensions:"));
			if (/pf-subagents/i.test(block)) {
				problems.push(`${join(dir, name)}: frontmatter extensions 引用 pf-subagents`);
			}
		}
	}
	return problems;
}

// ---- node --test 跑扩展的全部测试 ----

function runExtensionTests() {
	const r = spawnSync(process.execPath, ["--test", join("test", "*.test.ts")], {
		cwd: extRoot,
		shell: true,
		encoding: "utf8",
	});
	return r;
}

const results = [];
let failed = false;

console.log(`# 44-子代理 自动验收  (node ${process.version})`);
console.log(`# 扩展目录: ${extRoot}\n`);

const r = runExtensionTests();
const out = `${r.stdout ?? ""}\n${r.stderr ?? ""}`;
// spec reporter: `ℹ pass 138`；TAP fallback: `# pass 138`
const pass = Number(/(?:ℹ|#) pass (\d+)/.exec(out)?.[1] ?? 0);
const fail = Number(/(?:ℹ|#) fail (\d+)/.exec(out)?.[1] ?? 0);
console.log(`[extension] node --test: pass=${pass} fail=${fail}`);
if (r.status !== 0 || fail > 0) {
	failed = true;
	// 失败时把失败用例名捞出来，便于定位对应 case.md 条目
	const failedNames = [...out.matchAll(/✖\s+(.+)/g)].map((m) => m[1].trim()).slice(0, 40);
	if (failedNames.length) console.log("  失败用例:\n    " + failedNames.join("\n    "));
}

const problems = hostModelGuard();
if (problems.length) {
	failed = true;
	console.log(`[host-model] FAIL — depth≥2 已变为可达，必须重开 extensions/pf-subagents/PARITY.md`);
	for (const p of problems) console.log(`  - ${p}`);
} else {
	console.log("[host-model] pass — 无 profile 引用 pf-subagents，depth≥2 不可达结论仍成立");
}

console.log(
	`\n${failed ? "验收 FAIL" : "验收 PASS"}  （用例-断言映射见 test/44-子代理/*/case.md；人工项 B7/D11 除外）`,
);
process.exit(failed ? 1 : 0);
