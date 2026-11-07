/**
 * In-memory record of one delegated child (lifecycle phase / stats / usage /
 * child session file). Formerly persisted as a `subagent/descriptor` custom
 * entry in the parent transcript; persistence is removed (pi-flash-9eck,
 * 2026-11-08 用户定夺): the host card is fed by the tool result details and
 * per-message appends flooded parent session files (209MB 实测).
 */

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
	/** Activity counters (044-0 面板行二：轮次/发言数/工具调用次数). Optional:
	 * pre-stats descriptors lack it (panel shows "—"). Refreshed on every
	 * assistant turn while the child runs. */
	stats?: { turns: number; agentMessages: number; toolCalls: number };
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
