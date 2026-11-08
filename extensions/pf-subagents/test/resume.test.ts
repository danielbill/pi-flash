import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, describe, it } from "node:test";

import { LocalProvider } from "../src/backends/local.ts";
import { composeCompletion } from "../src/completion.ts";
import { SubagentErrorCode } from "../src/errors.ts";
import { SubagentManager } from "../src/manager.ts";
import { delegationDescription, runDelegation } from "../src/tool.ts";
import { baseRequest, ScriptedProvider } from "./helpers.ts";

/** 临时目录里的真会话文件：`assertResumable` 要求文件真实存在。 */
const dir = mkdtempSync(join(tmpdir(), "pf-resume-"));
const sessionFile = (name: string): string => {
	const path = join(dir, `${name}.jsonl`);
	writeFileSync(path, '{"type":"session","id":"s"}\n', "utf8");
	return path;
};
after(() => {
	// 临时目录随进程回收；不 rm 以免杀毒/句柄锁让测试假失败。
});

const setup = (options: ConstructorParameters<typeof ScriptedProvider>[1] = {}) => {
	const manager = new SubagentManager();
	const provider = new ScriptedProvider("scripted", options);
	manager.registry.register(provider);
	const context = { parentId: "parent-a", depth: 0, cwd: "/repo" };
	const delegate = (input: Record<string, unknown>, ctx: Partial<typeof context> & { parentSessionFile?: string } = {}) =>
		runDelegation({ manager, providerName: "scripted", inheritsParentContext: false }, input as never, {
			...context,
			...ctx,
		});
	return { manager, provider, delegate };
};

const errorCode = (result: { structuredContent?: unknown }): unknown => {
	const payload = result.structuredContent as Record<string, unknown> | undefined;
	return payload?.["errorCode"] ?? payload?.["code"];
};

describe("cold resume (044-5 §4.4)", () => {
	it("advertises resumeFrom in the description", () => {
		const description = delegationDescription({ inheritsParentContext: false });
		assert.match(description, /resumeFrom/);
	});

	it("hands the session file to the provider instead of starting fresh", async () => {
		const { provider, delegate } = setup();
		const target = sessionFile("child-run-1");
		const result = await delegate({ prompt: "keep digging", resumeFrom: target });
		assert.equal(result.content[0].text, "started subagent sub-1");
		assert.equal(provider.requests[0]?.resumeFrom, target);
	});

	it("rejects a missing file before spending a capacity slot", async () => {
		const { provider, delegate } = setup();
		const result = await delegate({ prompt: "keep digging", resumeFrom: join(dir, "nope.jsonl") });
		assert.equal(result.isError, true);
		assert.equal(errorCode(result), SubagentErrorCode.NOT_RESUMABLE);
		assert.equal(provider.prepareCalls, 0);
	});

	it("rejects a path that is not a session file", async () => {
		const { provider, delegate } = setup();
		const bogus = join(dir, "notes.txt");
		writeFileSync(bogus, "not a session", "utf8");
		const result = await delegate({ prompt: "keep digging", resumeFrom: bogus });
		assert.equal(errorCode(result), SubagentErrorCode.NOT_RESUMABLE);
		assert.equal(provider.prepareCalls, 0);
	});

	it("rejects the parent's own session (a child must not write the parent transcript)", async () => {
		const { provider, delegate } = setup();
		const parentFile = sessionFile("parent");
		const result = await delegate(
			{ prompt: "keep digging", resumeFrom: parentFile },
			{ parentSessionFile: parentFile },
		);
		assert.equal(errorCode(result), SubagentErrorCode.NOT_RESUMABLE);
		assert.equal(provider.prepareCalls, 0);
	});

	it("rejects a child session that is still running", async () => {
		const live = sessionFile("live-child");
		const { delegate } = setup({ sessionFile: live });
		await delegate({ prompt: "long job" });
		const result = await delegate({ prompt: "keep digging", resumeFrom: live });
		assert.equal(errorCode(result), SubagentErrorCode.NOT_RESUMABLE);
	});

	it("passes resumeSessionFile to the session factory (local provider)", async () => {
		const target = sessionFile("factory-resume");
		const seen: Array<Record<string, unknown>> = [];
		const provider = new LocalProvider({
			name: "spawn",
			inheritsParentContext: false,
			factory: async (options) => {
				seen.push(options as unknown as Record<string, unknown>);
				return {
					prompt: async () => {},
					lastAssistantText: () => "",
					abort: async () => {},
					dispose: () => {},
					sessionFile: target,
				};
			},
		});
		const handle = await provider.prepare({ ...baseRequest(), childId: "sub-9", resumeFrom: target });
		assert.equal(seen[0]?.["resumeSessionFile"], target);
		await handle.dispose();
	});

	it("points a late subagent_send at the resume entry", async () => {
		const file = sessionFile("settled");
		const { manager, provider } = setup({ sessionFile: file });
		const activation = await manager.start(baseRequest());
		provider.children[0].complete("done");
		await activation.wait();
		const error = (await activation.send("more work").catch((e: unknown) => e)) as {
			code?: string;
			message?: string;
		};
		assert.equal(error.code, SubagentErrorCode.ADMISSION_CLOSED);
		assert.match(String(error.message), /resumeFrom/);
		assert.ok(String(error.message).includes(file));
	});

	it("tells the parent how to resume a child that ran out of turns", () => {
		const target = sessionFile("maxed-out");
		const completion = composeCompletion({
			childId: "sub-3",
			text: "partial report",
			stopReason: "max_turns",
			session_file: target,
		} as never);
		assert.match(completion.content, /resumeFrom/);
		assert.ok(completion.content.includes(target));
	});
});
