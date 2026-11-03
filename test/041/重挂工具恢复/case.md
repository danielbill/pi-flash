# 041 重挂工具恢复 —— 扩展中途卸载后重挂，工具必须回到 active 集（pi-flash-3x95）

## 现象（真机 2026-11-03）

custom 档会话中卸载 pi-mcp-adapter（下次发送触发重绑 kill+respawn `--session` 复接）
后再勾选：扩展已加载（命令/面板在、`getAllTools` 里 mcp 工具全在）但工具恒不进
active 集，模型看不到 —— 用户视角「挂不上」。pi-web-access 同症；pi-freeflow
「正常」只因它不注册工具。

## 根因

pi≥0.86 transcript 工具账本：每轮 system 消息记录 `toolsAdded`/`toolsRemoved`。
resume 且未发 `--tools`（custom 档刻意不发）时 pi 跑 `_restoreToolsFromTranscript`：

```
active := 全分支 add/remove 回放后的当前集 ∪ 「从未在 transcript 出现过且注册即激活」的新工具
```

卸载轮把工具记入 `toolsRemoved` → 重挂时既不在当前集、又因「出现过」不算新工具 →
注册但永不激活。扩展侧无法自救（adapter 指纹未变跳过重注册；proxy 只补自己
deactivate 过的；web-access 设计性遵从 transcript 声明集）。

## 修复

`crates/app/assets/full_activate.ts`（custom 档恒最后挂载）：session_start 把
「transcript 声明过（toolsAdded 出现过）+ 本进程已注册 + exposure==="direct" +
尚未激活 + 不在 EXCLUDE」的工具并回。收敛性见该文件头注释。

## 驱动

`probe.ts` = 记录型探针（session_start / before_agent_start 两时点打印
active 集），挂在 full_activate 之后。三步循环（HEAD 路径换成本仓）：

```bash
cd tmp/mcp-remount-probe   # 任意隔离 cwd
EXT=~/.pi/agent/npm/node_modules/pi-mcp-adapter
FA=<repo>/crates/app/assets/full_activate.ts
CLI=<repo>/vendor/pi/node_modules/@earendil-works/pi-coding-agent/dist/bundle/cli.js
node $CLI --print --session t.jsonl -ne -e $EXT -e $FA -e probe.ts "hi"   # 首挂
node $CLI --print --session t.jsonl -ne -e $FA -e probe.ts "ok"           # 卸载轮
node $CLI --print --session t.jsonl -ne -e $EXT -e $FA -e probe.ts "..."  # 重挂
```

（前置：`~/.pi/agent/mcp.json` 配有任一 stdio server；探针需配 `log`/`stderr`
输出口径，见 probe.ts。）

## 断言（2026-11-03 实测通过）

1. **首挂 no-op**：active = `read,bash,edit,write,mcp,mcp__ghzw,grep,find,ls`
   —— 与修复前逐字一致（无 powershell、无额外工具）。
2. **卸载轮**：active = 7 内置（mcp 消失）。
3. **重挂轮**：active 含 `mcp`、`mcp__ghzw`（session_start 与
   before_agent_start 两时点都在），模型可真实调用 MCP 工具。
4. **guard-同清单重绑**：mount → turn → 同 `-e` 重启 → turn，mcp 工具恒在。
5. **guard-web-access**：三步循环后 `web_search/source_check/fetch_content/
   get_search_content` 回到 active。
6. **动态模式不误伤**（设计保证，未单测）：dynamic 模式从未激活的 web 工具
   从未入 transcript `toolsAdded` → 修复规则不触碰。

## 结果

2026-11-03 全部断言通过（诊断+验证探针见同目录 `probe.ts`；无差别 direct
并回的对照版 probe2.ts 在 tmp/，不入库——会误拉直动态模式）。
