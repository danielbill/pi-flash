# 030-流式卡死：快流 token 淹没下的 UI 可交互性

> bead pi-flash-yftj ｜ 2026-11-06 ｜ 模块：030 会话运行时（pump/notify）+ 090 插件宿主（扇出）

## 背景（真机现象）

v0.4.3 + DeepSeek V4.1 Flash（116.3 t/s）真机：agent 流式时整 app 卡死，
切快捷面板直接冻结。诊断见 bead 与 commit（泵批处理 + notify 节流 + 插件扇
出就绪检查）：

- 每条 delta 一次 `WeakEntity::update` + `cx.notify`（100+ 次/s），每拍全窗
  重建 + 流式尾 markdown 全量重解析 O(已流式字数) → 主线程前台队列饱和，
  输入饿死；
- `open_plugin_dock` → `broadcast::reset` → 下一拍对全体插件扇出，路径里
  `plugin_ready` 隐式首装 + 全量 render；`tick_session`「notified==0 不记
  账」在插件未就绪时退化为每 notify 全量扇出风暴。

## 载体

mock pi（`mock-pi-stream.mjs` emit 的 vendor 树），协议边界灌 `text_delta`：

- **flood**：2000 条 ~50B markdown 行，突发 1000 delta/s（真机 116 t/s 的
  8.6×），喂 markdown 重解析 + 全窗重建热路径；
- **exact**：130 条 3 位序号 delta（全文 390 字符 < 快照 `truncate(400)`），
  可做全文逐字节相等断言。

台架插件：`stream-test`（JS，清单同 `manifest/tests.rs` GOOD 夹具），平铺布
局 + `plugins.json` 台账条目（装载白名单 = 台账）。

## 用例

### S1 压测（修复后必须 pass）

流式中采样 10 次 `snapshot app` 往返 + 切插件面板：

- 单次往返 max < 3000ms、avg < 1000ms、slow（≥1500ms）≤2 次；
- `panel.dock panel=plugin ix=0` < 2500ms，且 `app.dock_panel_kind` 到
  plugin（ix=0）、可切回 sessions；
- 收场后头部标记 0..5 严格按序出现（批处理不乱序）。

### 修复前/后实测（2026-11-06，debug 构建，1000 delta/s 极限洪水）

| 指标 | 修复前 | 修复后 |
|---|---|---|
| pump 唤醒/2000 事件 | 2000（每事件一次 update+notify） | **13**（30ms 批窗口） |
| page_actived 派发 | **87 次**（notified==0 每 notify 重试风暴） | **2 次**（每会话键一次 + 装载补拍） |
| 洪水中 snapshot avg | 211–613ms（噪声带） | 260–613ms（同噪声带） |
| 切面板 exec | 322–582ms | 262–572ms（噪声带） |

残余延迟（两版共有）：流式块长到 ~2000 markdown 块后每帧重建 O(块数)，
属 gpui.md:185 架构项（Entity 无脏检查、list() 闭包吃不到 cached），不在本
bug 范围。真机卡死的机制（每事件泵唤醒 + 每 notify 插件扇出风暴 + 每 delta
markdown 全量重解析）由计数器行证明已消除。

### S2 完整性（修复后必须 pass）

exact 模式收场后，assistant 消息全文 == `"000001…129"`（130 条序号拼接）。
批处理窗口（30ms/256 条上限）不得丢、重、乱序任何一条 delta。

## 运行

```bash
cargo build
bash test/030-流式卡死/run.sh          # 全量
bash test/030-流式卡死/run.sh S1       # 只跑压测
```

产物落 `tmp/stream-jank/<CASE>/`（app.log 含 `PI_FLASH_PERF=1` 的泵批处理
耗时与 renders/s 行；wire.log 为协议留痕）。
