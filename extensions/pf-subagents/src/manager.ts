/**
 * SubagentManager — the activation lifecycle.
 *
 * Ported from dsh-subagent (`subagent/src/manager.ts`, 1493 lines) down to the
 * invariants that matter, with cordis/preset/browser-control surfaces dropped:
 *
 * - **Reserve, then publish.** Identity and a capacity slot are taken before the
 *   backend runs; `start()` resolves only after the child accepted the task, so a
 *   returned activation is always cancellable.
 * - **Settle after cleanup.** The result resolves after the handle is disposed and
 *   the slot returned. A child still shutting down keeps counting against quota.
 * - **Close admission on settle.** Late input after settlement starts is rejected
 *   rather than silently dropped; releasing the handle prevents late work from
 *   entering a released child.
 * - **Depth is a policy, capacity is a resource.** They are checked separately and
 *   never converted into one another.
 */

import { Capacity, type SlotRelease } from "./capacity.ts";
import { createDescriptor, type SubagentDescriptor } from "./descriptor.ts";
import { asSubagentError, SubagentError, SubagentErrorCode } from "./errors.ts";
import { ProviderRegistry } from "./registry.ts";
import type { Activation, ChildHandle, Delivery, Provider, SubagentRequest, SubagentResult } from "./types.ts";

export interface ManagerOptions {
	/** Per-parent online quota. Default 8, matching dsh-subagent. */
	capacity?: number;
	/** Maximum delegation depth. Default 1: children cannot delegate. */
	maxDepth?: number;
	/** Clock, injectable for tests. */
	now?: () => Date;
	/** Called whenever a child's descriptor changes, so the host can persist it via `pi.appendEntry`. */
	onDescriptor?: (descriptor: SubagentDescriptor) => void;
	/** Parent-delivery completion hook; caller delivery never fires it. */
	onCompleted?: (result: SubagentResult) => void;
}

type ActivationState = "starting" | "active" | "settling" | "settled" | "failed";

interface ActivationRecord {
	childId: string;
	parentId: string;
	providerName: string;
	depth: number;
	delivery: Delivery;
	handle?: ChildHandle;
	slot: SlotRelease;
	state: ActivationState;
	/** Admission switch; false once settlement starts. */
	admissionOpen: boolean;
	settled?: Promise<SubagentResult>;
	descriptor: SubagentDescriptor;
}

export class SubagentManager {
	readonly registry = new ProviderRegistry();
	#capacity: Capacity;
	#maxDepth: number;
	#now: () => Date;
	#onDescriptor?: ManagerOptions["onDescriptor"];
	#onCompleted?: ManagerOptions["onCompleted"];
	#records = new Map<string, ActivationRecord>();
	#childrenByParent = new Map<string, Set<string>>();
	#sequence = 0;

	constructor(options: ManagerOptions = {}) {
		this.#capacity = new Capacity(options.capacity ?? 8);
		this.#maxDepth = options.maxDepth ?? 1;
		this.#now = options.now ?? (() => new Date());
		this.#onDescriptor = options.onDescriptor;
		this.#onCompleted = options.onCompleted;
	}

	get capacity(): number {
		return this.#capacity.limit;
	}

	set capacity(limit: number) {
		this.#capacity.setLimit(limit);
	}

	get maxDepth(): number {
		return this.#maxDepth;
	}

	set maxDepth(limit: number) {
		if (!Number.isSafeInteger(limit) || limit < 0) {
			throw new RangeError(`maxDepth must be a non-negative safe integer, got ${limit}`);
		}
		this.#maxDepth = limit;
	}

