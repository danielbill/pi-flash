import assert from "node:assert/strict";
import { describe, it, after, beforeEach } from "node:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { SubagentManager } from "../src/manager.ts";
import { syncModuleConfig } from "../src/index.ts";
import {
	clampToBound,
	FALLBACK_MODULE_CONFIG,
	loadModuleConfig,
	moduleConfigPath,
	parseModuleConfig,
} from "../src/module-config.ts";
import { baseRequest, ScriptedProvider } from "./helpers.ts";

describe("module config document", () => {
	it("takes values AND bounds from the file (no code-side ceiling)", () => {
		const config = parseModuleConfig(
			JSON.stringify({
				capacity: { default: 10, min: 2, max: 10 },
				maxDepth: { default: 2, min: 1, max: 3 },
				maxTurns: 40,
				model: "deepseek/deepseek-flash",
			}),
		);
		assert.deepEqual(config.capacity, { default: 10, min: 2, max: 10 });
		assert.deepEqual(config.maxDepth, { default: 2, min: 1, max: 3 });
		assert.equal(config.maxTurns, 40);
		assert.equal(config.model, "deepseek/deepseek-flash");
	});

	it("clamps against the bounds declared in that same file", () => {
		const config = parseModuleConfig(JSON.stringify({ capacity: { default: 5, min: 2, max: 7 } }));
		assert.equal(config.capacity.max, 7);
		assert.equal(clampToBound(99, config.capacity), 7);
		assert.equal(clampToBound(1, config.capacity), 2);
		assert.equal(clampToBound(5, config.capacity), 5);
	});

	it("rejects documents whose default lies outside their own bounds", () => {
		assert.throws(
			() => parseModuleConfig(JSON.stringify({ capacity: { default: 12, min: 2, max: 10 } })),
			/MODULE_CONFIG_INVALID|within/,
		);
	});

	it("rejects a min above max and non-integer limits", () => {
		assert.throws(
			() => parseModuleConfig(JSON.stringify({ capacity: { min: 9, max: 3 } })),
			/exceeds/,
		);
		assert.throws(
			() => parseModuleConfig(JSON.stringify({ maxDepth: { default: 1.5 } })),
			/safe integer/,
		);
	});

	it("rejects malformed JSON and a non-provider/model model string", () => {
		assert.throws(() => parseModuleConfig("{not json"), /not valid JSON/);
		assert.throws(() => parseModuleConfig(JSON.stringify({ model: "deepseek-flash" })), /provider\/model/);
	});

	it("leaves unset fields at the fallback and ignores unknown keys", () => {
		const config = parseModuleConfig(JSON.stringify({ version: 1, uiHint: "x" }));
		assert.deepEqual(config.capacity, FALLBACK_MODULE_CONFIG.capacity);
		assert.deepEqual(config.maxDepth, FALLBACK_MODULE_CONFIG.maxDepth);
		assert.equal(config.maxTurns, FALLBACK_MODULE_CONFIG.maxTurns);
		assert.equal(config.model, undefined);
	});
});

describe("loadModuleConfig", () => {
	const dir = mkdtempSync(join(tmpdir(), "pf-subagents-config-"));
	after(() => rmSync(dir, { recursive: true, force: true }));

	it("reports the fallback with a reason when the file is missing", () => {
		const result = loadModuleConfig(dir);
		assert.equal(result.path, moduleConfigPath(dir));
		assert.deepEqual(result.config, FALLBACK_MODULE_CONFIG);
		assert.match(result.problem ?? "", /unreadable/);
	});

	it("reports the fallback with a reason when the file is broken", () => {
		writeFileSync(moduleConfigPath(dir), "{oops");
		const result = loadModuleConfig(dir);
		assert.deepEqual(result.config, FALLBACK_MODULE_CONFIG);
		assert.match(result.problem ?? "", /not valid JSON/);
	});
});

