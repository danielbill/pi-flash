import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";
import { homedir } from "node:os";

import { SubagentError, SubagentErrorCode } from "../src/errors.ts";
import { BUILTIN_IDS, defaultProfilePaths, listProfiles, listProfilesSync, loadProfile, parseFrontmatter, profileFromDocument, resolveProfileCwd } from "../src/profile.ts";
import { captureSync } from "./helpers.ts";

const code = (error: unknown) => (error as SubagentError).code;

describe("cwd token", () => {
	it("resolves pf-data to the PF data directory and absent to inherit", () => {
		assert.equal(resolveProfileCwd(undefined, "/pf/data"), undefined, "键省略 = 继承委派者 cwd");
		assert.equal(resolveProfileCwd("pf-data", "/pf/data"), "/pf/data");
		assert.equal(
			resolveProfileCwd("pf-data", undefined),
			undefined,
			"宿主没给 PI_FLASH_DIR 时退回继承，不猜项目目录",
		);
	});
});

describe("frontmatter", () => {
	it("reads scalars, lists, and the body", () => {
		const doc = [
			"---",
			"description: Explores the repo",
			"model: deepseek/flash",
			"tools:",
			"  - read",
			"  - grep",
			"---",
			"",
			"Work read-only. Report findings.",
		].join("\n");
		const { data, body } = parseFrontmatter(doc);
		assert.equal(data.description, "Explores the repo");
		assert.deepEqual(data.tools, ["read", "grep"]);
		assert.equal(body, "Work read-only. Report findings.");
	});

	it("treats a document without frontmatter as a pure prompt", () => {
		const { data, body } = parseFrontmatter("Just instructions.");
		assert.deepEqual(data, {});
		assert.equal(body, "Just instructions.");
	});

	it("fails on unterminated frontmatter", () => {
		const error = captureSync(() => parseFrontmatter("---\nmodel: x\n"));
		assert.match((error as Error).message, /unterminated frontmatter/);
	});
});

describe("profileFromDocument", () => {
	const doc = ["---", "name: 本地探索者", "description: lab", "tools: read-only", "isolation: worktree", "---", "body"].join("\n");

	it("builds a profile", () => {
		const profile = profileFromDocument("explorer", doc, "mem");
		assert.equal(profile.id, "explorer");
		assert.equal(profile.name, "本地探索者", "name = 点名句柄 + 显示名");
		assert.equal(profile.tools, "read-only");
		assert.equal(profile.isolation, "worktree");
		assert.equal(profile.appendSystemPrompt, "body");
		const unnamed = profileFromDocument("legacy-id", "---\ndescription: x\n---\n", "mem");
		assert.equal(unnamed.name, undefined, "无 name 键（迁移残留）= undefined，展示回落 id");
	});

	it("rejects a tools value outside the tier enum", () => {
		const bad = ["---", "tools: read", "---", "x"].join("\n");
		const error = captureSync(() => profileFromDocument("p", bad, "mem"));
		assert.match((error as Error).message, /tools must be one of/);
	});

	it("rejects a tools list — tiers replaced the allowlist", () => {
		const bad = ["---", "tools:", "  - read", "  - grep", "---", "x"].join("\n");
		const error = captureSync(() => profileFromDocument("p", bad, "mem"));
		assert.match((error as Error).message, /tools must be one of/);
	});

	it("rejects an unsupported isolation", () => {
		const bad = ["---", "isolation: docker", "---", "x"].join("\n");
		const error = captureSync(() => profileFromDocument("p", bad, "mem"));
		assert.match((error as Error).message, /unsupported isolation/);
	});

	it("parses the thinking level; absent = auto (2026-11-07 子代理表单)", () => {
		const doc = ["---", "description: lab", "thinking: high", "---", "body"].join("\n");
		const profile = profileFromDocument("p", doc, "mem");
		assert.equal(profile.thinking, "high");
		const auto = profileFromDocument("p", "---\ndescription: x\n---\n", "mem");
		assert.equal(auto.thinking, undefined, "no thinking key = auto = don't override");
	});

	it("parses cwd pf-data; absent = inherit (2026-11-08 子代理表单)", () => {
		const doc = ["---", "description: lab", "cwd: pf-data", "---", "body"].join("\n");
		const profile = profileFromDocument("p", doc, "mem");
		assert.equal(profile.cwd, "pf-data");
		const inherit = profileFromDocument("p", "---\ndescription: x\n---\n", "mem");
		assert.equal(inherit.cwd, undefined);
	});

	it("rejects a cwd value other than pf-data", () => {
		const bad = ["---", "cwd: /tmp", "---", "x"].join("\n");
		const error = captureSync(() => profileFromDocument("p", bad, "mem"));
		assert.match((error as Error).message, /cwd must be/);
	});

	it("rejects a thinking value outside pi's seven levels", () => {
		const bad = ["---", "thinking: ultra", "---", "x"].join("\n");
		const error = captureSync(() => profileFromDocument("p", bad, "mem"));
		assert.match((error as Error).message, /thinking must be one of/);
	});

	it("parses every v1 field and ignores unknown ones (M4.6)", () => {
		const doc = [
			"---",
			"description: reviewer",
			"model: deepseek/deepseek-chat",
			"tools: read-only",
			"maxTurns: 42",
			"injectAgentsMd: false",
			"futureFieldFromVNext: whatever",
			"---",
			"append body",
		].join("\n");
		const profile = profileFromDocument("reviewer", doc, "mem");
		assert.equal(profile.description, "reviewer");
		assert.equal(profile.model, "deepseek/deepseek-chat");
		assert.equal(profile.tools, "read-only");
		assert.equal(profile.maxTurns, 42);
		assert.equal(profile.injectAgentsMd, false);
		assert.equal(profile.appendSystemPrompt, "append body");
	});

	it("reads a systemPrompt block scalar as the replacement persona", () => {
		const doc = [
			"---",
			"description: strict reviewer",
			"systemPrompt: |",
			"  You are a strict reviewer.",
			"  Never edit files.",
			"---",
			"extra notes",
		].join("\n");
		const profile = profileFromDocument("strict", doc, "mem");
		assert.equal(profile.systemPrompt, "You are a strict reviewer.\nNever edit files.");
		assert.equal(profile.appendSystemPrompt, "extra notes");
	});

	it("accepts maxTurns: 0 as the unlimited tier and rejects negatives", () => {
		const unlimited = profileFromDocument("u", ["---", "maxTurns: 0", "---", "x"].join("\n"), "mem");
		assert.equal(unlimited.maxTurns, 0);
		const negative = captureSync(() =>
			profileFromDocument("n", ["---", "maxTurns: -1", "---", "x"].join("\n"), "mem"),
		);
		assert.match((negative as Error).message, /maxTurns must be a non-negative integer/);
	});
});

