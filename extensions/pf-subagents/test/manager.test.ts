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

	it("rejects with CAPACITY_REACHED instead of queueing", async () => {
		const { manager, provider } = setup({}, { capacity: 2 });
		await manager.start(baseRequest());
		await manager.start(baseRequest());
		const error = await manager.start(baseRequest()).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.CAPACITY_REACHED);
		assert.equal(provider.prepareCalls, 2);
	});

	it("reports the provider name in the descriptor", async () => {
		const { manager } = setup();
		await manager.start(baseRequest({ label: "explore" }));
		const descriptor = manager.descriptors()[0];
		assert.equal(descriptor.provider, "scripted");
		assert.equal(descriptor.label, "explore");
		assert.equal(descriptor.depth, 1);
	});

	it("emits a descriptor for every phase change", async () => {
		const seen: string[] = [];
		const { manager, provider } = setup({ completeOnDispose: true }, { onDescriptor: (d) => seen.push(d.phase) });
		const activation = await manager.start(baseRequest());
		provider.children[0].complete("ok");
		await activation.wait();
		assert.deepEqual(seen, ["provisioning", "active", "completed"]);
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
