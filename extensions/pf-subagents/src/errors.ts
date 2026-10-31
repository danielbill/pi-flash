/**
 * Stable failure codes.
 *
 * Every rejection the manager or a provider raises carries one of these codes, so
 * hosts and codemode scripts program against them instead of parsing prose.
 * Ported from dsh-subagent's failure discrimination (`subagent/src/control-types.ts`,
 * `manager.ts`), trimmed to the codes M1 can actually produce.
 */

export const SubagentErrorCode = {
	/** No provider registered under the requested name. */
	PROVIDER_UNKNOWN: "PROVIDER_UNKNOWN",
	/** The provider exists but cannot serve the requested capability. */
	BACKEND_UNSUPPORTED: "BACKEND_UNSUPPORTED",
	/** A provider name was registered twice. */
	DUPLICATE_PROVIDER: "DUPLICATE_PROVIDER",
	/** A child id is already owned by the same runtime. */
	DUPLICATE_CHILD: "DUPLICATE_CHILD",
	/** Per-parent online quota is full. Never queued: the caller must wait or delegate fewer. */
	CAPACITY_REACHED: "CAPACITY_REACHED",
	/** Requested delegation depth exceeds the configured limit. */
	DEPTH_LIMIT: "DEPTH_LIMIT",
	/** The child rejected the initial task or failed to start. */
	START_REJECTED: "START_REJECTED",
	/** Admission to a settling or released child. */
	ADMISSION_CLOSED: "ADMISSION_CLOSED",
	/** The profile document is missing, unreadable, or invalid. */
	PROFILE_INVALID: "PROFILE_INVALID",
	/** The work was aborted (parent turn cancel, session shutdown). */
	ABORTED: "ABORTED",
} as const;

export type SubagentErrorCode = (typeof SubagentErrorCode)[keyof typeof SubagentErrorCode];

export class SubagentError extends Error {
	readonly code: SubagentErrorCode;

	constructor(code: SubagentErrorCode, message: string, options?: { cause?: unknown }) {
		super(message, options);
		this.name = "SubagentError";
		this.code = code;
	}

	static is(value: unknown): value is SubagentError {
		return value instanceof SubagentError;
	}
}

/** Map any thrown value onto a stable code without losing the original error. */
export function asSubagentError(error: unknown, fallback: SubagentErrorCode): SubagentError {
	if (SubagentError.is(error)) return error;
	const message = error instanceof Error ? error.message : String(error);
	return new SubagentError(fallback, message, { cause: error });
}
