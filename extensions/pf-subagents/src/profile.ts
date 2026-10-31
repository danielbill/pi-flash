/**
 * Profile documents: `~/.pi/agent/agents/<id>.md` (global) and `.pi/agents/<id>.md` (project).
 *
 * Frontmatter is deliberately a small YAML subset (scalars, `|` block scalars, and
 * `- item` lists). A profile that cannot be read fails closed with `PROFILE_INVALID`
 * rather than silently running with the parent's composition.
 */

import { readdirSync, readFileSync } from "node:fs";
import { readFile, readdir } from "node:fs/promises";
import { homedir } from "node:os";
import { join } from "node:path";

import { SubagentError, SubagentErrorCode } from "./errors.ts";

/** Tool-permission tiers (034 recipe reuse). No `full+`: that stays delegator-only. */
export type ToolTier = "inherit" | "read-only" | "default" | "full";

export const TOOL_TIERS: readonly ToolTier[] = ["inherit", "read-only", "default", "full"];

/** Default turn budget when neither the delegation call nor the profile sets one. */
export const DEFAULT_MAX_TURNS = 100;

/**
 * Built-in profiles shipped with the extension (044-1 §0：内置 = 扩展代码内
 * 常量，只读，同名自定义文件覆盖；自定义 > 内置与目录 shadow 同序)。
 *
 * 内容须与 PF Settings 页的内置清单逐字一致（crates/pi-link/src/agents.rs
 * `builtin_agents`）——两侧各存一份，改动时同步。没有这份常量，委派模型
 * 永远无从得知有哪些 profile 可点名（文件系统之外它看不见任何清单）。
 */
export const BUILTIN_PROFILE_DOCS: Readonly<Record<string, string>> = {
	explorer: [
		"---",
		"description: 只读探索 · 调研与定位",
		"tools: read-only",
		"maxTurns: 0",
		"---",
		"你是只读探索员：调研代码、定位实现、回答问题。",
		"不执行任何写操作。",
	].join("\n"),
	// 2026-10-19 拍板：dsh 无出厂内置，对齐 ZCode 的两内置（general-purpose +
	// Explore，explorer 即 Explore 位）。不写 `tools:` = 继承委派者全部工具。
	"general-purpose": [
		"---",
		"description: 通用子代理 · 复杂调研、代码检索、多步任务",
		"maxTurns: 0",
		"---",
		"你是通用子代理：独立完成委派的任务——调研复杂问题、检索代码、执行多步操作。",
		"可使用会话的全部工具，按需读写文件与执行命令。",
	].join("\n"),
};

/**
 * Whitelist each tier resolves to before `createAgentSession`.
 *
 * `inherit` is handled separately (no allowlist): the child keeps pi's default
 * selection plus the codemode/tool_search overrides, i.e. the delegator's own
 * assembly. The three concrete tiers must name every tool explicitly because a
 * plain-name allowlist drops unlisted extension tools — `codemode`/`tool_search`
 * are therefore folded in for the tiers that keep them.
 */
export const TOOL_TIER_WHITELISTS: Record<Exclude<ToolTier, "inherit">, readonly string[]> = {
	// Read-only exploration: no scripting engine, no mutation, no MCP surface.
	"read-only": ["read", "grep", "find", "ls"],
	// Default coding set plus the codemode/tool_search infrastructure (M2.5).
	default: ["read", "bash", "edit", "write", "codemode", "tool_search"],
	// Every built-in tool; `agent` is absent by construction (children cannot delegate).
	full: ["bash", "read", "edit", "write", "grep", "find", "ls", "codemode", "tool_search"],
};

export interface Profile {
	id: string;
	description?: string;
	model?: string;
	/** Replacement persona; wins over the body when both are present. */
	systemPrompt?: string;
	/** Append persona — the document body. */
	appendSystemPrompt?: string;
	/** Tool tier; `undefined` means inherit the delegator's assembly. */
	tools?: ToolTier;
	/**
	 * Turn budget (soft limit). `undefined` means {@link DEFAULT_MAX_TURNS};
	 * `0` is the "unlimited" tier and disables the budget entirely.
	 */
	maxTurns?: number;
	/** Whether the project AGENTS.md loads into the child; `undefined` means true. */
	injectAgentsMd?: boolean;
	/** Character budget for inherited parent context. */
	inheritContextLimit?: number;
	/** `worktree` isolates the child in a separate checkout. */
	isolation?: "worktree";
	/** Where the profile came from, for diagnostics. */
	source: string;
}

