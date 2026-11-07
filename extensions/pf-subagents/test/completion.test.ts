import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { COMPLETION_TYPE, composeCompletion } from "../src/completion.ts";
import type { SubagentResult } from "../src/types.ts";

const result = (over: Partial<SubagentResult> = {}): SubagentResult => ({
	childId: "sub-3",
	text: "## 结论\n\n查了三个来源。",
	stopReason: "completed",
	...over,
});

describe("composeCompletion", () => {
	it("prefixes completed runs and keeps the full report in content", () => {
		const msg = composeCompletion(result());
		assert.equal(msg.toast.startsWith("子代理完成 sub-3: "), true);
		assert.equal(msg.content.startsWith("子代理完成 sub-3\n\n"), true);
		assert.equal(msg.content.includes("查了三个来源"), true);
		assert.equal(msg.content.includes("已截断"), false);
		assert.equal(COMPLETION_TYPE, "subagent-completion");
	});

	it("names the error code for non-completed runs", () => {
		const msg = composeCompletion(
			result({ stopReason: "error", errorCode: "MAX_TURNS", text: "预算用尽" }),
		);
		assert.equal(msg.toast.startsWith("子代理结束（MAX_TURNS） sub-3: "), true);
		assert.equal(msg.details.errorCode, "MAX_TURNS");
		assert.equal(msg.details.stopReason, "error");
	});

	it("falls back to stopReason when no error code is present", () => {
		const msg = composeCompletion(result({ stopReason: "aborted" }));
		assert.equal(msg.toast.startsWith("子代理结束（aborted）"), true);
		assert.equal("errorCode" in msg.details, false);
	});

	it("clips the toast but keeps a long report up to the content budget", () => {
		const long = "字".repeat(900);
		const msg = composeCompletion(result({ text: long }));
		assert.equal(msg.toast.length <= "子代理完成 sub-3: ".length + 401, true);
		assert.equal(msg.content.includes("字".repeat(900)), true);
		assert.equal(msg.content.includes("已截断"), false);
	});

	it("marks reports beyond the content budget as truncated", () => {
		const msg = composeCompletion(result({ text: "a".repeat(8100) }));
		assert.equal(msg.content.includes("已截断"), true);
		assert.equal(msg.content.length < 8100, true);
	});

	it("carries the child session file and duration when present", () => {
		const msg = composeCompletion(
			result({ session_file: "/sessions/sub-3.jsonl", durationMs: 1234 }),
		);
		assert.equal(msg.details.session_file, "/sessions/sub-3.jsonl");
		assert.equal(msg.details.durationMs, 1234);
	});
});
