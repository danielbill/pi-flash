# test/ext-ui：扩展 UI 调用链路自动化台架（bead pi-flash-zi2s.1）

一条命令跑完全部扩展 UI 用例，人工不点。epic pi-flash-zi2s 的每条改动都必须
带这里的用例且跑绿（epic 验收：`bash test/ext-ui/run.sh` 全 PASS，XFAIL 翻案）。

```bash
bash test/ext-ui/run.sh          # 全量 8 例
bash test/ext-ui/run.sh C1 C7    # 只跑指定用例
```

## 结构

| 文件 | 作用 |
|---|---|
| `fake-ext.ts` | 真实 vendored pi 的假扩展：/ext-status /ext-status-clear /ext-widget-ansi /ext-editor /ext-select /ext-bigselect /ext-notify，全 UI 零 agent run |
| `mock-pi.mjs` | 协议边界假 pi：`emit <dir> <scenario>` 生成假 vendor 树（VERSION + cli.js），`PI_FLASH_VENDOR_DIR` 指过去即被 spawn。scenario：`unknown-method` / `crash-with-dialog` |
| `run.sh` | 每用例独立隔离实例（PI_FLASH_DIR + wire 日志），PASS/FAIL/XFAIL 表，非零退出码 |

## 基线（2026-11-04，zi2s 动工前）

| 用例 | 断言 | 基线 | 归属 |
|---|---|---|---|
| C1 notify 冒烟 | 命令收场链路（waiting 收场 + notice + handled） | PASS | #7 回归哨兵 |
| C2 setStatus 数据面 | ext_status 写入/摘除（快照口径；渲染行当前隐藏＝`EXT_STATUS_ROW_VISIBLE=false`，待设计落位） | PASS | zi2s.4 前置 |
| C3 setWidget ANSI | widget 文本无 ESC（PlainTextTheme parity） | ~~XFAIL~~ → **PASS**（zi2s.8 已修：pi-link `strip_ansi` + on_ext_ui 对 setWidget/setStatus 应用） | zi2s.8 ✓ |
| C4 editor 多行弹窗 | prefill 两行原样进多行输入 + 取消回 cancelled | PASS（zi2s.6：专用 multiline TextInput + Enter 提交，折叠保命 hack 已移除） | zi2s.6 ✓ |
| C5 select 键盘导航 | ↓↓ 高亮 index=2 + Enter 选中 gamma（handler notify 回执） | ~~XFAIL~~ → **PASS**（zi2s.7 已修：overlay on_key_down 导航 + 首行预选高亮 + snapshot sel 出口） | zi2s.7 ✓ |
| C6 长标题 select | 40 行 title 弹窗可开可关 + shot 留档 | PASS | b9e2edce 回归 |
| C7 未知 method | mock 注入 futureWidget → PF 回 cancelled | ~~XFAIL~~ → **PASS**（zi2s.2 已修：parse 回 Unknown + 立刻 cancelled + eprintln 留痕） | zi2s.2 ✓ |
| C8 pi 崩溃清弹窗 | mock exit(1) → ext_dialog 清理 | ~~XFAIL~~ → **PASS**（zi2s.3 已修：泵尾 ext_queue.clear + SessionEvent::ExtUiDied → Chat ext_cancel_all；连跑 4 次确定性通过） | zi2s.3 ✓ |
| C9 内置斜杠命令 | /name /compact /auto-compact wire 命令帧 + /session /mcp 面板 + 全程零 prompt 帧 | PASS（zi2s.5 拦截实现即台架即验收） | zi2s.5 ✓ |
| C10 setTitle | snapshot window.title = "PF ext title ✓"（OS 标题映射） | PASS（zi2s.9：render 帧 apply-diff 落 set_window_title） | zi2s.9 ✓ |

\** C4 基线曾 FAIL：**editor 多行 prefill 直接崩掉整个应用**（GPUI
`text argument should not contain newlines` panic）——单行 TextInput 阶段
曾折行保命，zi2s.6 已换专用 multiline 输入（aux.ext_editor_input），
prefill 原样、Enter 提交（剥组件自插换行，composer 同款）。

## 台架机制（踩坑实录）

1. **draft 会话 spawn 是懒的**（首次发送才起进程），发送前 wire 里没有
   spawn 行，守门没得看。解法：写 `session-ext.json` 把**会话文件路径**映射
   到 fake-ext.ts，再 `session.open`——open_session 的 005 预热见台账非空
   会立即 spawn（pool.rs），wire spawn 行先于发送出现。
2. **守门**：发送 /ext-* 前必须 grep wire 确认 spawn 行含 fake-ext.ts，
   否则漏成普通 prompt 真调模型（issue7 实踩：烧配额）。
3. `ext_notice` 4 秒自动消失：先断言它，再等 waiting 收场。
4. 真实 pi 的**内置 mcp 扩展也会 setStatus("mcp")**，且带 ANSI——C2/C3 的
   断言只认自己的键（usage/ansi），不假设全表为空。
5. Git Bash：`--arg text=/ext-*` 必须 `MSYS2_ARG_CONV_EXCL='*'` 前缀
   （否则 /ext-* 被翻译成 C:/Program Files/Git/ext-*）。
6. snapshot session 直接返回面本体（无 `session` 包装）；jcheck 的
   node eval 变量名是 `d`。
7. 用例目录 `tmp/ext-ui/<C>`（上一轮句柄锁目录时自动换 `.PID` 后缀）。

## mock-pi scenario 语义

- `unknown-method`：prompt → 注入 `method:"futureWidget"` 的
  extension_ui_request，等 PF 的 `extension_ui_response`（期望 cancelled）后
  回 prompt handled；8s 兜底。
- `crash-with-dialog`：prompt → 注入阻塞 select → 回 handled → 4s 后
  `process.exit(1)` 模拟 pi 崩溃（弹窗在屏时进程死亡）。

线缆真相一律以各用例目录 `wire.log`（PI_FLASH_RPC_LOG 双向记录）为准。
