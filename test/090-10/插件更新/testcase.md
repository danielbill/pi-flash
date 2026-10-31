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
