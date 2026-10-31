/**
 * Per-parent online quota.
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
			throw new SubagentError(
				SubagentErrorCode.CAPACITY_REACHED,
				`subagent capacity reached: ${current}/${this.#limit} active for ${parentId}`,
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
