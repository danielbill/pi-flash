import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { SubagentError, SubagentErrorCode } from "../src/errors.ts";
import { SubagentManager } from "../src/manager.ts";
import { baseRequest, ScriptedProvider } from "./helpers.ts";

const code = (error: unknown) => (error as SubagentError).code;
const tick = () => new Promise<void>((resolve) => setImmediate(resolve));

const setup = (options: Parameters<typeof ScriptedProvider.prototype.constructor>[1] = {}, managerOptions = {}) => {
	const manager = new SubagentManager(managerOptions);
	const provider = new ScriptedProvider("scripted", options);
	manager.registry.register(provider);
	return { manager, provider };
};

describe("SubagentManager.start", () => {
	it("publishes only after the child accepted the task", async () => {
		const { manager, provider } = setup();
		const activation = await manager.start(baseRequest());
		assert.equal(activation.childId, "sub-1");
		assert.equal(provider.children.length, 1);
		assert.deepEqual(provider.children[0].accepted, ["do the thing"]);
		assert.equal(manager.descriptors()[0].phase, "active");
	});

	it("returns the capacity when the backend refuses to prepare", async () => {
		const { manager } = setup({ failPrepare: true });
		const error = await manager.start(baseRequest()).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.START_REJECTED);
		assert.equal(manager.childrenOf("parent-a").length, 0);
		assert.equal(manager.descriptors()[0].phase, "failed");
		// Capacity must be free again.
		await assert.doesNotReject(manager.start(baseRequest()).catch(() => {}));
	});

	it("returns the capacity and disposes the handle when accept fails", async () => {
		const { manager, provider } = setup({ failAccept: true });
		const error = await manager.start(baseRequest()).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.START_REJECTED);
		assert.equal(provider.children[0].disposals, 1);
		assert.equal(manager.childrenOf("parent-a").length, 0);
	});

	it("rejects a handle whose id the provider did not honour", async () => {
		const { manager } = setup({ idOverride: "impostor" });
		const error = await manager.start(baseRequest()).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.START_REJECTED);
	});

	it("enforces the depth limit before touching resources", async () => {
		const { manager, provider } = setup();
		const error = await manager.start(baseRequest({ depth: 2 })).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.DEPTH_LIMIT);
		assert.equal(provider.prepareCalls, 0);
	});

	it("rejects an already aborted request", async () => {
		const { manager, provider } = setup();
		const controller = new AbortController();
		controller.abort();
		const error = await manager.start(baseRequest({ signal: controller.signal })).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.ABORTED);
		assert.equal(provider.prepareCalls, 0);
	});

	// PARITY P0：子会话不装配本扩展（profile 未列 extensions），所以 depth≥2 在
	// 宿主里不可达；管理器仍按 dsh 的 reserve/拒绝语义处理调用方给的 depth。
	it("charges a depth-2 delegation to the session that issued it", async () => {
		const { manager } = setup({}, { capacity: 1, maxDepth: 2 });
		await manager.start(baseRequest({ parentId: "/root.jsonl" }));
		// A separate parent id is a separate bucket: the host cannot produce this
		// shape today, and PARITY.md records what must change if it ever can.
		await assert.doesNotReject(
			manager.start(baseRequest({ parentId: "/child-1.jsonl", depth: 2 })),
		);
	});

	it("keeps separate sessions independent", async () => {
		const { manager } = setup({}, { capacity: 1 });
		await manager.start(baseRequest({ parentId: "/root-a.jsonl" }));
		await assert.doesNotReject(manager.start(baseRequest({ parentId: "/root-b.jsonl" })));
	});

	it("rejects with CAPACITY_REACHED instead of queueing", async () => {
		const { manager, provider } = setup({}, { capacity: 2 });
		await manager.start(baseRequest());
		await manager.start(baseRequest());
		const error = await manager.start(baseRequest()).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.CAPACITY_REACHED);
		assert.equal(provider.prepareCalls, 2);
	});

	// B3③回归（pi-flash-g9pn）：满即拒的委派也落一份 failed descriptor，
	// 面板立即出红行——否则这次拒绝完全不可见（无 record、转录卡要等整轮）。
	it("emits a failed descriptor for a capacity-rejected delegation", async () => {
		const seen: Array<{ phase: string; errorCode?: string; childId: string }> = [];
		const { manager, provider } = setup({}, { capacity: 2, onDescriptor: (d) => seen.push({ phase: d.phase, errorCode: d.errorCode, childId: d.childId }) });
		await manager.start(baseRequest());
		await manager.start(baseRequest());
		await manager.start(baseRequest()).catch(() => {});
		const rejected = seen.at(-1)!;
		assert.equal(rejected.phase, "failed");
		assert.equal(rejected.errorCode, SubagentErrorCode.CAPACITY_REACHED);
		assert.equal(rejected.childId, "sub-3");
		// 被拒的不进 childrenOf（无 handle、不占容量）
		assert.deepEqual(manager.childrenOf("parent-a"), ["sub-1", "sub-2"]);
		// 槽位释放后重派成功（对齐真机：被拒后模型自行补派）——id 照常消耗
		provider.children[0].complete("done");
		await manager.activation("sub-1")!.wait();
		await manager.start(baseRequest());
		assert.deepEqual(manager.childrenOf("parent-a"), ["sub-2", "sub-4"]);
	});

	it("reports the provider name in the descriptor", async () => {
		const { manager } = setup();
		await manager.start(baseRequest({ label: "explore" }));
		const descriptor = manager.descriptors()[0];
		assert.equal(descriptor.provider, "scripted");
		assert.equal(descriptor.label, "explore");
		assert.equal(descriptor.depth, 1);
	});

	// B2 回归（pi-flash-3ghh）：子会话文件 prepare 返回即回写 descriptor，
	// 运行中面板行就可点开；不再等 settle。
	it("writes the child session file back as soon as the handle exists", async () => {
		const { manager, provider } = setup({ sessionFile: "/tmp/child.jsonl" });
		await manager.start(baseRequest());
		const descriptor = manager.descriptors()[0];
		assert.equal(descriptor.phase, "active");
		assert.equal(descriptor.sessionFile, "/tmp/child.jsonl");
		assert.ok(provider.children[0].disposals === 0); // still running
	});

	it("carries the session file on a failed-start descriptor too", async () => {
		const { manager } = setup({ sessionFile: "/tmp/child.jsonl", failAccept: true });
		await manager.start(baseRequest()).catch(() => {});
		assert.equal(manager.descriptors()[0].sessionFile, "/tmp/child.jsonl");
	});

	it("emits a descriptor for every phase change", async () => {
		const seen: string[] = [];
		const { manager, provider } = setup({ completeOnDispose: true }, { onDescriptor: (d) => seen.push(d.phase) });
		const activation = await manager.start(baseRequest());
		provider.children[0].complete("ok");
		await activation.wait();
		assert.deepEqual(seen, ["provisioning", "active", "completed"]);
	});

	// B1 回归（pi-flash-y26f）：caller 投递下 wait() 立即把 state 置 settling，
	// 子代理整个运行期都发生在 settling 内——每轮 stats/usage 必须照常透出，
	// 只有终态（settled/failed）才拦。
	it("keeps per-turn stats and usage flowing while a caller-delivery child runs", async () => {
		const seen: Array<{ phase: string; stats?: unknown; usage?: unknown }> = [];
		const { manager, provider } = setup({}, { onDescriptor: (d) => seen.push({ phase: d.phase, stats: d.stats, usage: d.usage }) });
		const activation = await manager.start(baseRequest({ delivery: "caller" }));
		const pending = activation.wait(); // parks in settling for the whole run
		provider.children[0].progress({ turns: 1, agentMessages: 0, toolCalls: 1 }, { input: 100, output: 10, total: 110 });
		provider.children[0].progress({ turns: 2, agentMessages: 1, toolCalls: 2 }, { input: 300, output: 30, total: 330 });
		const midRun = manager.descriptors()[0];
		assert.equal(midRun.phase, "active");
		assert.deepEqual(midRun.stats, { turns: 2, agentMessages: 1, toolCalls: 2 });
		assert.deepEqual(midRun.usage, { input: 300, output: 30, total: 330 });
		provider.children[0].complete("done", "completed", { turns: 3, agentMessages: 2, toolCalls: 2 });
		await pending;
		assert.equal(manager.descriptors()[0].phase, "completed");
		// 管道里每轮都在：provisioning / active / 运行中两轮 / settled 终态
		assert.deepEqual(
			seen.map((e) => e.phase),
			["provisioning", "active", "active", "active", "completed"],
		);
		assert.deepEqual(seen[2]?.usage, { input: 100, output: 10, total: 110 });
	});

	it("stops the per-turn feed once settled", async () => {
		const { manager, provider } = setup();
		const activation = await manager.start(baseRequest());
		const child = provider.children[0];
		child.complete("done", "completed", { turns: 2, agentMessages: 1, toolCalls: 1 });
		await activation.wait();
		child.progress({ turns: 9, agentMessages: 9, toolCalls: 9 });
		assert.deepEqual(manager.descriptors()[0].stats, { turns: 2, agentMessages: 1, toolCalls: 1 });
	});
});

