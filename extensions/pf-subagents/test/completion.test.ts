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
	// 044-6 §3.3：三段式结算消息（摘要 + Its closing message + Structured result）
	it("builds the three-part settlement notice for a completed run", () => {
		const msg = composeCompletion(result());
		assert.equal(
			msg.content.startsWith(
				"Background subagent sub-3 finished. Its closing message follows. It will do no further work unless you send it more.",
			),
			true,
		);
		assert.equal(msg.content.includes("\nIts closing message:\n## 结论\n\n查了三个来源。"), true);
		assert.equal(msg.content.includes('\nStructured result: {"childId":"sub-3","stopReason":"completed"}'), true);
		assert.equal(msg.content.includes("已截断"), false);
		assert.equal(COMPLETION_TYPE, "subagent-completion");
	});

	it("keeps the toast Chinese and short", () => {
		const long = "字".repeat(900);
		const msg = composeCompletion(result({ text: long }));
		assert.equal(msg.toast.startsWith("子代理完成 sub-3: "), true);
		assert.equal(msg.toast.length <= "子代理完成 sub-3: ".length + 401, true);
	});

	// dsh parity: 正文不截断（原先截 8000 字）
	it("never truncates the closing message", () => {
		const long = "a".repeat(8100);
		const msg = composeCompletion(result({ text: long }));
		assert.equal(msg.content.includes(long), true);
		assert.equal(msg.content.includes("已截断"), false);
	});

	it("names the child session when the report is huge", () => {
		const huge = "b".repeat(128 * 1024 + 1);
		const msg = composeCompletion(result({ text: huge, session_file: "/sessions/sub-3.jsonl" }));
		assert.equal(msg.content.includes("The full report is in the child session: /sessions/sub-3.jsonl"), true);
		// 附路径不代替正文
		assert.equal(msg.content.includes(huge), true);
	});

	it("summarises an error run with its code", () => {
		const msg = composeCompletion(
			result({ stopReason: "error", errorCode: "MAX_TURNS", text: "预算用尽" }),
		);
		assert.equal(
			msg.content.startsWith(
				"Background subagent sub-3 failed before it finished. (MAX_TURNS) Its closing message follows.",
			),
			true,
		);
		assert.equal(msg.details.errorCode, "MAX_TURNS");
		assert.equal(msg.toast.startsWith("子代理结束（MAX_TURNS） sub-3: "), true);
	});

	it("summarises aborted and max_turns runs in their own words", () => {
		const aborted = composeCompletion(result({ stopReason: "aborted" }));
		assert.equal(aborted.content.startsWith("Background subagent sub-3 was stopped before it finished."), true);
		const maxed = composeCompletion(result({ stopReason: "max_turns" }));
		assert.equal(maxed.content.startsWith("Background subagent sub-3 ran out of turn budget before it finished."), true);
	});

	it("falls back to stopReason when no error code is present", () => {
		const msg = composeCompletion(result({ stopReason: "aborted" }));
		assert.equal(msg.toast.startsWith("子代理结束（aborted）"), true);
		assert.equal("errorCode" in msg.details, false);
	});

	it("carries the child session file and duration in both payloads", () => {
		const msg = composeCompletion(
			result({ session_file: "/sessions/sub-3.jsonl", durationMs: 1234 }),
		);
		assert.equal(msg.details.session_file, "/sessions/sub-3.jsonl");
		assert.equal(msg.details.durationMs, 1234);
		assert.equal(
			msg.content.includes(
				'Structured result: {"childId":"sub-3","stopReason":"completed","durationMs":1234,"sessionFile":"/sessions/sub-3.jsonl"}',
			),
			true,
		);
	});
});
