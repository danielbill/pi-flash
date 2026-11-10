/**
 * pf-subagents — extension entry.
 *
 * Wires the dsh-subagent call mechanics (manager, registry, capacity, descriptor,
 * local providers) onto pi. Nothing here re-implements pi's own primitives:
 * the child is a real pi session, the result is a real tool result, and usage is
 * accounted by pi's nested-call bookkeeping.
 *
 * M1 scope: local `spawn` and `fork` providers, dispatch-and-return delivery, depth guard,
 * orphan reconciliation. Codemode fan-out and worktrees come later (see 045).
 */

import type { ExtensionAPI, ExtensionContext } from "@earendil-works/pi-coding-agent";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";

import type { ChildSession, ResolvedComposition, SessionFactory, SessionFactoryOptions } from "./backends/local.ts";
import { LocalProvider } from "./backends/local.ts";
import { COMPLETION_TYPE, composeCompletion, settledEntry } from "./completion.ts";
import { SubagentError, SubagentErrorCode } from "./errors.ts";
import { SubagentManager } from "./manager.ts";
import { TOOL_TIER_WHITELISTS, defaultProfilePaths, listProfilesSync, loadProfile } from "./profile.ts";
import {
	applyModuleConfig,
	boundFromScalar,
	FALLBACK_MODULE_CONFIG,
	loadModuleConfig,
	readSubagentsProjection,
	type ModuleConfig,
	type ModuleConfigReadResult,
	type ModuleConfigSyncResult,
} from "./module-config.ts";
import { DELEGATION_PARAMETERS, delegationDescription, runDelegation } from "./tool.ts";

// 044-0 模块设置：容量/深度/轮数/默认模型全部来自 `<profiles dir>/config.json`
// （见 module-config.ts）。代码不写死天花板——上下界与值同源，改文件即生效。
const TOOL_NAME = "agent";
/// Must match `SUBAGENT_CHILD_MARKER` in `pi-link/src/sessions.rs`: the side-bar
/// filter keys on this custom entry, never on `parentSession`.
const CHILD_MARKER_TYPE = "subagent/child";

/** Session file -> delegation depth, so a child that reloads this extension still knows its depth. */
const childDepths = new Map<string, number>();

/**
 * Sync the module settings (capacity / maxDepth) from the subagent module
 * config (`<profiles dir>/config.json`).
 *
 * env 只在 pi 进程 spawn 那一刻定格（进程级快照），PF 会话常驻——用户改设置
 * 后不重启 app 就永远命中旧值（真机 2026-11-06：设 3 照跑 4 并发）。所以这里
 * **每次委派入口实时重读** config.json：改完即生效，无需重启。上下界也来自
 * 同一份文件（见 module-config.ts），代码不再写死天花板。读失败（缺文件/坏
 * JSON）退回 FALLBACK_MODULE_CONFIG，不阻断委派。
 */
/**
 * Sync the module settings from the projection → legacy fallback chain
 * （001-2 §4，顺序固定）：
 *
 * 1. 投影 `ext-context.json` 读到且 version 匹配 → capacity / maxDepth /
 *    enabled 以投影为准（maxTurns / model 沿用 legacy：它们是文件政策，
 *    不在 PF 设置里）；
 * 2. 投影在但坏档 / 版本不匹配 → **保持现值**（绝不覆盖正在跑的上限），
 *    problem 上报；
 * 3. 投影缺失（首启 / 非 PF 宿主）→ legacy `agents/config.json`；
 * 4. 再无 → 扩展内置 FALLBACK。
 *
 * env 只在 pi 进程 spawn 那一刻定格，PF 会话常驻——每次委派入口实时重读，
 * 改完即生效，无需重启。
 */