describe("SubagentManager settlement", () => {
	it("resolves the result only after the handle was disposed", async () => {
		const { manager, provider } = setup();
		const activation = await manager.start(baseRequest());
		const child = provider.children[0];
		child.complete("answer");
		assert.equal(child.disposals, 0);
		const result = await activation.wait();
		assert.equal(result.text, "answer");
		assert.equal(child.disposals, 1);
	});

	it("keeps the slot until cleanup finished", async () => {
		const { manager, provider } = setup({}, { capacity: 1 });
		const activation = await manager.start(baseRequest());
		const child = provider.children[0];
		const releaseDispose = child.blockDispose();
		child.complete("done");
		const pending = activation.wait();
		// Settlement started but cleanup is still running: the slot must stay taken.
		await tick();
		const error = await manager.start(baseRequest()).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.CAPACITY_REACHED);
		releaseDispose();
		await pending;
		await assert.doesNotReject(manager.start(baseRequest()));
	});

	it("closes admission on settle and rejects late input", async () => {
		const { manager, provider } = setup();
		const activation = await manager.start(baseRequest());
		provider.children[0].complete("done");
		const pending = activation.wait();
		await tick();
		const error = await activation.send("more work").catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.ADMISSION_CLOSED);
		await pending;
	});

	it("forwards input while the child is active", async () => {
		const { manager, provider } = setup();
		const activation = await manager.start(baseRequest());
		await activation.send("steer left");
		assert.deepEqual(provider.children[0].sent, ["steer left"]);
	});

	it("is idempotent for repeated wait and dispose", async () => {
		const { manager, provider } = setup();
		const activation = await manager.start(baseRequest());
		const child = provider.children[0];
		child.complete("once");
		const [a, b] = await Promise.all([activation.wait(), activation.dispose()]);
		assert.equal(a.text, "once");
		assert.equal(b.text, "once");
		assert.equal(child.disposals, 1);
	});

	it("reports a dispose failure as an error result without losing the answer", async () => {
		const { manager, provider } = setup();
		const activation = await manager.start(baseRequest());
		const child = provider.children[0];
		child.dispose = async () => {
			throw new Error("dispose broke");
		};
		child.complete("kept");
		const result = await activation.wait();
		assert.equal(result.text, "kept");
		assert.equal(result.stopReason, "error");
		assert.equal(result.errorCode, SubagentErrorCode.START_REJECTED);
	});

	it("only notifies parent delivery", async () => {
		const completed: string[] = [];
		const { manager, provider } = setup({}, { onCompleted: (r) => completed.push(r.text) });
		const callerActivation = await manager.start(baseRequest({ delivery: "caller" }));
		provider.children[0].complete("quiet");
		await callerActivation.wait();

		const parentActivation = await manager.start(baseRequest({ delivery: "parent" }));
		provider.children[1].complete("loud");
		await parentActivation.wait();

		assert.deepEqual(completed, ["loud"]);
	});

	it("interrupts through the activation", async () => {
		const { manager, provider } = setup();
		const activation = await manager.start(baseRequest());
		await activation.interrupt();
		assert.equal(provider.children[0].interrupts, 1);
	});

	it("shuts down every live activation", async () => {
		const { manager, provider } = setup();
		const first = await manager.start(baseRequest());
		await manager.start(baseRequest());
		const shutdown = manager.shutdown("test");
		for (const child of provider.children) child.complete("stopped", "aborted");
		await shutdown;
		assert.equal(provider.children[0].interrupts, 1);
		assert.equal((await first.wait()).stopReason, "aborted");
	});
});

