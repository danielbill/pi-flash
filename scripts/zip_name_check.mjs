#!/usr/bin/env node
// zip 条目名编码硬门禁（release.sh 压缩后必跑，0.5.0 乱码实录）。
//
// 病灶：libarchive（System32 bsdtar）在 Windows 上默认把非 ASCII 条目名按
// 本机 ACP（zh-CN = GBK）写入 zip 且不置 EFS 标志；解压端三种行为各异——
// 同支 bsdtar 按 ACP 回解（npm 链，侥幸正确）、Rust zip crate 按 CP437 解
// （应用内更新链，落盘 `╞⌠╢»╦╡├≈.txt` 乱码名）。0.5.1 起打包统一
// `--options zip:hdrcharset=UTF-8` 写 EFS 名，本门禁保证产物不回退：
//
//   规则① 任何含非 ASCII 字节的条目名必须置 EFS（general purpose bit 11）
//         且为合法 UTF-8；
//   规则② `pi-flash/启动说明.txt` 必须在包（启动说明随包是发版约定）。
//
// 用法：node scripts/zip_name_check.mjs <zip>   （失败 exit 1，阻断发版）
// 说明：只认经典 EOCD（条目 < 65535、32 位偏移），本仓产物（~1.7 万条 /
// ~96MB）远未触 zip64 阈值；触线时 zip crate 会写 zip64 EOCD，届时再扩。
import { readFileSync } from "node:fs";

const zipPath = process.argv[2];
if (!zipPath) {
  console.error("用法: node scripts/zip_name_check.mjs <zip>");
  process.exit(2);
}
const buf = readFileSync(zipPath);
const eocd = buf.lastIndexOf("PK\x05\x06");
if (eocd < 0) {
  console.error(`✗ 不是 zip（找不到 EOCD）: ${zipPath}`);
  process.exit(1);
}
const count = buf.readUInt16LE(eocd + 10);
let p = buf.readUInt32LE(eocd + 16);
const utf8Decoder = new TextDecoder("utf-8", { fatal: true });
const names = new Set();
const bad = [];
for (let i = 0; i < count; i++) {
  if (buf.readUInt32LE(p) !== 0x02014b50) {
    console.error(`✗ 中央目录损坏（偏移 ${p}）: ${zipPath}`);
    process.exit(1);
  }
  const flags = buf.readUInt16LE(p + 8);
  const nameLen = buf.readUInt16LE(p + 28);
  const extraLen = buf.readUInt16LE(p + 30);
  const cmtLen = buf.readUInt16LE(p + 32);
  const raw = buf.subarray(p + 46, p + 46 + nameLen);
  const hasHigh = raw.some((b) => b >= 0x80);
  let name = null;
  if (hasHigh) {
    const efs = (flags & 0x800) !== 0;
    try {
      name = utf8Decoder.decode(raw);
    } catch {
      name = null;
    }
    if (!efs || name === null) {
      bad.push({ efs, name: raw.toString("latin1") });
    }
  }
  if (name === null) name = raw.toString("latin1");
  names.add(name);
  p += 46 + nameLen + extraLen + cmtLen;
}

if (bad.length > 0) {
  console.error(
    `✗ ${bad.length}/${count} 个条目名未按 EFS/UTF-8 写入——解压端会落盘乱码名` +
      `（0.5.0 实录：启动说明.txt → ╞⌠╢»╦╡├≈.txt）。压缩命令缺` +
      ` --options zip:hdrcharset=UTF-8？`,
  );
  for (const b of bad.slice(0, 10)) {
    console.error(`    efs=${b.efs} ${JSON.stringify(b.name)}`);
  }
  process.exit(1);
}
const readme = "pi-flash/启动说明.txt";
if (!names.has(readme)) {
  console.error(`✗ 包内缺 ${readme}（启动说明必须随包，release.sh heredoc）`);
  process.exit(1);
}
console.log(
  `    条目名编码合格（${count} 条全查，非 ASCII 名均 EFS/UTF-8；${readme} 在包）`,
);
