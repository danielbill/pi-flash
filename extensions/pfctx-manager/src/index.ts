/**
 * pfctx-manager — PF 动态自管理工具（001-4）。
 *
 * Agent 经本工具读写 Pi-Flash 设置，**与设置界面同一条执行链**：
 * set 不直接写文件，而是投递 `pfctx-manager-op` 自定义条目（pi 当帧经
 * `entry_appended` 推给 PF 宿主），宿主逐 op 调设置页同款函数（含主题
 * token 重映射、字号 sync、子代理重绑等副作用），落盘与投影随之联动。
 *
 * query 读 `<PF数据目录>/app-settings.json`（落盘态；宿主内存与落盘的
 * 合并窗口 ≤50ms）并附全部参数目录。
 *
 * 变更审计：`<PF数据目录>/pfctx.log`——`[agent]` 宿主执行行、`[write]`
 * 落盘行、`[watch]/[reload]` 外部变更行。
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const ENTRY_TYPE = "pfctx-manager-op";
const SETTINGS_FILE = "app-settings.json";
const LOG_FILE = "pfctx.log";

/** pi 工具结果契约：content 必须是块数组（纯字符串会让 pi 的 content.some 崩）。 */
type TextBlock = { type: "text"; text: string };
function asText(text: string): TextBlock[] {
	return [{ type: "text", text }];
}

/** 设置目录（单一事实源 = 宿主各设置页 setter；此处手工同步，见 001-1 §8）。 */
interface CatalogEntry {
	/** 含义（给 agent 看） */
	readonly desc: string;
	/** 可设值说明（枚举/范围） */
	readonly values: string;
	/** 类型闸：number / boolean / string / object（字体 {family,size}） */
	readonly type: "number" | "boolean" | "string" | "object";
	/** 数字范围（含端点）；枚举/自由串为 undefined */
	readonly min?: number;
	readonly max?: number;
	/** 枚举值 */
	readonly enum?: readonly string[];
}

const CATALOG: Record<string, CatalogEntry> = {
	// ---- subagents（044-0；容量双写 agents/config.json，扩展下次委派生效）----
	"subagents.capacity": {
		desc: "子代理同帧并发上限",
		values: "2..10 整数",
		type: "number",
		min: 2,
		max: 10,
	},
	"subagents.enabled": {
		desc: "子代理模块开关（关 = agent 委派拒绝；切换会重绑空闲会话进程）",
		values: "boolean",
		type: "boolean",
	},
	// ---- chat（界面页/090/023/024）----
	"chat.send_mode": {
		desc: "运行中发送方式：queue=扣本地队列逐条发，steer=当轮插队",
		values: '"queue" | "steer"',
		type: "string",
		enum: ["queue", "steer"],
	},
	"chat.show_thinking": {
		desc: "展示思考（开 = 思考块默认展开全文）",
		values: "boolean",
		type: "boolean",
	},
	"chat.git_markers": {
		desc: "文件树 git 状态标识（M/A/D 徽标）",
		values: "boolean",
		type: "boolean",
	},
	"chat.md_live_preview": {
		desc: "Markdown 所见即所得装饰（已知回车问题未修，慎开）",
		values: "boolean",
		type: "boolean",
	},
	"chat.content_width": {
		desc: "会话内容列宽 px",
		values: "560..1200（步进 20）",
		type: "number",
		min: 560,
		max: 1200,
	},
	// ---- sessions（052/v54/v60）----
	"sessions.startup_restore": {
		desc: "启动恢复最后会话",
		values: "boolean",
		type: "boolean",
	},
	"sessions.load_window_days": {
		desc: "启动装载时间窗（天）",
		values: "7 | 14 | 30",
		type: "number",
		min: 7,
		max: 30,
	},
	"sessions.display_count": {
		desc: "项目默认显示会话数（psp 初始页大小）",
		values: "3..10 整数",
		type: "number",
		min: 3,
		max: 10,
	},
	"sessions.auto_collapse_hours": {
		desc: "启动自动折叠阈值（小时）",
		values: "24 | 72 | 168",
		type: "number",
		min: 24,
		max: 168,
	},
	"sessions.auto_clean_days": {
		desc: "会话自动清理天数——**启动时把超期非置顶会话移入回收站**，0=关",
		values: "0..100 整数",
		type: "number",
		min: 0,
		max: 100,
	},
	// ---- index（@ 文件索引）----
	"index.at_index_ttl_secs": {
		desc: "@ 文件索引缓存有效期（秒）",
		values: "1..3600 整数",
		type: "number",
		min: 1,
		max: 3600,
	},
	"index.at_index_max_projects": {
		desc: "@ 文件索引缓存项目数上限",
		values: "1..100 整数",
		type: "number",
		min: 1,
		max: 100,
	},
	// ---- models（042；两键齐生效，新会话 spawn 时以 --model 落）----
	"models.default_model": {
		desc: "默认模型（成对设置，格式 \"provider/model\"；置 null = 取消）",
		values: '"provider/model" | null',
		type: "string",
	},
	// ---- appearance（006；theme/lang 即时重映射，字体含字号 sync）----
	"appearance.theme": {
		desc: "主题 id（内置集见设置-界面页；常用 mist / rose / one-light / default / dark）",
		values: "string（未知 id 会被拒绝）",
		type: "string",
	},
	"appearance.lang": {
		desc: "界面语言",
		values: "0=简体中文 1=繁體中文 2=English",
		type: "number",
		min: 0,
		max: 2,
	},
	"appearance.session_font": {
		desc: "会话字体",
		values: '{"family": string, "size": 10..30}',
		type: "object",
	},
	"appearance.panel_font": {
		desc: "功能面板字体",
		values: '{"family": string, "size": 10..30}',
		type: "object",
	},
	"appearance.markdown_font": {
		desc: "文件/Markdown 字体",
		values: '{"family": string, "size": 10..30}',
		type: "object",
	},
	// ---- shell / updates ----
	"shell.autohide_icons": {
		desc: "dock 操作栏不常用 icon 常态透明",
		values: "boolean",
		type: "boolean",
	},
	"shell.sound": {
		desc: "通知提示音",
		values: "boolean",
		type: "boolean",
	},
	"updates.auto_update": {
		desc: "自动更新（关 = 只提示新版本不下载）",
		values: "boolean",
		type: "boolean",
	},
	"agent.self_debug": {
		desc: "本工具总闸（关 = 新会话不再注入本工具，并重绑空闲会话）",
		values: "boolean",
		type: "boolean",
	},
};

