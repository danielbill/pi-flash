"use strict";
// pi-flash npm 薄壳的共享逻辑：Node/平台探测、下载源、载荷布局、校验、解压、落位。
// 设计蓝本：docs/模块设计/080-软件分发.md §3（冲突以该文为准）。
//
// 载荷布局（包目录内，npm uninstall 随包清除）：
//   <pkg>/payload/.payload-version          安装时的载荷版本戳，最后写入（戳在 = 完整）
//   <pkg>/payload/pi-flash.exe              稳定入口（npm 命令与快捷方式都指它）
//   <pkg>/payload-<版本>/.payload-version   应用内更新（081）解压出的新载荷目录
//   <pkg>/payload-<版本>/pi-flash.exe       ——只解压不 rename，启动时跑版本最高者
//
// 下载源：GitHub 直连优先，测速不过阈值自动按序切默认镜像列表（080 §3.2）；
// PI_FLASH_MIRROR 指定唯一源、PI_FLASH_MIRRORS 追加候选。镜像须保持同构布局
// <base>/v<version>/<asset>。
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const http = require("http");
const https = require("https");
const { spawnSync } = require("child_process");

const pkgDir = path.join(__dirname, "..");
const pkgVersion = require(path.join(pkgDir, "package.json")).version;

// 对齐 pi-web 的 Node 门槛（bin/node-version.js）
const MIN_NODE = "22.19.0";
// 本薄壳仅发布这两类资产；其余平台给出明确报错而不是瞎猜
const SUPPORTED_TAGS = ["win32-x64", "darwin-arm64"];

const TMP_PREFIX = "payload.tmp-";
const PAYLOAD_DIR = path.join(pkgDir, "payload");
const STAMP_FILE = path.join(PAYLOAD_DIR, ".payload-version");

// --- Node 版本 -------------------------------------------------------------

function parseNodeVersion(version) {
  const match = /^v?(\d+)\.(\d+)\.(\d+)/.exec(version || "");
  if (!match) return null;
  return match.slice(1).map(Number);
}

function isNodeVersionSupported(version) {
  const current = parseNodeVersion(version);
  const minimum = parseNodeVersion(MIN_NODE);
  if (!current || !minimum) return false;
  for (let i = 0; i < minimum.length; i += 1) {
    if (current[i] > minimum[i]) return true;
    if (current[i] < minimum[i]) return false;
  }
  return true;
}

function nodeVersionMessage(version) {
  return [
    `pi-flash 需要 Node.js ${MIN_NODE} 或更新版本。`,
    `当前 Node.js 版本：${version}。`,
    "升级 Node.js 后重试：https://nodejs.org/",
  ].join("\n");
}

// --- 平台与下载源 -----------------------------------------------------------

function platformTag() {
  const plat = { win32: "win32", darwin: "darwin" }[process.platform];
  const arch = { x64: "x64", arm64: "arm64" }[process.arch];
  const tag = plat && arch ? `${plat}-${arch}` : `${process.platform}-${process.arch}`;
  if (!SUPPORTED_TAGS.includes(tag)) {
    throw new Error(
      `暂无 ${tag} 平台的发布资产（当前支持：${SUPPORTED_TAGS.join(" / ")}）。` +
        `可从 GitHub Releases 手动下载绿色包，或设 PI_FLASH_MIRROR 指向自建镜像。`,
    );
  }
  return tag;
}

// 候选源：直连优先（官方源健康时不经第三方）；首段测速不过阈值按序切换。
// 默认镜像为 2026-10 实测可用的前缀加速服务（同构布局）；失效后用
// PI_FLASH_MIRROR / PI_FLASH_MIRRORS 指定，或直接 PR 更新此表。
const DIRECT_BASE = "https://github.com/danielbill/pi-flash/releases/download";
const DEFAULT_MIRRORS = [
  "https://gh-proxy.com/https://github.com/danielbill/pi-flash/releases/download",
  "https://ghfast.top/https://github.com/danielbill/pi-flash/releases/download",
  "https://ghproxy.net/https://github.com/danielbill/pi-flash/releases/download",
];

