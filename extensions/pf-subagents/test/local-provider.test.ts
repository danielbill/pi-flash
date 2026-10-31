import assert from "node:assert/strict";
import { describe, it } from "node:test";

import type { ChildSession, SessionFactoryOptions } from "../src/backends/local.ts";
import { LocalProvider } from "../src/backends/local.ts";
import { DEFAULT_MAX_TURNS, TOOL_TIER_WHITELISTS } from "../src/profile.ts";
import { SubagentError, SubagentErrorCode } from "../src/errors.ts";
import { SubagentManager } from "../src/manager.ts";
import { baseRequest } from "./helpers.ts";

/** The only fake in the suite: a scripted child session. */
class ScriptedSession implements ChildSession {
	prompts: string[] = [];
	aborts = 0;
	disposals = 0;
	steers: string[] = [];
	sessionFile = "/tmp/child.jsonl";
	#answer = "";
	#fail: Error | undefined;
	#hold: boolean;
	#listeners: Array<(event: unknown) => void> = [];
	#pending: Array<{ resolve: () => void; reject: (error: Error) => void }> = [];

	constructor(options: { hold?: boolean } = {}) {
		this.#hold = options.hold ?? false;
	}

	/** Emit one `message_end`; the turn limiter counts assistant turns only. */
	emitAssistantTurn(options: { toolCall?: boolean } = {}): void {
		const content = options.toolCall ? [{ type: "toolCall", name: "read" }] : [{ type: "text", text: "done" }];
		for (const listener of this.#listeners) {
			listener({ type: "message_end", message: { role: "assistant", content } });
		}
	}

	subscribe(listener: (event: unknown) => void): () => void {
		this.#listeners.push(listener);
		return () => {
			this.#listeners = this.#listeners.filter((l) => l !== listener);
		};
	}

	async steer(text: string): Promise<"queued"> {
		this.steers.push(text);
		return "queued";
	}

