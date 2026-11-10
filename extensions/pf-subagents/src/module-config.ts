/**
 * pf-subagents module config — one internal file, one source of truth.
 *
 * The document lives beside the profile documents
 * (`<profiles dir>/config.json`) and is deliberately **not** a tool parameter
 * and **not** mentioned in any tool description: a delegating model must never
 * be able to raise its own concurrency or depth budget.
 *
 * Limits are policy, so the policy lives in the file. The bounds (`min`/`max`)
 * are read from the same document as the value they bound — code validates and
 * clamps, it does not carry a second, competing ceiling. Raising `max` in the
 * file is the sanctioned way to allow more concurrency; there is no source edit
 * involved.
 *
 * Read fresh on every delegation entry (see `syncModuleConfig`), so an edit
 * applies without restarting the host: env vars are a process-level snapshot
 * and would go stale in a long-lived PF session.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";

import { SubagentError, SubagentErrorCode } from "./errors.ts";

export const MODULE_CONFIG_FILENAME = "config.json";

export interface BoundedSetting {
	/** Value applied when nothing more specific overrides it. */
	readonly default: number;
	/** Inclusive lower bound: smaller values clamp up to it. */
	readonly min: number;
	/** Inclusive upper bound: larger values clamp down to it. */
	readonly max: number;
}

export interface ModuleConfig {
	/** Per-parent concurrent child budget. */
	readonly capacity: BoundedSetting;
	/** Delegation depth budget. */
	readonly maxDepth: BoundedSetting;
	/** Turn budget for a child when neither the call nor its profile sets one; `0` = unlimited. */
	readonly maxTurns: number;
}

// 2026-11-10 用户定夺：模块级 `model` **已废除**（不再读、不再写）。默认模型
// 只有两条路——profile frontmatter 的 `model:` 与调用级 `model` 参数；
// 都没写 = 继承父会话模型。旧文件里残留的 `model` 键按未知键忽略（PF 写入
// 时顺手删除，见 pi-link `agents/module_config.rs`）。

/**
 * Used **only** when `config.json` is absent or unreadable — a host must keep
 * delegating instead of dying on a missing file. This is the one place the
 * module carries numbers of its own; once the file exists it wins entirely.
 */
export const FALLBACK_MODULE_CONFIG: ModuleConfig = Object.freeze({
	// Bounds mirror dsh's `maxActiveSubagents` / `maxDepth` schema
	// (`packages/subagent/subagent/src/index.ts`): 1..MAX_SAFE_INTEGER default 8,
	// and 0..MAX_SAFE_INTEGER default 1.
	capacity: Object.freeze({ default: 8, min: 1, max: Number.MAX_SAFE_INTEGER }),
	maxDepth: Object.freeze({ default: 1, min: 0, max: Number.MAX_SAFE_INTEGER }),
	maxTurns: 100,
});

export interface ModuleConfigReadResult {
	readonly config: ModuleConfig;
	readonly path: string;
	/** Why the fallback is in force; `undefined` when the file parsed cleanly. */
	readonly problem?: string;
}

/** {@link syncModuleConfig} 的结果：投影链下多带一个模块开关（001-1 §5）。
 * `enabled` 只在投影读到时给出；无投影 = `undefined`（开关由宿主侧决定）。 */
export interface ModuleConfigSyncResult extends ModuleConfigReadResult {
	readonly enabled?: boolean;
}

function invalidConfig(source: string, detail: string): SubagentError {
	return new SubagentError(SubagentErrorCode.MODULE_CONFIG_INVALID, `${source}: ${detail}`);
}

