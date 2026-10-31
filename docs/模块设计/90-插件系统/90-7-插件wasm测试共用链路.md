# 90-7 插件 wasm 测试共用链路

> 目标：**testcase 住插件仓，宿主运行时住 PF 仓**——插件的具体行为断言一条
> 都不进 PF；PF 的 wasmtime/WIT 宿主运行时也绝不复制进插件仓。两端以一条
> 瘦协议解耦。

## 背景

内层 wasm 插件（090-5 L3）的行为测试（fn_bar chrome、弹窗画布、命令回环、
验签门）需要**真宿主运行时**才能跑：wasmtime 组件装载、proof 验签、host
import（storage/measure/open-popup…）、动作通道。这些只在 PF 的
`pf-plugin-host` 里有。测试 case 属于插件、不属于宿主——PF 里出现任何插件
的文案/像素/镜像文件断言都是越界耦合。

## 分工

| 层 | 归属 | 载体 |
|---|---|---|
| 通用 runner（装载/验签/钩子/动作） | **PF 仓** | `crates/pf-plugin-host/src/bin/plugin-test.rs` |
| 场景编排 + 全部断言（case） | **插件仓** | `pi-flash-plugins/inner/scripts/wasm-test.mjs` |
| JS 壳自测（假宿主驱动 main.js） | 插件仓 | `packages/<slug>/mock-host.mjs`（check 门自动跑） |
| pi 扩展（通信本体）单测 | 插件仓 | `packages/<slug>/pi/*.test.mjs` |
| e2e（真机 PF + UI 自动化） | 插件仓 case + PF 的 `pif-ui` 链路 | 见 `.agents/skills/pi-flash-ui-test/SKILL.md` |

## runner 协议（plugin-test.exe）

进程级单实例会话：stdin 逐行 JSON 指令、stdout 逐行 JSON 结果、EOF 退出。

- `{"op":"load","wasm":"<path>","cache":"<dir>"}` —— 装载（cache = sha256
  编译缓存，未命中编一次后毫秒级命中）；回执带三钩子探测面。
- `{"op":"init","id":"...","proof":"<proof JSON 串>"}` —— 验签激活。proof 由
  **调用方**签发（签名流程见内部发布文档），runner 不持有任何密钥/kid。
  拒载后实例即弃（真机同语义）。
- `{"op":"fn_bar"|"session_row","env":"..."}` / `{"op":"render_popup"}` /
  `{"op":"render"}` —— 契约钩子直调；空语义与宿主一致（空串 → null/"null"）。
- `{"op":"on_event","kind":"command|broadcast|page-active","id":"...","path":"..."}`
- `{"op":"pump_actions"}` —— 排水宿主动作（open-popup/toast/open-file/…）。

runner 内建近似 `measure-text`（字符数 × 字号）：与真字体有偏差，但插件侧
「N 字 × S px」的居中断言同口径。存储 data_dir = wasm 同目录 `/data/`，插件
镜像文件测试脚本可直接读写（模拟 ext 侧写状态）。

## 解耦红线

- PF 仓（含 runner、cargo test、注释）**零插件身份**：id/wasm/proof/文案全由
  调用方传入。
- 插件仓**零宿主复制**：不得把 wasmtime/WIT 绑定/proof 验签宿主逻辑搬进
  `inner/`（验签只读腿 `crates/verify` 是插件侧参照实现，不是宿主）。
- runner 缺席 = 插件仓 wasm 测试 **SKIP**（exit 0）：`npm run check` 门不依赖
  PF 构建产物；跑全量 wasm 测试先在 PF 仓 `cargo build -p pf-plugin-host`。

## 新插件接入

1. `inner/crates/<slug>/` 写 wasm（wit 面照 `crates/pf-wxbot/wit/`）；
2. 仿 `inner/scripts/wasm-test.mjs` 写 `<slug>` 的 case（或抽公共驱动函数到
   该文件顶部复用）；场景与断言**全在本仓**；
3. `npm run test:wasm` 跑通后再走 `dist.mjs` 发版链。
