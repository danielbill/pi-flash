import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { restoreChildLineage } from "../src/index.ts";

/** Minimal `ExtensionContext` slice the lineage restore reads. */
const fakeContext = (sessionFile: string, entries: unknown[]) =>
	({ sessionManager: { sessionFile, getBranch: () => entries } }) as never;

const marker = (data: unknown) => ({ type: "custom", customType: "subagent/child", data });

describe("restoreChildLineage", () => {
	it("reads depth and root from the persisted marker", async () => {
		const ctx = fakeContext("/child.jsonl", [marker({ provider: "spawn", depth: 1, rootId: "/root.jsonl" })]);
		restoreChildLineage(ctx);
		const { currentChildLineage } = await import("../src/index.ts");
		assert.deepEqual(currentChildLineage("/child.jsonl"), { depth: 1, rootId: "/root.jsonl" });
	});

	it("ignores entries that are not the child marker", async () => {
		const ctx = fakeContext("/plain.jsonl", [
			{ type: "custom", customType: "subagent/descriptor", data: {} },
			{ type: "message", data: { depth: 3, rootId: "/bogus" } },
		]);
		restoreChildLineage(ctx);
		const { currentChildLineage } = await import("../src/index.ts");
		assert.deepEqual(currentChildLineage("/plain.jsonl"), { depth: 0, rootId: "/plain.jsonl" });
	});

	it("treats a session without a session file as a root", async () => {
		const ctx = { sessionManager: { getBranch: () => [] } } as never;
		assert.doesNotThrow(() => restoreChildLineage(ctx));
	});
});