function readBoundedSetting(
	raw: unknown,
	field: string,
	fallback: BoundedSetting,
	source: string,
): BoundedSetting {
	if (raw === undefined || raw === null) return fallback;
	if (typeof raw !== "object" || Array.isArray(raw)) {
		throw invalidConfig(source, `${field} must be an object like { "default": 3, "min": 2, "max": 10 }`);
	}
	const entry = raw as Record<string, unknown>;
	const pick = (key: keyof BoundedSetting): number => {
		const value = entry[key];
		if (value === undefined) return fallback[key];
		if (typeof value !== "number" || !Number.isSafeInteger(value)) {
			throw invalidConfig(source, `${field}.${key} must be a safe integer, got ${JSON.stringify(value)}`);
		}
		return value;
	};
	const min = pick("min");
	const max = pick("max");
	const dflt = pick("default");
	if (min > max) {
		throw invalidConfig(source, `${field}.min (${min}) exceeds ${field}.max (${max})`);
	}
	if (dflt < min || dflt > max) {
		throw invalidConfig(source, `${field}.default (${dflt}) must lie within ${min}..${max}`);
	}
	return { default: dflt, min, max };
}

function readMaxTurns(raw: unknown, source: string): number {
	if (raw === undefined || raw === null) return FALLBACK_MODULE_CONFIG.maxTurns;
	if (typeof raw !== "number" || !Number.isSafeInteger(raw) || raw < 0) {
		throw invalidConfig(source, `maxTurns must be a non-negative safe integer (0 = unlimited), got ${JSON.stringify(raw)}`);
	}
	return raw;
}

/**
 * Parse and validate a module config document.
 *
 * Unknown keys are ignored on purpose: hosts may annotate the file (`version`,
 * UI hints) without the extension rejecting it.
 *
 * @throws SubagentError `MODULE_CONFIG_INVALID` with the offending field named.
 */
export function parseModuleConfig(raw: string, source: string = MODULE_CONFIG_FILENAME): ModuleConfig {
	let data: unknown;
	try {
		data = JSON.parse(raw);
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		throw invalidConfig(source, `not valid JSON (${message})`);
	}
	if (!data || typeof data !== "object" || Array.isArray(data)) {
		throw invalidConfig(source, "top level must be an object");
	}
	const doc = data as Record<string, unknown>;
	return {
		capacity: readBoundedSetting(doc["capacity"], "capacity", FALLBACK_MODULE_CONFIG.capacity, source),
		maxDepth: readBoundedSetting(doc["maxDepth"], "maxDepth", FALLBACK_MODULE_CONFIG.maxDepth, source),
		maxTurns: readMaxTurns(doc["maxTurns"], source),
		// `model` 不再是模块字段（2026-11-10 废除）：残留值按未知键忽略
	};
}

export function moduleConfigPath(profilesDir: string): string {
	return join(profilesDir, MODULE_CONFIG_FILENAME);
}

/**
 * Read the config, falling back to {@link FALLBACK_MODULE_CONFIG} with a
 * `problem` description instead of throwing: a broken file must not stop the
 * host from delegating, but the caller can surface the reason.
 */
export function loadModuleConfig(profilesDir: string): ModuleConfigReadResult {
	const path = moduleConfigPath(profilesDir);
	let raw: string;
	try {
		raw = readFileSync(path, "utf8");
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		return { config: FALLBACK_MODULE_CONFIG, path, problem: `unreadable (${message})` };
	}
	try {
		return { config: parseModuleConfig(raw, path), path };
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		return { config: FALLBACK_MODULE_CONFIG, path, problem: message };
	}
}

/** Clamp `value` into the bounds declared by `bound`. */
export function clampToBound(value: number, bound: BoundedSetting): number {
	return Math.min(bound.max, Math.max(bound.min, value));
}

/** Minimal surface the config writes to — keeps this module free of manager imports. */
export interface ModuleConfigTarget {
	capacity: number;
	maxDepth: number;
}

/** Apply the file's policy to a live manager (running children keep their slots). */
export function applyModuleConfig(target: ModuleConfigTarget, config: ModuleConfig): void {
	target.capacity = clampToBound(config.capacity.default, config.capacity);
	target.maxDepth = clampToBound(config.maxDepth.default, config.maxDepth);
}

