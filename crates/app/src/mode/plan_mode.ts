/**
 * PF 计划模式扩展（031-1，随包分发：机制同 assets/full_activate.ts，
 * dev 直读本文件、release 落盘 ~/.pi-flash）。所有裁决以
 * docs/模块设计/031-输入面板/031-1-计划模式.md §2 为准：
 *
 * - 无工具集切换/无快照（裁决 2）：工具集始终是原选项，计划模式只靠
 *   tool_call 硬闸把写操作打回（裁决 3），批准 = 仅清除计划状态。
 * - 拦截面（裁决 6）：edit/write 一律 block；bash 按只读白名单
 *   （官方 plan-mode 示例 isSafeCommand 同款 + git 只读子集）；
 *   其余非白名单工具（含 MCP/子代理）一律 block。
 *
 * 通道契约（P1 消费）：
 * - PF→ext：`prompt "/plan"`（支持 on/off 参数）、`"/plan-approve"`、
 *   `"/plan-reject"` —— 扩展命令，流式中也立即执行（rpc-commands.md:31）。
 * - ext→PF：ctx.ui.setStatus("pf-plan", JSON) fire-and-forget 广播：
 *   `{"v":1,"enabled":bool,"pending":string|null}`，pending = 挂起计划正文。
 * - 持久化：pi.appendEntry("pf-plan-mode",{enabled})；session_start 恢复
 *   后重新广播（会话切换/分支/冷恢复零 PF 持久化成本，裁决 1）。
 *
 * 审批流（裁决 4/5）：模型收尾调 present_plan(plan) → 广播挂起 + 挂起等待
 *   （race abort signal）→ PF 批准发 /plan-approve（resolve 后 agent 同轮
 *   继续）；否决发 /plan-reject（本轮干净收尾，计划模式保持）；拒绝按钮走
 *   Abort+ClearQueue（present_plan 随 abort 取消）。
 */

import { Type } from "@earendil-works/pi-ai";
import { defineTool, type ExtensionAPI, type ExtensionContext } from "@earendil-works/pi-coding-agent";
import type { AgentMessage } from "@earendil-works/pi-agent-core";

const STATUS_KEY = "pf-plan";
const ENTRY_TYPE = "pf-plan-mode";
const CONTEXT_TYPE = "pf-plan-context";
const PLAN_TOOL = "present_plan";

const BLOCK_REASON =
	"Blocked by plan mode: file modifications are not allowed. " +
	"Discuss the plan with the user; when the plan is complete, call the " +
	`'${PLAN_TOOL}' tool with the full plan. Only the user can exit plan mode.`;

// ---- bash 破坏性命令黑名单（2026-11-08 用户裁决：只拒绝修改，缺省放行
// —— 压缩/Provider/子代理委派等扩展工具与只读管道必须原样可用） ----

const DESTRUCTIVE_PATTERNS = [
	/\brm\b/i, /\brmdir\b/i, /\bmv\b/i, /\bcp\b/i, /\bmkdir\b/i, /\btouch\b/i,
	/\bchmod\b/i, /\bchown\b/i, /\bchgrp\b/i, /\bln\b/i, /\btee\b/i,
	/\btruncate\b/i, /\bdd\b/i, /\bshred\b/i,
	/(^|[^<])>(?!>)/, />>/,
	/\bnpm\s+(install|uninstall|update|ci|link|publish)/i,
	/\bpnpm\s+(add|remove|install|publish)/i,
	/\byarn\s+(add|remove|install|publish)/i,
	/\bpip\s+(install|uninstall)/i,
	/\bapt(-get)?\s+(install|remove|purge|update|upgrade)/i,
	/\bgit\s+(add|commit|push|pull|merge|rebase|reset|checkout|stash|cherry-pick|revert|tag\s|init|clone)/i,
	/\bsudo\b/i, /\bsu\b/i,
	/\bkill\b/i, /\bpkill\b/i, /\bkillall\b/i, /\breboot\b/i, /\bshutdown\b/i,
	/\b(vim?|nano|emacs|code|subl)\b/i,
];

function isSafeCommand(command: string): boolean {
	// 2026-11-08 裁决反转：缺省**放行**，仅破坏性黑名单命中才拒
	//（原 SAFE_PATTERNS 白名单不再作闸门条件；testcase 可重建对拍）
	const cmd = command ?? "";
	return !DESTRUCTIVE_PATTERNS.some((p) => p.test(cmd));
}

