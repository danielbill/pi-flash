/**
 * Versioned custom entry that records one delegated child.
 *
 * Written by the extension on start, read back on `session_start` to detect
 * children that never reached a terminal state (orphans) and to restore the
 * composition snapshot a resume needs. Modelled on dsh-subagent's
 * `subagent/src/descriptor.ts`, reduced to what pi exposes through
 * `pi.appendEntry`.
 */

export const DESCRIPTOR_TYPE = "subagent/descriptor";
export const DESCRIPTOR_VERSION = 1;

export type DescriptorPhase = "provisioning" | "active" | "completed" | "interrupted" | "failed";

export interface SubagentDescriptor {
	version: number;
	childId: string;
	/** Parent tool call id, `<parent>/<n>` for nested calls. */
	parentToolCallId?: string;
	provider: string;
	label?: string;
	profile?: string;
	model?: string;
	cwd: string;
	depth: number;
	/** Session file of the child, when the child is a durable session. */
	sessionFile?: string;
	/** Composition snapshot, restored verbatim on resume. */
	composition: {
		model?: string;
		systemPrompt?: string;
		tools?: string[];
	};
	phase: DescriptorPhase;
	/** Set once the activation settled. `max_turns` = the soft limit was blown past. */
	stopReason?: "completed" | "aborted" | "error" | "max_turns";
	errorCode?: string;
	/** Child token usage, folded in at settle. Optional: v1 descriptors written
	 * before the field existed simply lack it (panel shows "—"). */
	usage?: { input: number; output: number; cacheRead?: number; cacheWrite?: number; total?: number };
	startedAt: string;
	updatedAt: string;
}

export function createDescriptor(input: {
	childId: string;
	provider: string;
	cwd: string;
	depth: number;
	parentToolCallId?: string;
	label?: string;
	profile?: string;
	model?: string;
	sessionFile?: string;
	composition?: SubagentDescriptor["composition"];
	now?: Date;
}): SubagentDescriptor {
	const timestamp = (input.now ?? new Date()).toISOString();
	return {
		version: DESCRIPTOR_VERSION,
		childId: input.childId,
		parentToolCallId: input.parentToolCallId,
		provider: input.provider,
		label: input.label,
		profile: input.profile,
		model: input.model,
		cwd: input.cwd,
		depth: input.depth,
		sessionFile: input.sessionFile,
		composition: input.composition ?? {},
		phase: "provisioning",
		startedAt: timestamp,
		updatedAt: timestamp,
	};
}

/**
 * Validate a stored descriptor.
 * Unknown versions and missing required fields fail closed: a descriptor we cannot
 * read is never treated as a live child.
 */
export function parseDescriptor(value: unknown): SubagentDescriptor {
	if (typeof value !== "object" || value === null) {
		throw new Error("descriptor must be an object");
	}
	const record = value as Record<string, unknown>;
	if (record.version !== DESCRIPTOR_VERSION) {
		throw new Error(`unsupported descriptor version: ${String(record.version)}`);
	}
	for (const key of ["childId", "provider", "cwd"] as const) {
		if (typeof record[key] !== "string" || record[key] === "") {
			throw new Error(`descriptor.${key} must be a non-empty string`);
		}
	}
	if (typeof record.depth !== "number" || !Number.isSafeInteger(record.depth) || record.depth < 1) {
		throw new Error("descriptor.depth must be a positive safe integer");
	}
	const composition = record.composition;
	if (typeof composition !== "object" || composition === null) {
		throw new Error("descriptor.composition must be an object");
	}
	return record as unknown as SubagentDescriptor;
}

/** Phases that mean "this child is still supposed to be running". */
export function isInFlight(phase: DescriptorPhase): boolean {
	return phase === "provisioning" || phase === "active";
}

/**
 * Reconcile descriptors from a restored session: anything in flight becomes
 * `interrupted`. `aborted` on the session's settle event distinguishes a
 * user cancel from a crash, so the caller passes it in.
 */
export function reconcileOnStart(
	descriptors: readonly SubagentDescriptor[],
	options: { aborted: boolean; now?: Date },
): SubagentDescriptor[] {
	const timestamp = (options.now ?? new Date()).toISOString();
	return descriptors.map((descriptor) => {
		if (!isInFlight(descriptor.phase)) return descriptor;
		return {
			...descriptor,
			phase: options.aborted ? "interrupted" : "failed",
			errorCode: options.aborted ? "ABORTED" : "START_REJECTED",
			updatedAt: timestamp,
		};
	});
}
