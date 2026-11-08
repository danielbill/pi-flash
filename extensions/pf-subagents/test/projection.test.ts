import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { mkdtempSync, writeFileSync, rmSync, mkdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { SubagentManager } from "../src/manager.ts";
import { syncModuleConfig } from "../src/index.ts";
import { PROJECTION_FILENAME, readSubagentsProjection } from "../src/module-config.ts";

function tmpPFDir(): string {
	return mkdtempSync(join(tmpdir(), "pf-subagents-projection-"));
}

function writeProjection(dir: string, doc: unknown): void {
	writeFileSync(join(dir, PROJECTION_FILENAME), typeof doc === "string" ? doc : JSON.stringify(doc));
}

describe("ext-context.json projection (001-2)", () => {
	it("takes capacity / max_depth / enabled from a valid projection", () => {
		const dir = tmpPFDir();
		try {
			writeProjection(dir, {
				version: 1,
				generated_at: "2026-11-08T08:20:00Z",
				subagents: { capacity: 10, max_depth: 1, enabled: true },
			});
			const read = readSubagentsProjection(dir);
			assert.equal(read.state, "ok");
			if (read.state === "ok") {
				assert.equal(read.value.capacity, 10);
				assert.equal(read.value.max_depth, 1);
				assert.equal(read.value.enabled, true);
			}
		} finally {
			rmSync(dir, { recursive: true, force: true });
		}
	});

	it("absent projection (missing file / no dir) is not a problem", () => {
		assert.equal(readSubagentsProjection(undefined).state, "absent");
		assert.equal(readSubagentsProjection(join(tmpdir(), "no-such-pf-dir")).state, "absent");
	});

	it("half-written / broken JSON is invalid with a problem, never a fallback value", () => {
		const dir = tmpPFDir();
		try {
			writeProjection(dir, "{not json");
			const read = readSubagentsProjection(dir);
			assert.equal(read.state, "invalid");
			if (read.state === "invalid") assert.match(read.problem, /not valid JSON/);
		} finally {
			rmSync(dir, { recursive: true, force: true });
		}
	});

	it("version mismatch is invalid (extension decides fallback, not the doc)", () => {
		const dir = tmpPFDir();
		try {
			writeProjection(dir, { version: 99, subagents: { capacity: 3, max_depth: 1, enabled: true } });
			const read = readSubagentsProjection(dir);
			assert.equal(read.state, "invalid");
			if (read.state === "invalid") assert.match(read.problem, /version/);
		} finally {
			rmSync(dir, { recursive: true, force: true });
		}
	});

	it("missing / malformed subagents section is invalid", () => {
		const dir = tmpPFDir();
		try {
			writeProjection(dir, { version: 1, market: {} });
			assert.equal(readSubagentsProjection(dir).state, "invalid");
			writeProjection(dir, { version: 1, subagents: { capacity: "ten", max_depth: 1, enabled: true } });
			const read = readSubagentsProjection(dir);
			assert.equal(read.state, "invalid");
			if (read.state === "invalid") assert.match(read.problem, /capacity/);
		} finally {
			rmSync(dir, { recursive: true, force: true });
		}
	});

	it("sync chain: projection wins over legacy config.json (both readable)", () => {
		const dir = tmpPFDir();
		const profiles = join(dir, "agents");
		mkdirSync(profiles, { recursive: true });
		try {
			writeProjection(dir, { version: 1, subagents: { capacity: 7, max_depth: 1, enabled: true } });
			writeFileSync(
				join(profiles, "config.json"),
				JSON.stringify({ capacity: { default: 3, min: 2, max: 10 } }),
			);
			const manager = new SubagentManager();
			const result = syncModuleConfig(manager, profiles, dir);
			assert.equal(manager.capacity, 7, "投影为准");
			assert.equal(result.enabled, true);
			assert.equal(result.problem, undefined);
		} finally {
			rmSync(dir, { recursive: true, force: true });
		}
	});

	it("sync chain: projection scalar clamps into narrower legacy bounds", () => {
		const dir = tmpPFDir();
		const profiles = join(dir, "agents");
		mkdirSync(profiles, { recursive: true });
		try {
			writeProjection(dir, { version: 1, subagents: { capacity: 10, max_depth: 1, enabled: true } });
			writeFileSync(
				join(profiles, "config.json"),
				JSON.stringify({ capacity: { default: 3, min: 2, max: 5 } }),
			);
			const manager = new SubagentManager();
			syncModuleConfig(manager, profiles, dir);
			assert.equal(manager.capacity, 5, "更窄的 legacy max 收窄投影值");
		} finally {
			rmSync(dir, { recursive: true, force: true });
		}
	});

	it("sync chain: absent projection falls back to legacy config.json", () => {
		const dir = tmpPFDir();
		const profiles = join(dir, "agents");
		mkdirSync(profiles, { recursive: true });
		try {
			writeFileSync(
				join(profiles, "config.json"),
				JSON.stringify({ capacity: { default: 6, min: 2, max: 10 } }),
			);
			const manager = new SubagentManager();
			const result = syncModuleConfig(manager, profiles, dir);
			assert.equal(manager.capacity, 6);
			assert.equal(result.enabled, undefined, "无投影不携带开关");
			assert.equal(result.problem, undefined);
		} finally {
			rmSync(dir, { recursive: true, force: true });
		}
	});

	it("sync chain: no projection and no legacy = FALLBACK (8)", () => {
		const dir = tmpPFDir();
		const profiles = join(dir, "agents");
		mkdirSync(profiles, { recursive: true });
		try {
			const manager = new SubagentManager();
			const result = syncModuleConfig(manager, profiles, dir);
			assert.equal(manager.capacity, 8);
			assert.ok(result.problem, "缺文件要有 problem 供宿主上报");
			assert.equal(result.enabled, undefined);
		} finally {
			rmSync(dir, { recursive: true, force: true });
		}
	});

	it("sync chain: broken projection keeps current manager values (never overwrites)", () => {
		const dir = tmpPFDir();
		const profiles = join(dir, "agents");
		mkdirSync(profiles, { recursive: true });
		try {
			writeFileSync(
				join(profiles, "config.json"),
				JSON.stringify({ capacity: { default: 4, min: 2, max: 10 } }),
			);
			const manager = new SubagentManager();
			syncModuleConfig(manager, profiles, dir); // 先按 legacy 定格 4
			writeProjection(dir, "{broken json");
			const result = syncModuleConfig(manager, profiles, dir);
			assert.equal(manager.capacity, 4, "坏投影不改变在跑上限");
			assert.ok(result.problem, "problem 上报");
		} finally {
			rmSync(dir, { recursive: true, force: true });
		}
	});

	it("projection enabled=false is surfaced so the tool entry can reject", () => {
		const dir = tmpPFDir();
		try {
			writeProjection(dir, { version: 1, subagents: { capacity: 3, max_depth: 1, enabled: false } });
			const manager = new SubagentManager();
			const result = syncModuleConfig(manager, undefined, dir);
			assert.equal(result.enabled, false);
		} finally {
			rmSync(dir, { recursive: true, force: true });
		}
	});
});
