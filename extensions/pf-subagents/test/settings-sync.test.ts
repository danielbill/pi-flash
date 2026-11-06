import assert from "node:assert/strict";
import { describe, it, after } from "node:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { SubagentManager } from "../src/manager.ts";
import { syncModuleSettings } from "../src/index.ts";
import { baseRequest, ScriptedProvider } from "./helpers.ts";

describe("syncModuleSettings (PF app-settings.json live reload)", () => {
	const dir = mkdtempSync(join(tmpdir(), "pf-subagents-settings-"));
	const file = join(dir, "app-settings.json");
	after(() => {
		delete process.env["PI_FLASH_APP_SETTINGS"];
		rmSync(dir, { recursive: true, force: true });
	});

	it("clamps and applies capacity/maxDepth from the settings file", () => {
		process.env["PI_FLASH_APP_SETTINGS"] = file;
		writeFileSync(file, JSON.stringify({ subagent_capacity: 5, subagent_max_depth: 2 }));
		const manager = new SubagentManager({ capacity: 3, maxDepth: 1 });
		syncModuleSettings(manager);
		assert.equal(manager.capacity, 5);
		assert.equal(manager.maxDepth, 2);
	});

	it("clamps out-of-range values into the 2..5 / 1..3 windows", () => {
		writeFileSync(file, JSON.stringify({ subagent_capacity: 99, subagent_max_depth: 0 }));
		const manager = new SubagentManager({ capacity: 3, maxDepth: 1 });
		syncModuleSettings(manager);
		assert.equal(manager.capacity, 5);
		assert.equal(manager.maxDepth, 1);
	});

	it("keeps current values on a broken/missing file", () => {
		writeFileSync(file, "{not json");
		const manager = new SubagentManager({ capacity: 4, maxDepth: 2 });
		syncModuleSettings(manager);
		assert.equal(manager.capacity, 4);
		assert.equal(manager.maxDepth, 2);
		rmSync(file);
		syncModuleSettings(manager);
		assert.equal(manager.capacity, 4);
	});

	// 真机 2026-11-06 取证场景：设置 3，同帧 4 连发——第 4 个必须被拒
	//（此前 env 快照过期导致 4 个全跑）。
	it("capacity 3 from the settings file rejects the 4th same-frame delegation", async () => {
		writeFileSync(file, JSON.stringify({ subagent_capacity: 3 }));
		const manager = new SubagentManager({ capacity: 8, maxDepth: 1 });
		const provider = new ScriptedProvider("scripted");
		manager.registry.register(provider);
		syncModuleSettings(manager);
		assert.equal(manager.capacity, 3);
		const runs = [
			manager.start(baseRequest()),
			manager.start(baseRequest()),
			manager.start(baseRequest()),
			manager.start(baseRequest()).catch((e: unknown) => e),
		];
		const fourth = await runs[3];
		assert.equal((fourth as Error).name, "SubagentError");
		assert.equal(provider.children.length, 3, "只应有 3 个真启动");
	});
});