export function syncModuleConfig(
	manager: SubagentManager,
	profilesDir: string = defaultProfilePaths(process.cwd()).globalDir,
	dataDir: string | undefined = pfDataDir(),
): ModuleConfigSyncResult {
	const projection = readSubagentsProjection(dataDir);
	if (projection.state === "invalid") {
		return {
			config: FALLBACK_MODULE_CONFIG,
			path: projection.path,
			problem: `ext-context.json: ${projection.problem}（保持现值，未覆盖在跑上限）`,
		};
	}
	const legacy = loadModuleConfig(profilesDir);
	if (projection.state === "ok") {
		const config: ModuleConfig = {
			capacity: boundFromScalar(projection.value.capacity, legacy.config.capacity),
			maxDepth: boundFromScalar(projection.value.max_depth, legacy.config.maxDepth),
			maxTurns: legacy.config.maxTurns,
		};
		applyModuleConfig(manager, config);
		return { config, path: projection.path, problem: legacy.problem, enabled: projection.value.enabled };
	}
	// 无投影：legacy 链（缺/坏 = FALLBACK + problem）
	if (!legacy.problem) applyModuleConfig(manager, legacy.config);
	return { ...legacy, enabled: undefined };
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
		// Cold resume (044-5 §4.6): open the child's own session instead of creating
		// one. The header, cwd and parent link are already in that file, so the
		// child comes back with its whole history — dsh parity (`coldResume`).
		const resumeFile = options.resumeSessionFile;
		// Children live beside the parent so hosts can find and open them, and the
		// header records the parent link (045: parentSession).
		const sessionManager = resumeFile
			? sdk.SessionManager.open(resumeFile, parent.getSessionDir?.())
			: sdk.SessionManager.create(options.cwd, parent.getSessionDir?.(), {
					parentSession: parentFile,
				});
		// A resumed session keeps the cwd it was started in: the transcript's tool
		// results are relative to it, and switching cwd mid-conversation would make
		// old references dangle.
		const cwd = resumeFile
			? ((sessionManager as { getCwd?: () => string }).getCwd?.() ?? options.cwd)
			: options.cwd;
		// Marker extension: the child stamps its own file with a `subagent/child`
		// custom entry at session_start. Session-file filtering (pi-link/sessions.rs)
		// keys on THIS marker, never on `parentSession` — pi-native forks carry that
		// header too and must keep their sidebar rows.
		// agentDir is mandatory for DefaultResourceLoader (resolvePath chokes on
		// undefined on Windows); reuse the parent's own agent dir.
		const loader = new sdk.DefaultResourceLoader({
			cwd,
			agentDir: sdk.getAgentDir(),
			// Persona from the profile. `systemPrompt` fully replaces pi's default; the
			// body (`appendSystemPrompt`) adds to it. Both were previously parsed but
			// never reaching the loader — M4.3 fixes that.
			...(options.systemPrompt ? { systemPrompt: options.systemPrompt } : {}),
			...(options.appendSystemPrompt ? { appendSystemPrompt: [options.appendSystemPrompt] } : {}),
			// `injectAgentsMd: false` suppresses project context files (AGENTS.md) —
			// dsh's agent-instructions loader has no off switch, we do (044 §3).
			...(options.injectAgentsMd === false ? { noContextFiles: true } : {}),
			// full+ 档的 pi-ext 选择（044-0）：清单即 -e，挂进 loader。
			...(options.extensions?.length ? { additionalExtensionPaths: options.extensions } : {}),
			// skills 定制（044-0）：清单缺省 = 发现集原样；有清单 = 按名字过滤
			// （skillsOverride 在 discovery 之后、装配之前生效）。
			...(options.skills?.length
				? {
						skillsOverride: (base: { skills: Array<{ name?: string; id?: string }>; diagnostics: unknown[] }) => ({
							skills: base.skills.filter((s) => {
								const id = s.name ?? s.id ?? "";
								return options.skills!.includes(id);
							}),
							diagnostics: base.diagnostics,
						}),
					}
				: {}),
			extensionFactories: [
				// 冷恢复不装标记扩展：会话文件里已经有一条 `subagent/child`，重复
				// append 会把侧栏过滤与深度恢复的输入变脏（044-5 §4.6）。
				...(resumeFile
					? []
					: [
							{
								name: "subagent-child-marker",
								hidden: true,
								factory: (childPi: { on(event: string, handler: () => void): void; appendEntry(type: string, data?: unknown): void }) => {
									childPi.on("session_start", () => {
										try {
											childPi.appendEntry(CHILD_MARKER_TYPE, {
												provider: options.providerName,
												depth: options.depth,
											});
										} catch {
											// Missing marker degrades to "child shows in the list"; never fatal.
										}
									});
								},
							},
						]),
				// 045 M2.5：子会话是“一等公民 pi session”——codemode / tool_search /
				// MCP 显式装配（SDK 会话默认不加载内建扩展，pi-web 就栽在这里）。
				// 官方姿势见 examples/sdk/14-codemode-mcp.ts。
				sdk.createCodemodeExtension?.({ mode: "on" }),
				sdk.createToolSearchExtension?.(),
				// full+ 档的 mcp 服务选择（044-0）：有清单 = 自带 loadConfig 按
				// 名字过滤（global + 可信项目两份 mcp.json 读后筛）；缺省 = 默认装配。
				options.mcpServers?.length
					? sdk.createMcpExtension?.({
							loadConfig: (ctx: { cwd: string; isProjectTrusted?: () => boolean }) => {
								const agentDir = sdk.getAgentDir();
								const servers: Array<Record<string, unknown>> = [];
								const errors: string[] = [];
								const read = (file: string, scope: string) => {
									try {
										const raw = JSON.parse(readFileSync(file, "utf8"));
										const map = raw?.mcpServers;
										if (map && typeof map === "object") {
											for (const [name, config] of Object.entries(map)) {
												servers.push({ name, config, source: file, scope });
											}
										}
									} catch (e) {
										if ((e as NodeJS.ErrnoException).code !== "ENOENT") {
											errors.push(`${file}: ${(e as Error).message}`);
										}
									}
								};
								read(join(agentDir, "mcp.json"), "global");
								const trusted = ctx.isProjectTrusted?.() ?? true;
								if (trusted) read(join(ctx.cwd, ".pi", "mcp.json"), "project");
								return {
									servers: servers.filter((s) => options.mcpServers!.includes(String(s.name))),
									errors,
								};
							},
						})
					: sdk.createMcpExtension?.(),
			].filter((f): f is NonNullable<typeof f> => Boolean(f)),
		});
		await loader.reload();
		// codemode / tool_search 注册为 inactive；用 defaultTools 的 `+name`
		// 追加语义激活。与 profile 白名单（createAgentSession 的 tools 参数）
		// 相互独立：白名单裁模型面工具，codemode 是子会话基础设施。
		const settingsManager = sdk.SettingsManager.create(cwd, sdk.getAgentDir());
		settingsManager.applyOverrides?.({ defaultTools: ["+codemode", "+tool_search"] });
		const { session } = await sdk.createAgentSession({
			cwd,
			model,
			sessionManager,
			settingsManager,
			resourceLoader: loader,
			// 思考强度（2026-11-07 子代理表单）：profile 有 `thinking` 才下发，
			// 缺省 = auto = 落 pi 自己的默认档
			...(options.thinkingLevel ? { thinkingLevel: options.thinkingLevel } : {}),
			// chat-only 档（044-0 六档）：noTools all = 一个工具都不开
			...(options.noTools ? { noTools: "all" } : {}),
			...(options.tools && options.tools.length > 0 ? { tools: options.tools } : {}),
		});
		// SDK sessions don't auto-bind the extension runtime: without this the
		// marker's session_start never fires and MCP never connects.
		await (session as unknown as { bindExtensions(bindings: unknown): Promise<void> }).bindExtensions({});
		const sessionFile = session.sessionFile;
		if (sessionFile) {
			childDepths.set(sessionFile, currentDepthOf(ctx) + 1);
		}
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
		/** Cold resume: open an existing session file (cwd comes from its header). */
		open(path: string, sessionDir?: string, cwdOverride?: string): unknown;
	};
	DefaultResourceLoader: new (options?: {
		cwd?: string;
		agentDir?: string;
		extensionFactories?: unknown[];
		[key: string]: unknown;
	}) => { reload(): Promise<void> };
	SettingsManager: {
		create(cwd?: string, agentDir?: string): {
			applyOverrides?(overrides: Record<string, unknown>): void;
		};
	};
	createCodemodeExtension?(options?: Record<string, unknown>): unknown;
	createToolSearchExtension?(): unknown;
	createMcpExtension?(options?: Record<string, unknown>): unknown;
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

