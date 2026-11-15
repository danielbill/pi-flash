// test/047-内存削减 bench-fixture：047 §4 度量验收的洪泛会话夹具。
//
// 生成一个 ~16MB 的真实形态 pi 会话 JSONL（leaf 链 + 死分支洪泛 + 大
// toolResult + 大量 system 段），供 bench.sh 在隔离实例里 session.open
// 装载，对照 047 前后的工作集数字。
//
// 用法：node bench-fixture.mjs <root>   # 写 root/sessions/<slug>/ + root/cwd/
//
// 形态（对齐 047 的三条热点路径）：
//   • 主链 1600 条（user / assistant 大 text+toolCall / toolResult 大块 /
//     system sections+toolsAdded 交替）——wire 解析、get_messages 投影、
//     system 折叠的负载；
//   • 中段一条 400 条的死分支（先写）+ 主链续写（后写）——文件序锚点
//     叶链重建（047-P1-3 两遍法）的分支挑选负载。
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const root = process.argv[2];
if (!root) {
  console.error("usage: node bench-fixture.mjs <root>");
  process.exit(1);
}
const cwd = join(root, "cwd");
const slug = "--" + cwd.replace(/[:/\\]/g, "-") + "--";
const groupDir = join(root, "sessions", slug);
mkdirSync(cwd, { recursive: true });
mkdirSync(groupDir, { recursive: true });
writeFileSync(join(cwd, "readme.txt"), "bench fixture cwd\n");

// ~8KB 文本块（64 字符行 × 128 行）；SYS 20KB（system sections 折叠负载
// ——1200 条 × 20KB ≈ 24MB 原始驻留，047-P1-4 前后差的主角）
const BIG = ("0123456789abcdef".repeat(4) + "\n").repeat(128);
const SYS = ("0123456789abcdef".repeat(4) + "\n").repeat(320);
const MID = BIG.slice(0, 2048);

const e = (id, parent, role, content) =>
  JSON.stringify({ type: "message", id, parentId: parent, message: { role, content } }) + "\n";
const sysMsg = (id, parent, i) =>
  JSON.stringify({
    type: "message",
    id,
    parentId: parent,
    message: {
      role: "system",
      content: "bench base prompt",
      sections: { rules: `<rules>bench rule ${i} ${SYS}</rules>`, preamble: `you are bench ${i}` },
      toolsAdded: [{ name: "read", description: "read a file", parameters: { type: "object" } }],
    },
  }) + "\n";

const MAIN = 3000;
const DEAD = 600;
const FORK_AT = 1500;
let out = "";
let prev = null;
let n = 0;
const id = () => `e${String(++n).padStart(5, "0")}`;

for (let i = 0; i < MAIN; i++) {
  const cur = id();
  if (i === FORK_AT) {
    // 死分支先写（文件序靠前）：leaf 重建按文件序锚定末条 + parentId 回溯，
    // 这 600 条不进叶链——047-P1-3 两遍法的分支挑选负载
    let dp = prev;
    for (let j = 0; j < DEAD; j++) {
      const d = id() + "d";
      out += e(d, dp, "assistant", [
        { type: "text", text: `dead branch ${j} ${BIG}` },
        { type: "toolCall", id: `dtc${j}`, name: "read", arguments: { path: `dead/${j}.rs` } },
      ]);
      dp = d;
    }
  }
  // 5 拍循环：user / assistant(+toolCall) / toolResult(大块) / system / system
  // → 1200 条 system sections（折叠负载主角）+ 600×3 条消息链
  switch (i % 5) {
    case 0:
      out += e(cur, prev, "user", `bench turn ${i}: ${MID}`);
      break;
    case 1:
      out += e(cur, prev, "assistant", [
        { type: "text", text: `assistant reply ${i} ${MID}` },
        { type: "toolCall", id: `tc${i}`, name: "read", arguments: { path: `src/${i}.rs` } },
      ]);
      break;
    case 2:
      out += e(cur, prev, "toolResult", [
        { type: "text", text: `tool result ${i} ${BIG}` },
      ]);
      break;
    default:
      out += sysMsg(cur, prev, i);
  }
  prev = cur;
}

const file = join(groupDir, "bench-flood.jsonl");
// pi 会话头行（SessionInfo.cwd 的来源；没有它 app 侧 same_ws 过滤直接丢）
const header =
  JSON.stringify({ type: "session", version: 3, id: "bench-flood", cwd }) + "\n";
writeFileSync(file, header + out);
console.log(`bench fixture: ${file} (${(out.length / 1024 / 1024).toFixed(1)} MB, ${n} entries)`);
