// 031-1 探针伴生扩展（仅探针用，不随包分发）：读/戳 pi.getActiveTools ·
// pi.setActiveTools，结果走 setStatus("pf-probe", JSON) 广播给驱动侧断言。
export default function probeToolsExtension(pi) {
	const emit = (ctx) => {
		try {
			let active;
			try {
				active = pi.getActiveTools();
			} catch (e) {
				active = { err: String(e) };
			}
			ctx.ui.setStatus("pf-probe", JSON.stringify(active));
		} catch (e) {
			try {
				ctx.ui.setStatus("pf-probe", JSON.stringify({ emitErr: String(e) }));
			} catch {
				/* 广播本身失败则驱动侧只会看到超时 */
			}
		}
	};
	pi.registerCommand("probe-tools", {
		description: "probe: broadcast active tool names",
		handler: async (_args, ctx) => emit(ctx),
	});
	pi.registerCommand("probe-add", {
		description: "probe: setActiveTools(...) +present_plan",
		handler: async (_args, ctx) => {
			try {
				const active = pi.getActiveTools();
				if (!active.includes("present_plan")) {
					pi.setActiveTools([...active, "present_plan"]);
				}
			} catch {
				/* 让 emit 报 err 或呈现旧态 */
			}
			emit(ctx);
		},
	});
	pi.registerCommand("probe-drop", {
		description: "probe: setActiveTools(...) -present_plan",
		handler: async (_args, ctx) => {
			try {
				const active = pi.getActiveTools();
				if (active.includes("present_plan")) {
					pi.setActiveTools(active.filter((n) => n !== "present_plan"));
				}
			} catch {
				/* 同上 */
			}
			emit(ctx);
		},
	});
}
