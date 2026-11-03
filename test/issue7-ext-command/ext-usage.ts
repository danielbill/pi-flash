// issue #7 复现扩展：模拟 /usage 类扩展——纯 UI（notify + setStatus + setWidget），
// 不调 pi.sendMessage，不产生任何 agent run。
export default function (pi) {
	pi.registerCommand("usage", {
		description: "Show usage dashboard (UI only, no agent run)",
		handler: async (args, ctx) => {
			ctx.ui.notify("usage: 128.4k tokens this week", "info");
			ctx.ui.setStatus("usage", "$1.24 / $10.00");
			ctx.ui.setWidget("usage", ["week: 128.4k tok  $1.24"]);
		},
	});

	// 对照组：命令里抛错（pi 会发 extension_error，但依旧不开 run）
	pi.registerCommand("usageboom", {
		description: "Throw inside command handler",
		handler: async () => {
			throw new Error("boom: quota fetch failed");
		},
	});

	// 看门狗验证组：handler 永不返回（模拟用户环境里菜单关闭后 handler 挂死）
	pi.registerCommand("usagehang", {
		description: "Hang forever after showing a transient notify",
		handler: async (_args, ctx) => {
			ctx.ui.notify("usage-hang: handler will never return", "info");
			await new Promise(() => {});
		},
	});
}
