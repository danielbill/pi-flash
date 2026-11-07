/**
 * Contracts shared by the manager, providers, and the delegation tool.
 *
 * Shape follows dsh-subagent (`subagent/src/types.ts`): a named provider registry,
 * a `prepareContinuable`-style local path versus a one-shot external path, and
 * delivery split into `caller` (await is the tool result) and `parent`
 * (immediate child id, completion notification). Interactive UI and cordis
 * registration are not part of the contract.
 */

/** What a backend can do. Requests asking for a missing capability are rejected, never silently downgraded. */
export interface ProviderCapabilities {
	/** Child accepts further messages after the initial task, and can be cold-resumed. */
	continuable: boolean;
	/** Child starts from the parent's completed history (fork) instead of fresh context. */
	inheritsParentContext: boolean;
	/**
	 * Numeric depth limit this backend honours, or `"provider-managed"` when the
	 * backend owns its own recursion budget (out-of-process children).
	 */
	depthLimit: number | "provider-managed";
	/** Accepts per-call model overrides. */
	modelOptions: boolean;
	/** Can isolate work in a separate git worktree. */
	worktree: boolean;
}

/** How the completed work reaches the caller. */
export type Delivery = "caller" | "parent";

export interface SubagentRequest {
	/** Provider name chosen by the tool configuration. */
	provider: string;
	/** Child identity reserved by the manager before the provider runs. */
	childId?: string;
	/** Owning session id, used for quota and ownership. */
	parentId: string;
	/** The parent's `agent` tool call id (044-0 descriptor field table): the
	 * key the host nav panel uses to group children under their turn. */
	parentToolCallId?: string;
	/** Depth of the child: parent depth + 1. Root callers pass 1. */
	depth: number;
	prompt: string;
	/** Working directory for the child; resolved against the parent cwd when relative. */
	cwd?: string;
	/** Profile id (`~/.pi/agent/agents/<id>.md` or `.pi/agents/<id>.md`). */
	profile?: string;
	/** Exact model override; rejected by backends that do not support model options. */
	model?: string;
	/** Short human label shown in the parent transcript. */
	label?: string;
	/**
	 * Per-call turn budget (the delegator's `maxloop` estimate). Wins over the
	 * profile's `maxTurns`; when neither is set the backend applies its default.
	 */
	maxTurns?: number;
	delivery: Delivery;
	/** Cancels work that has not been published yet. */
	signal?: AbortSignal;
}

export type StopReason = "completed" | "aborted" | "error" | "max_turns";

/** Live activity counters of one child (panel row 2 / card details, 044-0). */
export interface ChildSessionStats {
	/** Assistant turns taken. */
	turns: number;
	/** Assistant messages that carried a non-empty text block (agent 发言数). */
	agentMessages: number;
	/** Tool calls issued across all turns. */
	toolCalls: number;
}

/** Per-turn progress snapshot: counters plus the running usage total (B1 连带：
 * 面板行二运行中就要出 token，usage 随每轮 stats 顺带回传，不再只等 settle）. */
export interface ChildProgress {
	stats: ChildSessionStats;
	/** Cumulative token usage across the child's assistant turns so far. */
	usage?: SubagentUsage;
}

export interface SubagentUsage {
	input: number;
	output: number;
	cacheRead?: number;
	cacheWrite?: number;
	total?: number;
}

export interface SubagentResult {
	childId: string;
	text: string;
	stopReason: StopReason;
	usage?: SubagentUsage;
	durationMs?: number;
	/** Activity counters at settle (partial counts on error/max_turns). */
	stats?: ChildSessionStats;
	/** Persisted session file of the child (host "open child session" jump target). */
	session_file?: string;
	/** Present when `stopReason` is not `completed`. */
	errorCode?: string;
}

/** One live child, as owned by the manager. */
export interface ChildHandle {
	readonly id: string;
	/** Admission of the initial task. Resolves once the child has accepted it. */
	accept(prompt: string, options?: { signal?: AbortSignal }): Promise<void>;
	/** Deliver more work to a continuable child. Rejects when admission is closed. */
	send(message: string): Promise<void>;
	/** Stop the child's current turn; queued input and descendants are kept. */
	interrupt(): Promise<void>;
	/** Persisted session file of the child, when durable. */
	session_file(): string | undefined;
	/** Settles with the child's final answer. */
	result(): Promise<SubagentResult>;
	/**
	 * Observe live progress (one call per assistant turn: counters + cumulative
	 * usage). Returns an unsubscribe; absent when the backend cannot observe turns.
	 */
	watch?(listener: (progress: ChildProgress) => void): () => void;
	/** Release the child and its resources. Idempotent. */
	dispose(): Promise<void>;
}

export interface Provider {
	readonly name: string;
	readonly capabilities: ProviderCapabilities;
	/**
	 * Create a child and return a live handle. Reject before the handle is usable
	 * when the child cannot start at all; the manager then returns the capacity.
	 */
	prepare(request: SubagentRequest): Promise<ChildHandle>;
}

/** What a caller gets back for one activation. */
export interface Activation {
	readonly childId: string;
	readonly delivery: Delivery;
	/** Publish point: resolves after the child accepted the task. */
	readonly started: Promise<void>;
	/** For `caller` delivery the settlement every caller awaits. */
	wait(): Promise<SubagentResult>;
	/** Admit more work; throws `ADMISSION_CLOSED` after settlement starts. */
	send(message: string): Promise<void>;
	interrupt(): Promise<void>;
	/** Cancel and release; waits for cleanup, including descendants. */
	dispose(): Promise<void>;
}