export interface ProfilePaths {
	globalDir: string;
	projectDir: string;
}

export function defaultProfilePaths(cwd: string): ProfilePaths {
	return {
		globalDir: join(homedir(), ".pi", "agent", "agents"),
		projectDir: join(cwd, ".pi", "agents"),
	};
}

interface Frontmatter {
	data: Record<string, unknown>;
	body: string;
}

/** Minimal frontmatter reader: scalars, `|` block scalars, quoted scalars, and `- item` lists. */
export function parseFrontmatter(text: string, source = "<memory>"): Frontmatter {
	const normalized = text.replace(/\r\n/g, "\n");
	if (!normalized.startsWith("---\n")) {
		return { data: {}, body: normalized.trim() };
	}
	const end = normalized.indexOf("\n---", 4);
	if (end === -1) {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: unterminated frontmatter`);
	}
	const block = normalized.slice(4, end);
	const body = normalized.slice(end + 4).replace(/^\n/, "").trim();
	const data: Record<string, unknown> = {};
	let listKey: string | undefined;
	const lines = block.split("\n");
	for (let i = 0; i < lines.length; i += 1) {
		const rawLine = lines[i];
		if (!rawLine.trim() || rawLine.trim().startsWith("#")) continue;
		const listItem = rawLine.match(/^\s*-\s+(.*)$/);
		if (listItem && listKey) {
			const list = (data[listKey] as string[]) ?? [];
			list.push(unquote(listItem[1].trim()));
			data[listKey] = list;
			continue;
		}
		const pair = rawLine.match(/^([A-Za-z0-9_-]+):\s*(.*)$/);
		if (!pair) continue;
		const [, key, value] = pair;
		listKey = undefined;
		if (value === "|" || value === "|-" || value === "|+") {
			// Block scalar: consume every following indented line, then dedent.
			const collected: string[] = [];
			let indent: number | undefined;
			while (i + 1 < lines.length) {
				const next = lines[i + 1];
				if (next.trim() === "") {
					collected.push("");
					i += 1;
					continue;
				}
				const leading = next.length - next.trimStart().length;
				if (leading === 0) break;
				if (indent === undefined) indent = leading;
				collected.push(next.slice(Math.min(indent, leading)));
				i += 1;
			}
			data[key] = collected.join("\n").replace(/\n+$/, "");
			continue;
		}
		if (value === "") {
			listKey = key;
			data[key] = [];
			continue;
		}
		data[key] = unquote(value.trim());
	}
	return { data, body };
}

function unquote(value: string): string {
	if ((value.startsWith('"') && value.endsWith('"')) || (value.startsWith("'") && value.endsWith("'"))) {
		return value.slice(1, -1);
	}
	return value;
}

export function profileFromDocument(id: string, text: string, source: string): Profile {
	const { data, body } = parseFrontmatter(text, source);
	const tools = data.tools;
	if (tools !== undefined && !TOOL_TIERS.includes(tools as ToolTier)) {
		throw new SubagentError(
			SubagentErrorCode.PROFILE_INVALID,
			`${source}: tools must be one of ${TOOL_TIERS.join("/")} (got ${String(tools)})`,
		);
	}
	const isolation = data.isolation;
	if (isolation !== undefined && isolation !== "worktree") {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: unsupported isolation ${String(isolation)}`);
	}
	const limit = data.inheritContextLimit;
	if (limit !== undefined && (!Number.isSafeInteger(Number(limit)) || Number(limit) < 0)) {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: inheritContextLimit must be a number`);
	}
	const maxTurns = data.maxTurns;
	// `0` is the "unlimited" tier (no budget); a positive integer is an explicit cap.
	if (maxTurns !== undefined && (!Number.isSafeInteger(Number(maxTurns)) || Number(maxTurns) < 0)) {
		throw new SubagentError(
			SubagentErrorCode.PROFILE_INVALID,
			`${source}: maxTurns must be a non-negative integer (0 = unlimited)`,
		);
	}
	const inject = data.injectAgentsMd;
	if (inject !== undefined && inject !== "true" && inject !== "false") {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: injectAgentsMd must be true or false`);
	}
	return {
		id,
		description: typeof data.description === "string" ? data.description : undefined,
		model: typeof data.model === "string" && data.model !== "" ? data.model : undefined,
		systemPrompt: typeof data.systemPrompt === "string" && data.systemPrompt !== "" ? data.systemPrompt : undefined,
		appendSystemPrompt: body === "" ? undefined : body,
		tools: tools as ToolTier | undefined,
		maxTurns: maxTurns === undefined ? undefined : Number(maxTurns),
		injectAgentsMd: inject === undefined ? undefined : inject === "true",
		inheritContextLimit: limit === undefined ? undefined : Number(limit),
		isolation: isolation as "worktree" | undefined,
		source,
	};
}