function releaseBase() {
  const mirror = process.env.PI_FLASH_MIRROR;
  if (mirror) return mirror.replace(/\/+$/, "");
  return DIRECT_BASE;
}

// 显式 PI_FLASH_MIRROR = 只用这一个（跳过测速切换）；否则直连 + 追加 + 默认
function downloadBases() {
  if (process.env.PI_FLASH_MIRROR) return [releaseBase()];
  const extra = (process.env.PI_FLASH_MIRRORS || "").split(/[\s,]+/).filter(Boolean);
  return [DIRECT_BASE, ...extra, ...DEFAULT_MIRRORS];
}

// 版本预取（080，2026-10-15 实录）：npm 镜像同步滞后会让 @latest 装出旧壳，
// 壳版本再传导成旧载荷。下载载荷前先问 GitHub releases API 要真实最新版
// （api.github.com 轻量一般不被墙，与 app 侧更新器同名环境变量），任何失败
// （网络/超时/格式）都回退壳自身版本——预取永不阻塞、永不改变失败语义。
function releasesApiBase() {
  return (
    process.env.PI_FLASH_RELEASES_API ||
    "https://api.github.com/repos/danielbill/pi-flash/releases"
  );
}

async function resolveLatestVersion() {
  try {
    const res = await httpGet(`${releasesApiBase()}/latest`, 5, 5000);
    const chunks = [];
    let size = 0;
    for await (const chunk of res) {
      size += chunk.length;
      if (size > 1024 * 1024) throw new Error("latest 响应超过 1MB，拒绝");
      chunks.push(chunk);
    }
    const tag = String(JSON.parse(Buffer.concat(chunks).toString("utf8")).tag_name || "");
    const v = tag.trim().replace(/^v/, "");
    if (!/^\d+\.\d+\.\d+$/.test(v)) throw new Error(`latest 版本号异常：${tag}`);
    return v;
  } catch {
    return pkgVersion;
  }
}

function baseLabel(base) {
  return base === DIRECT_BASE ? "GitHub 直连" : `镜像 ${new URL(base).host}`;
}

function assetUrls(tag, version = pkgVersion) {
  const base = `${releaseBase()}/v${version}`;
  const asset = `pi-flash-${version}-${tag}.zip`;
  return {
    asset,
    zip: `${base}/${asset}`,
    sidecar: `${base}/${asset}.sha256`,
    sums: `${base}/SHA256SUMS`,
    releasePage: `https://github.com/danielbill/pi-flash/releases/tag/v${version}`,
  };
}

// --- 载荷路径与状态 -----------------------------------------------------------

function exeRelPath(tag) {
  return tag.startsWith("darwin")
    ? path.join("pi-flash.app", "Contents", "MacOS", "pi-flash")
    : "pi-flash.exe";
}

function exePath(tag = platformTag()) {
  return path.join(PAYLOAD_DIR, exeRelPath(tag));
}

// x.y.z 逐段数值比较（081：应用内更新后载荷可能比薄壳新）
function compareVersions(a, b) {
  const ka = String(a).trim().replace(/^v/, "").split(".").map((p) => parseInt(p, 10) || 0);
  const kb = String(b).trim().replace(/^v/, "").split(".").map((p) => parseInt(p, 10) || 0);
  const n = Math.max(ka.length, kb.length);
  for (let i = 0; i < n; i += 1) {
    const x = ka[i] || 0;
    const y = kb[i] || 0;
    if (x !== y) return x < y ? -1 : 1;
  }
  return 0;
}

// --- 载荷目录解析（081 v0.1.11 改版）---------------------------------------------
// 安装目录 payload/ 是稳定入口（薄壳命令与快捷方式都指它）；应用内更新把新载荷
// 解压到 <pkg>/payload-<版本>/，**只解压不 rename**（0.1.6~0.1.10 的落位死循环
// 全源自整目录 rename 撞 Windows 文件句柄）。这里选**版本最高的完整载荷**启动：
// 目录名带 -<x.y.z> 且 .payload-version 戳吻合、exe 在位（戳最后写，戳在 = 完整）。
function stampOfDir(dir) {
  try {
    return fs.readFileSync(path.join(dir, ".payload-version"), "utf8").trim() || null;
  } catch {
    return null;
  }
}

