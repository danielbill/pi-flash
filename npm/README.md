# pi-flash

Pi-Flash (PF) —— 极速 pi coding agent 桌面端（Rust + GPUI）。本包是 **npm 薄壳**：
安装时从 GitHub Releases 按平台下载载荷（exe / node / 钉版 pi），SHA-256 校验后落位；
仓库与设计见 <https://github.com/danielbill/pi-flash>。

> **平台范围：目前只发布 Windows x64（win32-x64），不发布 macOS 版本。**
> 在 macOS 上执行安装会报「暂无发布资产」——这是预期行为，不是 bug。需要 macOS 版本请等后续公告。

## 安装与更新（同一条命令）

```bash
npm install -g pi-flash@latest   # 没装 = 首次安装；装过 = 更新
pi-flash                          # 启动
```

需要 Node.js ≥ 22.19。卸载与回滚：

```bash
npm uninstall -g pi-flash
npm install -g pi-flash@<旧版本>   # 回滚
npx pi-flash@latest                # 不常驻安装直接跑
```

> **npm 11.6+ / 12 用户注意**：npm 默认不再执行第三方包的 install 脚本（供应链防护）。
> 安装时若见 `install scripts not yet covered by allowScripts` 警告属**预期行为**——
> 载荷会在首次运行 `pi-flash` 时自动补齐（带下载进度与 SHA-256 校验），完成后正常启动。

## 环境变量

| 变量 | 作用 |
|---|---|
| `PI_FLASH_MIRROR` | 覆盖下载源（默认 `https://github.com/danielbill/pi-flash/releases/download`）。镜像须保持同构布局：`<base>/v<版本>/<资产名>` |

## 常见问题

- **安装时提示下载失败 / 校验不过**：重跑 `npm rebuild pi-flash`；或到
  [Releases](https://github.com/danielbill/pi-flash/releases) 手动核对后重试。
- **更新时提示"载荷目录被占用"**：先退出正在运行的 pi-flash，再执行安装命令。
- **postinstall 被 `--ignore-scripts` 或 npm 的 `allowScripts` 门控禁用**：不影响——`pi-flash`
  首次启动会自愈补齐载荷（会先打印「载荷缺失或版本不符，正在补齐…」并下载，约一两分钟）。
- **macOS 上安装报错**：macOS 不在支持范围（暂不发布 darwin 资产），请在 Windows x64 上使用。
- **非 win32-x64 平台**（Linux / 32 位 Windows 等）：同理暂无发布资产。

载荷落在本包目录 `payload/` 内，`npm uninstall` 一并清除。
