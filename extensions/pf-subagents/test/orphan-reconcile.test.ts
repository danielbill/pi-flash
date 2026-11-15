import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { orphanChildren, scanSessionBranch } from "../src/index.ts";

/**
 * Orphan reconciliation on session_start: a background child's terminal state
 * only ever arrives as a `subagent-settled` entry, so a host process that dies
 * mid-run leaves the card replaying the ack (`status: started`) forever.
 * 2026-11-15 real case: second batch sub-3/sub-5 frozen at 13:56, still spinning
 * after the 15:12 reopen.
 */

const dispatchAck = (childId: string, extra: Record<string, unknown> = {}) => ({
	type: "message",
	message: {
		role: "toolResult",
		toolName: "agent",
		details: { child_id: childId, delivery: "parent", status: "started", ...extra },
	},
});

const settledEntry = (childId: string, stopReason = "completed", sessionFile?: string) => ({
	type: "custom",
	customType: "subagent-settled",
	data: sessionFile ? { childId, stopReason, session_file: sessionFile } : { childId, stopReason },
});

/** A completion message is the durable terminal record (and the load-visible one). */
const completionMessage = (childId: string, sessionFile?: string) => ({
	type: "custom_message",
	customType: "subagent-completion",
	content: "…",
	details: sessionFile ? { childId, stopReason: "aborted", session_file: sessionFile } : { childId, stopReason: "aborted" },
});

const childMarker = (depth: number) => ({ type: "custom", customType: "subagent/child", data: { depth } });

/** Minimal `ExtensionContext` slice both scans read. */
const fakeContext = (entries: unknown[], sessionFile: string | undefined = "/parent.jsonl") =>
	({ sessionManager: { sessionFile, getBranch: () => entries } }) as never;

describe("scanSessionBranch", () => {
	it("collects a non-terminal ack with its child session file", () => {
		const scan = scanSessionBranch(fakeContext([dispatchAck("sub-5", { session_file: "/child.jsonl" })]));
		assert.deepEqual(scan.acks, [{ childId: "sub-5", sessionFile: "/child.jsonl" }]);
		assert.equal(scan.settled.size, 0);
	});

	it("ignores results that already carry a terminal stop_reason", () => {
		const scan = scanSessionBranch(
			fakeContext([dispatchAck("sub-1", { stop_reason: "completed" }), dispatchAck("sub-2", { error_code: "START_REJECTED" })]),
		);
		assert.deepEqual(scan.acks, []);
	});

	it("records terminal records with no session file in the id-only fallback", () => {
		const scan = scanSessionBranch(fakeContext([dispatchAck("sub-1"), settledEntry("sub-1")]));
		assert.deepEqual([...scan.settledWithoutFile], ["sub-1"]);
	});

	it("keys terminal records by child + session file", () => {
		const scan = scanSessionBranch(
			fakeContext([dispatchAck("sub-1", { session_file: "/child.jsonl" }), settledEntry("sub-1", "completed", "/child.jsonl")]),
		);
		assert.deepEqual([...scan.settled], ["sub-1\u0000/child.jsonl"]);
	});

	it("treats a completion message as terminal too", () => {
		// The load path only folds completion messages (custom entries never reach
		// the message snapshot), so this is the marker every reopen can see.
		const scan = scanSessionBranch(fakeContext([dispatchAck("sub-5"), completionMessage("sub-5")]));
		assert.ok(scan.settledWithoutFile.has("sub-5"));
	});

	it("still restores the child depth marker (regression)", () => {
		const scan = scanSessionBranch(fakeContext([childMarker(2), dispatchAck("sub-9")]));
		assert.equal(scan.depth, 2);
	});

	it("reports the depth but caches nothing without a session file", () => {
		const scan = scanSessionBranch({
			sessionManager: { sessionFile: undefined, getBranch: () => [childMarker(1)] },
		} as never);
		assert.equal(scan.file, undefined);
		assert.equal(scan.depth, 1);
	});
});

describe("orphanChildren", () => {
	it("reports an ack whose child the manager no longer knows", () => {
		const scan = scanSessionBranch(fakeContext([dispatchAck("sub-5", { session_file: "/c.jsonl" })]));
		assert.deepEqual(orphanChildren(scan, () => false), [{ childId: "sub-5", sessionFile: "/c.jsonl" }]);
	});

	it("leaves a live child alone (same-process view switch)", () => {
		const scan = scanSessionBranch(fakeContext([dispatchAck("sub-5")]));
		assert.deepEqual(orphanChildren(scan, (id) => id === "sub-5"), []);
	});

	it("never re-settles a child that already has a settled entry", () => {
		const scan = scanSessionBranch(fakeContext([dispatchAck("sub-5"), settledEntry("sub-5", "aborted")]));
		assert.deepEqual(orphanChildren(scan, () => false), []);
	});

	it("a settled id from an earlier process does not mask this process's orphan", () => {
		// Real case (2026-11-15): batch one settled sub-3 (child file A), the host died
		// mid-run and batch two reused sub-3 for a different child (child file B).
		const scan = scanSessionBranch(
			fakeContext([
				dispatchAck("sub-3", { session_file: "/batch1/sub-3.jsonl" }),
				completionMessage("sub-3", "/batch1/sub-3.jsonl"),
				dispatchAck("sub-3", { session_file: "/batch2/sub-3.jsonl" }),
			]),
		);
		const orphans = orphanChildren(scan, () => false);
		assert.equal(orphans.length, 1);
		assert.equal(orphans[0].sessionFile, "/batch2/sub-3.jsonl");
	});

	it("never re-settles a child that already has a completion message", () => {
		const scan = scanSessionBranch(fakeContext([dispatchAck("sub-5"), completionMessage("sub-5")]));
		assert.deepEqual(orphanChildren(scan, () => false), []);
	});
});
