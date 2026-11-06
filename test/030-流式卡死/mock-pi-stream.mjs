// test/030-流式卡死 mock-pi：快流 token 淹没台架的协议边界假 pi。
//
// 用法一（生成 mock vendor 树）：
//   node mock-pi-stream.mjs emit <vendorDir> fast-stream
//   → <vendorDir>/pi/VERSION + .../cli.js（逻辑整段序列化进头部）
//   run.sh 以 PI_FLASH_VENDOR_DIR=<vendorDir>/pi 启动 pf。
//
// 用法二（被 pf spawn 起来后）：prompt → agent_start → message_start →
// 按 env 配置的速率灌 text_delta 洪水 → message_end（权威全文）→
// agent_end → agent_settled → prompt handled。
//
// env（运行期读，pf spawn 继承 run.sh 设的值）：
//   PF_STREAM_MODE  flood（默认，压测：2000 条 ~50B markdown 行 ≈100KB，
//                   突发 1000 delta/s，8.6× 于真机 116 t/s 实况）
//                   exact（完整性：130 条 3 位序号 delta，全文 390 字符 <
//                   快照 truncate(400)，可做**全文逐字节相等**断言）
//   PF_STREAM_N     条数（flood 默认 2000 / exact 固定 130）
//
// 线缆真相以 PI_FLASH_RPC_LOG 为准（client.rs 双向记录），mock 不自记日志。
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const CLI_REL = join(
  "node_modules",
  "@earendil-works",
  "pi-coding-agent",
  "dist",
  "bundle",
  "cli.js",
);

const [, , mode, vendorDir, scenario = "fast-stream"] = process.argv;
if (mode === "emit") {
  const root = join(vendorDir, "pi");
  mkdirSync(root, { recursive: true });
  writeFileSync(join(root, "VERSION"), "1.5.0\n");
  const cliPath = join(root, CLI_REL);
  mkdirSync(join(cliPath, ".."), { recursive: true });
  writeFileSync(
    cliPath,
    `"use strict";
const SCENARIO = ${JSON.stringify(scenario)};
${mockMain.toString()}
mockMain(SCENARIO);
`,
  );
  console.log(`mock-pi-stream vendor tree ready: ${cliPath} (scenario=${scenario})`);
  process.exit(0);
}

function mockMain(SCENARIO) {
  const enc = (o) => JSON.stringify(o) + "\n";
  const send = (o) => process.stdout.write(enc(o));
  const responded = new Set();

  const CANNED = {
    get_state: {
      model: {
        id: "mock-model",
        name: "Mock Model",
        provider: "mock",
        contextWindow: 100000,
      },
      thinkingLevel: "off",
      isStreaming: false,
      sessionId: "mock-session",
      messageCount: 0,
      sessionFile: null,
    },
    get_commands: { commands: [] },
    get_messages: { messages: [] },
    get_available_models: {
      models: [
        {
          id: "mock-model",
          name: "Mock Model",
          provider: "mock",
          contextWindow: 100000,
        },
      ],
    },
    get_session_stats: {
      tokens: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 },
      cost: 0,
      contextUsage: { tokens: 0, contextWindow: 100000, percent: 0 },
    },
    get_entries: { entries: [], leafId: null },
    get_tree: { tree: [], leafId: null },
    clear_queue: { steering: [], followUp: [] },
  };

  function respond(id, command, extraData) {
    if (responded.has(id)) return;
    responded.add(id);
    send({
      type: "response",
      id,
      command,
      success: true,
      data: extraData !== undefined ? extraData : CANNED[command] ?? {},
    });
  }

  // chunk 文本：flood = 带 markdown 语法的中文行（喂 markdown 重解析路径）；
  // exact = 3 位零垫序号（130 条 × 3 字符 = 390 字符 < 400 截断窗）
  const MODE = process.env.PF_STREAM_MODE ?? "flood";
  const N =
    MODE === "exact" ? 130 : Number(process.env.PF_STREAM_N ?? 2000);
  const chunkOf = (i) =>
    MODE === "exact"
      ? String(i).padStart(3, "0")
      : `- 流式台架标记 ${i}：一行带**粗体**与\`代码\`的中文文本\n`;
  const fullText = () => Array.from({ length: N }, (_, i) => chunkOf(i)).join("");

  let buf = "";
  process.stdin.setEncoding("utf8");
  process.stdin.on("data", (d) => {
    buf += d;
    let ix;
    while ((ix = buf.indexOf("\n")) !== -1) {
      const line = buf.slice(0, ix);
      buf = buf.slice(ix + 1);
      if (!line.trim()) continue;
      let msg;
      try {
        msg = JSON.parse(line);
      } catch {
        continue;
      }
      const { type, id, command } = msg;
      if (type === "prompt") {
        if (SCENARIO === "fast-stream") {
          startFlood(id);
          continue;
        }
        respond(id, "prompt", { disposition: "handled" });
        continue;
      }
      respond(id, command ?? type ?? "unknown");
    }
  });

  // 突发灌 delta：flood = 每 10ms 一波 10 条 ≈ 1000 delta/s（真机 116 t/s
  // 的 8.6 倍）；exact = 每 30ms 一条（窗口 ~4s，快照轮询稳稳落在流式态内）
  function startFlood(promptId) {
    send({ type: "agent_start" });
    send({
      type: "message_start",
      message: { role: "assistant", content: "", timestamp: Date.now() },
    });
    let i = 0;
    const exact = MODE === "exact";
    const BURST = exact ? 1 : 10;
    const t = setInterval(() => {
      for (let k = 0; k < BURST && i < N; k++, i++) {
        send({
          type: "message_update",
          assistantMessageEvent: {
            type: "text_delta",
            contentIndex: 0,
            delta: chunkOf(i),
          },
        });
      }
      if (i >= N) {
        clearInterval(t);
        send({
          type: "message_end",
          message: {
            role: "assistant",
            content: fullText(),
            stopReason: "stop",
            timestamp: Date.now(),
            usage: {
              input: 100,
              output: N,
              cacheRead: 0,
              cacheWrite: 0,
              cost: { total: 0.01 },
            },
          },
        });
        send({ type: "agent_end", willRetry: false });
        send({ type: "agent_settled" });
        respond(promptId, "prompt", { disposition: "handled" });
      }
    }, exact ? 30 : 10);
  }

  // PF 启动后轮询 get_state；mock 常驻直到 stdin 关闭（pf 杀进程）
  setInterval(() => {}, 1000);
}

// 直接 `node mock-pi-stream.mjs`（无 emit 参数）= 按 env 场景自跑（手工调试用）
if (mode !== "emit") {
  mockMain(process.env.PF_MOCK_SCENARIO ?? "fast-stream");
}