describe("profile loading", () => {
	it("prefers the project directory over the global one", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const globalDir = join(root, "global");
		const projectDir = join(root, "project");
		await mkdir(globalDir, { recursive: true });
		await mkdir(projectDir, { recursive: true });
		await writeFile(join(globalDir, "explorer.md"), "---\ndescription: global\n---\nglobal body");
		await writeFile(join(projectDir, "explorer.md"), "---\ndescription: project\n---\nproject body");

		const profile = await loadProfile("explorer", { globalDir, projectDir });
		assert.equal(profile.description, "project");
		assert.equal(profile.appendSystemPrompt, "project body");
	});

	it("falls back to the global directory", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const globalDir = join(root, "global");
		await mkdir(globalDir, { recursive: true });
		await writeFile(join(globalDir, "writer.md"), "---\ndescription: global\n---\nbody");

		const profile = await loadProfile("writer", { globalDir, projectDir: join(root, "missing") });
		assert.equal(profile.description, "global");
	});

	it("rejects unknown profiles with a stable code", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const error = await loadProfile("ghost", { globalDir: root, projectDir: root }).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.PROFILE_INVALID);
	});

	it("rejects path traversal in profile ids", async () => {
		const error = await loadProfile("../etc/passwd", { globalDir: "/tmp", projectDir: "/tmp" }).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.PROFILE_INVALID);
	});

	it("lists profiles from both directories without duplicates", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const globalDir = join(root, "global");
		const projectDir = join(root, "project");
		await mkdir(globalDir, { recursive: true });
		await mkdir(projectDir, { recursive: true });
		await writeFile(join(globalDir, "a.md"), "---\n---\n");
		await writeFile(join(globalDir, "b.md"), "---\n---\n");
		await writeFile(join(projectDir, "b.md"), "---\n---\n");
		await writeFile(join(projectDir, "c.md"), "---\n---\n");

		assert.deepEqual(await listProfiles({ globalDir, projectDir }), ["9f3d2b84", "a", "b", "c", "c58e07af", "e1a7c052"]);
	});

	it("falls back to the built-in explorer when addressed by name", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const paths = { globalDir: join(root, "missing"), projectDir: join(root, "also-missing") };
		const profile = await loadProfile("本地探索者", paths);
		assert.equal(profile.tools, "read-only");
		assert.equal(profile.maxTurns, undefined, "轮次「指定」= 不写 maxTurns（委派方给 maxloop）");
		assert.deepEqual(profile.skills, [], "skills 关闭（裸键 = 空清单）");
		assert.match(profile.appendSystemPrompt ?? "", /只读探索员/);
		assert.equal(profile.source, "<builtin>");
	});

	it("still resolves built-ins by id for legacy transcripts", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const paths = { globalDir: join(root, "missing"), projectDir: join(root, "also-missing") };
		const profile = await loadProfile(BUILTIN_IDS.explorer, paths);
		assert.equal(profile.id, BUILTIN_IDS.explorer);
		assert.equal(profile.name, "本地探索者");
		assert.equal(profile.source, "<builtin>");
	});

	it("falls back to the built-in general-purpose with inherited tools", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const paths = { globalDir: join(root, "missing"), projectDir: join(root, "also-missing") };
		const profile = await loadProfile("通用子代理", paths);
		assert.equal(profile.tools, undefined, "no tools key = inherit the delegator's assembly");
		assert.equal(profile.maxTurns, undefined, "轮次「指定」= 不写 maxTurns");
		assert.equal(profile.skills, undefined, "skills 继承 = 不写键");
		assert.match(profile.appendSystemPrompt ?? "", /通用子代理/);
		assert.equal(profile.source, "<builtin>");
	});

	it("falls back to the built-in summarizer with no project context (044-0)", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const paths = { globalDir: join(root, "missing"), projectDir: join(root, "also-missing") };
		const profile = await loadProfile("摘要机", paths);
		assert.equal(profile.tools, "read-only");
		assert.equal(profile.injectAgentsMd, false, "摘要机不注入 AGENTS.md（不继承环境）");
		assert.equal(profile.cwd, "pf-data", "不继承 cwd → PF 数据目录");
		assert.equal(profile.maxTurns, 5, "摘要机固定 5 轮");
		assert.deepEqual(profile.skills, [], "skills 关闭");
		assert.match(profile.appendSystemPrompt ?? "", /摘要机/);
		assert.equal(profile.source, "<builtin>");
	});

	it("a same-id file occupies the built-in's slot (id-level dedup, no name semantics)", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const globalDir = join(root, "global");
		await mkdir(globalDir, { recursive: true });
		await writeFile(join(globalDir, `${BUILTIN_IDS.explorer}.md`), "---\ndescription: mine\n---\nmy body");
		const profile = await loadProfile(BUILTIN_IDS.explorer, { globalDir, projectDir: join(root, "missing") });
		assert.equal(profile.description, "mine");
		assert.equal(profile.appendSystemPrompt, "my body");
		// 点名句柄仍是 name：同 id 文件没有 name 时按名称找不到，按 id 找到的是这份文件
		const byName = await loadProfile("本地探索者", { globalDir, projectDir: join(root, "missing") });
		assert.equal(byName.source, "<builtin>", "内置不受同 id 文件影响（无覆盖语义）");
	});

	it("resolves a custom profile by its Chinese name", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const globalDir = join(root, "global");
		await mkdir(globalDir, { recursive: true });
		await writeFile(join(globalDir, "a1b2c3d4.md"), "---\nname: 我的评审员\ndescription: reviews\n---\nbody");
		const profile = await loadProfile("我的评审员", { globalDir, projectDir: join(root, "missing") });
		assert.equal(profile.id, "a1b2c3d4");
		assert.equal(profile.name, "我的评审员");
	});

	it("lists built-in ids even with no directories on disk", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		assert.deepEqual(
			await listProfiles({ globalDir: join(root, "x"), projectDir: join(root, "y") }),
			[BUILTIN_IDS.generalPurpose, BUILTIN_IDS.summarizer, BUILTIN_IDS.explorer].sort(),
		);
	});
});

