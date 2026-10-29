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

export default function (pi) {
  const cfgPath = process.env.PF_SVC_CONFIG;
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
  let stopped = false;

  function dispatch(line) {
    const inst = insts.get(line.id);
    if (!inst) return;
    if (line.sys === "stop") {
      try { inst._onStop?.(); } catch (e) { console.error("[pf-svc] onStop 异常:", e); }
      return;
    }
    try { inst._onMessage?.(line.payload); } catch (e) { console.error("[pf-svc] onMessage 异常:", e); }
  }

  function pumpInbox() {
    if (stopped) return;
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

  pi.on("session_start", async (event, sessionCtx) => {
    if (stopped) return;
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
      } catch (e) {
        console.error(`[pf-svc] ${inst.id} 实例化失败:`, e?.message ?? e);
      }
    }
    if (insts.size > 0 && !timer) timer = setInterval(pumpInbox, TICK_MS);
  });

  pi.on("session_shutdown", () => {
    if (stopped) return;
    stopped = true;
    if (timer) { clearInterval(timer); timer = null; }
    for (const inst of insts.values()) {
      try { inst._onStop?.(); } catch (e) { console.error("[pf-svc] onStop 异常:", e); }
      append(inst.outbox, { id: inst.id, ev: "stopped" });
    }
  });
}
