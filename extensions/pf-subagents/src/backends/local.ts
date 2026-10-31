/**
 * Local child sessions, expressed against a narrow interface.
 *
 * The backend does not import the pi SDK directly: it receives a `SessionFactory`.
 * That keeps the mechanics testable with a scripted session (the only fake in the
 * suite) and lets the extension entry decide how a real session is created.
 */

import type { ChildHandle, Provider, ProviderCapabilities, SubagentRequest, SubagentResult, SubagentUsage } from "../types.ts";
import { asSubagentError, SubagentError, SubagentErrorCode } from "../errors.ts";
import { DEFAULT_MAX_TURNS } from "../profile.ts";

/** The slice of an `AgentSession` a subagent needs. */
export interface ChildSession {
	/** Run one prompt; resolves when the run finishes. */
	prompt(text: string): Promise<void>;
	/** Latest assistant text, used as the child's answer. */
	lastAssistantText(): string | undefined;
	/** Token usage of the child's last run (assistant message usage). */
	usage?(): SubagentUsage | undefined;
	/** Stop the current run, keeping queued work. */
	abort(): Promise<void>;
	/** Queue a steering message into the running turn; `undefined` when unsupported. */
	steer?(text: string): Promise<"queued" | "handled" | undefined> | "queued" | "handled" | undefined;
	/** Release the session. Idempotent. */
	dispose(): void;
	/** Persisted session file, when the child is durable. */
	sessionFile?: string;
	/** Subscribe to session events for progress reporting. */
	subscribe?(listener: (event: unknown) => void): () => void;
}

export interface SessionFactoryOptions {
	cwd: string;
	model?: string;
	/** Replacement persona from the profile; wins over {@link appendSystemPrompt}. */
	systemPrompt?: string;
	/** Prompt text appended to the child's default system prompt, from the profile. */
	appendSystemPrompt?: string;
	/** Tool tier resolved to an allowlist; `undefined` means inherit the delegator's. */
	tools?: string[];
	/** Whether the child loads the project AGENTS.md; `false` suppresses context files. */
	injectAgentsMd?: boolean;
	/** Turn budget for the soft limit; `undefined` means no budget. */
	maxTurns?: number;
	/** Session file of the parent, recorded in the child header for fork children. */
	parentSession?: string;
	/** True when the child must start from the parent's completed history. */
	inheritParentContext?: boolean;
	/** Stamped into the child's marker entry (side-bar filtering keys on it). */
	providerName?: string;
	/** Delegation depth, recorded in the marker entry. */
	depth?: number;
}

export type SessionFactory = (options: SessionFactoryOptions) => Promise<ChildSession>;

export interface LocalProviderOptions {
	name: string;
	factory: SessionFactory;
	/** Whether children start from the parent's completed history. */
	inheritsParentContext: boolean;
	/** Numeric depth limit honoured by this backend. */
	depthLimit?: number;
	/** Whether children accept further messages. */
	continuable?: boolean;
	/** Resolve per-child composition (profile) before the session is created. */
	resolveComposition?: (request: SubagentRequest) => Promise<ResolvedComposition>;
}

/** Everything the profile contributes to one child's setup. */
export interface ResolvedComposition {
	systemPrompt?: string;
	appendSystemPrompt?: string;
	tools?: string[];
	model?: string;
	injectAgentsMd?: boolean;
	maxTurns?: number;
}

/** Steering message injected at the turn budget; the child should wrap up now. */
export const TURN_LIMIT_STEER =
	"You have reached this task's turn budget. Do not start new work or new tool calls. " +
	"Summarise what you have established so far and give your final answer now.";

/** One `message_end` event as far as the turn limiter cares. */
interface MessageEndEvent {
	type?: string;
	message?: { role?: string; content?: unknown[] };
}

/** Turns already taken, and whether a wrap-up steer is still owed. */
class TurnBudget {
	#limit: number;
	#turns = 0;
	#steered = false;
	#exceeded = false;
	#onSteer: () => void;
	#onExceed: () => void;

	constructor(limit: number, onSteer: () => void, onExceed: () => void) {
		this.#limit = limit;
		this.#onSteer = onSteer;
		this.#onExceed = onExceed;
	}

	/** Record one completed assistant turn; fires the soft limit at the right moment. */
	assistantTurn(message: { content?: unknown[] } | undefined): void {
		if (this.#exceeded) return;
		this.#turns += 1;
		if (!this.#steered && this.#turns >= this.#limit) {
			// Budget reached: ask for a wrap-up instead of cutting the child off.
			this.#steered = true;
			this.#onSteer();
			return;
		}
		// The wrap-up turn itself counts; a further turn means the steer was ignored.
		if (this.#steered && this.#turns > this.#limit) {
			const content = message?.content ?? [];
			const stillWorking = content.some((block) => (block as { type?: string })?.type === "toolCall");
			if (stillWorking) {
				this.#exceeded = true;
				this.#onExceed();
			}
		}
	}

	/** Whether the budget was blown past despite the wrap-up steer. */
	get exceeded(): boolean {
		return this.#exceeded;
	}
}

/** Child handle over one real session. */
class LocalChild implements ChildHandle {
	readonly id: string;
	#session: ChildSession;
	#run: Promise<void> | undefined;
	#failure: unknown;
	#disposed = false;
	#started_at: number | undefined;
	#budget: TurnBudget | undefined;
	#unsubscribe: (() => void) | undefined;