/** 非法项：恒不可改（目录外语义）。 */
const REJECTED: Record<string, string> = {
	"subagents.max_depth": "嵌套深度恒 1（禁止递归委派，2026-11-07 拍板）",
	"appearance.icon_theme": "图标主题无运行时消费者（占位参数）",
};

function dataDir(): string | undefined {
	const dir = process.env.PI_FLASH_DIR;
	return dir && dir.length > 0 ? dir : undefined;
}

function readSettings(dir: string): { text?: string; error?: string } {
	try {
		return { text: readFileSync(join(dir, SETTINGS_FILE), "utf8") };
	} catch (error) {
		const message = error instanceof Error ? error.message : String(error);
		return { error: message };
	}
}

/** 目录校验：逐 op 查键/类型/枚举/范围。返回错误清单（空 = 全部合法）。 */
export function validateOps(ops: Array<{ setting?: unknown; value?: unknown }>): string[] {
	const errors: string[] = [];
	for (const [i, op] of ops.entries()) {
		const setting = typeof op?.setting === "string" ? op.setting : "";
		const value = op?.value;
		if (REJECTED[setting] !== undefined) {
			errors.push(`ops[${i}] ${setting}: ${REJECTED[setting]}`);
			continue;
		}
		const entry = CATALOG[setting];
		if (!entry) {
			errors.push(
				`ops[${i}] ${setting || "(空)"}: 目录外参数。先 query 查看全部可用参数名`,
			);
			continue;
		}
		if (entry.type === "number") {
			if (typeof value !== "number" || !Number.isFinite(value)) {
				errors.push(`ops[${i}] ${setting}: 需要 number，得到 ${JSON.stringify(value)}`);
				continue;
			}
			if (entry.min !== undefined && value < entry.min) {
				errors.push(`ops[${i}] ${setting}: ${value} < 最小值 ${entry.min}`);
				continue;
			}
			if (entry.max !== undefined && value > entry.max) {
				errors.push(`ops[${i}] ${setting}: ${value} > 最大值 ${entry.max}`);
				continue;
			}
		} else if (entry.type === "boolean") {
			if (typeof value !== "boolean") {
				errors.push(`ops[${i}] ${setting}: 需要 boolean，得到 ${JSON.stringify(value)}`);
				continue;
			}
		} else if (entry.type === "object") {
			// 字体 {family, size}: 宿主 save_font→validate 会钳 10..30，这里挡明显坏形
			if (!value || typeof value !== "object" || Array.isArray(value)) {
				errors.push(`ops[${i}] ${setting}: 需要 {family, size} 对象，得到 ${JSON.stringify(value)}`);
				continue;
			}
			const obj = value as Record<string, unknown>;
			if (typeof obj["family"] !== "string" || obj["family"].length === 0) {
				errors.push(`ops[${i}] ${setting}: family 需要 string`);
				continue;
			}
			if (typeof obj["size"] !== "number" || !Number.isFinite(obj["size"])) {
				errors.push(`ops[${i}] ${setting}: size 需要 number`);
				continue;
			}
		} else if (entry.type === "string") {
			if (setting === "models.default_model") {
				// null = 取消默认模型
				if (value !== null && (typeof value !== "string" || value.indexOf("/") <= 0)) {
					errors.push(`ops[${i}] ${setting}: 需要 "provider/model" 或 null`);
					continue;
				}
			} else if (typeof value !== "string" || value.length === 0) {
				errors.push(`ops[${i}] ${setting}: 需要 string，得到 ${JSON.stringify(value)}`);
				continue;
			}
			if (entry.enum && typeof value === "string" && !entry.enum.includes(value)) {
				errors.push(
					`ops[${i}] ${setting}: 需要 ${entry.enum.map((e) => JSON.stringify(e)).join(" | ")}`,
				);
				continue;
			}
		}
	}
	return errors;
}

