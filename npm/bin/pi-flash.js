#!/usr/bin/env node
"use strict";
// pi-flash 启动器（npm 薄壳的 bin 入口）：
//   1. Node 版本检查（对齐 pi-web 的门槛与文案）
//   2. 载荷自愈——版本戳不符/缺失就先补齐（兜底 --ignore-scripts、镜像剥 postinstall、半途断网）
//   3. spawn 平台载荷，stdio 透传、信号转发、退出码透传
// 设计蓝本：docs/模块设计/080-软件分发.md §3.1。
const { spawn } = require("child_process");
const {
  pkgVersion,
  isNodeVersionSupported,
  nodeVersionMessage,
  platformTag,
  payloadOk,
  exePath,
  ensure,
  busyMessage,
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

  if (!payloadOk(tag)) {
    console.error(`   载荷缺失或版本不符，正在补齐（v${pkgVersion}）…`);
    try {
      await ensure();
    } catch (error) {
      console.error(`\n✗ 载荷补齐失败：\n  ${error.message}`);
      console.error(`  重试：npm rebuild pi-flash    手动：${require("./lib").assetUrls(tag).releasePage}`);
      process.exit(1);
    }
  }

  const exe = exePath(tag);
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