function resolvePayload(tag = platformTag()) {
  let best = { dir: PAYLOAD_DIR, version: stampOfDir(PAYLOAD_DIR) };
  let entries = [];
  try {
    entries = fs.readdirSync(pkgDir);
  } catch {}
  for (const entry of entries) {
    const m = /^(.+)-(\d+(?:\.\d+)+)$/.exec(entry);
    if (!m) continue;
    const dir = path.join(pkgDir, entry);
    if (stampOfDir(dir) !== m[2]) continue;
    if (!fs.existsSync(path.join(dir, exeRelPath(tag)))) continue;
    if (compareVersions(m[2], best.version || "0") > 0) best = { dir, version: m[2] };
  }
  return best;
}

// 载荷可信 = 最优载荷的版本戳不旧于包版本（应用内自动更新先行：戳比包版本新
// 也算可信），且 exe 在位。戳更旧/缺失才走 ensure 补齐（不降级）。
function payloadOk(tag = platformTag()) {
  const best = resolvePayload(tag);
  if (compareVersions(best.version || "0", pkgVersion) < 0) return false;
  return fs.existsSync(path.join(best.dir, exeRelPath(tag)));
}

// --- 网络 -------------------------------------------------------------------

// 测速切换：开传 probeMs（默认 3s）时平均速度仍低于 minSpeedBps（默认
// 128KB/s）→ 放弃当前源换下一个。环境变量可调，便于慢网测试。
function minSpeedBps() {
  return (parseFloat(process.env.PI_FLASH_MIN_KBPS) || 128) * 1024;
}

function probeMs() {
  return parseInt(process.env.PI_FLASH_PROBE_MS, 10) || 3000;
}

function fmtMB(n) {
  return `${(n / 1024 / 1024).toFixed(1)}MB`;
}

function fmtSpeed(bps) {
  return bps >= 1024 * 1024
    ? `${(bps / 1048576).toFixed(1)}MB/s`
    : `${Math.round(bps / 1024)}KB/s`;
}

// 进度行：\r 原地刷新，尾部补空格清残影（postinstall 的 stderr 直接进终端）
let progressLen = 0;

function drawProgress(got, total, bps) {
  const speedText = bps > 0 ? ` ${fmtSpeed(bps)}` : "";
  const line =
    total > 0
      ? `   [${String(Math.min(99, Math.floor((got / total) * 100))).padStart(3)}%] ${fmtMB(got)}/${fmtMB(total)}${speedText}`
      : `   ${fmtMB(got)}${speedText}`;
  const pad = " ".repeat(Math.max(0, progressLen - line.length));
  process.stderr.write(`\r${line}${pad}`);
  progressLen = line.length;
}

function clearProgress() {
  if (progressLen > 0) {
    process.stderr.write(`\r${" ".repeat(progressLen)}\r`);
    progressLen = 0;
  }
}

function httpGet(url, redirectsLeft = 5, timeoutMs = 30_000) {
  return new Promise((resolve, reject) => {
    const lib = url.startsWith("http:") ? http : https;
    const req = lib.get(url, { headers: { "user-agent": "pi-flash-installer" } }, (res) => {
      const status = res.statusCode || 0;
      if (status >= 300 && status < 400 && res.headers.location) {
        res.resume();
        if (redirectsLeft <= 0) return reject(new Error(`重定向次数过多：${url}`));
        return resolve(httpGet(new URL(res.headers.location, url).toString(), redirectsLeft - 1, timeoutMs));
      }
      if (status !== 200) {
        res.resume();
        const err = new Error(`HTTP ${status}: ${url}`);
        err.status = status;
        return reject(err);
      }
      resolve(res);
    });
    req.setTimeout(timeoutMs, () => req.destroy(new Error(`请求超时：${url}`)));
    req.on("error", reject);
  });
}

