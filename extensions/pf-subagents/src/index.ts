/**
 * pf-subagents — extension entry.
 *
 * Wires the dsh-subagent call mechanics (manager, registry, capacity, descriptor,
 * local providers) onto pi. Nothing here re-implements pi's own primitives:
 * the child is a real pi session, the result is a real tool result, and usage is
 * accounted by pi's nested-call bookkeeping.
 *
 * M1 scope: local `spawn` and `fork` providers, caller delivery, depth guard,
 * orphan reconciliation. Background delivery, codemode fan-out and worktrees
 * come later (see 045).
 */

import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { homedir } from "node:os";
import { join } from "node:path";

import type { ChildSession, ResolvedComposition, SessionFactory, SessionFactoryOptions } from "./backends/local.ts";
import { LocalProvider } from "./backends/local.ts";
import { DESCRIPTOR_TYPE, parseDescriptor, reconcileOnStart, type SubagentDescriptor } from "./descriptor.ts";
import { SubagentError, SubagentErrorCode } from "./errors.ts";
import { SubagentManager } from "./manager.ts";
import { TOOL_TIER_WHITELISTS, defaultProfilePaths, listProfilesSync, loadProfile } from "./profile.ts";
import { DELEGATION_PARAMETERS, delegationDescription, runDelegation } from "./tool.ts";

const DEFAULT_CAPACITY = 8;
const DEFAULT_MAX_DEPTH = 1;
const TOOL_NAME = "agent";
/// Must match `SUBAGENT_CHILD_MARKER` in `pi-link/src/sessions.rs`: the side-bar
/// filter keys on this custom entry, never on `parentSession`.
const CHILD_MARKER_TYPE = "subagent/child";

/** Session file -> delegation depth, so a child that reloads this extension still knows its depth. */
const childDepths = new Map<string, number>();

function numberFromEnv(name: string, fallback: number): number {
	const raw = process.env[name];
	if (!raw) return fallback;
	const value = Number(raw);
	return Number.isSafeInteger(value) && value > 0 ? value : fallback;
}

/**
 * Build a child-session factory over the pi SDK.
 *
 * The SDK is imported lazily: this module must stay loadable (and testable)
 * without the package resolving, and a missing SDK is reported as a stable code
 * instead of a module-resolution stack trace.
 */
