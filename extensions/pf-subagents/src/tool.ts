/**
 * The `agent` tool: one tool, two delivery modes.
 *
 * - `delivery: "caller"` (M1 default): the call awaits the child and returns its
 *   answer, so the result is an ordinary tool result — pi's nested-call accounting
 *   covers usage, compaction and cancellation without a side channel.
 * - `delivery: "parent"` (v2 background): returns the child id immediately and the
 *   completion notification carries the answer.
 *
 * The description is generated from the profile's context inheritance so the model
 * cannot claim to see a conversation the child never received (dsh-subagent does
 * the same with two phrasings).
 */

import { SubagentError, SubagentErrorCode } from "./errors.ts";
import type { ProfileSummary } from "./profile.ts";
import type { SubagentManager } from "./manager.ts";
import type { Delivery, SubagentResult } from "./types.ts";

export interface DelegationToolOptions {
	manager: SubagentManager;
	providerName: string;
	/** Tool name; must be unique among loaded delegation tools. */
	name?: string;
	/** Default profile id when the model does not choose one. */
	defaultProfile?: string;
	/** Profiles the model may choose. Empty means it cannot override the default. */
	allowedProfiles?: string[];
	/** Whether the backend child starts from the parent's completed history. */
	inheritsParentContext: boolean;
}

export interface DelegationContext {
	parentId: string;
	/** Depth of the calling agent; the child is one deeper. */
	depth: number;
	cwd: string;
	signal?: AbortSignal;
	parentToolCallId?: string;
}

export interface ToolResultLike {
	content: Array<{ type: "text"; text: string }>;
	structuredContent?: Record<string, unknown>;
	details?: unknown;
	isError?: boolean;
	/** Child-session token usage; pi surfaces it on the tool result (not main-context accounting). */
	usage?: Record<string, unknown>;
}

export const DELEGATION_PARAMETERS = {
	type: "object",
	additionalProperties: false,
	properties: {
		prompt: { type: "string", description: "The task for the child agent, stated in full." },
		profile: { type: "string", description: "Profile id selecting the child's model, prompt and tools." },
		model: { type: "string", description: "Exact model override, `provider/model`." },
		cwd: { type: "string", description: "Working directory for the child; defaults to the caller's." },
		label: { type: "string", description: "Short (3-5 word) display name for the child." },
		maxloop: {
			type: "integer",
			minimum: 0,
			description:
				"Estimated turns this task needs; the child wraps up at the budget and is cut off if it ignores the wrap-up. " +
				"0 means no budget; omitted defaults to 100.",
		},
		background: {
			type: "boolean",
			description:
				"Return immediately with a child id; the runtime notifies you when it finishes.",
		},
	},
	required: ["prompt"],
} as const;

export function delegationDescription(
	options: Pick<DelegationToolOptions, "inheritsParentContext" | "allowedProfiles" | "defaultProfile"> & {
		/** Frontmatter digests for the nameable profiles; when present the
		 * description lists each profile with what it is, so the model can pick
		 * on intent instead of guessing from bare ids. */
		profileSummaries?: ProfileSummary[];
		/** Concurrency cap, injected so the model can pace itself and knows a
		 * CAPACITY_REACHED rejection means "retry after an earlier delegation
		 * settles", not "give up" (044-0 B3①). */
		capacity?: number;
	},
): string {
	const context = options.inheritsParentContext
		? "It does not see the current in-flight turn, only completed history."
		: "It does not see this conversation.";
	const capacity = options.capacity
		? ` At most ${options.capacity} subagents run concurrently in this session; if a call is rejected with CAPACITY_REACHED, re-issue it once an earlier delegation settles — slots free as they finish.`
		: "";
	const summaries = options.profileSummaries;
	const profiles = summaries && summaries.length > 0
		? ` Choose a profile:\n${summaries
				.map((p) =>
					`- ${p.id}${p.description ? `: ${p.description}` : ""}${p.tools ? ` (tools: ${p.tools})` : " (tools: inherit)"}`,
				)
				.join("\n")}.`
		: options.allowedProfiles && options.allowedProfiles.length > 0
			? ` Choose a profile from: ${options.allowedProfiles.join(", ")}.`
			: options.defaultProfile
				? ` The profile defaults to ${options.defaultProfile}.`
				: "";
	return [
		"Delegate a task to a child agent that works in its own session.",
		context,
		"The child's final answer is returned as this tool's result.",
		// dsh parity (044-5 §4): the async contract lives in the description, and the
		// dispatch result stays a bare sentence — the parent is woken by the
		// settlement message instead of being told to end its turn.
		"This tool starts an independently managed subagent; with `background: true` it returns its id immediately and the runtime notifies you when it finishes, and the notice includes its final answer.",
		"The child does not report mid-run; use `subagent_send` to steer it while running or continue its conversation after it finishes.",
		`${profiles} Delegate independent work in parallel by issuing several calls in one turn.${capacity}`,
		// 委派政策（对齐 ZCode task 工具描述的力度——实测只有机械说明时
		// 模型从不主动委派，哪怕系统提示词里有鼓励 bullet；见 buglist 第五批）
		"When to delegate: the task matches one of the profiles above, or answering means sweeping many files or whole directories — research, comparing implementations, locating where something is done, multi-step verification — and you only need the conclusions, not the file dumps. Delegate it and continue the main task; do not repeat the delegated searches yourself, wait for the results. For a single known file, symbol, or value, act directly instead.",
	].join(" ");
}

