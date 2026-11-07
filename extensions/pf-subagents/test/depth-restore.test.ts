import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { currentDepthOf, restoreChildDepth } from "../src/index.ts";

/** Minimal `ExtensionContext` slice the depth restore reads. */
const fakeContext = (sessionFile: string | undefined, entries: unknown[] = []) =>
	({ sessionManager: { sessionFile, getBranch: () => entries } }) as never;

const marker = (data: unknown) => ({ type: "custom", customType: "subagent/child", data });

describe("restoreChildDepth", () => {
	// dsh reads depth from the agent header and treats absence as top-level
	// (`depth.ts`); our header is the persisted marker entry.
	it("reads depth from the persisted marker", () => {
		const ctx = fakeContext("/child.jsonl", [marker({ provider: "spawn", depth: 1 })]);
		restoreChildDepth(ctx);
		assert.equal(currentDepthOf(ctx), 1);
	});

	it("ignores entries that are not the child marker", () => {
		const ctx = fakeContext("/plain.jsonl", [
			{ type: "custom", customType: "subagent/descriptor", data: {} },
			{ type: "message", data: { depth: 3 } },
		]);
		restoreChildDepth(ctx);
		assert.equal(currentDepthOf(ctx), 0);
	});

	it("treats a session without a session file as a root", () => {
		const ctx = fakeContext(undefined);
		assert.doesNotThrow(() => restoreChildDepth(ctx));
		assert.equal(currentDepthOf(ctx), 0);
	});
});