function createSessionFactory(ctx: ExtensionContext, parentSessionFile: string | undefined): SessionFactory {
	return async (options: SessionFactoryOptions): Promise<ChildSession> => {
		const sdk = await importSdk();
		const model = await resolveModel(sdk, ctx, options.model);
		const parent = ctx.sessionManager as unknown as {
			getSessionDir?: () => string | undefined;
			getSessionFile?: () => string | undefined;
		};
		const parentFile = parentSessionFile ?? parent.getSessionFile?.();
		// Children live beside the parent so hosts can find and open them, and the
		// header records the parent link (045: parentSession).
		const sessionManager = sdk.SessionManager.create(options.cwd, parent.getSessionDir?.(), {
			parentSession: parentFile,
		});
		// Marker extension: the child stamps its own file with a `subagent/child`
		// custom entry at session_start. Session-file filtering (pi-link/sessions.rs)
		// keys on THIS marker, never on `parentSession` — pi-native forks carry that
		// header too and must keep their sidebar rows.
		// agentDir is mandatory for DefaultResourceLoader (resolvePath chokes on
		// undefined on Windows); reuse the parent's own agent dir.
		const loader = new sdk.DefaultResourceLoader({
			cwd: options.cwd,
			agentDir: sdk.getAgentDir(),
			// Persona from the profile. `systemPrompt` fully replaces pi's default; the
			// body (`appendSystemPrompt`) adds to it. Both were previously parsed but
			// never reaching the loader — M4.3 fixes that.
			...(options.systemPrompt ? { systemPrompt: options.systemPrompt } : {}),
			...(options.appendSystemPrompt ? { appendSystemPrompt: [options.appendSystemPrompt] } : {}),
			// `injectAgentsMd: false` suppresses project context files (AGENTS.md) —
			// dsh's agent-instructions loader has no off switch, we do (044 §3).
			...(options.injectAgentsMd === false ? { noContextFiles: true } : {}),
			extensionFactories: [
				{
					name: "subagent-child-marker",
					hidden: true,
					factory: (childPi: { on(event: string, handler: () => void): void; appendEntry(type: string, data?: unknown): void }) => {
						childPi.on("session_start", () => {
							try {
								childPi.appendEntry(CHILD_MARKER_TYPE, { provider: options.providerName, depth: options.depth });
							} catch {
								// Missing marker degrades to "child shows in the list"; never fatal.
							}
						});
					},
				},
				// 045 M2.5：子会话是“一等公民 pi session”——codemode / tool_search /
				// MCP 显式装配（SDK 会话默认不加载内建扩展，pi-web 就栽在这里）。
				// 官方姿势见 examples/sdk/14-codemode-mcp.ts。
				sdk.createCodemodeExtension?.({ mode: "on" }),
				sdk.createToolSearchExtension?.(),
				sdk.createMcpExtension?.(),
			].filter((f): f is NonNullable<typeof f> => Boolean(f)),
		});
		await loader.reload();
		// codemode / tool_search 注册为 inactive；用 defaultTools 的 `+name`
		// 追加语义激活。与 profile 白名单（createAgentSession 的 tools 参数）
		// 相互独立：白名单裁模型面工具，codemode 是子会话基础设施。
		const settingsManager = sdk.SettingsManager.create(options.cwd, sdk.getAgentDir());
		settingsManager.applyOverrides?.({ defaultTools: ["+codemode", "+tool_search"] });
		const { session } = await sdk.createAgentSession({
			cwd: options.cwd,
			model,
			sessionManager,
			settingsManager,
			resourceLoader: loader,
			...(options.tools && options.tools.length > 0 ? { tools: options.tools } : {}),
		});
		// SDK sessions don't auto-bind the extension runtime: without this the
		// marker's session_start never fires and MCP never connects.
		await (session as unknown as { bindExtensions(bindings: unknown): Promise<void> }).bindExtensions({});
		const sessionFile = session.sessionFile;
		if (sessionFile) childDepths.set(sessionFile, currentDepthOf(ctx) + 1);
		return {
			prompt: (text: string) => session.prompt(text),
			lastAssistantText: () => session.getLastAssistantText(),
			usage: () => {
				// Child token usage = its final assistant message's usage.
				const messages = (session as unknown as { messages?: Array<{ role?: string; usage?: unknown }> }).messages ?? [];
				for (let i = messages.length - 1; i >= 0; i -= 1) {
					if (messages[i].role === "assistant") {
						return messages[i].usage as Record<string, unknown> | undefined;
					}
				}
				return undefined;
			},
			abort: () => session.abort(),
			// Turn-limit plumbing (M4.1): the backend steers a wrap-up at the budget
			// and aborts when the child ignores it. Both go through the real session.
			steer: (text: string) => session.steer(text),
			subscribe: (listener: (event: unknown) => void) => session.subscribe(listener as never),
			dispose: () => session.dispose(),
			sessionFile: sessionFile ?? parentFile,
		};
	};
}

interface SdkLike {
	createAgentSession(options: Record<string, unknown>): Promise<{ session: AgentSessionLike }>;
	getAgentDir(): string;
	SessionManager: {
		create(
			cwd: string,
			sessionDir?: string,
			options?: { id?: string; parentSession?: string },
		): unknown;
	};
	DefaultResourceLoader: new (options?: {
		cwd?: string;
		agentDir?: string;
		extensionFactories?: unknown[];
	}) => { reload(): Promise<void> };
	SettingsManager: {
		create(cwd?: string, agentDir?: string): {
			applyOverrides?(overrides: Record<string, unknown>): void;
		};
	};
	createCodemodeExtension?(options?: Record<string, unknown>): unknown;
	createToolSearchExtension?(): unknown;
	createMcpExtension?(): unknown;
}

interface AgentSessionLike {
	prompt(text: string): Promise<void>;
	getLastAssistantText(): string | undefined;
	abort(): Promise<void>;
	steer(text: string): Promise<"queued" | "handled">;
	subscribe(listener: (event: unknown) => void): () => void;
	dispose(): void;
	sessionFile?: string;
}

async function importSdk(): Promise<SdkLike> {
	try {
		return (await import("@earendil-works/pi-coding-agent")) as unknown as SdkLike;
	} catch (error) {
		throw new SubagentError(
			SubagentErrorCode.BACKEND_UNSUPPORTED,
			"cannot load @earendil-works/pi-coding-agent to create a child session",
			{ cause: error },
		);
	}
}

