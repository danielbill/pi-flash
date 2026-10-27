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
- 测速/刷新拉到的目录**直接合并进账本 models[]**（自定义商唯一真相源；按 id 去重，手调过的条目原样保留），并清掉 provider-catalog.json 里该商的缓存键——该文件只服务内置商「获取模型」的新鲜目录缓存（只读缓存，无同步语义）。不清的话 pi 注册的 provider 零模型，会话无模型可选（「测速有模型但聊天没反应」根因）
- 模型条目必带 pi 运行时形状 input/reasoning/cost：pi 对扩展注册模型不做 models.dev 缺省合并，缺字段聊天即崩（实测 reading 'includes' / reading 'tiers'）。`models_json::ensure_model_defaults` 统一补齐（缺省 input=["text"]、reasoning=false、cost 全 0）；boot 迁移 `migrate_model_defaults` 兑存量


## +模型/模型详情页说明 
500x500 小弹窗， 点击[+模型]按钮 or 点击模型列表中模型名称后弹出
该页面提供四个按钮：  自动填参、测试、保存、移除

UI设计：
1、控制栏右侧：  测试、保存、移除 三个按钮 + 关闭按钮， 按钮间隔10px
2、内容区：
- 模型ID：input  左对齐； [自动填参]右对齐   （来源models.dev隐藏，这是内部实现无需暴露）
- 上下文窗口 ； 最大输出
- 能力：文本[X]、图片[]、视频[]  ; 文本默认选中disable不得更改

其他原有设计全部删除，完全是心智负担；


## UI改造
1、自定义Provider和内置Provider分列， 用分隔线隔开， 自定义列表在下方
2、自定义Provider详情页， 【移除】改 【删除】 ， 移除指不显示，删除指删除自定义条目，包括相关自定义模型
3、删除自定义Provider时，提示：确认删除该Provider及其下所有模型？
 