	async prompt(text: string): Promise<void> {
		this.prompts.push(text);
		if (this.#fail) throw this.#fail;
		if (!this.#hold) {
			this.#answer = `answer:${text}`;
			return;
		}
		// Hold the run open so a test can decide how it ends.
		await new Promise<void>((resolve, reject) => {
			this.#pending.push({ resolve, reject });
		});
	}

	/** Finish the oldest held prompt. */
	settle(answer: string): void {
		this.#answer = answer;
		this.#pending.shift()?.resolve();
	}

	/** Fail the oldest held prompt. */
	crash(error: Error): void {
		this.#pending.shift()?.reject(error);
	}

	lastAssistantText(): string | undefined {
		return this.#answer;
	}

	async abort(): Promise<void> {
		this.aborts += 1;
	}

	dispose(): void {
		this.disposals += 1;
	}

	failWith(error: Error): void {
		this.#fail = error;
	}
}

const setup = (
	options: {
		inheritsParentContext?: boolean;
		factoryThrows?: boolean;
		sessions?: ScriptedSession[];
		hold?: boolean;
	} = {},
) => {
	const sessions = options.sessions ?? [];
	const seen: SessionFactoryOptions[] = [];
	const provider = new LocalProvider({
		name: "spawn",
		inheritsParentContext: options.inheritsParentContext ?? false,
		factory: async (factoryOptions) => {
			seen.push(factoryOptions);
			if (options.factoryThrows) throw new Error("factory exploded");
			const session = new ScriptedSession({ hold: options.hold });
			sessions.push(session);
			return session;
		},
	});
	const manager = new SubagentManager();
	manager.registry.register(provider);
	return { manager, provider, sessions, seen };
};

describe("LocalProvider", () => {
	it("declares its capabilities", () => {
		const { provider } = setup();
		assert.equal(provider.capabilities.continuable, true);
		assert.equal(provider.capabilities.inheritsParentContext, false);
		assert.equal(provider.capabilities.depthLimit, 1);
	});

	it("returns the child's final answer as the result", async () => {
		const { manager, sessions } = setup();
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		const result = await activation.wait();
		assert.equal(result.text, "answer:do the thing");
		assert.equal(result.stopReason, "completed");
		assert.deepEqual(sessions[0].prompts, ["do the thing"]);
		assert.equal(sessions[0].disposals, 1);
	});

	it("does not await the whole run before calling the child accepted", async () => {
		const { manager, sessions } = setup({ hold: true });
		// start() must resolve while the run is still in flight.
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		assert.equal(activation.childId, "sub-1");
		assert.deepEqual(sessions[0].prompts, ["do the thing"]);
		assert.equal(sessions[0].disposals, 0);
		sessions[0].settle("finished");
		const result = await activation.wait();
		assert.equal(result.text, "finished");
	});

	it("forwards further input to a continuable child", async () => {
		const { manager, sessions } = setup();
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		await activation.send("and also this");
		await activation.wait();
		assert.deepEqual(sessions[0].prompts, ["do the thing", "and also this"]);
	});

	it("interrupts through the session", async () => {
		const { manager, sessions } = setup();
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		await activation.interrupt();
		assert.equal(sessions[0].aborts, 1);
		await activation.wait();
	});

	it("reports a start failure with a stable code", async () => {
		const { manager } = setup({ factoryThrows: true });
		const error = await manager.start(baseRequest({ provider: "spawn" })).catch((e: unknown) => e);
		assert.equal((error as SubagentError).code, SubagentErrorCode.START_REJECTED);
	});

	it("reports a child failure as an error result instead of throwing from wait", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		sessions[0].crash(new Error("model exploded"));
		const result = await activation.wait();
		assert.equal(result.stopReason, "error");
		assert.equal(result.errorCode, SubagentErrorCode.START_REJECTED);
	});

	it("passes the turn budget to the factory and the child", async () => {
		const { manager, seen } = setup();
		await manager.start(baseRequest({ provider: "spawn", maxTurns: 7 }));
		assert.equal(seen[0].maxTurns, 7);
	});

	it("steers a wrap-up at the budget instead of cutting the child off", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn", maxTurns: 3 }));
		// Two turns stay under budget: no steer, no abort.
		sessions[0].emitAssistantTurn();
		sessions[0].emitAssistantTurn();
		assert.deepEqual(sessions[0].steers, []);
		assert.equal(sessions[0].aborts, 0);
		// The third turn hits the budget: a wrap-up steer fires, nothing is cut.
		sessions[0].emitAssistantTurn();
		assert.equal(sessions[0].steers.length, 1);
		assert.match(sessions[0].steers[0], /turn budget/i);
		assert.equal(sessions[0].aborts, 0);
		// The child obeys the steer and answers plainly: that is a completed run.
		sessions[0].emitAssistantTurn();
		sessions[0].settle("wrapped up");
		const result = await activation.wait();
		assert.equal(result.stopReason, "completed");
	});