// ---------------------------------------------------------------------------
// ext-context.json 投影（001-2，S3）：PF 是唯一写者，本模块只读。
// 回退链固定：投影（version 匹配）→ legacy agents/config.json → FALLBACK。
// 坏投影 / 版本不匹配 = **保持现值**（不覆盖正在跑的上限），problem 上报。
// 每次委派入口读一次，不缓存 mtime（readFileSync+parse 实测 ~90µs，噪声）。
// ---------------------------------------------------------------------------

export const PROJECTION_FILENAME = "ext-context.json";
const PROJECTION_VERSION = 1;

/** 投影 `subagents` 段：PF 设置页的生效值（PF 侧已 validate 过，这里仅防御）。 */
export interface ProjectionSubagents {
	readonly capacity: number;
	readonly max_depth: number;
	readonly enabled: boolean;
}

export type ProjectionRead =
	| { readonly state: "ok"; readonly value: ProjectionSubagents; readonly path: string }
	| { readonly state: "absent" }
	| { readonly state: "invalid"; readonly path: string; readonly problem: string };

/**
 * Read the `subagents` section of the host's context projection.
 *
 * 缺文件（首启 / 非 PF 宿主）= `absent`，走 legacy 链；文件在但解析失败 /
 * 版本不匹配 / 段坏 = `invalid`（调用方保持现值并上报 problem，绝不回退到
 * FALLBACK 覆盖在跑上限——坏档不许静默改行为）。
 */
export function readSubagentsProjection(pfDataDir: string | undefined): ProjectionRead {
	if (!pfDataDir) return { state: "absent" };
	const path = join(pfDataDir, PROJECTION_FILENAME);
	let raw: string;
	try {
		raw = readFileSync(path, "utf8");
	} catch {
		return { state: "absent" };
	}
	const invalid = (problem: string): ProjectionRead => ({ state: "invalid", path, problem });
	let data: unknown;
	try {
		data = JSON.parse(raw);
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		return invalid(`not valid JSON (${message})`);
	}
	if (!data || typeof data !== "object" || Array.isArray(data)) {
		return invalid("top level must be an object");
	}
	const doc = data as Record<string, unknown>;
	if (doc["version"] !== PROJECTION_VERSION) {
		return invalid(`unsupported projection version ${JSON.stringify(doc["version"])}`);
	}
	const section = doc["subagents"];
	if (!section || typeof section !== "object" || Array.isArray(section)) {
		return invalid("missing subagents section");
	}
	const entry = section as Record<string, unknown>;
	const capacity = entry["capacity"];
	const maxDepth = entry["max_depth"];
	const enabled = entry["enabled"];
	if (typeof capacity !== "number" || !Number.isSafeInteger(capacity) || capacity < 1) {
		return invalid(`subagents.capacity must be a safe integer >= 1, got ${JSON.stringify(capacity)}`);
	}
	if (typeof maxDepth !== "number" || !Number.isSafeInteger(maxDepth) || maxDepth < 0) {
		return invalid(`subagents.max_depth must be a safe integer >= 0, got ${JSON.stringify(maxDepth)}`);
	}
	if (typeof enabled !== "boolean") {
		return invalid(`subagents.enabled must be a boolean, got ${JSON.stringify(enabled)}`);
	}
	return { state: "ok", value: { capacity, max_depth: maxDepth, enabled }, path };
}

/**
 * 投影只有标量：默认值钳进 legacy 声明的边界（更窄的 min/max 可再收窄，
 * 001-1 §5「文件/投影更窄的 min/max 可再收窄」；legacy 坏档时边界 = FALLBACK）。
 */
export function boundFromScalar(value: number, bounds: BoundedSetting): BoundedSetting {
	return {
		default: Math.min(bounds.max, Math.max(bounds.min, value)),
		min: bounds.min,
		max: bounds.max,
	};
}