async function resolveModel(sdk: SdkLike, ctx: ExtensionContext, requested?: string): Promise<unknown> {
	if (!requested) return ctx.model;
	const separator = requested.indexOf("/");
	if (separator <= 0) {
		throw new SubagentError(SubagentErrorCode.PROFILE_INVALID, `model must be provider/model, got ${requested}`);
	}
	const provider = requested.slice(0, separator);
	const id = requested.slice(separator + 1);
	const registry = ctx.modelRegistry as unknown as { find?: (provider: string, id: string) => unknown };
	const model = registry.find?.(provider, id);
	if (!model) {
		throw new SubagentError(SubagentErrorCode.BACKEND_UNSUPPORTED, `model not available: ${requested}`);
	}
	void sdk;
	return model;
}

function currentDepthOf(ctx: ExtensionContext): number {
	const file = (ctx.sessionManager as unknown as { sessionFile?: string }).sessionFile;
	if (!file) return 0;
	return childDepths.get(file) ?? 0;
}

export default function piSubagents(pi: ExtensionAPI): void {
	// Descriptors are persisted as custom entries, so a restart can tell which
	// children never settled. `persist` is rebound per session; the manager reads it
	// through this closure.
	let persist: (descriptor: SubagentDescriptor) => void = () => {};

	// Parent-delivery completion hook (v2 background): routed to the live
	// session's ui.notify; rebound on every session_start.
	let notifyParent: (message: string) => void = () => {};

	const manager = new SubagentManager({
		capacity: numberFromEnv("PI_SUBAGENTS_CAPACITY", DEFAULT_CAPACITY),
		maxDepth: numberFromEnv("PI_SUBAGENTS_MAX_DEPTH", DEFAULT_MAX_DEPTH),
		onDescriptor: (descriptor) => persist(descriptor),
		onCompleted: (result) => {
			const head = result.stopReason === "completed" ? "子代理完成" : `子代理结束（${result.errorCode ?? result.stopReason}）`;
			const text = result.text.length > 400 ? `${result.text.slice(0, 400)}…` : result.text;
			notifyParent(`${head} ${result.childId}: ${text}`);
		},
	});

	const makeFactory = (ctx: ExtensionContext): SessionFactory =>
		createSessionFactory(ctx, (ctx.sessionManager as unknown as { sessionFile?: string }).sessionFile);

	const spawnProvider = (ctx: ExtensionContext) =>
		new LocalProvider({
			name: "spawn",
			factory: makeFactory(ctx),
			inheritsParentContext: false,
			resolveComposition: (request) => resolveComposition(ctx, request.profile, request.model),
		});

	const forkProvider = (ctx: ExtensionContext) =>
		new LocalProvider({
			name: "fork",
			factory: makeFactory(ctx),
			inheritsParentContext: true,
			resolveComposition: (request) => resolveComposition(ctx, request.profile, request.model),
		});

	// 模型靠这段描述知道有哪些 profile 可点名——不列出来，内置永远
	// 没人用（文件系统之外它看不见任何清单）。注册期同步扫盘；PF 每
	// 会话独立 spawn pi（cwd = 项目根），新建 profile 下次会话可见。
	// 清单带 frontmatter 摘要（description），模型按意图挑而不是盲猜 id。
	const profiles = listProfilesSync(defaultProfilePaths(process.cwd()));
	pi.registerTool({
		name: TOOL_NAME,
		label: "Subagent",
		description: delegationDescription({
			inheritsParentContext: false,
			allowedProfiles: profiles.map((p) => p.id),
			profileSummaries: profiles,
		}),
		// promptSnippet 进系统提示词的常驻工具清单（`- agent: …` 行）——这行
		// 是模型决定"要不要用这个工具"的第一触点，必须带政策不能只带机制
		//（实测纯机械说明 = 模型从不主动委派，见 buglist 第五批）
		promptSnippet:
			"Delegate self-contained research or multi-step tasks to a child agent and continue your own work while it runs",
		// dsh 对齐三件套之二（容量池早已移植，见 capacity.ts）：
		// ① isConcurrencySafe:true 的 pi 等价物——同帧多条委派真并行执行；
		// ② 系统提示词注入（dsh tool-subagent 末尾 section，pi 原生通道
		//    promptGuidelines → Rules 节）。
		promptGuidelines: [
			// dsh 原文（tool 名换成我们的 agent）
			"Start independent delegations with `agent` together in one assistant message and continue useful work while they run.",
			// PF 追加：点名前也该主动派——自包含子任务不占主线
			"Prefer `agent` for self-contained subtasks (research, code search, multi-step checks) instead of doing them inline.",
		],
		// ③ 同帧并行执行标记
		executionMode: "parallel",
		parameters: DELEGATION_PARAMETERS as unknown as never,
		// 045 M2.1：声明后 codemode 脚本拿 structuredContent（而非文本），
		// fan-out 结果可直接编程聚合。
		outputSchema: {
			type: "object",
			properties: {
				childId: { type: "string" },
				text: { type: "string" },
				status: { type: "string", description: "started (background) | completed | error" },
				stopReason: { type: "string" },
				usage: { type: "object" },
				durationMs: { type: "number" },
				errorCode: { type: "string" },
			},
			required: ["childId"],
		} as unknown as never,
		exposure: "direct",
		namespace: {
			name: "subagents",
			description: "Delegate work to child agents",
			instructions:
				"Each child runs in its own pi session. Ask for independent work in parallel by issuing several calls in one turn.",
		},
		async execute(
			toolCallId: string,
			params: Record<string, unknown>,
			_signal: AbortSignal | undefined,
			onUpdate: ((partial: unknown) => void) | undefined,
			ctx: ExtensionContext,
		) {
			const depth = currentDepthOf(ctx);
			const parentId = (ctx.sessionManager as unknown as { sessionFile?: string }).sessionFile ?? "draft";
			const result = await runDelegation(
				{
					manager,
					providerName: "spawn",
					inheritsParentContext: false,
				},
				{
					prompt: String(params.prompt ?? ""),
					profile: typeof params.profile === "string" ? params.profile : undefined,
					model: typeof params.model === "string" ? params.model : undefined,
					cwd: typeof params.cwd === "string" ? params.cwd : undefined,
					label: typeof params.label === "string" ? params.label : undefined,
					maxloop: typeof params.maxloop === "number" ? params.maxloop : undefined,
					background: params.background === true,
				},
				{
					parentId,
					depth,
					cwd: ctx.cwd,
					signal: ctx.signal,
					parentToolCallId: toolCallId,
				},
				onUpdate ? (partial) => onUpdate(partial as never) : undefined,
			);
			return result as never;
		},
	});

	// ---- 控制工具（045 M3.3；schema 复用：全局一份，父/可续子共用语义）----

	pi.registerTool({
		name: "subagent_send",
		label: "Subagent Send",
		description:
			"Send a follow-up message to a running or parked child agent (only local children accept input; external engines run once).",
		promptSnippet: "Send more work to a running child agent",
		parameters: {
			type: "object",
			properties: {
				childId: { type: "string", description: "Child id, e.g. sub-1." },
				message: { type: "string", description: "Follow-up message for the child." },
			},
			required: ["childId", "message"],
		} as unknown as never,
		exposure: "direct",
		namespace: { name: "subagents" },
		async execute(_toolCallId: string, params: Record<string, unknown>) {
			const childId = String(params.childId ?? "");
			const activation = manager.activation(childId);
			if (!activation) {
				return {
					content: [{ type: "text", text: `PROVIDER_UNKNOWN: unknown child ${childId}` }],
					structuredContent: { errorCode: "PROVIDER_UNKNOWN" },
					details: { errorCode: "PROVIDER_UNKNOWN" },
					isError: true,
				} as never;
			}
			try {
				await activation.send(String(params.message ?? ""));
				return {
					content: [{ type: "text", text: `delivered to ${childId}` }],
					structuredContent: { childId, delivered: true },
					details: { childId },
				} as never;
			} catch (error) {
				const code = SubagentError.is(error) ? error.code : "ADMISSION_CLOSED";
				return {
					content: [{ type: "text", text: `${code}: ${(error as Error).message}` }],
					structuredContent: { errorCode: code },
					details: { errorCode: code },
					isError: true,
				} as never;
			}
		},
	});

	pi.registerTool({
		name: "subagent_interrupt",
		label: "Subagent Interrupt",
		description:
			"Interrupt a child agent. Running local children stop their current turn (queued input and their own children are kept); settled children are a no-op.",
		promptSnippet: "Stop a child agent's current work",
		parameters: {
			type: "object",
			properties: { childId: { type: "string" } },
			required: ["childId"],
		} as unknown as never,
		exposure: "direct",
		namespace: { name: "subagents" },
		async execute(_toolCallId: string, params: Record<string, unknown>) {
			const childId = String(params.childId ?? "");
			try {
				const outcome = await manager.interruptChild(childId);
				return {
					content: [{ type: "text", text: `${childId}: ${outcome}` }],
					structuredContent: { childId, outcome },
					details: { childId, outcome },
				} as never;
			} catch (error) {
				const code = SubagentError.is(error) ? error.code : "PROVIDER_UNKNOWN";
				return {
					content: [{ type: "text", text: `${code}: ${(error as Error).message}` }],
					structuredContent: { errorCode: code },
					details: { errorCode: code },
					isError: true,
				} as never;
			}
		},
	});

	pi.registerTool({
		name: "subagent_list",
		label: "Subagent List",
		description: "List this session's child agents with id, phase, label, provider and depth.",
		promptSnippet: "List delegated child agents",
		parameters: { type: "object", properties: {} } as unknown as never,
		exposure: "direct",
		namespace: { name: "subagents" },
		async execute(
			_toolCallId: string,
			_params: Record<string, unknown>,
			_signal: AbortSignal | undefined,
			_onUpdate: unknown,
			ctx: ExtensionContext,
		) {
			const parentId = (ctx.sessionManager as unknown as { sessionFile?: string }).sessionFile ?? "draft";
			const children = manager.list(parentId).map((d) => ({
				childId: d.childId,
				phase: d.phase,
				label: d.label,
				provider: d.provider,
				depth: d.depth,
				profile: d.profile,
				stopReason: d.stopReason,
			}));
			return {
				content: [{ type: "text", text: JSON.stringify(children) }],
				structuredContent: { children },
				details: { children },
			} as never;
		},
	});

	pi.on("session_start", async (_event, ctx) => {
		// Re-register providers for this session's context (the factory closes over it).
		manager.registry.unregister("spawn");
		manager.registry.unregister("fork");
		manager.registry.register(spawnProvider(ctx));
		manager.registry.register(forkProvider(ctx));
		persist = (descriptor) => {
			try {
				pi.appendEntry(DESCRIPTOR_TYPE, descriptor);
			} catch {
				// Persistence is best-effort: a closed session must not break delegation.
			}
		};

		notifyParent = (message: string) => {
			try {
				void ctx.ui.notify(message, "info");
			} catch {
				// Notifications are best-effort (headless/JSON modes have no UI).
			}
		};

		const inFlight = readDescriptors(ctx);
		if (inFlight.length > 0) {
			// Orphan reconciliation: the host going away counts as an abort
			// (crash vs cancel is refined by agent_settled.aborted on live runs).
			const reconciled = reconcileOnStart(inFlight, { aborted: true });
			for (const descriptor of reconciled) persist(descriptor);
		}
	});

	pi.on("session_shutdown", async () => {
		await manager.shutdown("session shutdown");
	});
}

