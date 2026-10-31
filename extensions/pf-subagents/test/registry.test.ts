import assert from "node:assert/strict";
import { describe, it } from "node:test";

import { SubagentError, SubagentErrorCode } from "../src/errors.ts";
import { ProviderRegistry } from "../src/registry.ts";
import { baseRequest, captureSync, ScriptedProvider } from "./helpers.ts";

const code = (error: unknown) => (error as SubagentError).code;

describe("ProviderRegistry", () => {
	it("resolves a registered provider", () => {
		const registry = new ProviderRegistry();
		const provider = new ScriptedProvider("spawn");
		registry.register(provider);
		assert.equal(registry.get("spawn"), provider);
		assert.deepEqual(registry.names(), ["spawn"]);
	});

	it("rejects unknown providers", () => {
		const registry = new ProviderRegistry();
		const error = captureSync(() => registry.get("nope"));
		assert.equal(code(error), SubagentErrorCode.PROVIDER_UNKNOWN);
	});

	it("rejects duplicate registrations", () => {
		const registry = new ProviderRegistry();
		registry.register(new ScriptedProvider("spawn"));
		const error = captureSync(() => registry.register(new ScriptedProvider("spawn")));
		assert.equal(code(error), SubagentErrorCode.DUPLICATE_PROVIDER);
	});

	it("rejects a blank name", () => {
		const registry = new ProviderRegistry();
		const error = captureSync(() => registry.register(new ScriptedProvider("  ")));
		assert.equal(code(error), SubagentErrorCode.PROVIDER_UNKNOWN);
	});

	it("rejects model overrides on backends without model options", () => {
		const registry = new ProviderRegistry();
		registry.register(new ScriptedProvider("once", { capabilities: { modelOptions: false } }));
		const error = captureSync(() => registry.resolve("once", baseRequest({ provider: "once", model: "x/y" })));
		assert.equal(code(error), SubagentErrorCode.BACKEND_UNSUPPORTED);
		assert.match((error as Error).message, /modelOptions/);
	});

	it("rejects a numeric depth budget when the provider manages depth itself", () => {
		const registry = new ProviderRegistry();
		registry.register(new ScriptedProvider("remote", { capabilities: { depthLimit: "provider-managed" } }));
		const error = captureSync(() => registry.resolve("remote", baseRequest({ provider: "remote" })));
		assert.equal(code(error), SubagentErrorCode.BACKEND_UNSUPPORTED);
	});

	it("accepts requests the provider can serve", () => {
		const registry = new ProviderRegistry();
		const provider = new ScriptedProvider("spawn");
		registry.register(provider);
		assert.equal(registry.resolve("spawn", baseRequest()), provider);
		registry.unregister("spawn");
		assert.deepEqual(registry.names(), []);
	});
});