export function currentDepthOf(ctx: ExtensionContext): number {
	const file = (ctx.sessionManager as unknown as { sessionFile?: string }).sessionFile;
	if (!file) return 0;
	return childDepths.get(file) ?? 0;
}

/**
 * Restore this session's delegation depth from its own `subagent/child` marker.
 *
 * dsh reads depth from the agent header (`depth.ts`: absence = top-level depth
 * zero); our equivalent header is the marker persisted in the session file. The
 * in-memory map alone is enough while the child lives in this process, but a
 * child session the host reopens would otherwise look like a root.
 */
export function restoreChildDepth(ctx: ExtensionContext): void {
	const file = (ctx.sessionManager as unknown as { sessionFile?: string }).sessionFile;
	if (!file) return;
	const entries = (ctx.sessionManager as unknown as { getBranch?: () => unknown[] }).getBranch?.() ?? [];
	for (const entry of entries) {
		const record = entry as { type?: string; customType?: string; data?: unknown };
		if (record.type !== "custom" || record.customType !== CHILD_MARKER_TYPE) continue;
		const data = record.data as { depth?: unknown } | undefined;
		if (typeof data?.depth === "number") childDepths.set(file, data.depth);
	}
}

export default function piSubagents(pi: ExtensionAPI): void {
	// Parent-delivery completion hook (every delegation by default): routed to the live
	// session's ui.notify; rebound on every session_start.
	let notifyParent: (message: string) => void = () => {};
	// Parent-delivery result: background children settle after the spawning turn,
	// so the outcome is only a toast unless it is also pushed into the session.
	// `deliverParent` injects it as a custom message the model can read.
	let deliverParent: (content: string, details: Record<string, unknown>) => void = () => {};

	const initialConfig = loadModuleConfig(defaultProfilePaths(process.cwd()).globalDir).config;
	const manager = new SubagentManager({
		capacity: initialConfig.capacity.default,
		maxDepth: initialConfig.maxDepth.default,
		onCompleted: (result) => {
			const completion = composeCompletion(result);
			// 卡片自更新（044-0 B4）：终态先落成会话**自定义条目**——pi 当帧广播
			// `entry_appended`，不受父会话转向（steer）排队影响；宿主据此即时把
			// 派发卡片切到终态，无需等父 agent 下一轮把 completion 消息读进会话。
			try {
				const settled = settledEntry(result);
				pi.appendEntry(settled.customType, settled.data);
			} catch {
				// 条目只是宿主显示通道；失败仍有 completion 消息与 toast 兜底。
			}
			notifyParent(completion.toast);
			deliverParent(completion.content, completion.details);
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
			// B3①：容量上限进描述——模型按它节流，被拒也知道是等槽位重派
			// 而不是放弃。
			capacity: manager.capacity,
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
				status: { type: "string", description: "started | completed | error" },
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
			// 模块设置实时同步（容量/深度/开关，见 syncModuleConfig）；
			// 开关关着直接拒绝（001-1 §5：运行中 pi 进程的工具清单要到下次
			// spawn 才变，投影 enabled 让当轮委派即停）
			const synced = syncModuleConfig(manager, undefined, pfDataDir());
			if (synced.enabled === false) {
				throw new SubagentError(
					SubagentErrorCode.MODULE_DISABLED,
					"子代理模块已在 PF 设置中停用（subagents.enabled=false）",
				);
			}
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
					resumeFrom: typeof params.resumeFrom === "string" ? params.resumeFrom : undefined,
					blocking: params.blocking === true,
				},
				{
					parentId,
					parentSessionFile: (ctx.sessionManager as unknown as { sessionFile?: string }).sessionFile,
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
		description: "List this session's child agents with id, phase, label, provider and depth. This is a diagnostic snapshot, NOT a polling API: you will be notified when a subagent finishes, so there is no need to check its status after dispatching.",
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
				content: [
					{ type: "text", text: "Snapshot only — do not poll: finished children notify you on their own." },
					{ type: "text", text: JSON.stringify(children) },
				],
				structuredContent: { children },
				details: { children },
			} as never;
		},
	});

	pi.on("session_start", async (_event, ctx) => {
		// A child session reopened by the host has no in-memory depth yet.
		restoreChildDepth(ctx);
		// Re-register providers for this session's context (the factory closes over it).
		manager.registry.unregister("spawn");
		manager.registry.unregister("fork");
		manager.registry.register(spawnProvider(ctx));
		manager.registry.register(forkProvider(ctx));

		notifyParent = (message: string) => {
			try {
				void ctx.ui.notify(message, "info");
			} catch {
				// Notifications are best-effort (headless/JSON modes have no UI).
			}
		};

		// dsh parity (044-5 §4): an idle parent is woken into a new turn; a streaming
		// parent gets the notice steered into the current turn instead of queued.
		deliverParent = (content, details) => {
			try {
				pi.sendMessage(
					{ customType: COMPLETION_TYPE, content, display: true, details },
					ctx.isIdle() ? { triggerTurn: true } : { deliverAs: "steer" },
				);
			} catch {
				// Delivery is best-effort: closed/headless sessions keep the toast only.
			}
		};
	});

	pi.on("session_shutdown", async () => {
		await manager.shutdown("session shutdown");
	});
}

