import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { validateOps, default as pfctxManager } from "../src/index.ts";

describe("pfctx_manager catalog validation (001-4)", () => {
	it("accepts well-formed ops across sections", () => {
		const errors = validateOps([
			{ setting: "subagents.capacity", value: 8 },
			{ setting: "chat.send_mode", value: "steer" },
			{ setting: "shell.sound", value: false },
			{ setting: "models.default_model", value: "deepseek/deepseek-flash" },
			{ setting: "models.default_model", value: null },
			{ setting: "appearance.session_font", value: { family: "Inter", size: 14 } },
		]);
		assert.deepEqual(errors, []);
	});

	it("rejects out-of-catalog keys with guidance", () => {
		const errors = validateOps([{ setting: "no_such.param", value: 1 }]);
		assert.equal(errors.length, 1);
		assert.match(errors[0], /目录外/);
	});

	it("rejects permanently-denied params with reasons", () => {
		const errors = validateOps([
			{ setting: "subagents.max_depth", value: 2 },
			{ setting: "appearance.icon_theme", value: "x" },
		]);
		assert.equal(errors.length, 2);
		assert.match(errors[0], /恒 1/);
	});

	it("enforces numeric ranges from the catalog", () => {
		assert.equal(validateOps([{ setting: "subagents.capacity", value: 1 }]).length, 1);
		assert.equal(validateOps([{ setting: "subagents.capacity", value: 11 }]).length, 1);
		assert.equal(validateOps([{ setting: "sessions.auto_clean_days", value: -1 }]).length, 1);
		assert.deepEqual(validateOps([{ setting: "subagents.capacity", value: 10 }]), []);
	});

	it("enforces enums and types", () => {
		assert.equal(validateOps([{ setting: "chat.send_mode", value: "bogus" }]).length, 1);
		assert.equal(validateOps([{ setting: "shell.sound", value: "on" }]).length, 1);
		assert.equal(validateOps([{ setting: "subagents.capacity", value: "6" }]).length, 1);
		assert.equal(validateOps([{ setting: "models.default_model", value: "no-slash" }]).length, 1);
	});

	it("rejects malformed op entries", () => {
		assert.equal(validateOps([{ value: 1 } as never]).length, 1);
		assert.equal(validateOps([{} as never]).length, 1);
	});

	it("registers exactly one tool named pfctx_manager", () => {
		const registered: Array<{ name: string }> = [];
		const pi = {
			registerTool: (t: { name: string }) => registered.push(t),
		} as never;
		(pfctxManager as (pi: never) => void)(pi);
		assert.equal(registered.length, 1);
		assert.equal(registered[0].name, "pfctx_manager");
	});
});
