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
	emitAssistantTurn(options: { toolCall?: boolean; textCount?: number; toolCount?: number; usage?: unknown } = {}): void {
		let content: Array<{ type: string; text?: string; name?: string }>;
		if (options.textCount !== undefined || options.toolCount !== undefined) {
			content = [
				...Array.from({ length: options.textCount ?? 0 }, () => ({ type: "text", text: "speech" })),
				...Array.from({ length: options.toolCount ?? 0 }, () => ({ type: "toolCall", name: "read" })),
			];
		} else {
			content = options.toolCall ? [{ type: "toolCall", name: "read" }] : [{ type: "text", text: "done" }];
		}
		for (const listener of this.#listeners) {
			listener({
				type: "message_end",
				message: { role: "assistant", content, ...(options.usage ? { usage: options.usage } : {}) },
			});
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
		// 运行层归一（044-0 错误管理）：中途崩的 prompt 是模型商的错，不是准入失败。
		assert.equal(result.errorCode, SubagentErrorCode.PROVIDER_ERROR);
	});

	it("accumulates activity stats from assistant turns", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		sessions[0].emitAssistantTurn({ toolCall: true }); // 1 turn, 0 speech, 1 tool call
		sessions[0].emitAssistantTurn({ textCount: 1, toolCount: 2 }); // 1 turn, 1 speech, 2 calls
		sessions[0].emitAssistantTurn(); // 1 turn, 1 speech, 0 calls
		sessions[0].settle("done");
		const result = await activation.wait();
		assert.deepEqual(result.stats, { turns: 3, agentMessages: 2, toolCalls: 3 });
		assert.deepEqual(manager.descriptors()[0].stats, { turns: 3, agentMessages: 2, toolCalls: 3 });
	});

	it("keeps partial stats on an errored run", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		sessions[0].emitAssistantTurn({ toolCall: true });
		sessions[0].crash(new Error("429 too many requests"));
		const result = await activation.wait();
		assert.equal(result.stopReason, "error");
		assert.deepEqual(result.stats, { turns: 1, agentMessages: 0, toolCalls: 1 });
	});

	it("refreshes the descriptor stats on every assistant turn while active", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		sessions[0].emitAssistantTurn({ toolCall: true });
		assert.deepEqual(manager.descriptors()[0].stats, { turns: 1, agentMessages: 0, toolCalls: 1 });
		sessions[0].emitAssistantTurn({ textCount: 1 });
		assert.deepEqual(manager.descriptors()[0].stats, { turns: 2, agentMessages: 1, toolCalls: 1 });
		sessions[0].settle("done");
		await activation.wait();
		assert.equal(manager.descriptors()[0].phase, "completed");
	});

	// B1 连带（pi-flash-y26f）：usage 随每轮 message_end 累计并透出面板，
	// settle 的 result.usage 同口径（ΣtotalTokens），不再只报最后一轮。
	it("accumulates usage per turn and settles with the same total", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		const pending = activation.wait(); // caller delivery: the run sits in settling
		sessions[0].emitAssistantTurn({
			usage: { input: 100, output: 10, cacheRead: 5, cacheWrite: 0, totalTokens: 115, cost: { input: 10, output: 2, cacheRead: 0, cacheWrite: 0, total: 12 } },
		});
		assert.deepEqual(manager.descriptors()[0].usage, {
			input: 100,
			output: 10,
			cacheRead: 5,
			cacheWrite: 0,
			total: 115,
			totalTokens: 115,
			cost: { input: 10, output: 2, cacheRead: 0, cacheWrite: 0, total: 12 },
		});
		sessions[0].emitAssistantTurn({ usage: { input: 200, output: 20, cacheRead: 5, cacheWrite: 0, totalTokens: 225, cost: { input: 20, output: 4, cacheRead: 0, cacheWrite: 0, total: 24 } } });
		assert.equal(manager.descriptors()[0].usage?.total, 340);
		assert.equal(manager.descriptors()[0].usage?.totalTokens, 340);
		assert.equal(manager.descriptors()[0].usage?.cost?.total, 36);
		sessions[0].settle("done");
		const result = await pending;
		assert.equal(result.usage?.total, 340);
		assert.deepEqual(manager.descriptors()[0].usage, result.usage);
	});

	it("keeps turns without usage from zeroing the accumulated total", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn" }));
		const pending = activation.wait();
		sessions[0].emitAssistantTurn({ usage: { input: 50, output: 5, totalTokens: 55 } });
		sessions[0].emitAssistantTurn({ toolCall: true }); // no usage on this message
		assert.equal(manager.descriptors()[0].usage?.total, 55);
		assert.deepEqual(manager.descriptors()[0].stats, { turns: 2, agentMessages: 1, toolCalls: 1 });
		sessions[0].settle("done");
		const result = await pending;
		assert.equal(result.usage?.total, 55);
	});

	it("still counts stats without a turn budget", async () => {
		const { manager, sessions } = setup({ hold: true });
		const activation = await manager.start(baseRequest({ provider: "spawn", maxTurns: 0 }));
		sessions[0].emitAssistantTurn({ toolCall: true });
		sessions[0].settle("done");
		const result = await activation.wait();
		assert.deepEqual(result.stats, { turns: 1, agentMessages: 0, toolCalls: 1 });
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
		for (const tier of ["read-only", "default", "full", "full+"] as const) {
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
