use super::*;

/// 内置 Agent 定义（随扩展出厂，只读）。id 与内容须与扩展侧内置清单
/// （`extensions/pf-subagents/src/profile.ts` `BUILTIN_PROFILE_DOCS`）逐字
/// 一致——两侧各存一份，改动时同步。
/// 2026-10-19 拍板：dsh 无出厂内置，对齐 ZCode 两内置（general-purpose +
/// Explore，explorer 即 Explore 位）。
pub fn builtin_agents() -> Vec<AgentProfile> {
    vec![
        AgentProfile {
            id: "explorer".into(),
            description: "只读探索 · 调研与定位".into(),
            model: None,
            tools: Some("read-only".into()),
            thinking_level: None,
            max_turns: Some(0),
            inject_agents_md: Some(true),
            system_prompt: None,
            append_system_prompt: Some(
                "你是只读探索员：调研代码、定位实现、回答问题。\n不执行任何写操作。".into(),
            ),
            unknown: Vec::new(),
            extensions: Vec::new(),
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            origin: AgentOrigin::Builtin,
            path: None,
            error: None,
        },
        // 不写 `tools:` = inherit = 继承委派者全部工具（ZCode「全部工具」位）
        AgentProfile {
            id: "general-purpose".into(),
            description: "通用子代理 · 复杂调研、代码检索、多步任务".into(),
            model: None,
            tools: None,
            thinking_level: None,
            max_turns: Some(0),
            inject_agents_md: Some(true),
            system_prompt: None,
            append_system_prompt: Some(
                "你是通用子代理：独立完成委派的任务——调研复杂问题、检索代码、执行多步操作。\n可使用会话的全部工具，按需读写文件与执行命令。".into(),
            ),
            unknown: Vec::new(),
            extensions: Vec::new(),
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            origin: AgentOrigin::Builtin,
            path: None,
            error: None,
        },
        // 044-0 第三枚内置：摘要机——无状态摘要，不注入 AGENTS.md（不继承
        // 环境上下文），只读工具集。适用本地/廉价模型跑系统日志摘要等批量总结。
        AgentProfile {
            id: "summarizer".into(),
            description: "无状态摘要 · 阅读文件或内容按要求总结".into(),
            model: None,
            tools: Some("read-only".into()),
            thinking_level: None,
            max_turns: Some(0),
            inject_agents_md: Some(false),
            system_prompt: None,
            append_system_prompt: Some(
                "你是摘要机：无状态的摘要机器人——阅读指定的文件或一段内容，按要求总结。\n不继承任何环境上下文，不执行任何写操作，只输出摘要。".into(),
            ),
            unknown: Vec::new(),
            extensions: Vec::new(),
            mcp_servers: Vec::new(),
            skills: Vec::new(),
            origin: AgentOrigin::Builtin,
            path: None,
            error: None,
        },
    ]
}
