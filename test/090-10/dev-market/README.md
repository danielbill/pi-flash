# dev-market：本地假 npm registry（090-10 插件更新验收）

零依赖（Node 内置模块 + 系统 tar），**Rust 侧零改动**——pf_scan 全链路本来就走
`PI_FLASH_NPM_REGISTRY` 覆盖口（pi-link `market/mod.rs:31`）。

## 文件

- `server.mjs`：本地 registry。三个端点 = pf_scan 用到的全部：
  清单包 packument / 单包 packument（sha512 integrity 现算）/ tarball 直连。
  清单包 tgz 按 feed 现状自动生成（display 缺省回落 slug，验收够用）。
- `bump.mjs`：把 feed 里某包的最高版本造假成指定新版本（解包→改版本→重打）。
- `feed/`：放 `npm pack` 产物，命名 `pi-flash-plugins-<slug>-<version>.tgz`
  （npm pack 对 scoped 包的原生命名，不用手改）。

## 验收步骤（以 quick-panel 为例）

```bash
cd test/090-10/dev-market

# ① 拉"当前线上版"进 feed（只放一个版本——packument 永远给最高版）
npm pack @pi-flash-plugins/quick-panel --pack-destination feed/

# ② 起本地 registry
node server.mjs                # 默认 127.0.0.1:8477，--port 可换

# ③ PF 侧用 dev registry 启动（Git Bash）
PI_FLASH_NPM_REGISTRY=http://127.0.0.1:8477 cargo run -p app
```

```text
④ PF 市场【所有插件】找到 quick-panel → [安装] → 确认 ✓（此时装的是 feed 里的 v1）
⑤ 造新版（PF 可不关）：
   node bump.mjs quick-panel 9.9.9
⑥ PF 市场重开页面（open 即重扫）→ 卡片版本号 9.9.9 + ↑，✓ 变 [更新]
⑦ 已装页：列表 ↑、详情「更新 ↑」按钮
⑧ 点更新 → spinner → 完成 ✓ → 版本显示 9.9.9，面板/状态栏 tab 可用
⑨ 变体：
   - 更新中连点其他卡（单飞 M7）
   - 插件面板开着时点更新（cwasm mmap 释放，M9）
   - 带 pi 扩展挂载的插件更新后看会话 ext 重挂（M8）
   - node bump.mjs quick-panel 1.0.1 --drop-old 后重装/卸载回归（M10）
```

## 注意

- `PI_FLASH_NPM_REGISTRY` 是**全局覆盖**：dev registry 起着时，pi 扩展市场等
  其它 npm 消费方也会打到 127.0.0.1（它们不认我们的端点会失败）——只测 PF
  插件市场这条线，测完把环境变量去掉再测别的。
- 市场页空清单 = 扫描失败：看 server 控制台的请求日志与报错。
- `--drop-old` 模拟真实发版后旧版本消失；不加则 feed 双版本并存，方便反复演练
  「降版本 ↔ 出更新钮」。
- 本目录一切产物（feed/、.gen/）不入库——验收完可整目录删除。