export async function runDelegation(
	options: DelegationToolOptions,
	input: {
		prompt: string;
		profile?: string;
		model?: string;
		cwd?: string;
		label?: string;
		maxloop?: number;
		background?: boolean;
	},
	context: DelegationContext,
	on_update?: (partial: ToolResultLike) => void,
): Promise<ToolResultLike> {
	// B3②：失败即时上屏——先发一条 status=failed 的 onUpdate（compact 卡
	// 立刻变 [失败]+分码），再返回 failure。pi 的 toolResult 要等同帧全部
	// 工具收尾才写转录，没有这条分片时被拒委派在其余子代理运行期间完全
	// 不可见（044-0 真机 B3）。
	const fail = (code: SubagentErrorCode, message: string): ToolResultLike => {
		on_update?.({
			content: [{ type: "text", text: `${code}: ${message}` }],
			structuredContent: { errorCode: code, message },
			details: { status: "failed", error_code: code, ...(input.label ? { label: input.label } : {}) },
		});
		return failure(code, message);
	};
	if (options.allowedProfiles && options.allowedProfiles.length > 0 && input.profile) {
		if (!options.allowedProfiles.includes(input.profile)) {
			return fail(SubagentErrorCode.PROFILE_INVALID, `profile not allowed: ${input.profile}`);
		}
	}
	// `0` is the "unlimited" tier (no budget), so only a negative value is rejected.
	if (input.maxloop !== undefined && (!Number.isSafeInteger(input.maxloop) || input.maxloop < 0)) {
		return fail(SubagentErrorCode.START_REJECTED, `maxloop must be a non-negative integer, got ${input.maxloop}`);
	}
	const delivery: Delivery = input.background ? "parent" : "caller";
	try {
		const activation = await options.manager.start({
			provider: options.providerName,
			parentId: context.parentId,
			parentToolCallId: context.parentToolCallId,
			depth: context.depth + 1,
			prompt: input.prompt,
			profile: input.profile ?? options.defaultProfile,
			model: input.model,
			cwd: input.cwd ?? context.cwd,
			label: input.label,
			delivery,
			maxTurns: input.maxloop,
			signal: context.signal,
		});
		if (delivery === "parent") {
			return {
				content: [{ type: "text", text: `started subagent ${activation.childId}` }],
				structuredContent: { childId: activation.childId, status: "started" },
				details: { child_id: activation.childId, delivery, status: "started", label: input.label },
			};
		}
		// 运行中进度（044 卡片状态行数据源）：accept 完成即发一次。
		on_update?.({
			content: [{ type: "text", text: `subagent ${activation.childId} running` }],
			structuredContent: { childId: activation.childId, status: "running" },
			details: {
				child_id: activation.childId,
				status: "running",
				label: input.label,
				profile: input.profile,
			},
		});
		const result = await activation.wait();
		return success(result);
	} catch (error) {
		const failureError = SubagentError.is(error)
			? error
			: new SubagentError(SubagentErrorCode.START_REJECTED, error instanceof Error ? error.message : String(error));
		return fail(failureError.code, failureError.message);
	}
}

function success(result: SubagentResult): ToolResultLike {
	const usage = result.usage as Record<string, unknown> | undefined;
	const cost = usage && (usage["cost"] as Record<string, unknown> | undefined);
	return {
		content: [{ type: "text", text: result.text }],
		structuredContent: {
			childId: result.childId,
			text: result.text,
			stopReason: result.stopReason,
			usage: result.usage,
			...(result.stats ? { stats: result.stats } : {}),
			durationMs: result.durationMs,
		},
		// 卡片数据源（Rust 侧读 details，snake_case）。
		details: {
			child_id: result.childId,
			stop_reason: result.stopReason,
			session_file: result.session_file,
			duration_ms: result.durationMs,
			total_tokens: usage?.["totalTokens"],
			cost_total: cost?.["total"],
			// Activity counters (044-0 card/panel row 2), snake_case for the host.
			...(result.stats
				? {
						turns: result.stats.turns,
						agent_messages: result.stats.agentMessages,
						tool_calls: result.stats.toolCalls,
					}
				: {}),
			error_code: result.errorCode,
		},
		isError: result.stopReason === "error",
		...(usage ? { usage } : {}),
	};
}

function failure(code: SubagentErrorCode, message: string): ToolResultLike {
	return {
		content: [{ type: "text", text: `${code}: ${message}` }],
		structuredContent: { errorCode: code, message },
		// 卡片数据源读 snake_case（Rust tool_card）；camelCase 只进 structuredContent
		details: { error_code: code },
		isError: true,
	};
}
