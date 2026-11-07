import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { createDescriptor, DESCRIPTOR_VERSION } from "../src/descriptor.ts";

const sample = () =>
	createDescriptor({
		childId: "sub-1",
		provider: "spawn",
		cwd: "/repo",
		depth: 1,
		parentToolCallId: "call-7/1",
		label: "explore",
		profile: "explorer",
		model: "g/x",
		sessionFile: "/repo/child.jsonl",
		now: new Date("2026-10-18T00:00:00.000Z"),
	});

describe("descriptor", () => {
	it("creates a provisioning record with the injected clock", () => {
		const descriptor = sample();
		assert.equal(descriptor.version, DESCRIPTOR_VERSION);
		assert.equal(descriptor.phase, "provisioning");
		assert.equal(descriptor.startedAt, "2026-10-18T00:00:00.000Z");
		assert.equal(descriptor.updatedAt, "2026-10-18T00:00:00.000Z");
	});

	it("round-trips through JSON unchanged", () => {
		const descriptor = sample();
		const stored = JSON.parse(JSON.stringify(descriptor));
		assert.deepEqual(stored, descriptor);
	});
});
