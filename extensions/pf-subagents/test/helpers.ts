/**
 * Scripted provider for tests: a real handle with controllable failure points.
 * Mirrors dsh-subagent's `tool-subagent/tests/scripted-provider.spec.ts` intent —
 * only the backend is faked, the manager, capacity, and registry stay real.
 */

import type { ChildHandle, ChildProgress, ChildSessionStats, Provider, ProviderCapabilities, SubagentRequest, SubagentResult, SubagentUsage } from "../src/types.ts";

export interface ScriptedOptions {
	failPrepare?: boolean;
	failAccept?: boolean;
	/** Return a different id than the manager reserved. */
	idOverride?: string;
	/** Durable session file the handle reports (B2: panel rows click through it). */
	sessionFile?: string;
	capabilities?: Partial<ProviderCapabilities>;
	/** Complete automatically when disposed (default: keep waiting for `complete()`). */
	completeOnDispose?: boolean;
}

export class ScriptedChild implements ChildHandle {
	readonly id: string;
	accepted: string[] = [];
	sent: string[] = [];
	interrupts = 0;
	disposals = 0;
	#options: ScriptedOptions;
	#resolve!: (result: SubagentResult) => void;
	#result: Promise<SubagentResult>;
	#disposeBarrier: Promise<void> | undefined;
	#releaseDispose: (() => void) | undefined;
	#watchers: Array<(progress: ChildProgress) => void> = [];

	constructor(id: string, options: ScriptedOptions) {
		this.id = id;
		this.#options = options;
		this.#result = new Promise<SubagentResult>((resolve) => {
			this.#resolve = resolve;
		});
	}

	async accept(prompt: string): Promise<void> {
		if (this.#options.failAccept) throw new Error("scripted accept failure");
		this.accepted.push(prompt);
	}

	async send(message: string): Promise<void> {
		this.sent.push(message);
	}

	async interrupt(): Promise<void> {
		this.interrupts += 1;
	}

	session_file(): string | undefined {
		return this.#options.sessionFile;
	}

	watch(listener: (progress: ChildProgress) => void): () => void {
		this.#watchers.push(listener);
		return () => {
			this.#watchers = this.#watchers.filter((w) => w !== listener);
		};
	}

	/** Emit activity counters (and optional usage) as if an assistant turn just completed. */
	progress(stats: ChildSessionStats, usage?: SubagentUsage): void {
		for (const watcher of this.#watchers) watcher({ stats, ...(usage ? { usage } : {}) });
	}

	result(): Promise<SubagentResult> {
		return this.#result;
	}

	/** Hold cleanup open so a test can observe the settling window. */
	blockDispose(): () => void {
		this.#disposeBarrier = new Promise<void>((resolve) => {
			this.#releaseDispose = resolve;
		});
		return () => this.#releaseDispose?.();
	}

	async dispose(): Promise<void> {
		this.disposals += 1;
		if (this.#disposeBarrier) await this.#disposeBarrier;
		if (this.#options.completeOnDispose) this.complete("done");
	}

	complete(
		text: string,
		stopReason: SubagentResult["stopReason"] = "completed",
		stats?: ChildSessionStats,
	): void {
		this.#resolve({ childId: this.id, text, stopReason, ...(stats ? { stats } : {}) });
	}
}

export class ScriptedProvider implements Provider {
	readonly name: string;
	readonly capabilities: ProviderCapabilities;
	readonly children: ScriptedChild[] = [];
	prepareCalls = 0;
	#options: ScriptedOptions;

	constructor(name = "scripted", options: ScriptedOptions = {}) {
		this.name = name;
		this.#options = options;
		this.capabilities = {
			continuable: true,
			inheritsParentContext: false,
			depthLimit: 1,
			modelOptions: true,
			worktree: false,
			...options.capabilities,
		};
	}

	async prepare(request: SubagentRequest): Promise<ChildHandle> {
		this.prepareCalls += 1;
		if (this.#options.failPrepare) throw new Error("scripted prepare failure");
		const child = new ScriptedChild(this.#options.idOverride ?? request.childId ?? "unknown", this.#options);
		this.children.push(child);
		return child;
	}
}

export const baseRequest = (overrides: Partial<SubagentRequest> = {}): SubagentRequest => {
	return {
		provider: "scripted",
		parentId: "parent-a",
		depth: 1,
		prompt: "do the thing",
		delivery: "caller",
		...overrides,
	};
};

/** Node's `assert.throws` returns nothing; tests need the thrown value to inspect `code`. */
export function captureSync(fn: () => unknown): unknown {
	try {
		fn();
	} catch (error) {
		return error;
	}
	throw new Error("expected the call to throw");
}
