/**
 * Online quota for delegated children.
 *
 * Ported from dsh-subagent (`packages/subagent/subagent/src/activation.ts` →
 * `ActivationPool`), keeping the three invariants that matter:
 *
 * 1. A slot is reserved before the child starts and returned only after its
 *    cleanup finished, so a child that is still shutting down still counts.
 * 2. Exhaustion rejects with `CAPACITY_REACHED`; it never queues. Queueing here
 *    would let a parent that is waiting for its children also wait for a slot
 *    those children occupy — a self-deadlock.
 * 3. Lowering the limit never kills a running child; it only refuses new starts
 *    until usage drops below the limit.
 *
 * ## Scope: one bucket per delegating session (not per lineage root)
 *
 * dsh keys its pool by the *root* agent and lets descendants inherit it through
 * `resident.get(parent.id)?.pool ?? rootPool(parent)`, so a whole tree shares one
 * budget. That inheritance needs the parent to be a live registration in the same
 * manager — true in dsh, unreachable here:
 *
 * - one pi process serves one PF session (`crates/app/src/agent_session.rs:39`),
 *   so the delegating session is the only user of this manager;
 * - child sessions only mount extensions listed in the profile
 *   (`src/index.ts` → `additionalExtensionPaths`), and no shipped profile lists
 *   this extension, so a child has no `agent` tool and cannot delegate again.
 *
 * Hence per-parent and per-root are the same bucket today. `PARITY.md` records
 * what has to change if child sessions ever mount this extension.
 */

import { SubagentError, SubagentErrorCode } from "./errors.ts";

export interface SlotRelease {
	/** Idempotent. */
	release(): void;
}

export class Capacity {
	#limit: number;
	#used = new Map<string, number>();

	constructor(limit: number) {
		this.#limit = Capacity.validate(limit);
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

	used(parentId: string): number {
		return this.#used.get(parentId) ?? 0;
	}

	/**
	 * Reserve one slot for `parentId`.
	 * @throws SubagentError `CAPACITY_REACHED` when the parent is at its limit.
	 */
	acquire(parentId: string): SlotRelease {
		const current = this.used(parentId);
		if (current >= this.#limit) {
			// dsh's wording carries the only useful next step, so keep it verbatim.
			throw new SubagentError(
				SubagentErrorCode.CAPACITY_REACHED,
				`subagent limit reached (active child limit: ${this.#limit}); ` +
					"wait for an existing child to finish or complete this work with the current agents",
			);
		}
		this.#used.set(parentId, current + 1);
		let released = false;
		return {
			release: () => {
				if (released) return;
				released = true;
				const next = this.used(parentId) - 1;
				if (next <= 0) this.#used.delete(parentId);
				else this.#used.set(parentId, next);
			},
		};
	}
}
