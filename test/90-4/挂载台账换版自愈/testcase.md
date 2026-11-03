# 90-4 §9 挂载台账版本层自愈 — Testcase

背景（真机 2026-11-03，0.4.0 发版测试）：微信插件绑定后重启即失效——台账
`session-ext.json` 里存的是 `plugins/<pkg>/0.1.4/pi/index.mjs`（绑定当时的版本
目录），插件升到 0.2.0 后旧目录被清，spawn 把死路径当 `-e` 传给 pi，pi
**加载扩展失败直接退出**，会话进程起不来，ext（收发/轮询本体）无处安身：
无「已连接」上线通知、微信发消息无响应。

修法：`pi_link::session_ext::resolve_sources`（版本层重定位 / 死条目摘除 /
重复去重 / 外来绝对路径原样）＋各消费点自愈回写。设计见
`docs/模块设计/90-插件系统/90-4-插件存储与运行.md` §9。

## 一 单元（已自动化）

`RUST_MIN_STACK=33554432 cargo test -p pi-link resolve_mount`
（`crates/pi-link/src/session_ext.rs`）

| # | 用例 | 夹具 | 断言 |
|---|------|------|------|
| U1 | `stale_version_dir_heals_to_current` | `<pkg>/0.2.0/pi/index.mjs` 在盘，台账写 `0.1.4` | 重定位到 0.2.0 真文件；返回 changed=true（要求回写台账）；`npm:` 源原样 |
| U2 | `flat_layout_is_left_alone` | dev 平铺 `<pkg>/pi/index.mjs` | 原样返回、changed=false（不自愈不写盘） |
| U3 | `dead_plugin_entry_is_dropped` | 包在，但条目文件名已不存在 | 条目摘除（死 `-e` 不许喂给 pi） |
| U4 | `foreign_absolute_path_is_preserved` | 非插件包的绝对路径（不存在） | 原样保留（不替用户判生死） |
| U5 | `duplicates_collapse` | 同文件「0.2.0 + 0.1.4」两条 | 去重成一条（`-e` 挂两次 = ext 双开） |

`cargo test -p app has_live_service_tests`
（`crates/app/src/pf_plugin/host_actions.rs`）

| # | 用例 | 断言 |
|---|------|------|
| U6 | `mount_hit_exempts_session` | 反斜杠/正斜杠 + 盘符大小写不同仍命中（闲置回收豁免） |
| U7 | `version_layer_rewrite_still_counts_as_live` | 台账 0.1.4 vs 存活清单 0.2.0 = 同一挂载；不同文件名不命中 |
| U8 | `unrelated_mounts_do_not_exempt` | `npm:`/`git:` scheme 源不构成豁免 |
| U9 | `other_plugin_mounts_do_not_exempt` | 台账挂 A、存活清单只有 B → 不豁免 |

## 二 真机复现口径（0.4.0 发版包，无需改代码）

前置：`~/.pi-flash/plugins/@pi-flash-plugins__pf-wxbot/` 下只有 `0.2.0/`（升级已
清旧版本），`session-ext.json` 里绑定会话那条仍写 `0.1.4`。

| # | 步骤 | 修复前（复现） | 修复后 |
|---|------|----------------|--------|
| R1 | 启动 PF（绑定会话在恢复范围内） | pi 立即退出，应用 stderr：`pi stderr tail: Error: Failed to load extension "…0.1.4\pi\index.mjs": Extension path does not exist` | 无此错误；`session-ext.json` 该条已改写成 `…\0.2.0\pi\index.mjs` |
| R2 | 看会话进程 | 无 pi 子进程 | pi 存活，命令行 `-e …\0.2.0\pi/index.mjs` |
| R3 | 插件 `data/wx-state.json` | 无新增 `svc_log` | `svc_log` 出 `boot key=… pf_enabled=true` + `on` |
| R4 | 微信侧 | 无消息 | 收到「已连接」上线通知；发消息有回复 |

辅助命令：

```bash
# 看 spawn 实参（Windows）
powershell -NoProfile -Command "Get-CimInstance Win32_Process -Filter \"Name='node.exe'\" | \
  Where-Object {\$_.CommandLine -like '*pi-coding-agent*'} | Select-Object CommandLine | Format-List"
# 看台账与自愈结果
cat ~/.pi-flash/session-ext.json
```

## 三 人工验收清单（发版包）

| # | 场景 | 预期 |
|---|------|------|
| M1 | 已绑定微信 → 关闭 PF → 重启 | 微信收到「已连接」；发消息有回复（无需先手动发一条） |
| M2 | 插件市场把 pf-wxbot 0.1.x 升到 0.2.x（绑定态保留）→ 重启 | 同上；台账自动改写为新版本路径，不留旧版本条目 |
| M3 | 插件停用 → 启用（含重启后） | ext 重挂、恢复收发；台账无重复条目 |
| M4 | 插件卸载 → 重启 | 台账里该插件的死条目被摘除；其他会话正常启动（不被死 `-e` 毒死） |
| M5 | 绑定会话闲置超过回收时限（`idle_cap`） | 不被回收（豁免按版本无关比较），微信不静默 |
