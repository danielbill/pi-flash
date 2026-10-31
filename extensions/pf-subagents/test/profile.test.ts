import assert from "node:assert/strict";
import { mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, it } from "node:test";

import { SubagentError, SubagentErrorCode } from "../src/errors.ts";
import { listProfiles, listProfilesSync, loadProfile, parseFrontmatter, profileFromDocument } from "../src/profile.ts";
import { captureSync } from "./helpers.ts";

const code = (error: unknown) => (error as SubagentError).code;

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
	const doc = ["---", "description: lab", "tools: read-only", "isolation: worktree", "---", "body"].join("\n");

	it("builds a profile", () => {
		const profile = profileFromDocument("explorer", doc, "mem");
		assert.equal(profile.id, "explorer");
		assert.equal(profile.tools, "read-only");
		assert.equal(profile.isolation, "worktree");
		assert.equal(profile.appendSystemPrompt, "body");
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

		assert.deepEqual(await listProfiles({ globalDir, projectDir }), ["a", "b", "c", "explorer", "general-purpose"]);
	});

	it("falls back to the built-in explorer when no file exists", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const paths = { globalDir: join(root, "missing"), projectDir: join(root, "also-missing") };
		const profile = await loadProfile("explorer", paths);
		assert.equal(profile.tools, "read-only");
		assert.equal(profile.maxTurns, 0, "explorer is the unlimited tier");
		assert.match(profile.appendSystemPrompt ?? "", /只读探索员/);
		assert.equal(profile.source, "<builtin>");
	});

	it("falls back to the built-in general-purpose with inherited tools", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const paths = { globalDir: join(root, "missing"), projectDir: join(root, "also-missing") };
		const profile = await loadProfile("general-purpose", paths);
		assert.equal(profile.tools, undefined, "no tools key = inherit the delegator's assembly");
		assert.equal(profile.maxTurns, 0);
		assert.match(profile.appendSystemPrompt ?? "", /通用子代理/);
		assert.equal(profile.source, "<builtin>");
	});

	it("a same-id custom file overrides the built-in", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const globalDir = join(root, "global");
		await mkdir(globalDir, { recursive: true });
		await writeFile(join(globalDir, "explorer.md"), "---\ndescription: mine\n---\nmy body");
		const profile = await loadProfile("explorer", { globalDir, projectDir: join(root, "missing") });
		assert.equal(profile.description, "mine");
		assert.equal(profile.appendSystemPrompt, "my body");
	});

	it("lists built-in ids even with no directories on disk", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		assert.deepEqual(
			await listProfiles({ globalDir: join(root, "x"), projectDir: join(root, "y") }),
			["explorer", "general-purpose"],
		);
	});
});

describe("listProfilesSync", () => {
	it("carries built-in frontmatter summaries", () => {
		const root = join(tmpdir(), "pf-subagents-sync-missing");
		const summaries = listProfilesSync({ globalDir: join(root, "x"), projectDir: join(root, "y") });
		assert.deepEqual(summaries, [
			{ id: "explorer", description: "只读探索 · 调研与定位", tools: "read-only" },
			{ id: "general-purpose", description: "通用子代理 · 复杂调研、代码检索、多步任务" },
		]);
	});

	it("a custom file shadows the built-in summary, a corrupt one degrades to a bare id", async () => {
		const root = await mkdtemp(join(tmpdir(), "pf-subagents-"));
		const globalDir = join(root, "global");
		await mkdir(globalDir, { recursive: true });
		await writeFile(join(globalDir, "explorer.md"), "---\ndescription: mine\n---\nmy body");
		await writeFile(join(globalDir, "broken.md"), "---\ndescription: oops\n");
		const summaries = listProfilesSync({ globalDir, projectDir: join(root, "missing") });
		assert.deepEqual(
			summaries.find((s) => s.id === "explorer"),
			{ id: "explorer", description: "mine" },
		);
		assert.deepEqual(summaries.find((s) => s.id === "broken"), { id: "broken", description: "" });
	});
});
