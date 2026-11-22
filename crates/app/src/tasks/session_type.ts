/**
 * PF 会话类型标记扩展（046，随包分发：机制同 mode/plan_mode.ts——dev 直读
 * 本文件、release 落盘 ~/.pi-flash）。
 *
 * 职责单一：spawn env `PF_SESSION_TYPE=task` 的 pi 进程在 session_start 时
 * 落一条 custom entry（`pf/session-type`，data `{type:"task"}`）进会话文件，
 * 作为「会话 type 属性」（user/sub/task，2026-11-22 拍板）的 task 侧事实源。
 * user 会话（env 不带值）不落任何东西——无 marker = user，存量会话零迁移；
 * sub 复用 pf-subagents 已落的 `subagent/child` marker（单一事实源）。
 *
 * 为什么走扩展而不是 RPC：pi 的 rpc-mode 无 appendEntry 命令（rpc-mode.js
 * 全量 case 清单）；而 PF 扩展链路现成（本文件 + plan_mode.ts 同机制）。
 * 为什么落文件而不是 PF 台账：entry 随会话文件生死（052 过期删除连带清）、
 * 跨重启持久、pi 存储口径自洽——PF 侧零新增存储与清理点。
 *
 * 持久化走 ctx.sessionManager.appendCustomEntry（plan_mode.ts:85 同款：
 * pi.appendEntry 的 runtime action 在 RPC 模式命令 handler 里 unbound 会抛，
 * ctx.sessionManager 是当前会话本体，语义等价）。
 */

import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";

const ENTRY_TYPE = "pf/session-type";

export default function sessionTypeExtension(pi: ExtensionAPI): void {
	pi.on("session_start", async (_event, ctx) => {
		const t = (process.env.PF_SESSION_TYPE ?? "").trim().toLowerCase();
		if (t !== "task") return; // user（缺省）不落 entry；sub 由 pf-subagents 落自己的 marker
		try {
			// 幂等：已有标记不重复落（fork/冷恢复重放 session_start 的防御）
			const entries = ctx.sessionManager.getEntries() as Array<{
				type: string;
				customType?: string;
			}>;
			if (entries.some((e) => e.type === "custom" && e.customType === ENTRY_TYPE)) return;
			ctx.sessionManager.appendCustomEntry(ENTRY_TYPE, { type: "task" });
		} catch {
			// 落不了标记 = 该会话按 user 显形（降级无害，不阻断运行）
		}
	});
}