function readDescriptors(ctx: ExtensionContext): SubagentDescriptor[] {
	const manager = ctx.sessionManager as unknown as { getBranch?: () => unknown[] };
	const entries = manager.getBranch?.() ?? [];
	const out: SubagentDescriptor[] = [];
	for (const entry of entries) {
		const record = entry as { type?: string; customType?: string; data?: unknown };
		if (record.type !== "custom" || record.customType !== DESCRIPTOR_TYPE) continue;
		try {
			out.push(parseDescriptor(record.data));
		} catch {
			// A descriptor we cannot read is ignored, never treated as a live child.
		}
	}
	return out;
}

/**
 * Resolve the profile's contribution to one child's setup.
 *
 * The `tools` tier becomes an allowlist here (M4.5): `inherit` stays `undefined`
 * so the child keeps the delegator's own assembly, while the three concrete
 * tiers expand to whitelists. `model` passed by the delegator wins over the
 * profile's.
 */
async function resolveComposition(
	ctx: ExtensionContext,
	profileId: string | undefined,
	model: string | undefined,
): Promise<ResolvedComposition> {
	if (!profileId) return { model };
	const profile = await loadProfile(profileId, defaultProfilePaths(ctx.cwd));
	const tools = profile.tools && profile.tools !== "inherit" ? [...TOOL_TIER_WHITELISTS[profile.tools]] : undefined;
	return {
		systemPrompt: profile.systemPrompt,
		appendSystemPrompt: profile.appendSystemPrompt,
		tools,
		model: model ?? profile.model,
		injectAgentsMd: profile.injectAgentsMd,
		maxTurns: profile.maxTurns,
	};
}
