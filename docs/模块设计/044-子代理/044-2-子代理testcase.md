在pi-test项目下创建干净会话完成以下testcase

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

3、max_loop限制测试
- 主agent指定max_loop为10，让subagent执行11轮指令，看是否被限停，同时返回错误码是否正确

4、自定义subagent测试
- 工具设置测试
每种路径测试其工具权限是否正常
full+下
  - ext选择web-access
  - mcp选ghzw

- 技能设置测试

- AGENTS.MD注入测试

- 附加提示词测试
设置额外身份，如你是大恐龙
看subagent是否能正确回复

- model设置测试
  - 使用Deepseek/deepseek 4.1 flash做主模型， 测试继承模型是否正确
  - 创建自定义subagent，为其指定 OpenRouter/crouton alpha ， 看其是否按预期报地区限制错误
