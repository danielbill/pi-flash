/**
 * dsh-subagent parity invariants (PARITY.md P4).
 *
 * These pin the semantics that must survive refactors. Each test names the dsh
 * behaviour it mirrors, so a diff that breaks parity fails here instead of in the
 * field.
 */

import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { SubagentError, SubagentErrorCode } from "../src/errors.ts";
import { SubagentManager } from "../src/manager.ts";
import { baseRequest, ScriptedProvider } from "./helpers.ts";

const code = (error: unknown) => (error as SubagentError).code;

const setup = (managerOptions = {}) => {
	const manager = new SubagentManager(managerOptions);
	const provider = new ScriptedProvider("scripted");
	manager.registry.register(provider);
	return { manager, provider };
};

describe("dsh parity", () => {
	// activation.ts reserve(): lowering the limit only refuses new starts; it
	// never tears down a slot that is already held.
	it("lowering the limit mid-run keeps live children and refuses the next start", async () => {
		const { manager, provider } = setup({ capacity: 2 });
		await manager.start(baseRequest());
		await manager.start(baseRequest());
		manager.capacity = 1;
		const error = await manager.start(baseRequest()).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.CAPACITY_REACHED);
		assert.equal(provider.prepareCalls, 2);
		assert.equal(provider.children[0].disposals, 0);
		assert.equal(provider.children[1].disposals, 0);
	});

	// manager.ts: admission checks depth first (`assertDelegationDepth`), so an
	// over-deep request cannot consume a slot it would have to give back.
	it("checks depth before quota", async () => {
		const { manager } = setup({ capacity: 1, maxDepth: 1 });
		await manager.start(baseRequest());
		const error = await manager.start(baseRequest({ depth: 2 })).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.DEPTH_LIMIT);
	});

	// dsh shares one pool per root because one manager owns the whole tree. Our
	// host gives each session its own manager (PARITY.md P0), so buckets are
	// per-manager: two managers must not see each other's slots.
	it("keeps buckets per manager, not per process", async () => {
		const first = setup({ capacity: 1 });
		const second = setup({ capacity: 1 });
		await first.manager.start(baseRequest({ parentId: "/shared.jsonl" }));
		await assert.doesNotReject(second.manager.start(baseRequest({ parentId: "/shared.jsonl" })));
	});

	// activation.ts: the slot is returned only after cleanup, so a child that is
	// still disposing still counts toward the limit.
	it("counts a settling child until its cleanup finished", async () => {
		const { manager, provider } = setup({ capacity: 1 });
		const activation = await manager.start(baseRequest());
		const child = provider.children[0];
		const releaseDispose = child.blockDispose();
		child.complete("done");
		const pending = activation.wait();
		await new Promise<void>((resolve) => setImmediate(resolve));
		const error = await manager.start(baseRequest()).catch((e: unknown) => e);
		assert.equal(code(error), SubagentErrorCode.CAPACITY_REACHED);
		releaseDispose();
		await pending;
		await assert.doesNotReject(manager.start(baseRequest()));
	});
});