/**
 * Resolve the profile's contribution to one child's setup.
 *
 * The `tools` tier becomes an allowlist here (M4.5): `inherit` stays `undefined`
 * so the child keeps the delegator's own assembly, while the three concrete
 * tiers expand to whitelists. `model` passed by the delegator wins over the
 * profile's.
 */
/** PF data directory from the host-provided env. 001-2 §5：白名单主键 =
 * `PI_FLASH_DIR`（PF 数据目录直给）；`PI_FLASH_APP_SETTINGS`（旧版宿主给的
 * 设置文件绝对路径，取 dirname）保留**读取兼容一版**。都缺 = 非 PF 宿主，
 * 走 legacy 链。 */
function pfDataDir(): string | undefined {
	const dir = process.env.PI_FLASH_DIR;
	if (dir) return dir;
	const settings = process.env.PI_FLASH_APP_SETTINGS;
	if (!settings) return undefined;
	const i = Math.max(settings.lastIndexOf("/"), settings.lastIndexOf("\\"));
	return i > 0 ? settings.slice(0, i) : undefined;
}

async function resolveComposition(
	ctx: ExtensionContext,
	profileId: string | undefined,
	model: string | undefined,
): Promise<ResolvedComposition> {
	// 每次组合都重读：模型兜底与轮数预算同属 module config
	const config = loadModuleConfig(defaultProfilePaths(ctx.cwd).globalDir).config;
	// 模块级默认模型已废除（2026-11-10）：不点名 profile = 继承父会话模型
	if (!profileId) return { model };
	const profile = await loadProfile(profileId, defaultProfilePaths(ctx.cwd));
	const tier = profile.tools && profile.tools !== "inherit" ? profile.tools : undefined;
	return {
		systemPrompt: profile.systemPrompt,
		appendSystemPrompt: profile.appendSystemPrompt,
		// chat-only 档不发白名单（发 noTools 全关）；其余档位发对应清单
		tools: tier && tier !== "chat-only" ? [...TOOL_TIER_WHITELISTS[tier]] : undefined,
		noTools: tier === "chat-only" ? true : undefined,
		thinkingLevel: profile.thinking,
		model: model ?? profile.model,
		injectAgentsMd: profile.injectAgentsMd,
		maxTurns: profile.maxTurns ?? config.maxTurns,
		extensions: profile.extensions,
		mcpServers: profile.mcpServers,
		skills: profile.skills,
	};
}