	constructor(id: string, session: ChildSession, maxTurns?: number) {
		this.id = id;
		this.#session = session;
		if (maxTurns !== undefined && session.subscribe) {
			this.#budget = new TurnBudget(
				maxTurns,
				() => {
					void session.steer?.(TURN_LIMIT_STEER);
				},
				() => {
					void session.abort();
				},
			);
			this.#unsubscribe = session.subscribe((event) => this.#onEvent(event));
		}
	}

	#onEvent(event: unknown): void {
		const record = event as MessageEndEvent;
		if (record?.type !== "message_end") return;
		if (record.message?.role !== "assistant") return;
		this.#budget?.assistantTurn(record.message);
	}

	get sessionFile(): string | undefined {
		return this.#session.sessionFile;
	}

	async accept(prompt: string): Promise<void> {
		// Admission means "the child took the task", not "the child finished it".
		this.#run = this.#session.prompt(prompt).catch((error: unknown) => {
			this.#failure = error;
		});
		// Let a synchronous rejection surface before we call the child accepted.
		await Promise.resolve();
		if (this.#failure) throw asSubagentError(this.#failure, SubagentErrorCode.START_REJECTED);
	}

	async send(message: string): Promise<void> {
		if (this.#disposed) throw new SubagentError(SubagentErrorCode.ADMISSION_CLOSED, "child disposed");
		this.#run = this.#session.prompt(message).catch((error: unknown) => {
			this.#failure = error;
		});
		await Promise.resolve();
		if (this.#failure) throw asSubagentError(this.#failure, SubagentErrorCode.START_REJECTED);
	}

	async interrupt(): Promise<void> {
		await this.#session.abort();
	}

	session_file(): string | undefined {
		return this.#session.sessionFile;
	}

	async result(): Promise<SubagentResult> {
		if (this.#run) await this.#run;
		const usage = this.#session.usage?.();
		const durationMs = this.#started_at !== undefined ? Date.now() - this.#started_at : undefined;
		if (this.#failure) {
			const failure = asSubagentError(this.#failure, SubagentErrorCode.START_REJECTED);
			return { childId: this.id, text: "", stopReason: "error", errorCode: failure.code, usage, durationMs };
		}
		// A blown budget reports itself even when the run ended without throwing:
		// the abort surfaced as a normal settle, but the work did not complete.
		if (this.#budget?.exceeded) {
			return {
				childId: this.id,
				text: this.#session.lastAssistantText() ?? "",
				stopReason: "max_turns",
				usage,
				durationMs,
			};
		}
		return {
			childId: this.id,
			text: this.#session.lastAssistantText() ?? "",
			stopReason: "completed",
			usage,
			durationMs,
		};
	}

	async dispose(): Promise<void> {
		if (this.#disposed) return;
		this.#disposed = true;
		this.#unsubscribe?.();
		this.#session.dispose();
	}
}

/** Provider over local pi sessions: `spawn` (fresh) or `fork` (inherits completed history). */
export class LocalProvider implements Provider {
	readonly name: string;
	readonly capabilities: ProviderCapabilities;
	#options: LocalProviderOptions;
	readonly sessions = new Map<string, LocalChild>();

	constructor(options: LocalProviderOptions) {
		this.name = options.name;
		this.#options = options;
		this.capabilities = {
			continuable: options.continuable ?? true,
			inheritsParentContext: options.inheritsParentContext,
			depthLimit: options.depthLimit ?? 1,
			modelOptions: true,
			worktree: false,
		};
	}

	async prepare(request: SubagentRequest): Promise<ChildHandle> {
		const childId = request.childId;
		if (!childId) throw new SubagentError(SubagentErrorCode.START_REJECTED, "manager did not reserve a child id");
		const composition = this.#options.resolveComposition
			? await this.#options.resolveComposition(request)
			: {};
		// The delegator's per-call `maxloop` wins over the profile's `maxTurns`;
		// `0` is the "unlimited" tier, so it never falls through to the default.
		const requested = request.maxTurns ?? composition.maxTurns ?? DEFAULT_MAX_TURNS;
		// Only a positive budget arms the limiter; `0` means no budget at all.
		const maxTurns = requested > 0 ? requested : undefined;
		let session: ChildSession;
		try {
			session = await this.#options.factory({
				cwd: request.cwd ?? process.cwd(),
				model: composition.model ?? request.model,
				systemPrompt: composition.systemPrompt,
				appendSystemPrompt: composition.appendSystemPrompt,
				tools: composition.tools,
				injectAgentsMd: composition.injectAgentsMd,
				maxTurns,
				inheritParentContext: this.#options.inheritsParentContext,
				providerName: this.#options.name,
				depth: request.depth,
			});
		} catch (error) {
			throw asSubagentError(error, SubagentErrorCode.START_REJECTED);
		}
		const child = new LocalChild(childId, session, maxTurns);
		this.sessions.set(childId, child);
		return child;
	}
}
