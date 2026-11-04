// test/ext-ui mock-pi：协议边界假 pi（PI_FLASH_VENDOR_DIR 覆盖载体）。
//
// 用法一（生成 mock vendor 树）：
//   node mock-pi.mjs emit <vendorDir> <scenario>
//   → <vendorDir>/pi/VERSION + <vendorDir>/pi/node_modules/@earendil-works/
//     pi-coding-agent/dist/bundle/cli.js（scenario 烧进 cli.js 头部常量）
//   run.sh 以 PI_FLASH_VENDOR_DIR=<vendorDir>/pi 启动 pf，pi-link client.rs
//   会 spawn `node <vendorDir>/pi/.../cli.js --mode rpc ...` = 本 mock。
//
// 用法二（被 pf spawn 起来后）：按 scenario 脚本回 JSONL。所有命令一律
// success:true + 罐头 data（形状对齐 pi-link protocol.rs 解析口径）；
// prompt 按 scenario 注入协议边界事件。
//
// scenario:
//   unknown-method   prompt → 注入 PF 不认识的 extension_ui_request（method
//                    = "futureWidget"），收到 PF 的 extension_ui_response 后
//                    回 prompt handled。断言：PF 必须立刻回 cancelled（zi2s.2）。
//   crash-with-dialog prompt → 注入阻塞 select → 回 prompt handled →
//                    4s 后 process.exit(1) 模拟 pi 崩溃。断言：弹窗被清理
//                    （zi2s.3）。
//
// 线缆真相一律以 PI_FLASH_RPC_LOG 为准（client.rs 双向记录），mock 不自己
// 记日志。
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

// ---------------------------------------------------------------------------
// emit 模式：生成 vendor 树
// ---------------------------------------------------------------------------
const [, , mode, vendorDir, scenario = "unknown-method"] = process.argv;
if (mode === "emit") {
  const root = join(vendorDir, "pi");
  mkdirSync(root, { recursive: true });
  writeFileSync(join(root, "VERSION"), "1.5.0\n");
  const cliPath = join(root, CLI_REL);
  mkdirSync(join(cliPath, ".."), { recursive: true });
  // cli.js 自足：scenario 通过头部常量注入，逻辑与下面的 mockMain 等价
  writeFileSync(
    cliPath,
    `"use strict";
const SCENARIO = ${JSON.stringify(scenario)};
${mockMain.toString()}
mockMain(SCENARIO);
`,
  );
  console.log(`mock-pi vendor tree ready: ${cliPath} (scenario=${scenario})`);
  process.exit(0);
}

// ---------------------------------------------------------------------------
// mock 主体（emit 模式下整段序列化进 cli.js）
// ---------------------------------------------------------------------------
function mockMain(SCENARIO) {
  const enc = (o) => JSON.stringify(o) + "\n";
  const send = (o) => process.stdout.write(enc(o));
  const responded = new Set(); // 已应答的命令 id
  const extReqDone = new Set(); // 已收到回执的 extension_ui_request id

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
    // 一条假扩展命令：让 rt.commands 非空（真机有 45 条扩展/skill），
    // 菜单源因此走「进程清单优先」分支——内置 8 条并进该分支才可断言
    //（C9：websearch 与 compact 必须同在）
    get_commands: {
      commands: [{ name: "websearch", description: "mock extension command" }],
    },
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
      if (type === "extension_ui_response") {
        extReqDone.add(id);
        continue;
      }
      if (type === "prompt") {
        if (SCENARIO === "unknown-method") {
          // 先不回 prompt：等 PF 的 cancelled 回执到达后再收场 handled
          //（respond 有去重守卫，先回 {} 会把后面的 handled 吞掉）
          send({
            type: "extension_ui_request",
            id: "mock-unknown-1",
            method: "futureWidget",
            someFutureField: true,
          });
          waitForExtResponse("mock-unknown-1", id);
          continue;
        }
        respond(id, "prompt", ...scenarioPayload(SCENARIO, id));
        continue;
      }
      respond(id, command ?? type ?? "unknown");
    }
  });

  function scenarioPayload(scenario, promptId) {
    if (scenario === "unknown-method") {
      // 已上移到 prompt 处理臂（需要延迟回执，见彼处注释）
      return [];
    }
    if (scenario === "crash-with-dialog") {
      send({
        type: "extension_ui_request",
        id: "mock-crash-select-1",
        method: "select",
        title: "crash scenario select",
        options: ["a", "b", "c"],
      });
      setTimeout(() => process.exit(1), 4000); // 弹窗在屏时进程死亡
      return [{ disposition: "handled" }];
    }
    // 默认：普通 handled 收场（无 agent run）
    return [{ disposition: "handled" }];
  }

  function waitForExtResponse(reqId, promptId) {
    const t = setInterval(() => {
      if (extReqDone.has(reqId)) {
        clearInterval(t);
        respond(promptId, "prompt", { disposition: "handled" });
      }
    }, 50);
    setTimeout(() => {
      clearInterval(t);
      respond(promptId, "prompt", { disposition: "handled" }); // 兜底收场
    }, 8000);
  }

  // PF 启动后轮询 get_state；mock 常驻直到 stdin 关闭（pf 杀进程）
  setInterval(() => {}, 1000);
}

// 直接 `node mock-pi.mjs`（无 emit 参数）= 按 env 场景自跑（手工调试用）
if (mode !== "emit") {
  mockMain(process.env.PF_MOCK_SCENARIO ?? "unknown-method");
}
