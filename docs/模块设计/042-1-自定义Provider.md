自定义Provider分2步：
1、创建自定义的Provider
2、在其下创建模型

## 自定义Provider
UI复用当前settings-模型页的Provider详情面板
增加“自定义”变量，控制仅“自定义Provider” 才有的组件

## 自有组件
在Provider详情面板中增加如下新组件，仅当自定义时展示：
原只读显示的Provider name改为： name  input
在API key下方新增 ——
- Base URL   ： input（全宽）
- ~~API ： select~~ **已下线**（2026 改版）：API 形状由 PF 从 baseUrl 推断
  （`probe::infer_api`）随条目落盘，用户零选择——Provider = baseUrl + apiKey。
  规则：`*/anthropic` / 含 `/anthropic/` → anthropic-messages；
  `*/v1beta` → google-generative-ai；其余 → openai-completions。
  pi 对缺 api 硬 throw（extensionModelFromDefinition），条目必写具体值；
  openai-responses 与 completions URL 不可区分，一律默认 completions，
  误判走模型编辑器「API 覆盖」逃生门（高级设置内保留）。存量缺 api
  条目由 boot 迁移 `pf_providers::migrate_infer_api` 幂等补写。
- 操作栏：[测速] [保存] [+模型]  （[]代表按钮）
- 注释行：点击测速，连通后自动保存

## 操作栏说明
- 保存按钮从原先的apikey行挪至操作栏
- 测试 和 +模型 仅在自定义Provider时显示
- 自定义时，测试未通过前，保存和+模型 disable
- 用户点击测速，能通则：1、自动保存自定义Provider 2、自动从base url拉取模型列表 
- 自动拉取如果失败，提示用户：“无法自动获取该Provider模型列表，请点击[+模型]手动添加模型”
- base 带 /anthropic 段（Anthropic 兼容端点，如 DeepSeek /anthropic）：目录优先 GET /v1/models（Anthropic 官方列模型端点，网关多有实现）；拉不到再回退 POST /v1/messages 判连通（通 = 空目录 → 手动 +模型兜底）