// 单源下载：进度回调 + 速度守卫——开传满 probeMs 平均速度仍不达标则中止
// （ETOO_SLOW），由上层换源；30s 无活动超时兜底完全断流。
async function downloadToFile(url, dest, { onProgress } = {}) {
  const res = await httpGet(url);
  const total = parseInt(res.headers["content-length"] || "", 10) || 0;
  return new Promise((resolve, reject) => {
    const out = fs.createWriteStream(dest);
    let got = 0;
    let settled = false;
    const t0 = Date.now();
    const settle = (fail, err) => {
      if (settled) return;
      settled = true;
      clearInterval(timer);
      if (fail) {
        out.destroy();
        res.destroy();
        reject(err);
      } else {
        resolve();
      }
    };
    const timer = setInterval(() => {
      const elapsed = Date.now() - t0;
      const speed = elapsed > 0 ? got / (elapsed / 1000) : 0;
      if (onProgress) onProgress(got, total, speed);
      if (elapsed >= probeMs() && speed < minSpeedBps()) {
        const err = new Error(
          `${fmtSpeed(speed)} < 阈值 ${fmtSpeed(minSpeedBps())}`,
        );
        err.code = "ETOO_SLOW";
        settle(true, err);
      }
    }, 500);
    res.on("data", (chunk) => {
      got += chunk.length;
    });
    res.pipe(out);
    out.on("finish", () => settle(false));
    out.on("error", (err) => settle(true, err));
    res.on("error", (err) => settle(true, err));
  });
}

// 多源 failover：按序试 downloadBases()，速度过慢/网络错误都换下一个，
// 全灭才报错（附 release 页与 PI_FLASH_MIRROR 用法）。返回命中的 base，
// 供校验和钉定官方源用（080 §3.2）。
async function downloadAssetFile(asset, dest, version = pkgVersion) {
  const errors = [];
  for (const base of downloadBases()) {
    const label = baseLabel(base);
    process.stderr.write(`   源：${label}\n`);
    try {
      await downloadToFile(`${base}/v${version}/${asset}`, dest, {
        onProgress: drawProgress,
      });
      clearProgress();
      return base;
    } catch (err) {
      clearProgress();
      const why =
        err.code === "ETOO_SLOW" ? `测速不过（${err.message}）` : err.message;
      console.error(`   ${label} 不可用（${why}），换下一个源`);
      errors.push(`${label}：${why}`);
    }
  }
  throw new Error(
    `所有下载源均失败：\n    ${errors.join("\n    ")}\n` +
      `  可设 PI_FLASH_MIRROR=<base> 指定唯一源（布局 <base>/v<version>/<asset>），\n` +
      `  或手动下载：${assetUrls(platformTag()).releasePage}`,
  );
}

async function fetchText(url) {
  const res = await httpGet(url);
  const chunks = [];
  let size = 0;
  for await (const chunk of res) {
    size += chunk.length;
    if (size > 2 * 1024 * 1024) {
      res.destroy();
      throw new Error(`响应异常（超过 2MB，拒绝）：${url}`);
    }
    chunks.push(chunk);
  }
  return Buffer.concat(chunks).toString("utf8");
}

// --- 校验与解压 ---------------------------------------------------------------

function sha256File(filePath) {
  return new Promise((resolve, reject) => {
    const hash = crypto.createHash("sha256");
    const stream = fs.createReadStream(filePath);
    stream.on("error", reject);
    stream.on("data", (chunk) => hash.update(chunk));
    stream.on("end", () => resolve(hash.digest("hex")));
  });
}

