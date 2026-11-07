import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { Capacity } from "../src/capacity.ts";
import { SubagentError, SubagentErrorCode } from "../src/errors.ts";
import { captureSync } from "./helpers.ts";

describe("Capacity", () => {
	it("rejects instead of queueing when the bucket is full", () => {
		const capacity = new Capacity(2);
		capacity.acquire("p");
		capacity.acquire("p");
		const error = captureSync(() => capacity.acquire("p"));
		assert.equal((error as SubagentError).code, SubagentErrorCode.CAPACITY_REACHED);
		assert.equal(capacity.used("p"), 2);
	});

	// dsh wording carries the only useful next step, so the message is shared.
	it("tells the caller to wait for a child or finish with the current agents", () => {
		const capacity = new Capacity(1);
		capacity.acquire("p");
		const error = captureSync(() => capacity.acquire("p")) as SubagentError;
		assert.equal(
			error.message,
			"subagent limit reached (active child limit: 1); wait for an existing child to finish " +
				"or complete this work with the current agents",
		);
	});

	it("counts each parent session separately", () => {
		const capacity = new Capacity(1);
		capacity.acquire("a");
		const second = capacity.acquire("b");
		assert.equal(capacity.used("a"), 1);
		assert.equal(capacity.used("b"), 1);
		second.release();
		assert.equal(capacity.used("b"), 0);
	});

	it("keeps instances independent (one manager per session)", () => {
		// `PARITY.md` P0: a child session never mounts this extension, so two
		// Capacity instances never describe one tree. No shared registry needed.
		const root = new Capacity(1);
		const other = new Capacity(1);
		root.acquire("/root.jsonl");
		assert.equal(other.used("/root.jsonl"), 0);
		assert.doesNotThrow(() => other.acquire("/root.jsonl"));
	});

	it("drops an empty bucket so session ids do not accumulate", () => {
		const capacity = new Capacity(1);
		const slot = capacity.acquire("/root.jsonl");
		assert.equal(capacity.used("/root.jsonl"), 1);
		slot.release();
		assert.equal(capacity.used("/root.jsonl"), 0);
	});

	it("returns the slot exactly once", () => {
		const capacity = new Capacity(1);
		const slot = capacity.acquire("p");
		slot.release();
		slot.release();
		assert.equal(capacity.used("p"), 0);
		assert.doesNotThrow(() => capacity.acquire("p"));
	});

	it("lowering the limit does not evict running children", () => {
		const capacity = new Capacity(2);
		const a = capacity.acquire("p");
		const b = capacity.acquire("p");
		capacity.setLimit(1);
		assert.equal(capacity.used("p"), 2);
		const error = captureSync(() => capacity.acquire("p"));
		assert.equal((error as SubagentError).code, SubagentErrorCode.CAPACITY_REACHED);
		a.release();
		assert.equal(capacity.used("p"), 1);
		// Still at the (new) limit: no eviction, but no new start either.
		assert.throws(() => capacity.acquire("p"));
		b.release();
		assert.doesNotThrow(() => capacity.acquire("p"));
	});

	it("refuses non-positive or fractional limits", () => {
		assert.throws(() => new Capacity(0), RangeError);
		assert.throws(() => new Capacity(-1), RangeError);
		assert.throws(() => new Capacity(1.5), RangeError);
	});
});
