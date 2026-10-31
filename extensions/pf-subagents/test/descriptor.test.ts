import assert from "node:assert/strict";
import { describe, it } from "node:test";

import {
	createDescriptor,
	DESCRIPTOR_VERSION,
	isInFlight,
	parseDescriptor,
	reconcileOnStart,
} from "../src/descriptor.ts";

const sample = () =>
	createDescriptor({
		childId: "sub-1",
		provider: "spawn",
		cwd: "/repo",
		depth: 1,
		parentToolCallId: "call-7/1",
		label: "explore",
		profile: "explorer",
		now: new Date("2026-10-18T00:00:00.000Z"),
	});

describe("descriptor", () => {
	it("round-trips through parse", () => {
		const descriptor = sample();
		assert.equal(descriptor.version, DESCRIPTOR_VERSION);
		assert.equal(descriptor.phase, "provisioning");
		const stored = JSON.parse(JSON.stringify(descriptor));
		assert.deepEqual(parseDescriptor(stored), stored);
	});

	it("fails closed on an unknown version", () => {
		assert.throws(() => parseDescriptor({ ...sample(), version: 99 }), /unsupported descriptor version/);
	});

	it("rejects missing required fields", () => {
		const base = JSON.parse(JSON.stringify(sample()));
		assert.throws(() => parseDescriptor({ ...base, childId: "" }), /childId/);
		assert.throws(() => parseDescriptor({ ...base, provider: undefined }), /provider/);
		assert.throws(() => parseDescriptor({ ...base, depth: 0 }), /depth/);
		assert.throws(() => parseDescriptor({ ...base, composition: null }), /composition/);
		assert.throws(() => parseDescriptor(null), /must be an object/);
	});

	it("knows which phases are still in flight", () => {
		assert.equal(isInFlight("provisioning"), true);
		assert.equal(isInFlight("active"), true);
		assert.equal(isInFlight("interrupted"), false);
		assert.equal(isInFlight("failed"), false);
	});

	it("marks orphaned children interrupted when the session was aborted", () => {
		const active = { ...sample(), phase: "active" as const };
		const settled = { ...active, childId: "sub-2", phase: "interrupted" as const };
		const [reconciled, untouched] = reconcileOnStart([active, settled], { aborted: true });
		assert.equal(reconciled.phase, "interrupted");
		assert.equal(reconciled.errorCode, "ABORTED");
		assert.equal(untouched, settled);
	});

	it("marks orphaned children failed when the session crashed", () => {
		const active = { ...sample(), phase: "active" as const };
		const [reconciled] = reconcileOnStart([active], { aborted: false });
		assert.equal(reconciled.phase, "failed");
		assert.equal(reconciled.errorCode, "START_REJECTED");
	});
});
