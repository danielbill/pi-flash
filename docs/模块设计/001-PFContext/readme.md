本目录存放Pi-Flash上下文设计；
包括PF的全部settings和运行时全部上下文变量。
覆盖前期的settings、plugins模块，以及后续的任务、管家模块。
统一管理来自配置文件和数据库的配置信息和全局变量。

## 文档地图

- [001-0-PFContext总体设计](001-0-PFContext总体设计.md) —— 三子集契约、分层与优先级、失效语义、与 pi 文件的边界（本目录唯一事实源）
- [001-1-settings正式化](001-1-settings正式化.md) —— `app-settings.json` v2：section 化、读写模型、25 键迁移、Section 契约
- [001-2-跨进程投影](001-2-跨进程投影.md) —— `ext-context.json`：形态、扩展读取契约、env 白名单、安全与观测
- [001-3-切片与验收](001-3-切片与验收.md) —— 验收指标、测试清单、风险与回滚
- [001-4-PFContext内部工具化](001-4-PFContext内部工具化.md) —— pfctx_manager：agent 设置面、执行链、参数目录
