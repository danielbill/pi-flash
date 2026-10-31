#!/usr/bin/env node
// dev-market 本地假 npm registry（090-10 插件更新人工验收专用）。
//
// 用途：PF 插件市场扫描链（pi-link market/pf_scan.rs）走 `PI_FLASH_NPM_REGISTRY`
// 可整体指向本服务器，从而在本地演练「发新版 → 市场出更新钮 → 覆盖安装」全链路，
// 不碰真实 npm、不让线上用户见到测试版本。
//
// 端点（只实现 pf_scan 用到的三个）：
//   GET /@pi-flash-plugins%2fmarket/latest     清单包 packument
//   GET /@pi-flash-plugins%2f<slug>/latest     单包 packument（integrity 现算）
//   GET /files/<file>.tgz                      tarball 直连下载（entry.url 指向这）
//
// feed 约定：本目录 feed/ 下放 npm pack 产物，命名 `pi-flash-plugins-<slug>-<version>.tgz`
// （npm pack 对 scoped 包的原生命名）。同 slug 多版本并存时 packument 取最高版。
// 清单包 tgz 不用手备：首次请求按 feed 现状自动生成（schema pf-market/1，
// display/desc/icon 缺省——PF 端回落 slug 展示，验收够用）。
//
// 用法：
//   node server.mjs [--port 8477]
//   PF 侧：PI_FLASH_NPM_REGISTRY=http://127.0.0.1:8477 cargo run -p app

