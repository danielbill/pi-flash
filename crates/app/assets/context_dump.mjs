// 上下文精确测量 dump（040 延迟加载 v3 的 C' 通道）。
//
// 单会话方案：pi-flash 把**全部**已安装包用 `-e` 挂进一个 `pi --mode rpc`
// 进程，再挂上本脚本。pi 在 bind 时就会派发 `session_start`（reason:
// "startup"，无需任何 prompt/模型调用），懒注册的扩展（computer-use 的
// 58 颗、pi-fff 的 replace 等）在这一步把工具挂齐。本脚本等所有扩展的
// handler 跑完（debounce），把 `pi.getAllTools()` 全量（含 sourceInfo.source
// = 配置的包源字符串）原子写到 `CONTEXT_DUMP_FILE`，pi-flash 轮询读取后
// 杀进程。按包归属在 Rust 侧完成（CJK 感知估算）。
export default function (pi) {
  const file = process.env.CONTEXT_DUMP_FILE;
  if (!file) return;
  pi.on("session_start", async () => {
    // debounce：别的扩展的 session_start handler 可能排在我们后面
    setTimeout(async () => {
      try {
        const tools = pi.getAllTools().map((t) => ({
          name: t.name,
          description: t.description ?? "",
          schema_text: JSON.stringify(t.parameters ?? {}),
          exposure: t.exposure ?? "direct",
          source: t.sourceInfo?.source ?? "",
          path: t.sourceInfo?.path ?? "",
        }));
        const payload = JSON.stringify({ tools });
        // tmp + rename：轮询方读到的文件要么不存在要么完整
        const { writeFileSync, renameSync } = await import("node:fs");
        const tmp = `${file}.tmp`;
        writeFileSync(tmp, payload);
        renameSync(tmp, file);
      } catch {
        // 写失败 = pi-flash 超时回退静态数
      }
    }, 5000);
  });
}
