# 042-1 回归：auth.json 标记条目去路径化（pif-secret 入用户级 PATH）

## 背景（0.4.0→0.4.1 升级 auth 全崩事故）

PF 写进 auth.json 的标记条目携带 `pif-secret.exe` **绝对路径** =
`current_exe()` 同目录 = 版本化载荷目录 `payload-<版本>\`。升级后旧目录被
`cleanup_older`（updater）清掉 → pi 的 `!` 命令执行失败 → 全部 provider 判
「未配置」。用户手改成 `payload-0.4.1` 路径自救，0.4.2 升级还会再崩一次。

**治本（051 扩展）**：helper 落用户级稳定目录 `%LOCALAPPDATA%\pi-flash\bin`
并加入 HKCU `Environment\Path`（判重追加、保 REG_EXPAND_SZ 原类型、
WM_SETTINGCHANGE 广播、本进程 PATH 同步追加），标记条目改写**零路径裸命令**
`!pif-secret get <provider>`——升级/重装/搬家永不再崩。pi 侧 `!` 整串交
`bash -c`/`execSync`，均走 PATH 解析（vendor/pi resolve-config-value.js 已核实）。

## 代码落点

- `pi-link/src/secret_bin.rs`：`ensure_installed()`（幂等）+ `path_mode_ok()`
  + `path_has_segment`（纯函数）。
- `pi-link/src/auth_json.rs`：`marker_key_with(path_mode, provider)` 双形态；
  `migrate_markers_pathless[_at]`（只动 `is_pf_managed` 且 key 含 `/` 或 `\`
  的条目；oauth/用户明文/用户 `!` 引用不动；不可读拒写；幂等）。
- 挂载：`startup::spawn_boot_tasks` 最前（后台线程，抢先于升级 cleanup 线程）
  + `updater::fetch::stage_update_in` 落位后 + `auth_json::store_secret`
  PATH_MODE 未决时同步 ensure。

## 单测（pi-link，全绿）

- `secret_bin::tests::path_segment_match_is_normalized`：判重（大小写/尾分隔
  符/空段/`%VAR%` 不误判/前缀同段不同不误判）。
- `secret_bin::tests::stable_bin_dir_layout`。
- `auth_json::tests::marker_key_is_shell_safe`：双形态 shell 安全。
- `auth_json::tests::migrate_rewrites_path_markers_to_bare_form`：路径形态
  （含 0.4.1 用户手改形态）→ 裸；oauth/明文/用户引用不动；幂等二跑 0。
- `auth_json::tests::migrate_off_leaves_file_untouched`。
- `auth_json::tests::migrate_refuses_unreadable_file`：拒写 = 文件零改动。

## 人工验收（PATH 写入是系统级副作用，agent 只验文件与单测，动注册表须用户确认）

1. 起 debug PF → `%LOCALAPPDATA%\pi-flash\bin\pif-secret.exe` 在位；
2. `reg query HKCU\Environment /v Path`：含该段、原 `%VAR%` 展开变量原样、
   不重复追加（二次启动幂等）；
3. auth.json 中 PF 条目被迁成 `!pif-secret get <provider>`（含手改的
   payload-0.4.1 形态）；
4. 已配 provider 发消息确认真 key 注入（PF 内 pi 解析链路）；
5. 老版本残留场景：auth.json 预置 `!'…/payload-0.4.0/pif-secret.exe' get x`
   → 启动一次后迁移成功，且旧目录删掉后 pi 仍能取 key（裸命令走 PATH）。
