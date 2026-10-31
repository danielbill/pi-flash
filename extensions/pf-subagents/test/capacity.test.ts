import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { Capacity } from "../src/capacity.ts";
import { SubagentError, SubagentErrorCode } from "../src/errors.ts";
import { captureSync } from "./helpers.ts";

describe("Capacity", () => {
	it("rejects instead of queueing when the parent is full", () => {
		const capacity = new Capacity(2);
		capacity.acquire("p");
		capacity.acquire("p");
		const error = captureSync(() => capacity.acquire("p"));
		assert.equal((error as SubagentError).code, SubagentErrorCode.CAPACITY_REACHED);
		assert.equal(capacity.used("p"), 2);
	});

	it("counts per parent", () => {
		const capacity = new Capacity(1);
		capacity.acquire("a");
		const second = capacity.acquire("b");
		assert.equal(capacity.used("a"), 1);
		assert.equal(capacity.used("b"), 1);
		second.release();
		assert.equal(capacity.used("b"), 0);
	});

	it("returns the slot exactly once", () => {
		const capacity = new Capacity(1);
		const slot = capacity.acquire("p");
		slot.release();
		slot.release();
		assert.equal(capacity.used("p"), 0);
		assert.doesNotThrow(() => capacity.acquire("p"));
	});

	it("keeps running children when the limit is lowered", () => {
		const capacity = new Capacity(3);
		capacity.acquire("p");
		capacity.acquire("p");
		capacity.setLimit(1);
		assert.equal(capacity.used("p"), 2);
		const error = captureSync(() => capacity.acquire("p"));
		assert.equal((error as SubagentError).code, SubagentErrorCode.CAPACITY_REACHED);
	});

	it("rejects invalid limits", () => {
		assert.throws(() => new Capacity(0), RangeError);
		assert.throws(() => new Capacity(-1), RangeError);
		assert.throws(() => new Capacity(1.5), RangeError);
	});
});
