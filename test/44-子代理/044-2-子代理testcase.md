在pi-test项目下创建干净会话完成以下testcase

0、套娃测试
向子代理派发委派子代理的任务，子代理派发失败为预期通过，子代理派发成功为系统失败。

1、测试内部并发限制
- 设置10个并发，派发10个 ，预期通过
- 设置2个并发， 派发5个，预期失败三个
- 设置6个并发，派发5个，预期通过

注：使用最小指令

2、测试外部并发限制
模型设置为z.ai/glm 5.3 flash， 智谱服务器并发限制通常为3-5个
PF设置并发为10
- 派发3个，预期通过
- 派发5个，预期不确定
- 派发10个，预期失败，返回智谱服务器报错

注（2026-11-10 实测踩坑）：并发必须在 PF 设置页调整（或改 app-settings.json 的
subagent_capacity）——投影 ext-context.json 会覆盖 agents/config.json 的 capacity，
只写 config.json 会被 PF 自家闸（缺省 3）先拦下，表现为 CAPACITY_REACHED 轮流重发，
不是智谱限流。

3、max_loop限制测试
- 主agent指定max_loop为10，让subagent执行11轮指令，看是否被限停，同时返回错误码是否正确

4、自定义subagent测试
- 工具设置测试
每种路径测试其工具权限是否正常
full+下
  - ext选择web-access
  - mcp选ghzw

- 技能设置测试
创建没有任何技能的subagent，让它执行妙想查询技能，期待返回错误
创建【东财财经员】subagent —— 
1、为其只设置mx系列skill
2、设置环境注入为off
3、工具：default
4、模型：继承
5、描述：使用东财财经SKILL获取专业财经资讯的Agent
6、提示词：你是东财财经信息搜集员，使用mx-search、mx-data、mx-xuangu技能提供专业财经信息

让它执行财经新闻查询，期待返回结果，并让它执行jev skill，期待返回错误。

- AGENTS.MD注入测试
询问【东财财经员】环境问题，期待回答失败。

- 附加提示词测试
设置额外身份，如你是大恐龙
看subagent是否能正确回复

- model设置测试
  - 使用Deepseek/deepseek 4.1 flash做主模型， 测试继承模型是否正确
  - 创建自定义subagent，为其指定 OpenRouter/crouton alpha ， 看其是否按预期报地区限制错误
  注：crouton alpha 已于 2026-11-09 被 OpenRouter 下架（实测返回 404 retired），
  复测需另选受限模型；验收口径=错误原样上抛父会话

5、运行时操作与面板（2026-11-10 补缺口）
- 中断测试
  - 派发一个长任务子代理，从 UI 中断它，预期面板/消息正确显示中断终态
  - 对已结束的子代理再次中断，预期为 no-op 不报错
- 重启对账测试
  - 派发子代理后未等回传即退出 app，重开该会话，预期孤儿子代理被标记 interrupted/failed
- 面板显示测试
  - 子代理面板正确显示描述符各阶段（provisioning→active→终态）
  - 派发失败的委派在面板/消息留痕（failed 描述符），CAPACITY_REACHED 不画成红点
  - 点开子代理可查看子会话内容
- caller 投递测试
  - 提示词引导模型用前台模式派发（同步等子代理答复再回话），预期答复原样转告
  （2026-11-10 真机走查 7 次派发全是 background，caller 模式未真机验证）