	it("aborts with max_turns when the child ignores the wrap-up and keeps working", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn", maxTurns: 2 }));
		sessions[0].emitAssistantTurn();
		sessions[0].emitAssistantTurn(); // budget hit -> steer
		assert.equal(sessions[0].steers.length, 1);
		// One more tool-calling turn past the budget: the steer was ignored.
		sessions[0].emitAssistantTurn({ toolCall: true });
		assert.equal(sessions[0].aborts, 1);
		sessions[0].settle("partial");
		const result = await activation.wait();
		assert.equal(result.stopReason, "max_turns");
	});

	it("keeps a child under the default budget unsteered", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		// The default budget is 100 (DEFAULT_MAX_TURNS); 12 turns stays under it.
		for (let i = 0; i < 12; i += 1) sessions[0].emitAssistantTurn({ toolCall: true });
		assert.deepEqual(sessions[0].steers, []);
		assert.equal(sessions[0].aborts, 0);
		sessions[0].settle("done");
		assert.equal((await activation.wait()).stopReason, "completed");
	});

	it("honours the unlimited tier (maxTurns: 0) by never steering", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn", maxTurns: 0 }));
		// Far past any plausible budget: the unlimited tier must stay silent.
		for (let i = 0; i < 250; i += 1) sessions[0].emitAssistantTurn({ toolCall: true });
		assert.deepEqual(sessions[0].steers, []);
		assert.equal(sessions[0].aborts, 0);
		sessions[0].settle("done");
		assert.equal((await activation.wait()).stopReason, "completed");
	});

	it("falls back to the default budget when the profile leaves it unset", async () => {
		const seen: SessionFactoryOptions[] = [];
		const provider = new LocalProvider({
			name: "spawn",
			inheritsParentContext: false,
			resolveComposition: async () => ({}),
			factory: async (o) => {
				seen.push(o);
				return new ScriptedSession();
			},
		});
		const manager = new SubagentManager();
		manager.registry.register(provider);
		await manager.start(baseRequest({ provider: "spawn" }));
		assert.equal(seen[0].maxTurns, DEFAULT_MAX_TURNS);
	});

	it("prefers the per-call maxloop over the profile's maxTurns", async () => {
		const seen: SessionFactoryOptions[] = [];
		const provider = new LocalProvider({
			name: "spawn",
			inheritsParentContext: false,
			resolveComposition: async () => ({ maxTurns: 40 }),
			factory: async (o) => {
				seen.push(o);
				return new ScriptedSession();
			},
		});
		const manager = new SubagentManager();
		manager.registry.register(provider);
		// Call-level budget wins.
		await manager.start(baseRequest({ provider: "spawn", maxTurns: 5 }));
		assert.equal(seen[0].maxTurns, 5);
		// Without a call-level budget the profile's applies.
		await manager.start(baseRequest({ provider: "spawn" }));
		assert.equal(seen[1].maxTurns, 40);
	});

	it("forwards the profile composition to the factory", async () => {
		const seen: SessionFactoryOptions[] = [];
		const provider = new LocalProvider({
			name: "spawn",
			inheritsParentContext: false,
			resolveComposition: async () => ({
				systemPrompt: "You are a reviewer.",
				appendSystemPrompt: "Focus on boundaries.",
				tools: [...TOOL_TIER_WHITELISTS["read-only"]],
				injectAgentsMd: false,
				model: "deepseek/deepseek-chat",
			}),
			factory: async (o) => {
				seen.push(o);
				return new ScriptedSession();
			},
		});
		const manager = new SubagentManager();
		manager.registry.register(provider);
		await manager.start(baseRequest({ provider: "spawn" }));
		assert.equal(seen[0].systemPrompt, "You are a reviewer.");
		assert.equal(seen[0].appendSystemPrompt, "Focus on boundaries.");
		assert.deepEqual(seen[0].tools, ["read", "grep", "find", "ls"]);
		assert.equal(seen[0].injectAgentsMd, false);
		assert.equal(seen[0].model, "deepseek/deepseek-chat");
	});

	it("keeps codemode/tool_search in the tiers that drop unlisted extension tools", () => {
		for (const tier of ["default", "full"] as const) {
			const list = TOOL_TIER_WHITELISTS[tier];
			assert.ok(list.includes("codemode"), `${tier} must keep codemode`);
			assert.ok(list.includes("tool_search"), `${tier} must keep tool_search`);
		}
		// Read-only has no scripting engine: neither belongs there.
		assert.ok(!TOOL_TIER_WHITELISTS["read-only"].includes("codemode"));
		// No tier may hand a child the agent tool: children cannot delegate.
		for (const tier of ["read-only", "default", "full"] as const) {
			assert.ok(!TOOL_TIER_WHITELISTS[tier].includes("agent"), `${tier} must not expose agent`);
		}
	});

	it("surfaces an immediate prompt rejection as a start failure", async () => {
		const manager = new SubagentManager();
		manager.registry.register(
			new LocalProvider({
				name: "spawn",
				inheritsParentContext: false,
				factory: async () => {
					const session = new ScriptedSession();
					session.failWith(new Error("rejected immediately"));
					return session;
				},
			}),
		);
		const error = await manager.start(baseRequest({ provider: "spawn" })).catch((e: unknown) => e);
		assert.equal((error as SubagentError).code, SubagentErrorCode.START_REJECTED);
		assert.equal(manager.childrenOf("parent-a").length, 0);
	});
});
