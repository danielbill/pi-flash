import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { SubagentManager } from "../src/manager.ts";
import { delegationDescription, runDelegation } from "../src/tool.ts";
import { ScriptedProvider } from "./helpers.ts";

const setup = (options: Parameters<typeof ScriptedProvider.prototype.constructor>[1] = {}, toolOptions = {}) => {
	const manager = new SubagentManager();
	const provider = new ScriptedProvider("scripted", options);
	manager.registry.register(provider);
	const context = { parentId: "parent-a", depth: 0, cwd: "/repo" };
	const delegate = (input: Record<string, unknown>) =>
		runDelegation({ manager, providerName: "scripted", inheritsParentContext: false, ...toolOptions }, input as never, context);
	return { manager, provider, delegate };
};

describe("delegation tool", () => {
	it("returns the child's answer for caller delivery", async () => {
		const { provider, delegate } = setup();
		const pending = delegate({ prompt: "count files" });
		// The child completes after the call started.
		await new Promise((resolve) => setImmediate(resolve));
		provider.children[0].complete("42 files");
		const result = await pending;
		assert.equal(result.content[0].text, "42 files");
		assert.equal(result.isError, false);
		assert.deepEqual(result.structuredContent, {
			childId: "sub-1",
			text: "42 files",
			stopReason: "completed",
			usage: undefined,
			durationMs: undefined,
		});
	});

	it("returns a child id immediately for background delivery", async () => {
		const { delegate } = setup();
		const result = await delegate({ prompt: "long job", background: true });
		assert.match(result.content[0].text, /started subagent sub-1/);
		assert.deepEqual(result.structuredContent, { childId: "sub-1", status: "started" });
	});

	it("maps failures onto stable codes", async () => {
		const { delegate } = setup({ failPrepare: true });
		const result = await delegate({ prompt: "will fail" });
		assert.equal(result.isError, true);
		assert.match(result.content[0].text, /START_REJECTED/);
		assert.deepEqual(result.details, { errorCode: "START_REJECTED" });
	});

	it("rejects a profile outside the allowlist before delegating", async () => {
		const { provider, delegate } = setup({}, { allowedProfiles: ["explorer"] });
		const result = await delegate({ prompt: "x", profile: "other" });
		assert.equal(result.isError, true);
		assert.match(result.content[0].text, /PROFILE_INVALID/);
		assert.equal(provider.prepareCalls, 0);
	});

	it("rejects depth beyond the limit", async () => {
		const { manager, provider } = setup();
		const result = await runDelegation(
			{ manager, providerName: "scripted", inheritsParentContext: false },
			{ prompt: "deep" },
			{ parentId: "parent-a", depth: 1, cwd: "/repo" },
		);
		assert.equal(result.isError, true);
		assert.match(result.content[0].text, /DEPTH_LIMIT/);
		assert.equal(provider.prepareCalls, 0);
	});

	it("marks an errored result as an error without hiding the text", async () => {
		const { provider, delegate } = setup();
		const pending = delegate({ prompt: "x" });
		await new Promise((resolve) => setImmediate(resolve));
		provider.children[0].complete("partial", "error");
		const result = await pending;
		assert.equal(result.isError, true);
		assert.equal(result.content[0].text, "partial");
	});
});

describe("delegation description", () => {
	it("says the child cannot see the conversation for fresh children", () => {
		const text = delegationDescription({ inheritsParentContext: false });
		assert.match(text, /does not see this conversation/);
	});

	it("says the child cannot see the in-flight turn for fork children", () => {
		const text = delegationDescription({ inheritsParentContext: true });
		assert.match(text, /does not see the current in-flight turn/);
	});

	it("lists the allowed profiles", () => {
		const text = delegationDescription({ inheritsParentContext: false, allowedProfiles: ["explorer", "writer"] });
		assert.match(text, /explorer, writer/);
	});

	it("lists profile summaries with descriptions when given", () => {
		const text = delegationDescription({
			inheritsParentContext: false,
			profileSummaries: [
				{ id: "explorer", description: "只读探索 · 调研与定位", tools: "read-only" },
				{ id: "general-purpose", description: "通用子代理 · 复杂调研、代码检索、多步任务" },
			],
		});
		assert.match(text, /- explorer: 只读探索 · 调研与定位 \(tools: read-only\)/);
		assert.match(text, /- general-purpose: 通用子代理.*\(tools: inherit\)/);
		assert.doesNotMatch(text, /explorer, writer/);
	});

	it("degrades a summary without a description to a bare id line", () => {
		const text = delegationDescription({
			inheritsParentContext: false,
			profileSummaries: [{ id: "broken", description: "" }],
		});
		assert.match(text, /- broken/);
	});

	it("carries the when-to-delegate policy", () => {
		const text = delegationDescription({ inheritsParentContext: false });
		// 委派政策是模型主动委派的实际驱动（第五批实测：只有机械说明时
		// 即使系统提示词有鼓励 bullet 也不派）——丢了就是回归
		assert.match(text, /When to delegate/);
		assert.match(text, /do not repeat the delegated searches/);
	});
});
