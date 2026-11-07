/**
 * Per-lineage online quota.
 *
 * Two rules ported from dsh-subagent (`subagent/README.md` → "Activation 容量"):
 *
 * 1. A slot is reserved before the child starts and returned only after its
 *    cleanup finished, so a child that is still shutting down still counts.
 * 2. Exhaustion rejects with `CAPACITY_REACHED`; it never queues. Queueing here
 *    would let a parent that is waiting for its children also wait for a slot
 *    those children occupy — a self-deadlock.
 *
 * Lowering the limit never kills a running child; it only refuses new starts
 * until usage drops below the limit.
 *
 * ## Why the key is the lineage root, not the immediate parent
 *
 * dsh resolves the pool as `resident.get(parent.id)?.pool ?? rootPool(parent)`:
 * a child that delegates again inherits its root's pool, so one tree can never
 * hold more than `limit` live descendants in total.
 *
 * Our children are separate sessions with their own manager instance, so the
 * inheritance has to be explicit: every request carries the lineage `rootId`,
 * and the slot is charged to that root. Callers without a root fall back to
 * their own id (single-level behaviour stays exactly as before).
 *
 * ## Why the pools live in a process-wide registry
 *
 * A child session that mounts this extension gets its own manager; a
 * per-instance counter would restart from zero on every level. The slot sets are
 * therefore kept in a registry on `globalThis`, so every instance in the process
 * charges the same pool. Slots only describe live children, so this never
 * outlives a settled child: an empty pool is dropped immediately.
 *
 * Limits stay per-instance (each instance reads the module config), matching
 * dsh's `pool.reserve(this.maxActiveSubagents())` — the pool is shared data, the
 * limit is a policy read at reservation time.
 */

import { SubagentError, SubagentErrorCode } from "./errors.ts";

export interface SlotRelease {
	/** Idempotent. */
	release(): void;
}

/** Live slots per lineage root. Shared between managers in one process. */
export type PoolRegistry = Map<string, Set<symbol>>;

/** Registry key on `globalThis`; `Symbol.for` so duplicate module copies still meet. */
const POOL_REGISTRY_KEY = Symbol.for("pf-subagents.capacity.pools");

/**
 * The process-wide pool registry.
 *
 * Duplicate copies of this module (extension path aliases, test reloads) must
 * share one registry, otherwise a grandchild's manager would see an empty pool.
 */
export function sharedPoolRegistry(): PoolRegistry {
	const holder = globalThis as unknown as Record<symbol, unknown>;
	const existing = holder[POOL_REGISTRY_KEY];
	if (existing instanceof Map) return existing as PoolRegistry;
	const created: PoolRegistry = new Map<string, Set<symbol>>();
	holder[POOL_REGISTRY_KEY] = created;
	return created;
}

/**
 * Drop a registry entry no other instance is using. Test helper; the extension
 * never needs it because pools empty themselves as children settle.
 */
export function resetPoolRegistry(registry: PoolRegistry = sharedPoolRegistry()): void {
	registry.clear();
}

export class Capacity {
	#limit: number;
	#pools: PoolRegistry;

	/**
	 * @param limit - maximum simultaneous children per lineage root.
	 * @param pools - slot storage. Defaults to a private map (unit tests stay
	 *   isolated); the extension passes {@link sharedPoolRegistry}.
	 */
	constructor(limit: number, pools: PoolRegistry = new Map()) {
		this.#limit = Capacity.validate(limit);
		this.#pools = pools;
	}

	static validate(limit: number): number {
		if (!Number.isSafeInteger(limit) || limit <= 0) {
			throw new RangeError(`capacity limit must be a positive safe integer, got ${limit}`);
		}
		return limit;
	}

	get limit(): number {
		return this.#limit;
	}

	/** Change the limit. Running children keep their slots. */
	setLimit(limit: number): void {
		this.#limit = Capacity.validate(limit);
	}

	/** Live slots charged to one lineage root. */
	used(rootId: string): number {
		return this.#pools.get(rootId)?.size ?? 0;
	}

	/**
	 * Reserve one slot for `rootId`.
	 * @throws SubagentError `CAPACITY_REACHED` when the lineage is at its limit.
	 */
	acquire(rootId: string): SlotRelease {
		const pool = this.#pools.get(rootId) ?? new Set<symbol>();
		if (pool.size >= this.#limit) {
			throw new SubagentError(
				SubagentErrorCode.CAPACITY_REACHED,
				`subagent capacity reached: ${pool.size}/${this.#limit} active for ${rootId}`,
			);
		}
		const slot = Symbol("subagent-slot");
		pool.add(slot);
		this.#pools.set(rootId, pool);
		let released = false;
		return {
			release: () => {
				if (released) return;
				released = true;
				pool.delete(slot);
				// Empty pools are dropped: nothing to count, and root session files
				// would otherwise accumulate for the life of the process.
				if (pool.size === 0 && this.#pools.get(rootId) === pool) this.#pools.delete(rootId);
			},
		};
	}
}
