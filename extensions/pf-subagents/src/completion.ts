import type { SubagentResult } from "./types.ts";

/**
 * customType of the message injected into the parent session when a
 * parent-delivery (background) child settles. Parent models only see tool
 * results for `delivery: "result"`; background runs settle after the spawning
 * turn ended, so the outcome has to enter the conversation as its own message.
 */
export const COMPLETION_TYPE = "subagent-completion";

/** Toast keeps to one line-ish preview; the message carries the full report. */
const TOAST_CHARS = 400;

/**
 * Reports beyond this also name the child session file. The body is never
 * truncated (dsh parity): the model decides what to read, compaction is the
 * safety net.
 */
const SESSION_HINT_CHARS = 128 * 1024;

export interface CompletionMessage {
	/** Short UI toast text (host notification strip). */
	toast: string;
	/** Message body delivered to the parent session. */
	content: string;
	/** Structured payload for host renderers. */
	details: Record<string, unknown>;
}

function title(result: SubagentResult): string {
	return result.stopReason === "completed"
		? "子代理完成"
		: `子代理结束（${result.errorCode ?? result.stopReason}）`;
}

function clip(text: string, max: number): string {
	return text.length > max ? `${text.slice(0, max)}…` : text;
}

/**
 * One line per stopReason, mirroring dsh `settlementSummary`
 * (`packages/subagent/subagent/src/continuation-messages.ts:104-130`).
 * pf children always accept follow-up messages (`subagent_send`), so a
 * completed run also says it will do no further work.
 */
function settlementSummary(result: SubagentResult): string {
	const head = `Background subagent ${result.childId}`;
	switch (result.stopReason) {
		case "completed":
			return `${head} finished. Its closing message follows. It will do no further work unless you send it more.`;
		case "aborted":
			return `${head} was stopped before it finished. Its closing message follows.`;
		case "max_turns":
			return `${head} ran out of turn budget before it finished. Its closing message follows.`;
		default:
			return `${head} failed before it finished. (${result.errorCode ?? result.stopReason}) Its closing message follows.`;
	}
}

/** `Structured result: {...}` tail — the machine-readable half of the notice. */
function structuredResult(result: SubagentResult): string {
	const payload: Record<string, unknown> = {
		childId: result.childId,
		stopReason: result.stopReason,
	};
	if (result.durationMs !== undefined) payload.durationMs = result.durationMs;
	if (result.session_file) payload.sessionFile = result.session_file;
	if (result.errorCode) payload.errorCode = result.errorCode;
	return JSON.stringify(payload);
}

/** Builds the toast + parent-session message for one settled child. */
export function composeCompletion(result: SubagentResult): CompletionMessage {
	const details: Record<string, unknown> = {
		childId: result.childId,
		stopReason: result.stopReason,
	};
	if (result.errorCode) details.errorCode = result.errorCode;
	if (result.session_file) details.session_file = result.session_file;
	if (result.durationMs !== undefined) details.durationMs = result.durationMs;

	const parts = [
		settlementSummary(result),
		"Its closing message:",
		result.text,
		`Structured result: ${structuredResult(result)}`,
	];
	if (result.text.length > SESSION_HINT_CHARS && result.session_file) {
		parts.push(`The full report is in the child session: ${result.session_file}`);
	}

	return {
		toast: `${title(result)} ${result.childId}: ${clip(result.text, TOAST_CHARS)}`,
		content: parts.join("\n"),
		details,
	};
}
