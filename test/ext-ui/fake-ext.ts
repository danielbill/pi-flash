// test/ext-ui 假扩展：zi2s epic 台架的唯一真实扩展载体。
// 挂载方式见 run.sh：写 <PI_FLASH_DIR>/session-ext.json 把 draft-0 映射到
// 本文件绝对路径，spawn 拼 `-e` 显式挂载（项目级扩展在 RPC 无 UI 时不信任、
// 不加载——test/issue7-ext-command/README.md 实踩）。
// 全部命令只动 UI、不 sendMessage，不产生 agent run，不烧任何模型配额。
export default function (pi) {
	// setStatus 基线（zi2s.4）：quota 行文案照抄 pi-usage wire 实测
	pi.registerCommand("ext-status", {
		description: "Set status line (data present, render parity pending)",
		handler: async (_args, ctx) => {
			ctx.ui.setStatus("usage", "zai 61% ↻ 1h18m · 88% ↻ 6d14h");
			ctx.ui.notify("ext-status: set", "info");
		},
	});

	// setStatus 清空（zi2s.4）：statusText=null → 该 key 整行摘除
	pi.registerCommand("ext-status-clear", {
		description: "Clear the usage status line",
		handler: async (_args, ctx) => {
			ctx.ui.setStatus("usage", null);
			ctx.ui.notify("ext-status: cleared", "info");
		},
	});

	// setWidget 带 ANSI（zi2s.8）：pi rpc-mode 是真彩色主题，文本里带
	// \x1b[..m 转义码；PF 若不剥就原样渲染
	pi.registerCommand("ext-widget-ansi", {
		description: "Widget text containing ANSI escape codes",
		handler: async (_args, ctx) => {
			ctx.ui.setWidget("ansi", [
				"\x1b[31m█ RED\x1b[0m after-code plain",
				"plain line 2",
			]);
			ctx.ui.notify("ext-widget-ansi: set", "info");
		},
	});

	// editor 阻塞弹窗（zi2s.6）：prefill 多行文本；回执 notify 给 wire 断言
	pi.registerCommand("ext-editor", {
		description: "Blocking editor dialog with multiline prefill",
		handler: async (_args, ctx) => {
			const value = await ctx.ui.editor("ext editor title", "prefill L1\nprefill L2");
			ctx.ui.notify(`ext-editor: submitted ${JSON.stringify(value)}`, "info");
		},
	});

	// select 阻塞弹窗（zi2s.7）：5 项，键盘导航目标
	pi.registerCommand("ext-select", {
		description: "Blocking select dialog with 5 options",
		handler: async (_args, ctx) => {
			const value = await ctx.ui.select("Pick one", [
				"alpha",
				"beta",
				"gamma",
				"delta",
				"epsilon",
			]);
			ctx.ui.notify(`ext-select: picked ${JSON.stringify(value)}`, "info");
		},
	});

	// 长标题 select（回归 b9e2edce 的 600×600 双面板壳）：标题 40+ 行，
	// 旧版不限高会把 Close 顶出视口、弹窗关不掉
	pi.registerCommand("ext-bigselect", {
		description: "Select dialog with 40+ line title (600x600 shell regression)",
		handler: async (_args, ctx) => {
			const title = Array.from({ length: 40 }, (_, i) => `title line ${i + 1}`).join("\n");
			const value = await ctx.ui.select(title, ["opt A", "opt B", "opt C", "opt D", "opt E"]);
			ctx.ui.notify(`ext-bigselect: picked ${JSON.stringify(value)}`, "info");
		},
	});

	// notify 冒烟（收场链路哨兵）：handler 即返回，等待行应立即收场
	pi.registerCommand("ext-notify", {
		description: "Fire-and-forget notify, handler returns immediately",
		handler: async (_args, ctx) => {
			ctx.ui.notify("ext-notify: hello", "info");
		},
	});

	// setTitle（zi2s.9）：映射 OS 窗口标题（pi-web document.title parity）
	pi.registerCommand("ext-title", {
		description: "Set window title via ctx.ui.setTitle",
		handler: async (_args, ctx) => {
			ctx.ui.setTitle("PF ext title ✓");
			ctx.ui.notify("ext-title: set", "info");
		},
	});
}
