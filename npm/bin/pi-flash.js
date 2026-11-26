#!/usr/bin/env node
"use strict";
// pi-flash 启动器（npm 薄壳的 bin 入口）：
//   1. Node 版本检查（对齐 pi-web 的门槛与文案）
//   2. 载荷目录解析——应用内更新把新载荷解压到 <pkg>/payload-<版本>/（081，
//      只解压不 rename），选版本最高的完整载荷启动；双击场景由 exe 自己的
//      启动交接兜底
//   3. 载荷自愈——戳不旧于包版本才算可信，否则先补齐（兜底 --ignore-scripts、
//      镜像剥 postinstall、半途断网；应用内更新先行 = 戳比包版本新也不降级）
//   4. spawn 平台载荷，stdio 透传、信号转发、退出码透传
// 设计蓝本：docs/模块设计/080-软件分发.md §3.1、081-软件自动更新.md。
const { spawn } = require("child_process");
const path = require("path");
const {
  pkgVersion,
  isNodeVersionSupported,
  nodeVersionMessage,
  platformTag,
  exeRelPath,
  resolvePayload,
  payloadOk,
  ensure,
  busyMessage,
  rmLeftovers,
  assetUrls,
} = require("./lib");

if (!isNodeVersionSupported(process.versions.node)) {
  console.error(nodeVersionMessage(process.versions.node));
  process.exit(1);
}

(async () => {
  let tag;
  try {
    tag = platformTag();
  } catch (error) {
    console.error(`✗ ${error.message}`);
    process.exit(1);
  }

  // 清上次中断的半截下载残留（>1 小时的才删）；失败不拦启动
  try {
    rmLeftovers();
  } catch {}

  if (!payloadOk(tag)) {
    console.error(`   载荷缺失或版本不符，正在补齐（v${pkgVersion}）…`);
    console.error(`   （npm 11.6+ 默认不跑安装脚本，首次运行自动补载属正常流程）`);
    try {
      await ensure();
    } catch (error) {
      console.error(`\n✗ 载荷补齐失败：\n  ${error.message}`);
      console.error(`  重试：npm rebuild pi-flash    手动：${assetUrls(tag).releasePage}`);
      process.exit(1);
    }
  }

  // 081：应用内更新的新载荷在 <pkg>/payload-<版本>/，选版本最高的完整载荷跑
  const payload = resolvePayload(tag);
  const exe = path.join(payload.dir, exeRelPath(tag));
  const child = spawn(exe, process.argv.slice(2), {
    stdio: "inherit",
    cwd: process.cwd(),
    env: process.env,
    windowsHide: false,
  });

  child.on("error", (error) => {
    if (["EBUSY", "EPERM"].includes(error.code)) {
      console.error(`✗ ${busyMessage()}`);
    } else {
      console.error(`✗ 启动失败：${error.message}（${exe}）\n  可尝试：npm rebuild pi-flash 重新落载荷`);
    }
    process.exit(1);
  });

  for (const sig of ["SIGINT", "SIGTERM"]) {
    process.on(sig, () => {
      if (child.exitCode === null && !child.killed) child.kill(sig);
    });
  }

  child.on("exit", (code, signal) => {
    // 信号退出时给常规 shell 一个非零码；正常退出码原样透传
    process.exit(signal ? 1 : code ?? 0);
  });
})().catch((error) => {
  console.error(`✗ ${error.message}`);
  process.exit(1);
});
