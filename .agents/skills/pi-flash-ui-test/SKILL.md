---
name: pi-flash-ui-test
description: pi-flash UI 自动化测试工作流：用内置 pif-ui 链路驱动界面（启动隔离实例 → snapshot 断言 → exec 操作 → wait 等待 → shot 截屏），全程不抢真实屏幕/鼠标/键盘。凡是要验证 UI 改动、复现界面 bug、写 UI 自动化断言，或提到 pif-ui / UI 测试 / 自动化测试 / 界面快照 / 界面截屏，一律先按本 skill 操作。
---

# pi-flash UI 自动化测试

驱动 pi-flash 界面做自动化验证的唯一合法链路：app 进程内起自动化服务
（127.0.0.1 NDJSON），`pif-ui` CLI 发指令、读 JSON 结果。背景与完整设计见
`docs/模块设计/000-自动化测试.md`；方法清单唯一事实源是
`pi_link::automation::method` 常量表。

## 铁律

- **禁止抢真实屏幕/鼠标/键盘，禁止 OS 截屏**（PrintWindow/CopyFromScreen 都
  不行）。看界面用 `snapshot`（数据），看像素用 `shot`（进程内渲染回读）。
- **快照优先于像素，方法直调优先于事件模拟**——断言读 `snapshot` 的 JSON，
  不要靠截图猜。
- 操作一律走 `pif-ui`，不要手写 TCP 客户端（握手/token/重连细节都在 CLI 里）。

## 标准流程（从零到断言）

```bash
# 0. 构建（改动后先编，automation 是运行时开关不需要特殊 feature）
cargo build
# 产物：target/debug/pi-flash.exe（app）+ target/debug/pif-ui.exe（CLI 驱动）

# 1. 起一个隔离实例（每次测试会话用全新 tmp 目录，避免撞别人的会话/登记）
mkdir -p tmp/ui-test-$$
PI_FLASH_DIR=D:/ai_workspace/pi-flash/tmp/ui-test-$$ \
PI_FLASH_AUTOMATION=auto \
D:/ai_workspace/pi-flash/target/debug/pi-flash.exe \
  > D:/ai_workspace/pi-flash/tmp/ui-test-$$/app.log 2>&1 &

# 2. pif-ui 必须 export 同一个 PI_FLASH_DIR！不设它会去读 ~/.pi-flash/automation，
#    连到别的实例（或被过期登记糊一脸）。后面每条 pif-ui 命令都在这个环境下跑。
export PI_FLASH_DIR=D:/ai_workspace/pi-flash/tmp/ui-test-$$

# 3. 等启动完成，然后干活
D:/ai_workspace/pi-flash/target/debug/pif-ui.exe wait --path app.booted --truthy --timeout 30
D:/ai_workspace/pi-flash/target/debug/pif-ui.exe snapshot app --only window

# 4. 收尾：退出实例（别留后台进程）
D:/ai_workspace/pi-flash/target/debug/pif-ui.exe exec app.quit
```

## 事务式断言范式（核心）

每个测试 = `exec`（操作）→ `wait`（等状态）→ `snapshot`（读 JSON 断言），
全程 bash + 文本：

```bash
PUI=D:/ai_workspace/pi-flash/target/debug/pif-ui.exe
$PUI exec settings.open                          # 操作
$PUI wait --path app.dock_panel --eq settings    # 等状态成立
$PUI snapshot settings                           # 读 JSON 断言字段
```

- `wait --path` 支持 `a.b[0].c` 点分路径；断言三选一 `--eq/--contains/--truthy`；
  `--surface` 限定轮询面；成功只打印命中值，超时才附最后快照。
- 改状态的 exec 是同步的：返回后 `snapshot` 立即反映新状态；但 **`shot` 截的
  是最近一帧像素**，改状态后先 `wait` 再 `shot`（或隔一次 CLI 调用）。

## 命令速查

```bash
pif-ui list                        # 本机实例（探活标注，不删）
pif-ui snapshot [surface] [--only k]   # 8 面：app/sessions/session/composer/files/git/settings/dialogs
pif-ui exec <method> [--arg k=v]... [--params-file f.json]   # 任意 op
pif-ui keys "ctrl-s"               # 合成按键，走真实键位表
pif-ui type <text>                 # composer.set_text 的糖
pif-ui shot [D:/x/out.png]         # 截最近一帧 → PNG；缺省写 <配置目录>/automation/shots/
pif-ui wait --path a.b --eq v [--timeout 30]
pif-ui clean                       # 清死登记（只删明确拒绝连接的）
pif-ui exec input.focus '{"target":"git_commit"}'   # 焦点类 bug 用元素级聚焦
```

返回都是单行 JSON；`shot` 回 `{path,width,height,bytes}`，宽高可直接对
`snapshot app` 的 `window.width_px/height_px` 断言（物理像素）。

## 陷阱（每条都真踩过）

1. **pif-ui 与 app 必须同一个 `PI_FLASH_DIR`**。实例发现读
   `<配置目录>/automation/<pid>.json`；不 export 就读默认 `~/.pi-flash`，
   会连到用户自己的 pi-flash 上。
2. **过期登记会糊脸**：连不上报 `ConnectionRefused` 先 `pif-ui clean` 再重试；
   还不行就从 app 启动日志拿直连三件套——日志行
   `pi-flash automation: 127.0.0.1:<port> pid=<pid> token=<tok>`，
   `pif-ui --addr 127.0.0.1:<port> --token <tok> <命令>` 万能救回。
3. **token 只存在登记文件里**，误删登记 = 实例永久失联。`clean` 判死只认
   ConnectionRefused（超时/起不来都保守保留），不要手动删「看着像死了」的登记。
4. **全局选项必须在子命令之前**：`pif-ui --timeout 30 shot` 对，
   `pif-ui shot --timeout 30` 会把 `--timeout` 当 shot 的参数。
5. **Windows 路径用正斜杠** `C:/x/y`，或交给 `--arg path=C:/x/y`（值合法
   JSON 按 JSON、否则按字符串，免三层转义）。
6. shot 拍不到 OS 级装饰（标题栏/阴影是 DWM 的）；透明/模糊窗口的 alpha
   未验证（主窗不透明没事）。

## 加新 op / 深挖

- 新 op 四步：`pi_link::automation::method` 加常量 → `handlers.rs` dispatch
  加分支（改状态必须 `cx.notify()` 收尾）→（可选）pif-ui 加糖命令 →
  `docs/模块设计/000-自动化测试.md` §3 op 清单同步。
- 服务端线程形制、四层 Result 解包、`input.keys` 的 defer 重入坑等实现细节：
  读 `docs/模块设计/000-自动化测试.md` §2/§8，别凭直觉写。