// 解析 sha256sum 格式（兼容 `<hash>  file` 与 `<hash> *file`）
function parseHashFor(text, filename) {
  for (const line of text.split(/\r?\n/)) {
    const m = /^([a-fA-F0-9]{64})\s+\*?(.+)$/.exec(line.trim());
    if (m && m[2].trim().replace(/^\.\//, "") === filename) return m[1].toLowerCase();
  }
  return null;
}

// 校验和取用顺序：直连 sidecar → 直连 SHA256SUMS → 命中源 sidecar → 命中源
// SHA256SUMS。直连优先钉定官方哈希（zip 走镜像时防串包）；单请求不做重试，
// 源足够多，网络不通（非 404）直接换下一源。
async function resolveExpectedHash(asset, chosenBase, version = pkgVersion) {
  const bases =
    chosenBase === DIRECT_BASE ? [DIRECT_BASE] : [DIRECT_BASE, chosenBase];
  for (const base of bases) {
    for (const url of [
      `${base}/v${version}/${asset}.sha256`,
      `${base}/v${version}/SHA256SUMS`,
    ]) {
      let text;
      try {
        text = await fetchText(url);
      } catch (err) {
        if (err.status === 404) continue; // sidecar 缺失 → 同源兜底 SHA256SUMS
        break; // 该源网络不可用 → 换源
      }
      const hash = parseHashFor(text, asset);
      if (hash) return hash;
    }
  }
  throw new Error(
    `在 release 资产里找不到 ${asset} 的 SHA-256（试过直连与 ${chosenBase}）。\n` +
      `  请到 ${assetUrls(platformTag()).releasePage} 核对；不要在未校验的情况下手动安装载荷。`,
  );
}

function extractZip(zipPath, destDir) {
  const args = ["-xf", zipPath, "-C", destDir];
  const quiet = { stdio: ["ignore", "pipe", "pipe"], encoding: "utf8" };
  if (process.platform === "win32") {
    // 条目名无需任何特判：release.sh 产物非 ASCII 名一律 EFS/UTF-8
    // （zip_name_check.mjs 硬门禁），bsdtar / Expand-Archive 都按标志正确解
    // （0.5.0 曾发 GBK 无标志名，Rust 更新器按 CP437 解出乱码文件名）。
    // 首选系统自带 bsdtar 的绝对路径（Win10 1803+ 支持 zip）。不能裸调 PATH 里的
    // "tar"：git-bash 环境会抢到 GNU tar，而它把 "D:" 当远程主机名直接报错。
    const sysTar = process.env.SystemRoot
      ? path.join(process.env.SystemRoot, "System32", "tar.exe")
      : null;
    const tarBin = sysTar && fs.existsSync(sysTar) ? sysTar : "tar";
    // 首次尝试静默：回落成功时不给用户看噪音；双双失败才带输出报错
    let r = spawnSync(tarBin, args, { ...quiet, windowsHide: true });
    if (r.status === 0) return;
    const tarErr = (r.error ? r.error.message : r.stderr || "").trim() || "(无输出)";
    const ps =
      `Expand-Archive -LiteralPath '${zipPath.replace(/'/g, "''")}' ` +
      `-DestinationPath '${destDir.replace(/'/g, "''")}' -Force`;
    r = spawnSync(
      "powershell.exe",
      ["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", ps],
      { ...quiet, windowsHide: true },
    );
    if (r.status === 0) return;
    const psErr = (r.error ? r.error.message : r.stderr || "").trim() || "(无输出)";
    throw new Error(`解压失败：\n    ${tarBin}: ${tarErr}\n    PowerShell: ${psErr}`);
  }
  const r = spawnSync("tar", args, quiet);
  if (r.status !== 0) {
    throw new Error(`解压失败：tar 退出码 ${r.status} ${(r.stderr || "").trim()}`);
  }
}

// zip 根布局：win32 带 pi-flash/ 包装目录（release.sh 压缩产物约定，压缩器无关），darwin 根即 .app
function locateContentRoot(extractDir, tag) {
  const exeRel = exeRelPath(tag);
  for (const root of [path.join(extractDir, "pi-flash"), extractDir]) {
    if (fs.existsSync(path.join(root, exeRel))) return root;
  }
  throw new Error(`载荷结构异常：${extractDir} 下找不到 ${exeRel}`);
}

// --- 落位 ---------------------------------------------------------------------

function isBusyError(err) {
  return ["EBUSY", "EPERM", "ENOTEMPTY"].includes(err && err.code);
}

function busyMessage() {
  return "载荷目录被占用——请先退出正在运行的 pi-flash，再重新执行安装/更新。";
}

// 清本机残留（080 §3.3）：超过 1 小时未动过的半截下载 payload.tmp-<pid>
// （活跃下载持续写盘 mtime 常新，停更 1 小时 = 进程已死）。每次启动与下载前都跑。
function rmLeftovers() {
  let entries;
  try {
    entries = fs.readdirSync(pkgDir);
  } catch {
    return;
  }
  for (const entry of entries) {
    if (!entry.startsWith(TMP_PREFIX)) continue;
    const p = path.join(pkgDir, entry);
    try {
      if (Date.now() - fs.statSync(p).mtimeMs > 3600_000) {
        fs.rmSync(p, { recursive: true, force: true });
      }
    } catch {}
  }
}

// 整目录替换：删旧 payload → 逐项 rename（同卷、原子）→ 失败给可操作的提示
function replacePayload(contentRoot) {
  try {
    fs.rmSync(PAYLOAD_DIR, { recursive: true, force: true });
  } catch (err) {
    if (isBusyError(err)) throw new Error(busyMessage());
    throw err;
  }
  fs.mkdirSync(PAYLOAD_DIR, { recursive: true });
  for (const entry of fs.readdirSync(contentRoot)) {
    const from = path.join(contentRoot, entry);
    const to = path.join(PAYLOAD_DIR, entry);
    try {
      fs.renameSync(from, to);
    } catch (err) {
      if (isBusyError(err)) throw new Error(busyMessage());
      throw err;
    }
  }
}

// 主流程：版本戳不合 → 清残留 → 预取真实最新版（壳可能来自滞后的 npm 镜像，
// 载荷版本以 GitHub releases 为准）→ 测速选源下载 → 校验 → 解压 → 落位 →
// 最后写版本戳
async function ensure({ force = false, version = null } = {}) {
  const tag = platformTag(); // 不支持的平台在这里就明确报错
  if (!force && payloadOk(tag)) return;

  const ver = version || (await resolveLatestVersion());
  const urls = assetUrls(tag, ver);
  rmLeftovers();
  const tmp = path.join(pkgDir, `${TMP_PREFIX}${process.pid}`);
  fs.rmSync(tmp, { recursive: true, force: true });
  fs.mkdirSync(tmp, { recursive: true });

  try {
    const zipPath = path.join(tmp, urls.asset);
    const t0 = Date.now();
    console.error(`   下载载荷 ${urls.asset}`);
    const chosen = await downloadAssetFile(urls.asset, zipPath, ver);
    console.error(
      `   下载完成，用时 ${Math.max(1, Math.round((Date.now() - t0) / 1000))}s（源：${baseLabel(chosen)}）`,
    );

    const expected = await resolveExpectedHash(urls.asset, chosen, ver);
    const actual = await sha256File(zipPath);
    if (actual !== expected) {
      throw new Error(
        `SHA-256 不符（期望 ${expected}，实际 ${actual}）。\n` +
          `  已删除下载文件。请到 ${urls.releasePage} 手动核对，或稍后重试。`,
      );
    }

    const extractDir = path.join(tmp, "x");
    fs.mkdirSync(extractDir, { recursive: true });
    extractZip(zipPath, extractDir);

    replacePayload(locateContentRoot(extractDir, tag));
    fs.writeFileSync(STAMP_FILE, `${ver}\n`); // 版本戳最后写：中途崩溃 = 未完成，下次重来
    console.error(`   载荷就绪：${exePath(tag)}（v${ver}）`);
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

module.exports = {
  pkgDir,
  pkgVersion,
  MIN_NODE,
  parseNodeVersion,
  isNodeVersionSupported,
  nodeVersionMessage,
  platformTag,
  releaseBase,
  downloadBases,
  baseLabel,
  assetUrls,
  exeRelPath,
  exePath,
  resolvePayload,
  payloadOk,
  compareVersions,
  minSpeedBps,
  probeMs,
  fmtMB,
  fmtSpeed,
  sha256File,
  parseHashFor,
  resolveExpectedHash,
  downloadToFile,
  downloadAssetFile,
  resolveLatestVersion,
  extractZip,
  ensure,
  busyMessage,
  rmLeftovers,
};