export default function pfctxManager(pi: ExtensionAPI): void {
	pi.registerTool({
		name: "pfctx_manager",
		label: "PF-settings-manager",
		description: [
			"Pi-Flash 动态自管理工具：查询并修改 PF 应用设置，效果与用户在设置界面操作完全一致（同一执行链，含即时生效与持久化）。",
			"",
			"用法：",
			'1. {action:"query"} — 返回全部参数目录（名称/含义/可设值）与当前值、pfctx.log 路径。首次先 query。',
			'2. {action:"set", ops:[{"setting":"subagents.capacity","value":6}]} — 批量修改。目录外参数/越界值会被拒绝；set 为异步受理，之后用 query 确认生效值。',
			"3. 所有变更记录在 pfctx.log（[agent] 宿主执行行 / [write] 落盘行 / [reload] 外部变更行），可用 bash tail 查看。",
			"",
			"注意：修改即持久生效并影响用户全局设置——仅在任务明确需要时修改，完成后考虑改回。",
		].join("\n"),
		// 注册级 allowlist 点名（spawn --tools）；与设置页同为常驻能力
		promptSnippet:
			"pfctx_manager: 查询/修改 Pi-Flash 应用设置（动态自管理，先 query 看目录）",
		parameters: {
			type: "object",
			properties: {
				action: {
					type: "string",
					enum: ["query", "set"],
					description: "query=查全部参数与当前值；set=批量修改",
				},
				ops: {
					type: "array",
					items: {
						type: "object",
						properties: {
							setting: { type: "string", description: "参数点路径，如 subagents.capacity" },
							value: { description: "目标值（类型/范围见 query 返回的目录）" },
						},
						required: ["setting", "value"],
					},
					description: "set 必填：修改清单（全成功才受理，任一非法整批拒绝）",
				},
			},
			required: ["action"],
		},
		async execute(
			_toolCallId: string,
			params: Record<string, unknown>,
			_signal: AbortSignal | undefined,
			_onUpdate: ((partial: unknown) => void) | undefined,
			_ctx: unknown,
		) {
			const dir = dataDir();
			if (!dir) {
				return {
					isError: true,
					content: asText("非 PF 宿主（缺 PI_FLASH_DIR env），本工具不可用"),
				};
			}

			const action = params.action;
			if (action === "query") {
				const read = readSettings(dir);
				if (read.error !== undefined) {
					return {
						isError: true,
						content: asText(
							`读 ${SETTINGS_FILE} 失败: ${read.error}（文件可能尚不存在——改动任一设置后会创建）`,
						),
					};
				}
				const lines = [
					`## 参数目录（${Object.keys(CATALOG).length} 项，点路径 → 含义 / 可设值）`,
					...Object.entries(CATALOG).map(
						([k, e]) => `- **${k}** — ${e.desc}；可设值: ${e.values}`,
					),
					`## 拒绝项（恒不可改）`,
					...Object.entries(REJECTED).map(([k, why]) => `- **${k}** — ${why}`),
					`## 当前值（${SETTINGS_FILE} 落盘态，与宿主内存合并窗口 ≤50ms）`,
					"```json",
					read.text ?? "",
					"```",
					`## 变更日志`,
					`${join(dir, LOG_FILE)}（[agent] 执行 / [write] 落盘 / [reload] 外部变更）`,
				];
				return { content: asText(lines.join("\n")) };
			}

			if (action === "set") {
				const raw = Array.isArray(params.ops) ? params.ops : [];
				const ops = raw as Array<{ setting?: unknown; value?: unknown }>;
				if (ops.length === 0) {
					return {
						isError: true,
						content: asText(
							'set 需要 ops 数组，如 [{"setting":"subagents.capacity","value":6}]',
						),
					};
				}
				const errors = validateOps(ops);
				if (errors.length > 0) {
					return {
						isError: true,
						content: asText(
							["整批拒绝（任一非法即不受理，未做任何修改）：", ...errors].join("\n"),
						),
					};
				}
				try {
					// 当帧广播 entry_appended → 宿主逐 op 调设置页同款函数（同链执行）
					pi.appendEntry(ENTRY_TYPE, {
						ops: ops.map((op) => ({ setting: op.setting, value: op.value })),
					});
				} catch (error) {
					const message = error instanceof Error ? error.message : String(error);
					return { isError: true, content: asText(`投递失败: ${message}`) };
				}
				const lines = [
					`已受理 ${ops.length} 项（异步执行，宿主将按设置界面同链逐项应用）：`,
					...ops.map((op) => `- ${op.setting} = ${JSON.stringify(op.value)}`),
					`确认方式：稍候 {action:"query"} 重读当前值；或查 ${join(dir, LOG_FILE)} 的 [agent]/[write] 行。`,
				];
				return { content: asText(lines.join("\n")) };
			}

			return { isError: true, content: asText('未知 action，需要 "query" 或 "set"') };
		},
	});
}
