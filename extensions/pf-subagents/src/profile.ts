/**
 * Profile documents: PF-owned directories — global `<PF数据目录>/agents/<id>.md`
 * (injected via `PI_SUBAGENTS_PROFILES_DIR`; fallback `~/.pi-flash/agents`) and
 * project `<cwd>/.pi-flash/agents/<id>.md`.
 *
 * 2026-11-06 迁出 `~/.pi/agent/agents`：pi 扩展形态曾跟随 pi 生态约定，但
 * profile frontmatter schema 是 pf-subagents 私有的，与 pi 市场社区 subagent
 * 扩展共用目录 = 同一批 `.md` 被两套 schema 各自解释（命名空间污染）。
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

/** Tool-permission tiers (034 recipe reuse). `full+` adds per-profile pi-extension
 * and MCP-server selection (044-0 工具定制 6 档); plain `full` is the built-in set only. */
export type ToolTier = "inherit" | "chat-only" | "read-only" | "default" | "full" | "full+";

export const TOOL_TIERS: readonly ToolTier[] = ["inherit", "chat-only", "read-only", "default", "full", "full+"];

/** Thinking levels for the profile `thinking` key — pi `ThinkingLevel` seven
 * keys (pi-agent-core). `undefined`/absent = auto = don't override. */
export const THINKING_LEVELS = ["off", "minimal", "low", "medium", "high", "xhigh", "max"] as const;

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
		// 044-0 技能设置三态：键省略 = 继承系统开启集；裸键 = 关闭
		"skills:",
		"---",
		"你是只读探索员：调研代码、定位实现、回答问题。",
		"不执行任何写操作。",
	].join("\n"),
	// 2026-10-19 拍板：dsh 无出厂内置，对齐 ZCode 的两内置（general-purpose +
	// Explore，explorer 即 Explore 位）。不写 `tools:` = 继承委派者全部工具。
	"general-purpose": [
		"---",
		"description: 通用子代理 · 复杂调研、代码检索、多步任务",
		// 工具/技能/环境全继承：不写 tools / skills / injectAgentsMd；
		// 轮次「指定」= 不写 maxTurns（委派方给 maxloop）
		"---",
		"你是通用子代理：独立完成委派的任务——调研复杂问题、检索代码、执行多步操作。",
		"可使用会话的全部工具，按需读写文件与执行命令。",
	].join("\n"),
	// 044-0 第三枚内置：摘要机——无状态摘要，不注入 AGENTS.md（不继承环境
	// 上下文），只读工具集。适用本地/廉价模型跑系统日志摘要等批量总结。
	summarizer: [
		"---",
		"description: 无状态摘要 · 阅读文件或内容按要求总结",
		"tools: read-only",
		// 不继承环境（044-0「no cwd」）：cwd 落 PF 数据目录、不注入 AGENTS.md
		"cwd: pf-data",
		"injectAgentsMd: false",
		// 轮次固定 5（044-0 内置清单）
		"maxTurns: 5",
		"skills:",
		"---",
		"你是摘要机：无状态的摘要机器人——阅读指定的文件或一段内容，按要求总结。",
		"不继承任何环境上下文，不执行任何写操作，只输出摘要。",
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
	// Chat-only: no tools at all (model answers from context alone; 044-0 chat-only 档).
	"chat-only": [],
	// Read-only exploration: no scripting engine, no mutation, no MCP surface.
	"read-only": ["read", "grep", "find", "ls"],
	// Default coding set plus the codemode/tool_search infrastructure (M2.5).
	default: ["read", "bash", "edit", "write", "codemode", "tool_search"],
	// Every built-in tool; `agent` is absent by construction (children cannot delegate).
	full: ["bash", "read", "edit", "write", "grep", "find", "ls", "codemode", "tool_search"],
	// full+ = full toolset + the profile's own pi-extension/MCP selection
	// (044-0: 档位选 full+ 时额外挂 extensions/mcp 清单)。
	"full+": ["bash", "read", "edit", "write", "grep", "find", "ls", "codemode", "tool_search"],
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
	 * Thinking level for the child session (2026-11-07 子代理表单). One of
	 * pi's `ThinkingLevel` seven keys; `undefined` = auto = don't override
	 * (the child falls back to pi's own default for its model).
	 */
	thinking?: string;
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
	/** Working directory override (2026-11-08 子代理表单). `"pf-data"` runs the
	 * child in the PF data directory; `undefined` = inherit the delegator's cwd.
	 * Resolve it with `resolveProfileCwd` — never pass the raw token down. */
	cwd?: string;
	/** pi-extension paths to mount in the child (full+ 档的 pi-ext 选择；044-0). */
	extensions?: string[];
	/** MCP server names the child may use (full+ 档的 mcp 服务选择；044-0). */
	mcpServers?: string[];
	/**
	 * Skill ids to inject (044-0 skills 定制, three states): **absent** = the
	 * system-enabled set (inherit), **present but empty** = no skills at all
	 * (关闭), non-empty list = exactly those ids.
	 */
	skills?: string[];
	/** Where the profile came from, for diagnostics. */
	source: string;
}

export interface ProfilePaths {
	globalDir: string;
	projectDir: string;
}

export function defaultProfilePaths(cwd: string): ProfilePaths {
	// PF 自有目录（2026-11-06 迁出 ~/.pi/agent/agents——pi 市场社区扩展共用
	// 旧目录，schema 撞车）。PF 宿主 spawn 时注入 PI_SUBAGENTS_PROFILES_DIR
	// （= <PF数据目录>/agents 绝对路径）；兜底 ~/.pi-flash/agents 只在非 PF
	// 宿主单独挂载本扩展时生效。
	const globalDir = process.env["PI_SUBAGENTS_PROFILES_DIR"] || join(homedir(), ".pi-flash", "agents");
	return {
		globalDir,
		projectDir: join(cwd, ".pi-flash", "agents"),
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
	const thinking = data.thinking;
	if (
		thinking !== undefined &&
		!THINKING_LEVELS.includes(String(thinking) as (typeof THINKING_LEVELS)[number])
	) {
		throw new SubagentError(
			SubagentErrorCode.PROFILE_INVALID,
			`${source}: thinking must be one of ${THINKING_LEVELS.join("/")} (got ${String(thinking)})`,
		);
	}
	const strList = (key: string): string[] | undefined => {
		const v = data[key];
		if (v === undefined) return undefined;
		if (!Array.isArray(v) || v.some((x) => typeof x !== "string")) {
			throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: ${key} must be a list of strings`);
		}
		return v as string[];
	};
	const extensions = strList("extensions");
	const mcpServers = strList("mcp");
	const skills = strList("skills");
	const isolation = data.isolation;
	if (isolation !== undefined && isolation !== "worktree") {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `${source}: unsupported isolation ${String(isolation)}`);
	}
	const cwd = data.cwd;
	if (cwd !== undefined && cwd !== "pf-data") {
		throw new SubagentError(
			SubagentErrorCode.PROFILE_INVALID,
			`${source}: cwd must be "pf-data" (got ${String(cwd)})`,
		);
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
		thinking: thinking === undefined ? undefined : String(thinking),
		maxTurns: maxTurns === undefined ? undefined : Number(maxTurns),
		injectAgentsMd: inject === undefined ? undefined : inject === "true",
		inheritContextLimit: limit === undefined ? undefined : Number(limit),
		isolation: isolation as "worktree" | undefined,
		cwd: cwd === undefined ? undefined : String(cwd),
		extensions,
		mcpServers,
		skills,
		source,
	};
}

/**
 * Resolve a profile's `cwd` token to an absolute path.
 *
 * `undefined` (key absent/empty) = inherit the delegator's cwd. `"pf-data"` =
 * the PF data directory (`PI_FLASH_DIR`); a host that does not expose it gets
 * `undefined` — inherit beats guessing a project directory, and PF always sets
 * the env var.
 */
export function resolveProfileCwd(
	profileCwd: string | undefined,
	dataDir: string | undefined,
): string | undefined {
	if (profileCwd === undefined) return undefined;
	return dataDir;
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
