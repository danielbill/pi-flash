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
const CONTENT_CHARS = 8000;

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

/** Builds the toast + parent-session message for one settled child. */
export function composeCompletion(result: SubagentResult): CompletionMessage {
	const head = title(result);
	const details: Record<string, unknown> = {
		childId: result.childId,
		stopReason: result.stopReason,
	};
	if (result.errorCode) details.errorCode = result.errorCode;
	if (result.session_file) details.session_file = result.session_file;
	if (result.durationMs !== undefined) details.durationMs = result.durationMs;
	const truncated = result.text.length > CONTENT_CHARS;
	return {
		toast: `${head} ${result.childId}: ${clip(result.text, TOAST_CHARS)}`,
		content: `${head} ${result.childId}\n\n${clip(result.text, CONTENT_CHARS)}${
			truncated ? "\n\n（内容已截断，完整回复见子会话）" : ""
		}`,
		details,
	};
}
