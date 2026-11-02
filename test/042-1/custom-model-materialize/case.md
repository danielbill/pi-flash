# 042-1 自定义商：勾选物化落盘 / 详情一律可编辑 / 存量自愈（0.3.2 回归锁）

## 背景（2026-11-02，0.3.2 发版后真机）

`253ca038` 给 `mj_materialize_selection` 加的「清单仓命中 → 原样复制」分支 push 完
直接 `return Ok(())`，没走到落盘。探测拉到的模型必在仓里 → 勾选永远只改内存克隆：
`models.json` 没有该模型 → pi 不认识（对话出错）；`mj_open_model` 在账本找不到 id →
落只读覆盖视图。真机证据：`~/.pi/agent/models.json` 的 myds 只有骨架，
`~/.pi-flash/custom-provider.json` 有两条探测定义（fetchedAt = 11-02）。

修法三件：① 纯变换下沉 `pi_link::models_json::materialize_model`（单一 save 出口）；
② 自定义商详情一律可编辑（仓草稿 `cfg.mj_draft_store_id`：追加槽 + 仓定义预填，
保存只进仓、显式勾选才物化）；③ 启动/设置页装载对账 `mj_reconcile_custom_selection`
（仅显式勾选补物化，all_enabled 零写盘）。附带：探测解析补 `max_output_tokens` /
`input_modalities` / `effort`（DeepSeek `/models` 实录全给）+ 仓 merge 字段级补缺。

## 自动化：`run.sh`（离线、确定性）

预置 `models.json`（骨架商 myds，无 models[]）+ `custom-provider.json`（两条探测定义）
+ `settings.json`（`enabledModels: ["myds/deepseek-flash"]` = 真机故障现场），不走
网络、不要 key。隔离 `PI_FLASH_DIR` + `PI_CODING_AGENT_DIR` 指进 `tmp/`，pif-ui 直调。

```bash
cargo build                                   # pi-flash.exe + pif-ui.exe
bash test/042-1/custom-model-materialize/run.sh
```

| 用例 | 断言 |
|---|---|
| CASE0 | 预置账本无 models[] |
| CASE1 | 启动即自愈：显式勾选的 deepseek-flash 进账本（带 api/baseUrl/input/cost/reasoning）；未勾选的不整批写；打开设置页幂等 |
| CASE2 | 仓里有/账本没有的模型 → `mj:m:myds:1`（追加槽，非 `mj:o:` 只读）；表单预填仓 id/contextWindow |
| CASE3 | 仓草稿保存：仓 maxTokens/reasoning 更新，账本不变（投影规则） |
| CASE4 | **根因回归锁**：勾选 → 账本出现，完整形状，仓里手调 maxTokens / reasoning=false 原样带入 |
| CASE5 | 取消 → 账本移除，仓定义保留，另一条不受影响 |
| CASE6 | 账本行 → `mj:m:myds:0` 索引模式；关闭退回 `mj:p:myds` |
| CASE7 | 仓草稿关闭后【+模型】仍是空白草稿（不串值） |
| CASE8 | 删商连 `settings.json` `enabledModels` 残留一起清（精确 / glob / :pin），别家 pattern 不动；盘上现读再写，不冲掉外部写入 |

真端点补充：`test/provider/run-cases.sh` CASE11（智谱 /v4 探测 → 勾选物化）。

## 单测

- `pi-link models_json::tests::materialize_*` 3 例（仓命中落盘 / off-on 往返 / 展示条目兜底）
- `pi-link probe::tests::entry_parse_maps_each_provider_shape`（DeepSeek 形状）
- `app services::custom_models_store::tests::*` 3 例（字段级补缺 / 模态+推理入定义）

## 人工验收

重启 PF（不必开设置页）→ `models.json` 的 myds 出现 deepseek-flash → 对话成功；
设置-模型 点 myds 任一模型 → 详情可编辑（有【测试】【移除】【保存】）；
myds 测速一次 → `custom-provider.json` 两条补上 `maxTokens: 393216` / `input`。
