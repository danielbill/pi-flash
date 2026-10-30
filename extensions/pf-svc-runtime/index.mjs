// pf-svc-runtime —— 执行域宿主 shim（契约 v4 contributes.service，90-6）。
// 宿主自带基础设施：pi 以 -e 挂载本文件；进程级 env PF_SVC_CONFIG 指向
// 宿主写好的实例配置（PF 仓 crates/pi-link/src/service_ext.rs 写入侧）。
//
// 职责（全部是机械转发，无任何插件语义）：
//   1. session_start：按配置 import 各实例执行面入口 → activate(svc) → 播报 ready；
//   2. svc.post = outbox 追加（{id, payload}，shim 是 outbox 唯一写者）；
//   3. inbox 轮询（消费即截断）→ 按实例 id 分发 onMessage（宿主是 inbox 唯一写者）；
//   4. session_shutdown：逐实例 onStop → 播报 stopped。
// 事件协议：{id, payload}（通道消息）| {id, ev:"ready"|"stopped"}（生命周期播报）。

import { readFileSync, existsSync, appendFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const TICK_MS = 120;

function append(f, obj) {
  try {
    appendFileSync(f, JSON.stringify(obj) + "\n");
  } catch (e) {
    console.error("[pf-svc] outbox 追加失败:", e?.message ?? e);
  }
}

function crumb(line) {
  try {
    const base = (process.env.PF_SVC_CONFIG || "").split("\\").join("/");
    const m = base.match(/^(.*)\/pf-svc\/sessions\//);
    if (!m) return;
    appendFileSync(m[1] + "/pf-svc/shim.log", `[shim ${Date.now()}] ${line}
`);
  } catch { /* 排障留痕，尽力而为 */ }
}

export default function (pi) {
  const cfgPath = process.env.PF_SVC_CONFIG;
  crumb(`load cfg=${cfgPath ?? "(unset)"} exists=${cfgPath ? existsSync(cfgPath) : false}`);
  if (!cfgPath || !existsSync(cfgPath)) return; // 无实例配置 = 纯旁路

  let cfg;
  try {
    cfg = JSON.parse(readFileSync(cfgPath, "utf8"));
  } catch (e) {
    console.error("[pf-svc] 配置读取失败:", e?.message ?? e);
    return;
  }

  const insts = new Map();
  let timer = null;
  // live = 本会话域活着。**不是**进程级终态：pi 会在会话切换/重载时
  // session_shutdown → session_start，把它当终态会让通道在第一次切换后
  // 永久死掉（inbox 里的意图再也没人消费——灯/徽标/二维码全哑）。
  let live = false;

  function dispatch(line) {
    if (!live) return;
    const inst = insts.get(line.id);
    if (!inst) return;
    if (line.sys === "stop") {
      try { inst._onStop?.(); } catch (e) { console.error("[pf-svc] onStop 异常:", e); }
      return;
    }
    try { inst._onMessage?.(line.payload); } catch (e) { console.error("[pf-svc] onMessage 异常:", e); crumb(`dispatch THREW ${e?.message ?? e}`); }
  }

  function pumpInbox() {
    if (!live) return;
    for (const inst of insts.values()) {
      if (!existsSync(inst.inbox)) continue;
      let text = "";
      try { text = readFileSync(inst.inbox, "utf8"); } catch { continue; }
      if (text.length === 0) continue;
      try { writeFileSync(inst.inbox, ""); } catch (e) { console.error("[pf-svc] inbox 截断失败:", e); return; }
      for (const l of text.split("\n")) {
        if (l.trim() === "") continue;
        try { dispatch(JSON.parse(l)); } catch (e) { console.error("[pf-svc] 坏行跳过:", e?.message); }
      }
    }
  }

  const start = async (sessionCtx) => {
    if (live) return;
    live = true;
    crumb(`start instances=${(cfg.instances ?? []).length}`);
    for (const inst of cfg.instances ?? []) {
      if (insts.has(inst.id)) continue;
      const svc = {
        anchor: inst.anchor,
        dir: inst.dir,
        globalDir: cfg.global_dir,
        pi,
        sessionCtx,
        post: (payload) => append(inst.outbox, { id: inst.id, payload }),
        onMessage: (h) => { inst._onMessage = h; },
        onStop: (h) => { inst._onStop = h; },
      };
      try {
        const mod = await import(pathToFileURL(inst.entry).href);
        const activate = mod.activate ?? mod.default;
        if (typeof activate !== "function") {
          console.error(`[pf-svc] ${inst.id} 入口无 activate 导出，跳过`);
          continue;
        }
        await activate(svc);
        insts.set(inst.id, inst);
        append(inst.outbox, { id: inst.id, ev: "ready" });
        crumb(`activate ok id=${inst.id} anchor=${inst.anchor}`);
      } catch (e) {
        console.error(`[pf-svc] ${inst.id} 实例化失败:`, e?.message ?? e);
        crumb(`activate FAIL id=${inst.id} err=${e?.message ?? e}`);
      }
    }
    if (!timer) timer = setInterval(pumpInbox, TICK_MS);
  };

  // 装载时机晚于 session_start（扩展随进程启动挂载，事件已过）——就地起一次，
  // 再保留事件钩子兜后续会话；start 幂等（insts.has / stopped 双闸）。
  const early = pi.sessionCtx ?? {};
  start(early).catch((e) => crumb(`start FAIL ${e?.message ?? e}`));
  pi.on("session_start", (_event, sessionCtx) => {
    start(sessionCtx).catch((e) => crumb(`start FAIL ${e?.message ?? e}`));
  });

  pi.on("session_shutdown", () => {
    if (!live) return;
    live = false;
    for (const inst of insts.values()) {
      try { inst._onStop?.(); } catch (e) { console.error("[pf-svc] onStop 异常:", e); }
      append(inst.outbox, { id: inst.id, ev: "stopped" });
    }
    insts.clear(); // 下一轮 session_start 重新 activate（实例对象随会话走）
  });
}
