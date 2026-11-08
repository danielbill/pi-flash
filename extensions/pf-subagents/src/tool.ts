/**
 * The `agent` tool: dispatch, not wait.
 *
 * dsh parity (`tool-subagent/src/index.ts` `startActivation({ delivery: 'parent' })`):
 * the call only admits the child and returns its id; the run is owned by the
 * manager and its answer arrives as a settlement message. The parent's turn is
 * therefore never parked by a delegation — whether the parent keeps working or
 * yields to the human is a property of the code, not of the wording.
 *
 * `delivery: "caller"` survives for programmatic callers that must consume the
 * answer inline (code-mode fan-out reads `structuredContent.text`); it is an
 * explicit `blocking: true` opt-in because it parks the parent turn for the
 * child's whole run.
 *
 * The description is generated from the profile's context inheritance so the model
 * cannot claim to see a conversation the child never received (dsh-subagent does
 * the same with two phrasings).
 */

import { resolve } from "node:path";

import { assertResumable } from "./backends/local.ts";
import { asSubagentError, SubagentError, SubagentErrorCode } from "./errors.ts";
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
	/** Parent's own session file: resuming it would fork the parent into a child
	 * that writes the parent's transcript, so it is rejected. */
	parentSessionFile?: string;
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
		resumeFrom: {
			type: "string",
			description:
				"Session file of an earlier child run to continue instead of starting fresh: " +
				"`resumeFrom: <session_file>` (the path the card / completion notice reports). " +
				"The child keeps its whole history, so resumed work never has to be redone.",
		},
		blocking: {
			type: "boolean",
			description:
				"Only for a programmatic caller that must read the answer inline: parks your turn for the child's whole run, " +
				"so the user cannot reach you until it returns. Leave unset for every interactive delegation.",
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
		// dsh parity (044-5 §4 + tool-subagent/src/index.ts): dispatch never waits.
		// The wording follows the code — the tool really does return a bare id, so
		// no prompt can lure the model into believing it must sit and wait.
		"This tool starts an independently managed subagent and immediately returns its id: it never parks your turn. The runtime notifies you when it finishes, and the notice includes its final answer.",
		"Right after dispatching, keep working or reply to the user — do not wait for the child and do not poll it (no `sleep`, no repeated `subagent_list`/status checks): the notice is steered into your turn if you are still working, or starts a new one when you are idle, so the user can talk to you at any time.",
		"The child does not report mid-run; use `subagent_send` to steer it while running or continue its conversation after it finishes.",
		// 冷恢复（044-5 §4.6 / dsh coldResume）：轮次用尽或中断的子代理可以带着
		// 全部历史续跑，别重做调研——这是 token 预算最容易漏掉的一条。
		"To continue a child whose run already ended (for example it ran out of turns), pass `resumeFrom` with its session file: it resumes that session with all its history instead of starting over. The notice and the card both carry the path.",
		`${profiles} Delegate independent work in parallel by issuing several calls in one turn.${capacity}`,
		// 委派政策（对齐 ZCode task 工具描述的力度——实测只有机械说明时
		// 模型从不主动委派，哪怕系统提示词里有鼓励 bullet；见 buglist 第五批）
		"When to delegate: the task matches one of the profiles above, or answering means sweeping many files or whole directories — research, comparing implementations, locating where something is done, multi-step verification — and you only need the conclusions, not the file dumps. Delegate it and continue the main task; do not repeat the delegated searches yourself. For a single known file, symbol, or value, act directly instead.",
	].join(" ");
}

/** Path identity for the resume guard (case-insensitive on Windows). */
function sameFile(a: string, b: string): boolean {
	const normalize = (p: string) => resolve(p).toLowerCase();
	return normalize(a) === normalize(b);
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
		/** 冷恢复：把这一轮接到既有子会话文件上（044-5 §4.6）。 */
		resumeFrom?: string;
		/** 显式等待模式（code-mode 聚合用）；交互派发一律不传，见文件头。 */
		blocking?: boolean;
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
	// 冷恢复的准入检查先于容量：坏路径/父会话/仍在跑的会话都不该花掉一个名额。
	const resumeFrom = typeof input.resumeFrom === "string" && input.resumeFrom.trim() ? input.resumeFrom.trim() : undefined;
	if (resumeFrom !== undefined) {
		try {
			assertResumable(resumeFrom);
		} catch (error) {
			const failure = asSubagentError(error, SubagentErrorCode.NOT_RESUMABLE);
			return fail(failure.code, failure.message);
		}
		if (context.parentSessionFile && sameFile(context.parentSessionFile, resumeFrom)) {
			return fail(SubagentErrorCode.NOT_RESUMABLE, "resumeFrom must be a child session, not the parent's own session file");
		}
		// 路径比对要归一：manager 记的是 prepare 返回的绝对路径，模型给的可能
		// 是相对路径或大小写不同的写法。
		for (const live of options.manager.liveSessionFiles()) {
			if (sameFile(live, resumeFrom)) {
				return fail(SubagentErrorCode.NOT_RESUMABLE, `that child session is still running: ${resumeFrom}`);
			}
		}
	}
	// 默认派发即返回（dsh 的唯一投递形态）；只有显式 `blocking: true` 才等结果。
	const delivery: Delivery = input.blocking === true ? "caller" : "parent";
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
			resumeFrom,
			delivery,
			maxTurns: input.maxloop,
			signal: context.signal,
		});
		if (delivery === "parent") {
			return {
				content: [
					{
						type: "text",
						// 派发结果就是反轮询契约的落点：模型在这条消息上最容易"等"
						// （实测它去 bash sleep 30 轮询）。机械说明比提示词有效。
						text:
							`started subagent ${activation.childId} — it runs in the background. ` +
							"Do NOT poll for it (no `sleep`, no repeated `subagent_list`): " +
							"its result arrives as a notification on its own.",
					},
				],
				structuredContent: { childId: activation.childId, status: "started" },
				// B4（044）：session_file 在 prepare 返回时就存在，派发当帧就带上
				// ——否则派发后的子代理整段运行期卡片点不开（要等 completion
				// 回灌才有路径）。
				details: {
					child_id: activation.childId,
					delivery,
					status: "started",
					label: input.label,
					...(activation.sessionFile ? { session_file: activation.sessionFile } : {}),
				},
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
				...(activation.sessionFile ? { session_file: activation.sessionFile } : {}),
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