describe("syncModuleConfig (live reload, config.json is the only source)", () => {
	const dir = mkdtempSync(join(tmpdir(), "pf-subagents-sync-"));
	const file = moduleConfigPath(dir);
	after(() => rmSync(dir, { recursive: true, force: true }));
	beforeEach(() => rmSync(file, { force: true }));

	it("applies capacity and depth from the file", () => {
		writeFileSync(file, JSON.stringify({ capacity: { default: 5, min: 2, max: 10 }, maxDepth: { default: 2, min: 1, max: 3 } }));
		const manager = new SubagentManager({ capacity: 3, maxDepth: 1 });
		syncModuleConfig(manager, dir);
		assert.equal(manager.capacity, 5);
		assert.equal(manager.maxDepth, 2);
	});

	it("honours a max of 10 — the old hardcoded 5 ceiling is gone", () => {
		writeFileSync(file, JSON.stringify({ capacity: { default: 10, min: 2, max: 10 } }));
		const manager = new SubagentManager({ capacity: 3, maxDepth: 1 });
		syncModuleConfig(manager, dir);
		assert.equal(manager.capacity, 10);
	});

	it("re-reads on every sync, so an edit applies without a restart", () => {
		const manager = new SubagentManager({ capacity: 3, maxDepth: 1 });
		writeFileSync(file, JSON.stringify({ capacity: { default: 2, min: 2, max: 10 } }));
		syncModuleConfig(manager, dir);
		assert.equal(manager.capacity, 2);
		writeFileSync(file, JSON.stringify({ capacity: { default: 9, min: 2, max: 10 } }));
		syncModuleConfig(manager, dir);
		assert.equal(manager.capacity, 9);
	});

	it("keeps current values on a broken/missing file", () => {
		const manager = new SubagentManager({ capacity: 4, maxDepth: 2 });
		writeFileSync(file, "{not json");
		syncModuleConfig(manager, dir);
		assert.equal(manager.capacity, 4);
		assert.equal(manager.maxDepth, 2);
		rmSync(file, { force: true });
		syncModuleConfig(manager, dir);
		assert.equal(manager.capacity, 4);
	});

	// 真机 2026-11-06 取证场景：设置 3，同帧 4 连发——第 4 个必须被拒
	//（此前 env 快照过期导致 4 个全跑）。
	it("capacity 3 from the config rejects the 4th same-frame delegation", async () => {
		writeFileSync(file, JSON.stringify({ capacity: { default: 3, min: 2, max: 10 } }));
		const manager = new SubagentManager({ capacity: 8, maxDepth: 1 });
		const provider = new ScriptedProvider("scripted");
		manager.registry.register(provider);
		syncModuleConfig(manager, dir);
		assert.equal(manager.capacity, 3);
		const runs = [
			manager.start(baseRequest()),
			manager.start(baseRequest()),
			manager.start(baseRequest()),
			manager.start(baseRequest()).catch((e: unknown) => e),
		];
		const fourth = await runs[3];
		assert.equal((fourth as Error).name, "SubagentError");
		assert.equal((fourth as { code?: string }).code, "CAPACITY_REACHED");
		assert.equal(provider.children.length, 3, "只应有 3 个真启动");
	});

	// 同一场景、上限 10：第 4 个必须放行——证明天花板来自文件
	it("capacity 10 from the config admits the 4th delegation", async () => {
		writeFileSync(file, JSON.stringify({ capacity: { default: 10, min: 2, max: 10 } }));
		const manager = new SubagentManager({ capacity: 3, maxDepth: 1 });
		const provider = new ScriptedProvider("scripted");
		manager.registry.register(provider);
		syncModuleConfig(manager, dir);
		assert.equal(manager.capacity, 10);
		const runs = [0, 1, 2, 3].map(() => manager.start(baseRequest()));
		await Promise.all(runs);
		assert.equal(provider.children.length, 4, "上限 10 时 4 个都应真启动");
	});
});
