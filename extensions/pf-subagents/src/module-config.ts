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
	/** `provider/model` handed to profiles that name none; `undefined` = inherit the parent's model. */
	readonly model?: string;
}

/**
 * Used **only** when `config.json` is absent or unreadable — a host must keep
 * delegating instead of dying on a missing file. This is the one place the
 * module carries numbers of its own; once the file exists it wins entirely.
 */
export const FALLBACK_MODULE_CONFIG: ModuleConfig = Object.freeze({
	capacity: Object.freeze({ default: 3, min: 2, max: 10 }),
	maxDepth: Object.freeze({ default: 1, min: 1, max: 3 }),
	maxTurns: 100,
	model: undefined,
});

export interface ModuleConfigReadResult {
	readonly config: ModuleConfig;
	readonly path: string;
	/** Why the fallback is in force; `undefined` when the file parsed cleanly. */
	readonly problem?: string;
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

function readModel(raw: unknown, source: string): string | undefined {
	if (raw === undefined || raw === null || raw === "") return undefined;
	if (typeof raw !== "string") {
		throw invalidConfig(source, `model must be a "provider/model" string, got ${JSON.stringify(raw)}`);
	}
	if (raw.indexOf("/") <= 0) {
		throw invalidConfig(source, `model must be "provider/model", got "${raw}"`);
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
		model: readModel(doc["model"], source),
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
