# 081 zip 条目名乱码：GBK 名无 EFS 标志 → CP437 落盘（0.5.0 实录）

## 背景（0.5.0 用户实录）

安装目录里出现乱码文件 `╞⌠╢»╦╡├≈.txt`，内容是 v0.5.0 启动说明，且与正确的
`启动说明.txt` 并存于稳定入口 `payload\`。完整链条：

1. **打包**：release.sh 的 System32 bsdtar（libarchive）把 zip 里唯一的非
   ASCII 条目名按**本机 ACP（zh-CN = GBK）**字节写入（`启动说明` =
   `C6F4 B6AF CBB5 C3F7`）且**不置 EFS 标志**（general purpose bit 11）；
2. **npm 安装链**（侥幸正确）：解压用同支 bsdtar，libarchive 对无标志名按
   ACP 回解 → 还原 `启动说明.txt`；
3. **应用内更新链**（病灶）：Rust zip crate 对无标志名一律按 **CP437** 解码 →
   `╞⌠╢»╦╡├≈.txt` 落进 `payload-0.5.0/`；
4. **并存机制**：`precopy_stable_entry`/`refresh_stable_entry` 把版本化目录
   整树**覆盖拷**进稳定入口，但 copy_payload_tree 只增不删 → 旧版正确名留存
   + 乱码名拷入，两份并存、乱码文件永驻。

## 修复（0.5.1，三重防御）

- **打包端**：release.sh bsdtar 加 `--options zip:hdrcharset=UTF-8`（实测写出
  UTF-8 名 + EFS 标志）；bsdtar 失败留半截产物先 rm 再回落 7z（防追加成混合包）
- **门禁**：`scripts/zip_name_check.mjs` 压缩后全查中央目录——非 ASCII 名必须
  EFS + 合法 UTF-8，且 `pi-flash/启动说明.txt` 必须在包；不过即拒发（压缩器无关）
- **解压端**：updater `decode_zip_entry_name`（download.rs）——ASCII/EFS-UTF-8
  原样收，其余按 GBK 回解（encoding_rs，救 0.5.0 遗留 zip），双解不动再 lossy
- **存量清扫**：`sweep_legacy_mojibake`（stable_entry.rs）在稳定入口刷新/预拷
  成功后点名删除两个已知乱码变体（CP437 `╞⌠╢»╦╡├≈.txt` / CP1252
  `Æô¶¯ËµÃ÷.txt`，码位写 `\u` 转义防抄错），正确名与无关文件不动

## 单测（app，updater::tests）

- `zip_entry_name_decode`：ASCII/EFS 原样、GBK 字节回解、0xFF 双不可解 lossy
- `extract_zip_rescues_legacy_gbk_name`：手造 0.5.0 形态 STORE zip（GBK 名 +
  标志位 0，`build_stored_zip` 手拼字节 + 就地 CRC-32）端到端——落盘正确名、
  乱码名不得出现
- `sweep_legacy_mojibake_targets_exact_names`：只删两个变体、正确名/无关文件
  不动、幂等
- 门禁脚本实测：0.5.0 坏 zip 拒（`1/16871` 点名乱码条目）；新参数打的包过

## 人工验收

1. `node scripts/zip_name_check.mjs dist/pi-flash-0.5.0-win32-x64.zip` → exit 1
   （存量坏产物被门禁识别）
2. 下个版本走完整 release.sh：第 6 步打印「条目名编码合格」；
3. 装了 0.5.0 的机器应用内更新到新版后，稳定入口 `payload\` 的乱码文件消失，
   `启动说明.txt` 为新版内容且唯一；
4. 解压目录（解压版）确认只有 `启动说明.txt`、无乱码名文件。
