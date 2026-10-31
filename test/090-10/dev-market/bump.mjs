#!/usr/bin/env node
// bump.mjs —— 给 feed 里的插件包造假"新版"（090-10 更新验收专用）。
//
// 取 feed 中该 slug 的最高版本 tgz → 解包（系统 tar，npm package/ 布局）→
// 改 package.json + pf-plugin.toml 的 version → 按 package/ 前缀重打 tgz →
// 落 feed 为 <新版本> 文件。PF 的更新判定只看 packument 版本 vs 台账版本，
// 包内容不变也不影响链路——但 honest 起见包内版本一并改掉。
//
// 用法：
//   node bump.mjs <slug> <newVersion> [--drop-old]
// 例：
//   node bump.mjs quick-panel 9.9.9            # 出 quick-panel@9.9.9，保留旧版
//   node bump.mjs quick-panel 9.9.9 --drop-old # 模拟真实发版：feed 只剩新版

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const FEED = join(HERE, "feed");
const PREFIX = "pi-flash-plugins-";

const [slug, newVersion] = process.argv.slice(2);
const dropOld = process.argv.includes("--drop-old");
if (!slug || !newVersion || !/^\d+\.\d+\.\d+/.test(newVersion)) {
  console.error("用法: node bump.mjs <slug> <newVersion> [--drop-old]");
  process.exit(1);
}

const prefix = PREFIX + slug + "-";
const files = readdirSync(FEED).filter((f) => f.startsWith(prefix) && f.endsWith(".tgz"));
if (files.length === 0) {
  console.error(`✗ feed 里没有 ${slug} 的包（找 ${prefix}*.tgz）`);
  process.exit(1);
}
const src = files.sort().at(-1); // 字典序最高版（同 major 位数下足够）
console.log(`[bump] 源包: ${src}`);

// 解包（npm 布局：package/）。cwd + 相对路径调 tar：Git Bash 下 GNU tar 会把
// 参数中的 `D:` 当远程主机（Cannot connect to D），相对路径不含盘符即绕开
const tmp = join(FEED, ".bump-tmp");
rmSync(tmp, { recursive: true, force: true });
mkdirSync(tmp, { recursive: true });
const untar = spawnSync("tar", ["-xzf", src, "-C", ".bump-tmp"], { cwd: FEED, stdio: "pipe" });
if (untar.status !== 0) {
  console.error(`✗ tar 解包失败: ${String(untar.stderr).trim()}`);
  process.exit(1);
}
const pkgDir = join(tmp, "package");
if (!existsSync(pkgDir)) {
  console.error("✗ tarball 顶层缺 package/（不是 npm pack 产物？）");
  process.exit(1);
}

// 改版本：package.json 必改；pf-plugin.toml 有 version 行则一并改
const pkgJsonPath = join(pkgDir, "package.json");
if (existsSync(pkgJsonPath)) {
  const pkg = JSON.parse(readFileSync(pkgJsonPath, "utf8"));
  pkg.version = newVersion;
  writeFileSync(pkgJsonPath, JSON.stringify(pkg, null, 2) + "\n");
}
const tomlPath = join(pkgDir, "pf-plugin.toml");
if (existsSync(tomlPath)) {
  const toml = readFileSync(tomlPath, "utf8");
  writeFileSync(tomlPath, toml.replace(/^version\s*=\s*"[^"]*"/m, `version = "${newVersion}"`));
}

// 重打（package/ 前缀，与 npm pack / pi-link extract 的 strip 约定一致）
const dest = join(FEED, `${PREFIX}${slug}-${newVersion}.tgz`);
const tar = spawnSync("tar", ["-czf", `../${basename(dest)}`, "package"], {
  cwd: tmp,
  stdio: "pipe",
});
rmSync(tmp, { recursive: true, force: true });
if (tar.status !== 0) {
  console.error(`✗ tar 重打失败: ${String(tar.stderr).trim()}`);
  process.exit(1);
}
if (dropOld) {
  for (const f of files) rmSync(join(FEED, f), { force: true });
}
console.log(`[bump] 完成: ${dest}`);
console.log(`[bump] PF 市场重开页面即见 ${slug}@${newVersion} 的更新钮（无需重启 PF）。`);
