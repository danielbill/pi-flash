# 081 全局 pi 注册：内置内核注册为系统默认 `pi`

## 背景

系统 PATH 里的 `pi` 是老 npm 全局安装，PF 内置 vendored pi（pin 1.5.0）从不
对外注册。081 扩展（2026-11-04）：updater 落位 / 启动两个时机，把 `pi.exe`
前置转发器注册到 `%LOCALAPPDATA%\pi-flash\bin`（用户 Path **首位**段，shadow
老安装不卸载）+ 写 `pi-target.json` 指针指向载荷内 `node.exe` +
`vendor/pi/…/cli.js`。debug 构建默认零接触，`PI_FLASH_PI_SHIM=1` 显式开启。

## 代码落点

- `pi-link/src/register_to_sys.rs`：`ensure_pi_shim(node, cli)`（PATH 前置 +
  复制 pi.exe + 写指针，全幂等）、`pi_shim_enabled()`、
  `path_with_segment_front`（纯函数）、`write_pi_pointer`；
  `secret_bin.rs` 已改名 `register_to_sys.rs`。
- `pi-link/src/bin/pi.rs`：前置转发器（指针缺失/内核缺席 → stderr + exit 3）。
- 挂载：`startup::spawn_boot_tasks`（vendor 自愈）+ `updater::fetch::stage_update_in`
  （staged 新载荷）。
- `scripts/release.sh`：`--bin pi` 构建 + 包内硬门禁 + 启动说明。

## 单测（pi-link，全绿）

- `register_to_sys::tests::path_front_insert_move_and_noop`：缺席前插 / 已首位
  None / 大小写尾分隔符归一 / 空值 / 重复段摘除 / `%VAR%` 不误摘 / 空入参。
- `register_to_sys::tests::pi_pointer_idempotent`：同内容二写幂等、内容变化更新。
- `path_segment_match_is_normalized` / `stable_bin_dir_layout`（沿用）。

## 人工验收（注册是系统级副作用；agent 用 PI_FLASH_PI_SHIM=1 + 隔离实例验）

1. debug + `PI_FLASH_PI_SHIM=1` 起隔离实例 → bin 目录出现 `pi.exe` +
   `pi-target.json`（dev 上下文：node="node"、cli=仓内 vendor/pi）；
2. 直接跑 `%LOCALAPPDATA%\pi-flash\bin\pi.exe --version` → 1.5.0；
3. `reg query HKCU\Environment //v Path`：我们的段在**首位**、唯一、原类型
   保真（REG_SZ 机器不漂移成 REG_EXPAND_SZ）；
4. 新开 bash `pi --version` → 1.5.0（shadow 老 npm 全局安装；已开终端不变）；
5. 二轮启动幂等：指针零改动、Path 无重复；
6. `pi`（无参数）→ 正常进 pi TUI（vendored 1.5.0）；指针文件删掉再跑 →
   stderr 提示 + exit 3；
7. release.sh 改动走下个版本完整发版流程验收（本版不重打包）。