// ---- 状态（单布尔，无工具快照）----

let planEnabled = false;
let pendingPlan: string | null = null;
let resolvePending: ((verdict: "approved" | "rejected" | "aborted") => void) | null = null;

function broadcast(ctx: ExtensionContext): void {
	try {
		ctx.ui.setStatus(STATUS_KEY, JSON.stringify({ v: 1, enabled: planEnabled, pending: pendingPlan }));
	} catch {
		// 广播失败不阻断本地状态（PF 侧下次 session_start 广播会补齐）
	}
}

function persist(ctx: ExtensionContext): void {
	// 走 ctx.sessionManager 直写：命令 handler 里 pi.appendEntry 的 runtime
	// action 未绑定会抛错（RPC 模式实测，探针 2026-11-08），ctx 上的
	// sessionManager 是当前会话本体，语义等价（custom 条目 + data）。
	ctx.sessionManager.appendCustomEntry(ENTRY_TYPE, { enabled: planEnabled });
}

function settle(verdict: "approved" | "rejected" | "aborted"): void {
	if (resolvePending) {
		const res = resolvePending;
		resolvePending = null;
		res(verdict);
	}
}

/** 否决走 Abort 后，transcript 里的中止错误结果在模型视角改写为用户
 * 裁决的明确反馈（否则下一轮模型看到的是 "This operation was aborted"）。
 * 判定收敛：present_plan + isError + 文本含 aborted——其余错误原样保留。 */
function rewriteAbortedPlanResult(m: AgentMessage): AgentMessage {
	const tm = m as AgentMessage & {
		role?: string;
		toolName?: string;
		isError?: boolean;
		content?: Array<{ type: string; text?: string }>;
	};
	if (tm.role !== "toolResult" || tm.toolName !== PLAN_TOOL || !tm.isError) return m;
	const aborted = tm.content?.some(
		(c) => c.type === "text" && (c.text ?? "").includes("aborted"),
	);
	if (!aborted) return m;
	return {
		...tm,
		content: [{ type: "text", text: "用户拒绝或取消了你的计划。" }],
	} as AgentMessage;
}