import http from "node:http";
import { createHash } from "node:crypto";
import {
  mkdirSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from "node:fs";
import { join, resolve, dirname, basename } from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const FEED = join(HERE, "feed");
const GEN = join(FEED, ".gen");
const SCOPE_FILE_PREFIX = "pi-flash-plugins-";
const HOST_DEFAULT_PORT = 8477;

const argv = process.argv.slice(2);
const port = (() => {
  const i = argv.indexOf("--port");
  const v = i >= 0 ? Number(argv[i + 1]) : NaN;
  return Number.isInteger(v) && v > 0 ? v : HOST_DEFAULT_PORT;
})();

// ---------------------------------------------------------------------------
// feed 扫描与 integrity 缓存
// ---------------------------------------------------------------------------

/** feed 目录下所有插件包：[{ slug, version, file }]（同名 slug 取最高版）。 */
function scanFeed() {
  mkdirSync(FEED, { recursive: true });
  const pkgs = new Map(); // slug -> { slug, version, file }
  for (const file of readdirSync(FEED)) {
    if (!file.endsWith(".tgz") || !file.startsWith(SCOPE_FILE_PREFIX)) continue;
    const stem = file.slice(0, -4);
    // 文件名 = pi-flash-plugins-<slug>-<version>；slug 自身含 -，
    // 但 pi-flash-plugins-<slug>- 前缀匹配后剩余整段即 version（semver 不含 slug 尾段）
    const rest = stem.slice(SCOPE_FILE_PREFIX.length);
    const m = rest.match(/^(.+?)-(\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?)$/);
    if (!m) {
      console.warn(`[dev-market] 跳过命名不合规的包文件: ${file}`);
      continue;
    }
    const slug = m[1];
    const version = m[2];
    const prev = pkgs.get(slug);
    if (!prev || cmpSemver(version, prev.version) > 0) {
      pkgs.set(slug, { slug, version, file });
    }
  }
  return [...pkgs.values()];
}

/** 极简 semver 比较（major.minor.patch 逐段；预发布号 < 正式）。够验收用。 */
function cmpSemver(a, b) {
  const pa = a.split(/[-+]/)[0].split(".").map(Number);
  const pb = b.split(/[-+]/)[0].split(".").map(Number);
  for (let i = 0; i < 3; i++) {
    if ((pa[i] ?? 0) !== (pb[i] ?? 0)) return (pa[i] ?? 0) - (pb[i] ?? 0);
  }
  const preA = a.includes("-") ? a.split("-")[1] : null;
  const preB = b.includes("-") ? b.split("-")[1] : null;
  if (preA === preB) return 0;
  return preA === null ? 1 : -1; // 无预发布号 = 正式 = 更大
}

const integrityCache = new Map(); // file -> { mtimeMs, size, integrity }

/** sha512（base64）现算 + 按 mtime 缓存：npm pack 产物 ≤ 8MB，代价可忽略。 */
function integrityOf(file) {
  const full = join(FEED, file);
  const st = statSync(full);
  const hit = integrityCache.get(file);
  if (hit && hit.mtimeMs === st.mtimeMs && hit.size === st.size) return hit.integrity;
  const integrity = "sha512-" + createHash("sha512").update(readFileSync(full)).digest("base64");
  integrityCache.set(file, { mtimeMs: st.mtimeMs, size: st.size, integrity });
  return integrity;
}

// ---------------------------------------------------------------------------
// 清单包（@pi-flash-plugins/market）按需生成
// ---------------------------------------------------------------------------

let manifestSig = "";

/** feed 变化时重建清单包 tgz（系统 tar，与 pi-link extract 同一布局 package/ 前缀）。
 *  产物按命名约定落 feed 根（pi-flash-plugins-market-1.0.0.tgz），与真实清单包
 *  同构同待遇——/files 守卫、integrity 缓存、packument 全走同一条路。
 *  feed 里已有手放的 market 包时（slug=market）不生成，直接用那份。 */
function ensureManifestPkg(pkgs) {
  if (pkgs.some((p) => p.slug === "market")) return;
  mkdirSync(GEN, { recursive: true });
  const sig = pkgs
    .map((p) => `${p.file}:${statSync(join(FEED, p.file)).mtimeMs}`)
    .sort()
    .join("|");
  if (sig === manifestSig) return;
  const staging = join(GEN, "staging", "package");
  rmSync(join(GEN, "staging"), { recursive: true, force: true });
  mkdirSync(staging, { recursive: true });
  const manifest = {
    schema: "pf-market/1",
    packages: pkgs
      .filter((p) => p.slug !== "market")
      .map((p) => ({ name: `@pi-flash-plugins/${p.slug}` })),
  };
  writeFileSync(join(staging, "manifest.json"), JSON.stringify(manifest, null, 2));
  // cwd + 相对路径调 tar：Git Bash 下 PATH 里的 GNU tar 会把参数中的 `D:`
  // 当远程主机（Cannot connect to D），相对路径不含盘符即绕开
  const r = spawnSync("tar", ["-czf", "pi-flash-plugins-market-1.0.0.tgz", "-C", "staging", "package"], {
    cwd: GEN,
    stdio: ["ignore", "ignore", "pipe"],
  });
  if (r.status !== 0) {
    throw new Error(`tar 打包清单失败: ${String(r.stderr).trim()}`);
  }
  // 产物挪进 feed 根（scanFeed 下次会把它当 slug=market 收进来）
  const destFile = join(FEED, "pi-flash-plugins-market-1.0.0.tgz");
  rmSync(destFile, { force: true }); // Windows rename 不覆盖已存在目标
  renameSync(join(GEN, "pi-flash-plugins-market-1.0.0.tgz"), destFile);
  rmSync(join(GEN, "staging"), { recursive: true, force: true });
  manifestSig = sig;
  console.log(`[dev-market] 清单包已重建（${manifest.packages.length} 个插件）`);
}

// ---------------------------------------------------------------------------
// 路由
// ---------------------------------------------------------------------------

function packument(name, version, file, req) {
  const tarball = `http://${req.headers.host}/files/${encodeURIComponent(file)}`;
  return { name, version, dist: { integrity: integrityOf(file), tarball } };
}

function handle(req, res) {
  const url = decodeURIComponent(new URL(req.url, "http://x").pathname);
  try {
    // tarball 直连
    if (url.startsWith("/files/")) {
      const file = basename(url.slice("/files/".length));
      if (!file.endsWith(".tgz") || !file.startsWith(SCOPE_FILE_PREFIX)) {
        return json(res, 404, { error: "not found" });
      }
      console.log(`[dev-market] ${req.method} ${url}`);
      res.writeHead(200, { "content-type": "application/x-gzip" });
      res.end(readFileSync(join(FEED, file)));
      return;
    }
    // packument：/latest
    const m = url.match(/^\/@pi-flash-plugins\/([^/]+)\/latest$/);
    if (m) {
      const slug = m[1];
      const pkgs = scanFeed();
      console.log(`[dev-market] ${req.method} ${url}`);
      if (slug === "market") {
        ensureManifestPkg(pkgs);
        const hit = scanFeed().find((p) => p.slug === "market");
        if (!hit) return json(res, 500, { error: "清单包生成失败" });
        return json(res, 200, packument(hit.name ?? "@pi-flash-plugins/market", hit.version, hit.file, req));
      }
      const hit = pkgs.find((p) => p.slug === slug);
      if (!hit) return json(res, 404, { error: `feed 里没有 ${slug}` });
      return json(res, 200, packument(`@pi-flash-plugins/${slug}`, hit.version, hit.file, req));
    }
    return json(res, 404, { error: "not found" });
  } catch (e) {
    console.error(`[dev-market] ${url} 失败: ${e.message}`);
    return json(res, 500, { error: e.message });
  }
}

function json(res, code, body) {
  res.writeHead(code, { "content-type": "application/json" });
  res.end(JSON.stringify(body));
}

// ---------------------------------------------------------------------------
// 启动
// ---------------------------------------------------------------------------

mkdirSync(FEED, { recursive: true });
const boot = scanFeed();
http.createServer(handle).listen(port, "127.0.0.1", () => {
  console.log(`[dev-market] http://127.0.0.1:${port}  feed=${FEED}`);
  console.log(
    `[dev-market] 包: ${boot.map((p) => `${p.slug}@${p.version}`).join(", ") || "（空——往 feed/ 放 npm pack 产物）"}`,
  );
  console.log(`[dev-market] PF 侧启动: PI_FLASH_NPM_REGISTRY=http://127.0.0.1:${port} cargo run -p app`);
});
