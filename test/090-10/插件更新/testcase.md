# 090-10 PF 插件更新 — Testcase

测试项目：`D:\ai_workspace\pf_test`（pf-flash-dev 数据目录隔离）
前置：市场索引可拉取（`@pi-flash-plugins/market`），至少一个插件存在比台账更高版本。

## 单元（已自动化，cargo test -p app）

| # | 用例 | 断言 |
|---|------|------|
| U1 | semver 逐段比较 | `pf_updatable("1.9.0","1.10.0")` = true（字符串比较会错判） |
| U2 | 预发布号 | `("1.2.0-rc.1","1.2.0")` = true |
| U3 | 相等/降级 | `("1.2.0","1.2.0")`、`("2.0.0","1.9.9")` = false |
| U4 | 非 semver 回落 | `("dev","1.0.0")` = true（宁误报不漏报） |
| U5 | 空版本 | 任意一端为空 = false |

## UI/动作（人工验收清单）

**测法：dev-market 本地假 registry**（`test/090-10/dev-market/README.md`，Rust 零改动，
`PI_FLASH_NPM_REGISTRY` 指向本地即可整链路演练"发新版→出更新钮→覆盖安装"）：
feed 放线上版 tgz → 装它 → `node bump.mjs <slug> <新版>` → 市场重开 → 演练更新。
M1-M10 全部在此环境完成；台账降版本造假只作 dev-market 不可用时的备选。

| # | 场景 | 预期 |
|---|------|------|
| M1 | 市场页【所有插件】：已装且索引有更高版 | 版本号旁 ↑（accent 色），按钮由 ✓ 变 `[更新]` |
| M2 | 市场页：已装且同版本 | ✓ 绿不可点，无 ↑ |
| M3 | 已装页列表 | 可更新条目版本号旁 ↑ |
| M4 | 已装页详情 | 标题行垃圾桶左侧出「更新 ↑」按钮（accent 盒）；无新版不渲染 |
| M5 | 点击更新（市场卡或详情） | 按钮转 spinner；状态栏/错误行无报错 |
| M6 | 更新完成 | 按钮 ✓；台账版本 = 新版本；插件面板/状态栏 tab 可用（懒装载） |
| M7 | 更新中重复点击 | `pending` 单飞：其余更新/安装动作被忽略 |
| M8 | 带会话清单已挂载 pi 扩展的插件 | 更新后 ext 重挂回当前会话（`plugin_attach_pi_exts`，空闲即重启 pi） |
| M9 | 正在运行的插件更新 | evict 先行 unload + 释放 cwasm mmap → 覆盖目录不报 os error 32 |
| M10 | 回归：全新安装/启用/卸载 | 行为与更新前一致（`reload_plugin_by_pkg` 新参传 false 不影响原链路） |

## 迁移体验（0.3.2：契约 v5 一刀切后旧包的升级路径）

背景：市场安装链落 `<pkg>/<version>/`，此前 `dataDir = 清单目录/data` 挂在版本目录下，
更新一次丢一次；旧 schema 2 包被 `scan_manifests` 静默跳过（release 下无任何提示）。
修法：`PluginManifest.pkg_dir` + `data_dir()`（用户数据统一 `<pkg>/data`，与 90-4 设计对齐）、
启动期 `migrate_data_dirs` 一次性搬迁、`scan_manifests_full` 收集失效包 → `PfState.stale`
→ 启动 toast 一次 + 市场头部常驻通知行 + 已安装页「需更新」标签 + 详情状态文案。

### 单元（已自动化）

`cargo test -p pi-link migrate_data` / `cargo test -p app -- pf_plugin::manifest i18n_audit`

| # | 用例 | 断言 |
|---|------|------|
| U6 | 版本目录 data 搬到 `<pkg>/data` | `migrate_data_dirs` 返回包名；内容原样；源目录搬空（rename） |
| U7 | `<pkg>/data` 已存在 | 不动（幂等，二次启动 / 新版本目录又长 data 都不覆盖） |
| U8 | 平铺 dev 布局 | no-op；`assets/data` 之类非版本形目录不被误当版本层 |
| U9 | 台账版本目录 data 空 | 回落 mtime 最新且非空的版本目录 |
| U10 | 哪都没 data | 什么都不做 |
| U11 | `pkg_dir` 两档布局 | 版本布局 = 父目录、平铺 = 清单目录；`data_dir()` 都落 `<pkg>/data`；`dir` 仍是版本目录 |
| U12 | `scan_root` 失效包收集 | schema 2 台账包 → `StaleKind::SchemaUnsupported{2}` + version + title（peek）；合法包进 manifests；野包两边都不进 |
| U13 | 停用 / 其他清单错误 | 停用不进 stale（用户主动关的不催）；非 schema 错误归 `Invalid(原因)`，version 缺失回落目录名 |
| U14 | `scan_root` 只认台账版本目录（mda8） | 三个合法版本目录只装台账那个；0.1.10/0.1.9 字符串序陷阱不影响；残留不算失效；台账版本目录缺失不拿别的顶上 |
| U15 | `prune_old_versions` | 只删非台账版本的版本形目录（含其 `.cache`）；台账版本、`<pkg>/data`、`assets/` 不动 |
| U16 | prune 数据抢救 / 全量版 | 旧版本目录里唯一一份 data 先救到包层再删；`prune_stale_versions` 按台账遍历，台账外的包不碰 |

### L3 接线（pif-ui 自动化）

脚本：`bash test/090-10/插件更新/l3-stale-migrate.sh`（先 `cargo build`）。
播种 = 台账 quick-panel 0.1.4 + feed 里的 0.1.4.tgz（schema 2）+ `0.1.4/data/pins.lst`。

| # | 断言 | 读哪里 |
|---|------|--------|
| L1 | `<pkg>/data/pins.lst` 到位、`0.1.4/data` 搬空、内容两行原样 | 文件系统 |
| L2 | 揭幕后 toast「1 个插件与本版 PF 不兼容…快捷面板 v0.1.4」 | `snapshot app` → `status_toast`（揭幕前不播：2.5s 窗口会白播） |
| L3 | `pf_market_stale[0] = {name, version:"0.1.4", title:"快捷面板", kind:"schema:2"}` | `snapshot app` → `pf_market_stale`（只读字段） |
| L4 | `content.view view=pf-market` 开页 → `shot`：页签行下黄点通知行可见 | 截帧人眼/视觉复核 |
| L5 | 播种的游离 `0.1.3/` 残留目录启动后被清，台账版本 `0.1.4/` 保留（mda8） | 文件系统 |

### 人工验收（交付用户）

| # | 场景 | 预期 |
|---|------|------|
| M11 | 真机 `~/.pi-flash` 现有 quick-panel 0.1.4 → 启动新 PF | toast 一次；菜单 → PF 插件市场：头部通知行；【已安装】行「需更新」黄标签，详情「状态」= 与本版 PF 不兼容（清单 schema 2），请更新 |
| M12 | 市场 0.1.5 已发（`engines.pf >= 0.3.2`）→ 点 [更新] | 装到 `0.1.5/`，状态栏「快捷」tab 回来，**快捷项全在**（data 在 `<pkg>/data`）；通知行/标签消失 |
| M13 | 旧 PF 0.3.1 看到市场 0.1.5 | 点更新被 `min_pf` 挡下，提示需更新 PF；0.1.4 照常运行 |