export default function planModeExtension(pi: ExtensionAPI): void {
	// ---- 工具闸门（裁决 2/3/6）----
	pi.on("tool_call", async (event) => {
		if (!planEnabled) return;
		if (event.toolName === PLAN_TOOL) return; // 自有工具，handler 自行 guard
		if (event.toolName === "edit" || event.toolName === "write") {
			return { block: true, reason: BLOCK_REASON };
		}
		if (event.toolName === "bash") {
			const command = (event.input as { command?: string } | undefined)?.command ?? "";
			if (!isSafeCommand(command)) {
				return { block: true, reason: `${BLOCK_REASON}\nCommand: ${command}` };
			}
			return;
		}
		// 2026-11-08 用户裁决：其余工具一律放行——压缩/Provider（freeflow）/
		// 子代理委派/MCP 等扩展生态不下掉，计划模式只拒绝「修改」本身。
	});

	// ---- 每轮提醒（仅在计划态；背景任务触发的 run 不重复注入）----
	pi.on("before_agent_start", async (event) => {
		if (!planEnabled || event.messageType !== "user") return;
		return {
			message: {
				customType: CONTEXT_TYPE,
				display: false,
				content: `[PF PLAN MODE ACTIVE]
You are in plan mode. The user wants a plan, not execution.

Restrictions:
- You MUST NOT modify any files or system state.
- edit/write and destructive bash are BLOCKED and return errors.
- Other tools (compaction, provider switching, subagent delegation, MCP) remain available — use them normally, but NOT to work around the restriction: never modify files or state through them.

Explore read-only where appropriate and DISCUSS the work plan with the user.

When the plan is complete (and only then), call the '${PLAN_TOOL}' tool with the complete plan in Markdown as the 'plan' argument. The user will approve or reject it. If the user replies with feedback, refine the plan; only user approval ends plan mode.`,
			},
		};
	});

	// ---- 计划态关闭后，历史提醒不再进上下文（官方示例同款清理）----
	pi.on("context", async (event) => {
		// 031-1 两层：(a) 内部提醒卡（pf-plan-context）不进上下文；
		// (b) 否决走 Abort 后 transcript 留下的是中止错误结果（"This
		// operation was aborted"），模型视角在本钩子改写为用户裁决的明确
		// 反馈文案——run 已被 Abort 立即停止，下一轮模型看到的就是这句话
		// （2026-11-08 用户定稿：反馈 = 用户拒绝或取消了你的计划）。
		return {
			messages: event.messages
				.filter((m) => {
					const msg = m as AgentMessage & { customType?: string };
					return msg.customType !== CONTEXT_TYPE;
				})
				.map((m) => rewriteAbortedPlanResult(m)),
		};
	});

	// ---- present_plan：呈现计划并挂起等待裁决（裁决 4）----
	pi.registerTool(
		defineTool({
			name: PLAN_TOOL,
			label: "Present plan",
			description:
				"Present your implementation plan for user review. Only available in plan mode. " +
				"Call it when the plan is complete after discussing with the user; the user will approve or reject.",
			parameters: Type.Object({
				plan: Type.String({ description: "The complete plan in Markdown", minLength: 1 }),
			}),
			async execute(_toolCallId, params, signal, _onUpdate, ctx) {
				if (!planEnabled) {
					throw new Error("present_plan is only available in plan mode.");
				}
				pendingPlan = params.plan;
				broadcast(ctx);
				const verdict = await new Promise<"approved" | "rejected" | "aborted">((resolve) => {
					resolvePending = resolve;
					if (signal) {
						if (signal.aborted) resolve("aborted");
						else signal.addEventListener("abort", () => resolve("aborted"), { once: true });
					}
				});
				pendingPlan = null;
				broadcast(ctx);
				if (verdict === "approved") {
					planEnabled = false;
					persist(ctx);
					broadcast(ctx);
					return {
						content: [{
							type: "text",
							text: "The user APPROVED the plan and plan mode is now off: all tools are available again. Start implementing the plan now.",
						}],
						details: undefined,
					};
				}
				if (verdict === "rejected") {
					if (planEnabled) {
						return {
							content: [{
								type: "text",
								text: "The user rejected the plan (plan mode is still on). End this turn and wait for the user's next input; re-plan when they reply.",
							}],
							details: undefined,
						};
					}
					return {
						content: [{ type: "text", text: "Plan dismissed: the user turned plan mode off. End this turn." }],
						details: undefined,
					};
				}
				return { content: [{ type: "text", text: "Plan approval was aborted." }], details: undefined };
			},
		}),
	);

	// ---- 三命令（PF 控制通道）----
	pi.registerCommand("plan", {
		description: "Toggle PF plan mode (args: on | off, default toggle)",
		handler: async (args, ctx) => {
			const arg = (args ?? "").trim().toLowerCase();
			const next = arg === "on" ? true : arg === "off" ? false : !planEnabled;
			if (next !== planEnabled) {
				planEnabled = next;
				if (!planEnabled) settle("rejected"); // 关闭时若有挂起等待，先放行
				persist(ctx);
			}
			broadcast(ctx);
		},
	});

	pi.registerCommand("plan-approve", {
		description: "Approve the pending plan (PF plan mode)",
		handler: async (_args, ctx) => {
			if (!pendingPlan) {
				ctx.ui.notify("plan-approve: no pending plan.");
				return;
			}
			settle("approved");
		},
	});

	pi.registerCommand("plan-reject", {
		description: "Reject the pending plan, keep plan mode (PF plan mode)",
		handler: async (_args, ctx) => {
			if (!pendingPlan) {
				ctx.ui.notify("plan-reject: no pending plan.");
				return;
			}
			pendingPlan = null;
			settle("rejected");
		},
	});

	// ---- 会话切换/冷恢复：重置后从会话条目恢复并广播（裁决 1）----
	pi.on("session_start", async (_event, ctx) => {
		planEnabled = false;
		pendingPlan = null;
		resolvePending = null;
		try {
			const entries = ctx.sessionManager.getEntries() as Array<{
				type: string;
				customType?: string;
				data?: { enabled?: boolean };
			}>;
			const last = entries
				.filter((e) => e.type === "custom" && e.customType === ENTRY_TYPE)
				.pop();
			if (last?.data && typeof last.data.enabled === "boolean") {
				planEnabled = last.data.enabled;
			}
		} catch {
			// 读不到条目按非计划态处理
		}
		broadcast(ctx);
	});
}
