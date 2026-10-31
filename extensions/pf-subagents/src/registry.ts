/**
 * Named provider registry with capability negotiation.
 *
 * A provider is chosen by name; a request that asks for a capability the provider
 * does not declare is rejected with `BACKEND_UNSUPPORTED` instead of degrading
 * quietly (dsh-subagent: "未实现任何执行方法的提供方会在注册时以
 * `UNSUPPORTED_CAPABILITY` 被拒绝；请求不受支持的能力会以同一码使创建失败").
 */

import { SubagentError, SubagentErrorCode } from "./errors.ts";
import type { Provider, SubagentRequest } from "./types.ts";

export interface Requirement {
	continuable?: boolean;
	inheritsParentContext?: boolean;
	modelOptions?: boolean;
	worktree?: boolean;
	/** A numeric recursion budget is required; providers that manage depth themselves reject. */
	numericDepthLimit?: boolean;
}

export class ProviderRegistry {
	#providers = new Map<string, Provider>();

	register(provider: Provider): void {
		const name = provider.name?.trim();
		if (!name) {
			throw new SubagentError(SubagentErrorCode.PROVIDER_UNKNOWN, "provider name must be a non-empty string");
		}
		if (this.#providers.has(name)) {
			throw new SubagentError(SubagentErrorCode.DUPLICATE_PROVIDER, `provider already registered: ${name}`);
		}
		this.#providers.set(name, provider);
	}

	unregister(name: string): void {
		this.#providers.delete(name);
	}

	names(): string[] {
		return [...this.#providers.keys()];
	}

	get(name: string): Provider {
		const provider = this.#providers.get(name);
		if (!provider) {
			throw new SubagentError(SubagentErrorCode.PROVIDER_UNKNOWN, `unknown subagent provider: ${name}`);
		}
		return provider;
	}

	/**
	 * Resolve a provider for a request and assert the capabilities the request implies.
	 * @throws SubagentError `PROVIDER_UNKNOWN` | `BACKEND_UNSUPPORTED`
	 */
	resolve(name: string, request: SubagentRequest): Provider {
		const provider = this.get(name);
		const required = requirementsOf(request);
		const capabilities = provider.capabilities;
		const missing: string[] = [];
		if (required.continuable && !capabilities.continuable) missing.push("continuable");
		if (required.inheritsParentContext && !capabilities.inheritsParentContext) missing.push("inheritsParentContext");
		if (required.modelOptions && !capabilities.modelOptions) missing.push("modelOptions");
		if (required.worktree && !capabilities.worktree) missing.push("worktree");
		if (required.numericDepthLimit && capabilities.depthLimit === "provider-managed") missing.push("numericDepthLimit");
		if (missing.length > 0) {
			throw new SubagentError(
				SubagentErrorCode.BACKEND_UNSUPPORTED,
				`provider ${name} does not support: ${missing.join(", ")}`,
			);
		}
		return provider;
	}
}

function requirementsOf(request: SubagentRequest): Requirement {
	return {
		modelOptions: request.model !== undefined,
		// M1 only delegates fresh children with a numeric budget; fork/worktree land later.
		numericDepthLimit: true,
	};
}
