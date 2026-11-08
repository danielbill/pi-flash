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
	const delegate = (input: Record<string, unknown>, on_update?: (partial: never) => void) =>
		runDelegation(
			{ manager, providerName: "scripted", inheritsParentContext: false, ...toolOptions },
			input as never,
			context,
			on_update,
		);
	return { manager, provider, delegate };
};

describe("delegation tool", () => {
	it("returns the child's answer only when a caller opts into blocking", async () => {
		const { provider, delegate } = setup();
		const pending = delegate({ prompt: "count files", blocking: true });
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

	it("returns a child id immediately by default (dispatch never waits)", async () => {
		const { delegate } = setup();
		const result = await delegate({ prompt: "long job" });
		// dsh parity (044-5 §4.2): the dispatch result is a bare sentence; the async
		// contract lives in the tool description and in this result text — the model
		// used to answer it with `bash sleep 30` polling, so the ban is explicit.
		assert.match(result.content[0].text, /^started subagent sub-1/);
		assert.match(result.content[0].text, /Do NOT poll/);
		assert.deepEqual(result.structuredContent, { childId: "sub-1", status: "started" });
	});

	// B4（044）回归：子会话文件在 prepare 就存在，派发/运行中的 details 必须
	// 带上 session_file——卡片运行中点行才能打开只读观察窗（原先只有
	// completion 才给路径，in-flight 行点击静默丢弃）。
	it("carries the child session file in the dispatch details", async () => {
		const { delegate } = setup({ sessionFile: "/sessions/sub-1.jsonl" });
		const result = await delegate({ prompt: "long job" });
		const details = result.details as Record<string, unknown>;
		assert.equal(details["session_file"], "/sessions/sub-1.jsonl");
	});

	it("carries the child session file in the running update", async () => {
		const { provider, delegate } = setup({ sessionFile: "/sessions/sub-1.jsonl" });
		const updates: Array<Record<string, unknown> | undefined> = [];
		const pending = delegate({ prompt: "count files", blocking: true }, (partial: never) =>
			updates.push(partial as Record<string, unknown>),
		);
		await new Promise((resolve) => setImmediate(resolve));
		assert.equal(updates.length, 1);
		const details = updates[0]?.details as Record<string, unknown>;
		assert.equal(details["status"], "running");
		assert.equal(details["session_file"], "/sessions/sub-1.jsonl");
		assert.equal(details["child_id"], "sub-1");
		provider.children[0].complete("done");
		await pending;
	});

	it("maps failures onto stable codes", async () => {
		const { delegate } = setup({ failPrepare: true });
		const result = await delegate({ prompt: "will fail" });
		assert.equal(result.isError, true);
		assert.match(result.content[0].text, /START_REJECTED/);
		// 卡片数据源读 snake_case（Rust tool_card）
		assert.deepEqual(result.details, { error_code: "START_REJECTED" });
	});

	// B3②回归（pi-flash-g9pn）：失败先发 status=failed 的 onUpdate 分片，
	// compact 卡即时变 [失败]+分码，不等同帧其余工具收尾。
	it("emits a failed onUpdate before returning a failure", async () => {
		const { delegate } = setup({ failPrepare: true });
		const updates: Array<Record<string, unknown> | undefined> = [];
		const pending = delegate({ prompt: "will fail", label: "scan" }, (partial: never) =>
			updates.push(partial as Record<string, unknown>),
		);
		const result = await pending;
		assert.equal(result.isError, true);
		assert.equal(updates.length, 1);
		const details = updates[0]?.details as Record<string, unknown>;
		assert.equal(details["status"], "failed");
		assert.equal(details["error_code"], "START_REJECTED");
		assert.equal(details["label"], "scan");
	});

	it("reports CAPACITY_REACHED with the failed update too", async () => {
		const { manager, delegate } = setup();
		manager.capacity = 1;
		const updates: Array<Record<string, unknown> | undefined> = [];
		// 占住唯一槽位（默认派发即返回，槽位随子代理才归还）
		void delegate({ prompt: "slot 1" });
		await new Promise((resolve) => setImmediate(resolve));
		const result = await delegate({ prompt: "slot 2" }, (partial: never) =>
			updates.push(partial as Record<string, unknown>),
		);
		assert.equal(result.isError, true);
		const details = updates[0]?.details as Record<string, unknown>;
		assert.equal(details["error_code"], "CAPACITY_REACHED");
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
		const pending = delegate({ prompt: "x", blocking: true });
		await new Promise((resolve) => setImmediate(resolve));
		provider.children[0].complete("partial", "error");
		const result = await pending;
		assert.equal(result.isError, true);
		assert.equal(result.content[0].text, "partial");
	});

	// 044-0 验收⑥回归：parentToolCallId 必须贯穿 runDelegation → manager →
	// descriptor——宿主导航轮卡第三行按它与父会话 agent 工具调用 id 对号
	// 入座；曾经 DelegationContext 收了但 manager.start 没传，所有 descriptor
	// 都丢字段，导航第三行+悬浮菜单整体哑火（真机 2026-11-07）。
	it("threads parentToolCallId into the descriptor", async () => {
		const { manager, provider } = setup();
		const pending = runDelegation(
			{ manager, providerName: "scripted", inheritsParentContext: false },
			{ prompt: "x" } as never,
			{ parentId: "parent-a", depth: 0, cwd: "/repo", parentToolCallId: "call_00_abc" } as never,
		);
		await new Promise((resolve) => setImmediate(resolve));
		provider.children[0].complete("done");
		await pending;
		const desc = manager.descriptors().find((d) => d.childId === "sub-1");
		assert.equal(desc?.parentToolCallId, "call_00_abc");
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
		// 也不能再用「wait for the results」劝模型等——它把 wait 读成「去 sleep」。
		assert.ok(!/wait for the results/.test(text));
	});

	it("bans polling right after dispatch", () => {
		const text = delegationDescription({ inheritsParentContext: false });
		// 真机回归（2026-11-08）：模型派发后去 `bash sleep 30` 轮询；
		// 描述里必须点名 sleep / subagent_list 轮询是非法的。
		assert.match(text, /do not wait for the child/);
		assert.match(text, /no `sleep`/);
	});

	// 044-5 §4.1：异步契约写在描述里（dsh 位置），结果不再下命令
	it("carries the async contract in the description, not as an order", () => {
		const text = delegationDescription({ inheritsParentContext: false });
		assert.match(text, /immediately returns its id/);
		assert.match(text, /notifies you when it finishes/);
		assert.match(text, /use `subagent_send` to steer it while running/);
		assert.doesNotMatch(text, /end your turn/);
		assert.doesNotMatch(text, /Do not poll/);
	});

	// B3①（pi-flash-g9pn）：容量上限与「被拒等槽位重派」引导进描述
	it("injects the concurrency cap and the retry guidance", () => {
		const text = delegationDescription({ inheritsParentContext: false, capacity: 3 });
		assert.match(text, /At most 3 subagents/);
		assert.match(text, /CAPACITY_REACHED/);
		assert.match(text, /re-issue it once an earlier delegation settles/);
		// 未给容量时不注入（别的宿主可能不限并发）
		const bare = delegationDescription({ inheritsParentContext: false });
		assert.doesNotMatch(bare, /CAPACITY_REACHED/);
	});
});