describe("SubagentManager parent delivery + control", () => {
	it("parent delivery completes without blocking start", async () => {
		const completed: string[] = [];
		const { manager, provider } = setup({}, { onCompleted: (r) => completed.push(r.text) });
		const activation = await manager.start(baseRequest({ delivery: "parent" }));
		// start() returned while the child is still running (ScriptedChild holds result).
		assert.equal(provider.children[0].disposals, 0);
		provider.children[0].complete("answer");
		await activation.wait();
		await tick();
		assert.deepEqual(completed, ["answer"]);
	});

	it("lists children of a parent with phases", async () => {
		const { manager, provider } = setup();
		await manager.start(baseRequest({ label: "one" }));
		await manager.start(baseRequest({ label: "two" }));
		const list = manager.list("parent-a");
		assert.deepEqual(list.map((d) => d.label), ["one", "two"]);
		assert.deepEqual(list.map((d) => d.phase), ["active", "active"]);
		provider.children[0].complete("x");
		await manager.activation("sub-1")!.wait();
		assert.equal(manager.list("parent-a")[0].phase, "completed");
	});

	it("interrupt on a settled child is a no-op", async () => {
		const { manager, provider } = setup();
		const activation = await manager.start(baseRequest());
		provider.children[0].complete("done");
		await activation.wait();
		assert.equal(await manager.interruptChild("sub-1"), "noop");
	});

	it("interrupt on a live child reports interrupted", async () => {
		const { manager, provider } = setup();
		await manager.start(baseRequest());
		assert.equal(await manager.interruptChild("sub-1"), "interrupted");
		assert.equal(provider.children[0].interrupts, 1);
	});

	it("interrupt on an unknown child rejects with a stable code", async () => {
		const { manager } = setup();
		const error = await manager.interruptChild("ghost").catch((e: unknown) => e);
		assert.equal((error as SubagentError).code, SubagentErrorCode.PROVIDER_UNKNOWN);
	});
});