describe("listProfilesSync", () => {
	it("carries built-in frontmatter summaries", () => {
		const root = join(tmpdir(), "pf-subagents-sync-missing");
		const summaries = listProfilesSync({ globalDir: join(root, "x"), projectDir: join(root, "y") });
		assert.deepEqual(summaries, [
			{ id: "9f3d2b84", name: "通用子代理", description: "通用子代理 · 复杂调研、代码检索、多步任务" },
			{ id: "c58e07af", name: "摘要机", description: "无状态摘要 · 阅读文件或内容按要求总结", tools: "read-only" },
			{ id: "e1a7c052", name: "本地探索者", description: "只读探索 · 调研与定位", tools: "read-only" },
		]);
	});

	it("custom files list beside built-ins, a corrupt one degrades to a bare id", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const globalDir = join(root, "global");
		await mkdir(globalDir, { recursive: true });
		await writeFile(join(globalDir, "a1b2c3d4.md"), "---\nname: 我的评审员\ndescription: mine\n---\nmy body");
		await writeFile(join(globalDir, "broken.md"), "---\ndescription: oops\n");
		const summaries = listProfilesSync({ globalDir, projectDir: join(root, "missing") });
		assert.deepEqual(
			summaries.find((s) => s.id === "a1b2c3d4"),
			{ id: "a1b2c3d4", name: "我的评审员", description: "mine" },
		);
		assert.deepEqual(summaries.find((s) => s.id === "broken"), { id: "broken", name: "broken", description: "" });
	});
});

describe("defaultProfilePaths (PF-owned dirs)", () => {
	it("honours the injected PF profiles dir and uses .pi-flash for the project", () => {
		process.env["PI_SUBAGENTS_PROFILES_DIR"] = "/pf-data/agents";
		const p = defaultProfilePaths("/repo");
		assert.equal(p.globalDir, "/pf-data/agents");
		assert.equal(p.projectDir, join("/repo", ".pi-flash", "agents"));
		delete process.env["PI_SUBAGENTS_PROFILES_DIR"];
	});

	it("falls back to ~/.pi-flash/agents when no PF host injected the dir", () => {
		delete process.env["PI_SUBAGENTS_PROFILES_DIR"];
		const p = defaultProfilePaths("/repo");
		assert.equal(p.globalDir, join(homedir(), ".pi-flash", "agents"));
		assert.equal(p.projectDir, join("/repo", ".pi-flash", "agents"));
	});
});
