自定义Provider分2步：
1、创建自定义的Provider
2、在其下创建模型

## 自定义Provider
UI复用当前settings-模型页的Provider详情面板
增加“自定义”变量，控制仅“自定义Provider” 才有的组件

## 自有组件
在Provider详情面板中增加如下新组件，仅当自定义时展示：
原只读显示的Provider name改为： name  input（半宽，与【删除】【保存】同列右对齐）
在API key下方新增 ——
- Base URL   ： input（全宽）
- API：无用户选择——API 形状由 PF 从 baseUrl 推断
  （`probe::infer_api`）在【保存】时落盘；pi 对缺 api 硬 throw
  （extensionModelFromDefinition），条目必写具体值。
  规则：`*/anthropic` / 含 `/anthropic/` → anthropic-messages；
  `*/v1beta` → google-generative-ai；其余 → openai-completions
  （openai-responses 与 completions URL 不可区分，一律 completions）。
  推断只在 PF 托管条目上随保存刷新（auth.json 有 PF 标记条目 = 托管；
  新建草稿天然托管）；手写条目只写 name/baseUrl 两键，显式 api 不被覆盖
- API key 与内置商同一条凭据路径：保存走 `auth_json::store_secret`
  （钥匙库 + auth.json 标记条目），models.json 里**不写 apiKey**；
  只清理 PF 自己以前写下的 `$PF_KEY_*` 遗留引用
- 操作栏：[测速] [+模型]  （[]代表按钮；【保存】在 name 行）
- 注释行：点击测速，连通后自动保存

## 操作栏说明
- 测试 和 +模型 仅在自定义Provider时显示
- 测速只制约【+模型】；【保存】随时可用（新建商可先存 key/baseUrl，
  连通性由测速与自动填参负责）
- 用户点击测速，能通则：1、自动保存自定义Provider（已存在的商按盘上老名
  保存，表单改名不随测速生效）2、自动从base url拉取模型列表
- 自动拉取如果失败，提示用户：“无法自动获取该Provider模型列表，请点击[+模型]手动添加模型”
- 测速连通判定 = `probe::probe_connect`（baseUrl + key，与 api type 解耦，
  候选阶梯零厂商知识）；base 带 /anthropic 段（Anthropic 兼容端点，如
  DeepSeek /anthropic）也在阶梯内覆盖（GET /v1/models 优先，通 = 空目录 →
  手动 +模型兜底）
- 测速/刷新拉到的目录**合并进清单仓 `custom-provider.json`**（只补缺不覆盖
  手调），并刷新内存目录——不整批入账本，勾选哪个才把哪个物化进 models.json
- 模型条目必带 pi 运行时形状 input/reasoning/cost：pi 对扩展注册模型不做
  models.dev 缺省合并，缺字段聊天即崩。`models_json::ensure_model_defaults`
  在保存时统一补齐（缺省 input=["text"]、reasoning=false、cost 四键全 0）；
  存量残缺条目由启动/设置页装载时的 `mj_repair_models_json` 自愈（normalize
  兑现，不可读拒写）


## +模型/模型详情页说明 
500x600 小弹窗， 点击[+模型]按钮 or 点击模型列表中模型名称后弹出
内置模型的模型行点开为**只读参数视图**（输入框禁用、无按钮；落 modelOverrides 覆盖）
该页面提供四个按钮：  自动填参、测试、保存、移除（页脚右对齐三钮 [测试][移除][保存]，同宽84 间隔10）

UI设计：
1、控制栏右侧：  测试、保存、移除 三个按钮 + 关闭按钮， 按钮间隔10px
2、内容区：
- 模型ID：input  左对齐； [自动填参]右对齐
- 上下文窗口 ； 最大输出
- 每百万 TOKENS 价格：输入/输出/缓存读取/缓存写入 四框（手填=显式覆盖；
  自动填参只填空/零）
- 推理：单勾选（落 model.reasoning）
- 能力：文本[X]、图片[]  ; 文本默认选中disable不得更改（模型输入天然含 text）


## UI改造
1、自定义Provider和内置Provider分列， 用分隔线隔开， 自定义列表在下方
2、自定义Provider详情页， 【移除】改 【删除】 ， 移除指不显示，删除指删除自定义条目，包括相关自定义模型
3、删除自定义Provider时，提示：确认删除该Provider及其下所有模型？
   （删除 = 删 models.json 条目 + 其下全部模型 + 凭据条目 + 清单仓条目，
   并清 enabledModels 残留 pattern；内置商的【移除】只删凭据让商家不显示）