/** Load one profile by id; the project directory shadows the global one, and
 * the built-in constant is the last fallback before failing. */
export async function loadProfile(id: string, paths: ProfilePaths): Promise<Profile> {
	if (!/^[A-Za-z0-9._-]+$/.test(id)) {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `invalid profile id: ${id}`);
	}
	for (const dir of [paths.projectDir, paths.globalDir]) {
		const file = join(dir, `${id}.md`);
		try {
			return profileFromDocument(id, await readFile(file, "utf8"), file);
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code === "ENOENT") continue;
			throw asProfileError(error, file);
		}
	}
	const doc = BUILTIN_PROFILE_DOCS[id];
	if (doc !== undefined) {
		return profileFromDocument(id, doc, "<builtin>");
	}
	throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `profile not found: ${id}`);
}

/** List profile ids from both directories plus the built-ins; project entries
 * win on collision (ids are unique regardless of which side provides them). */
export async function listProfiles(paths: ProfilePaths): Promise<string[]> {
	const ids = new Set<string>(Object.keys(BUILTIN_PROFILE_DOCS));
	for (const dir of [paths.globalDir, paths.projectDir]) {
		let entries: string[];
		try {
			entries = await readdir(dir);
		} catch {
			continue;
		}
		for (const entry of entries) {
			if (entry.endsWith(".md")) ids.add(entry.slice(0, -3));
		}
	}
	return [...ids].sort();
}

/** Frontmatter digest for the delegation list — enough for the model to pick
 * a profile without reading the whole document. */
export interface ProfileSummary {
	id: string;
	description: string;
	/** Tool tier; `undefined` = inherit the delegator's assembly. */
	tools?: ToolTier;
}

/** Sync twin of {@link listProfiles} carrying frontmatter summaries, for
 * tool-description building — `registerTool` takes a static string, so the
 * delegation tool's view of the nameable profiles is computed once at
 * registration. Fresh sessions see newly written profile files (PF spawns one
 * pi process per session). Shadow order matches {@link loadProfile}: project
 * file > global file > built-in. A corrupt document still lists (bare id);
 * actually delegating to it fails closed via {@link loadProfile}. */
export function listProfilesSync(paths: ProfilePaths): ProfileSummary[] {
	const docs = new Map<string, string>(Object.entries(BUILTIN_PROFILE_DOCS));
	for (const dir of [paths.globalDir, paths.projectDir]) {
		try {
			for (const entry of readdirSync(dir)) {
				if (!entry.endsWith(".md")) continue;
				const id = entry.slice(0, -3);
				try {
					docs.set(id, readFileSync(join(dir, entry), "utf8"));
				} catch {
					// unreadable file: keep the previous doc, or bare id if none
				}
			}
		} catch {
			// dir missing: built-ins still count
		}
	}
	return [...docs.entries()]
		.sort(([a], [b]) => (a < b ? -1 : 1))
		.map(([id, doc]) => {
			const summary: ProfileSummary = { id, description: "" };
			try {
				const { data } = parseFrontmatter(doc, id);
				if (typeof data.description === "string") summary.description = data.description;
				if (TOOL_TIERS.includes(data.tools as ToolTier)) summary.tools = data.tools as ToolTier;
			} catch {
				// corrupt frontmatter: list the bare id; use fails closed later
			}
			return summary;
		});
}

function asProfileError(error: unknown, source: string): SubagentError {
	if (error instanceof SubagentError) return error;
	const message = error instanceof Error ? error.message : String(error);
	return new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: ${message}`, { cause: error });
}
