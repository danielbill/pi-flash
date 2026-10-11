"use strict";
// pi-flash npm 薄壳的共享逻辑：Node/平台探测、下载源、载荷布局、校验、解压、落位。
// 设计蓝本：docs/模块设计/080-软件分发.md §3（冲突以该文为准）。
//
// 载荷布局（包目录内，npm uninstall 随包清除）：
//   <pkg>/payload/.payload-version       版本戳，最后写入——缺失/不符即视为载荷不可信
//   <pkg>/payload/pi-flash.exe           win32-x64（release zip 根为 pi-flash/ 包装目录）
//   <pkg>/payload/pi-flash.app/…         darwin-arm64（release zip 根即 .app）
//
// 下载源：默认 GitHub Releases；环境变量 PI_FLASH_MIRROR 整体覆盖 base
// （镜像须保持同构布局 <base>/v<version>/<asset>，见 080 §3.2）。
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

function releaseBase() {
  const mirror = process.env.PI_FLASH_MIRROR;
  if (mirror) return mirror.replace(/\/+$/, "");
  return "https://github.com/danielbill/pi-flash/releases/download";
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

// 载荷可信 = 版本戳与包版本一致、或戳比包版本**新**（应用内自动更新先行，
// 薄壳还是旧版），且 exe 存在。戳更旧/缺失才走 ensure 补齐（不降级）。
function payloadOk(tag = platformTag()) {
  try {
    const stamp = fs.readFileSync(STAMP_FILE, "utf8").trim();
    if (compareVersions(stamp, pkgVersion) < 0) return false;
    return fs.existsSync(exePath(tag));
  } catch {
    return false;
  }
}

// --- 应用内更新落位（081）：exe 把新载荷下到 staging + 标记，swap 在这里做
// （彼时旧进程已退出、目录无锁）。布局：
//   <pkg>/payload.stage-<ver>/    新载荷（exe 自带 .payload-version 戳）
//   <pkg>/update-staged.json      {"version","staged_at"}
const STAGE_MARKER = path.join(pkgDir, "update-staged.json");
const STAGE_PREFIX = "payload.stage-";
const OLD_PREFIX = "payload.old-";

function readStaged() {
  try {
    const staged = JSON.parse(fs.readFileSync(STAGE_MARKER, "utf8"));
    if (staged && typeof staged.version === "string" && staged.version.trim()) {
      return { version: staged.version.trim().replace(/^v/, "") };
    }
  } catch {}
  return null;
}

// 清残留：上次 swap 中断的 payload.old-* / 缺文件的 staging + 标记
function cleanupStale() {
  for (const entry of fs.readdirSync(pkgDir)) {
    if (entry.startsWith(OLD_PREFIX)) {
      try {
        fs.rmSync(path.join(pkgDir, entry), { recursive: true, force: true });
      } catch {}
    }
  }
  const staged = readStaged();
  if (!staged) {
    try {
      fs.rmSync(STAGE_MARKER, { force: true });
    } catch {}
    return;
  }
  const dir = path.join(pkgDir, `${STAGE_PREFIX}${staged.version}`);
  const complete =
    fs.existsSync(path.join(dir, exeRelPath(platformTag()))) &&
    fs.existsSync(path.join(dir, ".payload-version"));
  if (!complete) {
    try {
      fs.rmSync(dir, { recursive: true, force: true });
      fs.rmSync(STAGE_MARKER, { force: true });
    } catch {}
  }
}

// swap：rename payload→old → rename staging→payload → 删标记 → 删 old。
// 旧实例仍在跑（rename EBUSY/EPERM）则跳过，标记留给下次启动再试。
function applyStagedUpdate() {
  let staged;
  try {
    staged = readStaged();
  } catch {}
  if (!staged) return null;
  cleanupStale();
  const stageDir = path.join(pkgDir, `${STAGE_PREFIX}${staged.version}`);
  if (!fs.existsSync(stageDir)) {
    try {
      fs.rmSync(STAGE_MARKER, { force: true });
    } catch {}
    return null;
  }
  // 已应用过（戳不旧于 staging）→ 清标记走正常启动
  let stamp = null;
  try {
    stamp = fs.readFileSync(STAMP_FILE, "utf8").trim();
  } catch {}
  if (stamp && compareVersions(staged.version, stamp) <= 0) {
    try {
      fs.rmSync(STAGE_MARKER, { force: true });
    } catch {}
    return null;
  }
  const oldDir = path.join(pkgDir, `${OLD_PREFIX}${Date.now()}`);
  try {
    fs.renameSync(PAYLOAD_DIR, oldDir);
  } catch (err) {
    if (isBusyError(err)) return null; // 旧实例在跑：下次启动再换
    throw err;
  }
  try {
    fs.renameSync(stageDir, PAYLOAD_DIR);
    fs.rmSync(STAGE_MARKER, { force: true });
  } catch (err) {
    // swap 半途失败：payload 可能已不在——把 old 搬回来兜底
    try {
      if (!fs.existsSync(PAYLOAD_DIR)) fs.renameSync(oldDir, PAYLOAD_DIR);
    } catch {}
    throw err;
  }
  try {
    fs.rmSync(oldDir, { recursive: true, force: true });
  } catch {}
  return { from: stamp || "(缺失)", to: staged.version };
}

// --- 网络 -------------------------------------------------------------------

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function httpGet(url, redirectsLeft = 5) {
  return new Promise((resolve, reject) => {
    const lib = url.startsWith("http:") ? http : https;
    const req = lib.get(url, { headers: { "user-agent": "pi-flash-installer" } }, (res) => {
      const status = res.statusCode || 0;
      if (status >= 300 && status < 400 && res.headers.location) {
        res.resume();
        if (redirectsLeft <= 0) return reject(new Error(`重定向次数过多：${url}`));
        return resolve(httpGet(new URL(res.headers.location, url).toString(), redirectsLeft - 1));
      }
      if (status !== 200) {
        res.resume();
        const err = new Error(`HTTP ${status}: ${url}`);
        err.status = status;
        return reject(err);
      }
      resolve(res);
    });
    req.setTimeout(30_000, () => req.destroy(new Error(`请求超时（30s）：${url}`)));
    req.on("error", reject);
  });
}

async function downloadToFile(url, dest, redirectsLeft = 5) {
  const res = await httpGet(url, redirectsLeft);
  await new Promise((resolve, reject) => {
    const out = fs.createWriteStream(dest);
    res.pipe(out);
    out.on("finish", resolve);
    out.on("error", reject);
    res.on("error", reject);
  });
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

// 有限次重试 + 线性退避（仅网络类操作；校验失败不做无谓重试）
async function retry(op, label, attempts = 3) {
  let lastErr;
  for (let i = 1; i <= attempts; i += 1) {
    try {
      return await op();
    } catch (err) {
      lastErr = err;
      if (i < attempts) {
        console.error(`   ${label}失败（${err.message}），${i}s 后重试 ${i}/${attempts - 1}`);
        await sleep(i * 1000);
      }
    }
  }
  throw lastErr;
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

// 校验和取用顺序：<asset>.sha256（平台 sidecar，CI 生成）→ SHA256SUMS（release.sh 生成）
async function resolveExpectedHash(urls) {
  for (const url of [urls.sidecar, urls.sums]) {
    let text;
    try {
      text = await retry(() => fetchText(url), "获取校验和", 2);
    } catch (err) {
      if (err.status === 404) continue; // sidecar 缺失 → 兜底 SHA256SUMS
      throw err;
    }
    const hash = parseHashFor(text, urls.asset);
    if (hash) return hash;
  }
  throw new Error(
    `在 release 资产里找不到 ${urls.asset} 的 SHA-256（试过 ${urls.sidecar} 与 ${urls.sums}）。\n` +
      `  请到 ${urls.releasePage} 核对；不要在未校验的情况下手动安装载荷。`,
  );
}

function extractZip(zipPath, destDir) {
  const args = ["-xf", zipPath, "-C", destDir];
  const quiet = { stdio: ["ignore", "pipe", "pipe"], encoding: "utf8" };
  if (process.platform === "win32") {
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

// zip 根布局：win32 带 pi-flash/ 包装目录（release.sh Compress-Archive 行为），darwin 根即 .app
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

function rmStaleTmp() {
  for (const entry of fs.readdirSync(pkgDir)) {
    if (entry.startsWith(TMP_PREFIX)) {
      fs.rmSync(path.join(pkgDir, entry), { recursive: true, force: true });
    }
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

// 主流程：版本戳不合 → 下载 → 校验 → 解压 → 落位 → 最后写版本戳
async function ensure({ force = false } = {}) {
  const tag = platformTag(); // 不支持的平台在这里就明确报错
  if (!force && payloadOk(tag)) return;

  const urls = assetUrls(tag);
  const tmp = path.join(pkgDir, `${TMP_PREFIX}${process.pid}`);
  fs.rmSync(tmp, { recursive: true, force: true });
  rmStaleTmp();
  fs.mkdirSync(tmp, { recursive: true });

  try {
    const zipPath = path.join(tmp, urls.asset);
    console.error(`   下载载荷 ${urls.asset}`);
    console.error(`   ${urls.zip}`);
    await retry(() => downloadToFile(urls.zip, zipPath), "下载载荷");

    const expected = await resolveExpectedHash(urls);
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
    fs.writeFileSync(STAMP_FILE, `${pkgVersion}\n`); // 版本戳最后写：中途崩溃 = 未完成，下次重来
    console.error(`   载荷就绪：${exePath(tag)}（v${pkgVersion}）`);
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
  assetUrls,
  exeRelPath,
  exePath,
  payloadOk,
  compareVersions,
  readStaged,
  applyStagedUpdate,
  retry,
  sha256File,
  parseHashFor,
  resolveExpectedHash,
  extractZip,
  ensure,
  busyMessage,
};
