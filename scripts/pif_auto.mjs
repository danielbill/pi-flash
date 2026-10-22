#!/usr/bin/env node
// pif_auto.mjs <method> [paramsJSON] [timeout_ms]
// 对运行中的 pi-flash 发一条自动化请求（协议见 crates/pi-link/src/automation.rs）：
//   TCP 行 JSON：server hello → client auth → request → response（单发即断）。
// 例：
//   node scripts/pif_auto.mjs wx.qr_open                 # 打开微信扫码弹窗
//   node scripts/pif_auto.mjs ui.snapshot                # 断言 wx_qr 状态机
//   node scripts/pif_auto.mjs settings.open '{"tab":7}'  # 打开设置页
import { connect } from "node:net";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { homedir } from "node:os";

const [,, method, paramsArg, timeoutArg] = process.argv;
if (!method) {
  console.error("用法: node scripts/pif_auto.mjs <method> [paramsJSON] [timeout_ms]");
  process.exit(2);
}
const params = paramsArg ? JSON.parse(paramsArg) : {};
const timeoutMs = Number(timeoutArg) || 5000;

const home = homedir();
// base 目录发现：debug 构建 = <repo>/pi-flash-dev（dev_dir），release = ~/.pi-flash；
// PI_FLASH_DIR 显式覆盖一切（pi_link::paths 同逻辑）。多候选里取最新实例。
const repoRoot = join(new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1"));
const candidateDirs = [
  process.env.PI_FLASH_DIR && join(process.env.PI_FLASH_DIR, "automation"),
  join(repoRoot, "pi-flash-dev", "automation"),
  join(home, ".pi-flash", "automation"),
  join(home, ".pi-flash-dev", "automation"),
].filter(Boolean);
let infos = [];
for (const dir of candidateDirs) {
  try {
    infos.push(
      ...readdirSync(dir)
        .filter((f) => f.endsWith(".json"))
        .map((f) => JSON.parse(readFileSync(join(dir, f), "utf8"))),
    );
  } catch {}
}
infos.sort((a, b) => b.started_at_ms - a.started_at_ms); // 新实例优先

function attempt(info, onFail) {
  const sock = connect(info.port, "127.0.0.1");
  let buf = "";
  let done = false;
  const finish = (code, msg) => {
    if (done) return;
    done = true;
    sock.destroy();
    if (msg) console.log(msg);
    process.exit(code);
  };
  sock.setTimeout(timeoutMs, () => finish(1, "超时无响应"));
  sock.on("error", (e) => {
    if (done) return;
    // 登记文件可能是陈旧实例（进程已死），交给上层换下一个
    if (!done) {
      done = true;
      sock.destroy();
      onFail(e);
    }
  });
  sock.on("data", (d) => {
    buf += d.toString();
    let i;
    while ((i = buf.indexOf("\n")) >= 0 && !done) {
      const line = buf.slice(0, i);
      buf = buf.slice(i + 1);
      if (!line.trim()) continue;
      let rec;
      try { rec = JSON.parse(line); } catch { continue; }
      if (rec.type === "hello") {
        sock.write(JSON.stringify({ type: "auth", token: info.token }) + "\n");
      } else if (rec.type === "auth_ok") {
        sock.write(JSON.stringify({ id: "1", method, params }) + "\n");
      } else if (rec.type === "auth_error") {
        finish(1, `auth 失败: ${rec.error?.code} ${rec.error?.message ?? ""}`);
      } else if ("ok" in rec) {
        // response 记录无 type 字段（同 request/response record 契约），按 ok 字段识别
        finish(rec.ok ? 0 : 1, JSON.stringify(rec, null, 2));
      }
    }
  });
}

(function next(idx) {
  if (idx >= infos.length) {
    console.error("所有登记实例均连接失败（实例已死？重新以 PI_FLASH_AUTOMATION=auto 启动）");
    process.exit(1);
  }
  attempt(infos[idx], () => next(idx + 1));
})(0);