	/** Live (unsettled) children of a parent, in start order. */
	childrenOf(parentId: string): string[] {
		return [...(this.#childrenByParent.get(parentId) ?? [])];
	}

	descriptors(): SubagentDescriptor[] {
		return [...this.#records.values()].map((record) => record.descriptor);
	}

	/**
	 * Start one activation.
	 *
	 * Rejections (never a half-published activation): `DEPTH_LIMIT`,
	 * `PROVIDER_UNKNOWN`, `BACKEND_UNSUPPORTED`, `CAPACITY_REACHED`,
	 * `DUPLICATE_CHILD`, `START_REJECTED`, `ABORTED`.
	 */
	async start(request: Omit<SubagentRequest, "childId">): Promise<Activation> {
		if (!Number.isSafeInteger(request.depth) || request.depth < 1) {
			throw new SubagentError(SubagentErrorCode.DEPTH_LIMIT, `invalid delegation depth: ${request.depth}`);
		}
		if (request.depth > this.#maxDepth) {
			throw new SubagentError(
				SubagentErrorCode.DEPTH_LIMIT,
				`delegation depth ${request.depth} exceeds limit ${this.#maxDepth}`,
			);
		}
		if (request.signal?.aborted) {
			throw new SubagentError(SubagentErrorCode.ABORTED, "delegation aborted before start");
		}

		// Capability negotiation happens before any resource is taken.
		const provider = this.registry.resolve(request.provider, { ...request, childId: "" });

		const childId = this.#nextChildId();
		if (this.#records.has(childId)) {
			throw new SubagentError(SubagentErrorCode.DUPLICATE_CHILD, `child id already in use: ${childId}`);
		}
		const slot = this.#capacity.acquire(request.parentId);
		const record: ActivationRecord = {
			childId,
			parentId: request.parentId,
			providerName: provider.name,
			depth: request.depth,
			delivery: request.delivery,
			slot,
			state: "starting",
			admissionOpen: false,
			descriptor: createDescriptor({
				childId,
				provider: provider.name,
				cwd: request.cwd ?? process.cwd(),
				depth: request.depth,
				label: request.label,
				profile: request.profile,
				model: request.model,
				now: this.#now(),
			}),
		};
		this.#records.set(childId, record);
		this.#trackChild(record);
		this.#emit(record);

		try {
			const handle = await this.#prepare(provider, { ...request, childId });
			record.handle = handle;
			await handle.accept(request.prompt, { signal: request.signal });
			record.state = "active";
			record.admissionOpen = true;
			record.descriptor = { ...record.descriptor, phase: "active", updatedAt: this.#now().toISOString() };
			this.#emit(record);
		} catch (error) {
			record.state = "failed";
			record.admissionOpen = false;
			// Release anything that did start; a failed start owns nothing afterwards.
			try {
				await record.handle?.dispose();
			} catch {
				// A dispose failure must not mask the start failure.
			}
			record.slot.release();
			this.#untrackChild(record);
			const failure = asSubagentError(error, SubagentErrorCode.START_REJECTED);
			record.descriptor = {
				...record.descriptor,
				phase: "failed",
				errorCode: failure.code,
				stopReason: "error",
				updatedAt: this.#now().toISOString(),
			};
			this.#emit(record);
			throw failure;
		}

		return this.#activationOf(record);
	}

	#prepare(provider: Provider, request: SubagentRequest & { childId: string }): Promise<ChildHandle> {
		return provider.prepare(request).then(
			(handle) => {
				if (handle.id !== request.childId) {
					throw new SubagentError(
						SubagentErrorCode.START_REJECTED,
						`provider ${provider.name} returned child ${handle.id}, expected ${request.childId}`,
					);
				}
				return handle;
			},
			(error: unknown) => {
				throw asSubagentError(error, SubagentErrorCode.START_REJECTED);
			},
		);
	}

	#activationOf(record: ActivationRecord): Activation {
		const manager = this;
		const started = Promise.resolve();
		return {
			childId: record.childId,
			delivery: record.delivery,
			started,
			wait: () => manager.#settle(record),
			send: async (message: string) => {
				if (!record.admissionOpen) {
					throw new SubagentError(
						SubagentErrorCode.ADMISSION_CLOSED,
						`child ${record.childId} is ${record.state === "settled" ? "settled" : "not accepting input"}`,
					);
				}
				try {
					await record.handle?.send(message);
				} catch (error) {
					throw asSubagentError(error, SubagentErrorCode.ADMISSION_CLOSED);
				}
			},
			interrupt: async () => {
				try {
					await record.handle?.interrupt();
				} catch (error) {
					throw asSubagentError(error, SubagentErrorCode.ABORTED);
				}
			},
			dispose: async () => {
				record.admissionOpen = false;
				return await manager.#settle(record);
			},
		};
	}

	/**
	 * Settlement: result -> cleanup -> quota -> notification.
	 * Idempotent; returns the same promise for repeated calls.
	 */
	#settle(record: ActivationRecord): Promise<SubagentResult> {
		if (record.settled) return record.settled;
		record.state = "settling";
		record.admissionOpen = false;
		// No descriptor emission here: the phase only changes when settlement finishes.
		// Emitting with an unchanged phase would make observers see duplicate states.

		record.settled = (async () => {
			let result: SubagentResult;
			try {
				result = await record.handle!.result();
			} catch (error) {
				const failure = asSubagentError(error, SubagentErrorCode.START_REJECTED);
				result = {
					childId: record.childId,
					text: "",
					stopReason: "error",
					errorCode: failure.code,
				};
			}
			try {
				await record.handle!.dispose();
			} catch (error) {
				const failure = asSubagentError(error, SubagentErrorCode.START_REJECTED);
				result = { ...result, stopReason: "error", errorCode: result.errorCode ?? failure.code };
			}
			record.slot.release();
			record.state = "settled";
			this.#untrackChild(record);
			// 回写子会话文件路径：宿主“打开子会话”跳转的数据源（044）。
			record.descriptor.sessionFile = record.handle?.session_file() ?? record.descriptor.sessionFile;
			result.session_file = record.descriptor.sessionFile;
			record.descriptor = {
				...record.descriptor,
				phase:
					result.stopReason === "completed"
						? "completed"
						: result.stopReason === "aborted"
							? "interrupted"
							: "failed",
				stopReason: result.stopReason,
				// A blown turn budget is a distinct terminal state, not a generic
				// failure: the card shows "轮次用尽" rather than a bare error code.
				errorCode: result.stopReason === "max_turns" ? "MAX_TURNS" : result.errorCode,
				// 子代理 token 用量落盘（面板行的 tokens 列，pi-web Agents 对齐）
				...(result.usage ? { usage: result.usage } : {}),
				updatedAt: this.#now().toISOString(),
			};
			this.#emit(record);
			if (record.delivery === "parent") {
				this.#onCompleted?.(result);
			}
			return result;
		})();
		return record.settled;
	}

	/**
	 * Descriptors of one parent's children — the full history (live, settled,
	 * failed), start order. This is the parent's catalog: settled children stay
	 * listed (dsh 的 subagentCatalog 语义), unlike `childrenOf` which tracks
	 * only in-flight ownership.
	 */
	list(parentId: string): SubagentDescriptor[] {
		return [...this.#records.values()]
			.filter((record) => record.parentId === parentId)
			.map((record) => ({ ...record.descriptor }));
	}

	/** Find a live activation by child id. */
	activation(childId: string): Activation | undefined {
		const record = this.#records.get(childId);
		return record ? this.#activationOf(record) : undefined;
	}

	/** Descriptor snapshot for one child. */
	descriptorOf(childId: string): SubagentDescriptor | undefined {
		const record = this.#records.get(childId);
		return record ? { ...record.descriptor } : undefined;
	}

	/**
	 * Three-layer interrupt (dsh semantics): a settled child is a no-op, a live
	 * child's current turn stops while its queue and descendants are kept, and
	 * a missing child is a stable error. Never throws for already-settled ids.
	 */
	async interruptChild(childId: string): Promise<"interrupted" | "noop"> {
		const record = this.#records.get(childId);
		if (!record) {
			throw new SubagentError(SubagentErrorCode.PROVIDER_UNKNOWN, `unknown child: ${childId}`);
		}
		if (record.state === "settled" || record.state === "settling" || !record.admissionOpen) {
			return "noop";
		}
		await this.activation(childId)!.interrupt();
		return "interrupted";
	}

	/** Abort every live activation, then wait for all of them to finish cleanup. */
	async shutdown(reason: string): Promise<void> {
		const live = [...this.#records.values()].filter((record) => record.state === "active" || record.state === "starting");
		await Promise.allSettled(
			live.map(async (record) => {
				record.admissionOpen = false;
				try {
					await record.handle?.interrupt();
				} catch {
					// Interrupt is best-effort during shutdown.
				}
				await this.#settle(record);
			}),
		);
		void reason;
	}

	#trackChild(record: ActivationRecord): void {
		const set = this.#childrenByParent.get(record.parentId) ?? new Set<string>();
		set.add(record.childId);
		this.#childrenByParent.set(record.parentId, set);
	}

	#untrackChild(record: ActivationRecord): void {
		const set = this.#childrenByParent.get(record.parentId);
		if (!set) return;
		set.delete(record.childId);
		if (set.size === 0) this.#childrenByParent.delete(record.parentId);
	}

	#nextChildId(): string {
		this.#sequence += 1;
		return `sub-${this.#sequence}`;
	}

	#emit(record: ActivationRecord): void {
		this.#onDescriptor?.({ ...record.descriptor });
	}
}
