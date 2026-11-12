use super::*;

/// 内置 Agent 定义（随扩展出厂，只读）。id、name 与内容须与扩展侧内置
/// 清单（`extensions/pf-subagents/src/profile.ts` `BUILTIN_PROFILE_DOCS`）
/// 逐字一致——两侧各存一份，改动时同步。
/// 2026-10-19 拍板：dsh 无出厂内置，对齐 ZCode 两内置（general-purpose +
/// Explore，explorer 即 Explore 位）。
///
/// 2026-11-10 id 语义重定义：id = 不可见短 UUID（程序内调用句柄 + 文件
/// 名），用户与模型都不可见、不需要有含义、只需唯一；委派点名走
/// `name`（中文显示名，全局唯一）。内置三件同样如此。
///
/// 2026-11-10 按 044-0「内置的 subagent」定稿对齐：
/// - general-purpose：工具/技能/环境全继承（`tools`/`skills` 键省略），
///   轮次「指定」（不写 maxTurns，由委派方给 maxloop）；
/// - explorer：read-only + skills **关闭**（裸 `skills:` 键 = Some(空)）、
///   环境继承、轮次「指定」；
/// - summarizer：不继承环境（injectAgentsMd false + cwd = PF 数据目录）、
///   read-only + skills 关闭、轮次固定 5。
pub const EXPLORER_ID: &str = "e1a7c052";
pub const GENERAL_PURPOSE_ID: &str = "9f3d2b84";
pub const SUMMARIZER_ID: &str = "c58e07af";

pub fn builtin_agents() -> Vec<AgentProfile> {
    vec![
        AgentProfile {
            id: EXPLORER_ID.into(),
            name: Some("本地探索者".into()),
            description: "本地只读探索 · 目录调研、资料收集与定位".into(),
            model: None,
            tools: Some("read-only".into()),
            thinking_level: None,
            cwd: None,
            // 轮次「指定」= 键省略（委派方给 maxloop）
            max_turns: None,
            inject_agents_md: Some(true),
            system_prompt: None,
            append_system_prompt: Some(
                "你是本地探索员：探索目录、调研代码、定位信息、搜寻答案。\n不执行写操作。".into(),
            ),
            unknown: Vec::new(),
            extensions: Vec::new(),
            mcp_servers: Vec::new(),
            // skills 关闭（044-0）：键存在但空清单
            skills: Some(Vec::new()),
            origin: AgentOrigin::Builtin,
            path: None,
            error: None,
        },
        // 不写 `tools:` = inherit = 继承委派者全部工具（ZCode「全部工具」位）
        AgentProfile {
            id: GENERAL_PURPOSE_ID.into(),
            name: Some("通用子代理".into()),
            description: "委派者克隆体 · 可执行复杂任务".into(),
            model: None,
            tools: None,
            thinking_level: None,
            cwd: None,
            // 轮次「指定」= 键省略（委派方给 maxloop）
            max_turns: None,
            inject_agents_md: Some(true),
            system_prompt: None,
            append_system_prompt: Some(
                "你是委派者Clone，请明智使用手头工具与技能，按需完成任务".into(),
            ),
            unknown: Vec::new(),
            extensions: Vec::new(),
            mcp_servers: Vec::new(),
            // skills 继承（键省略）= 系统开启集
            skills: None,
            origin: AgentOrigin::Builtin,
            path: None,
            error: None,
        },
        // 044-0 第三枚内置：摘要机——无状态摘要，不注入 AGENTS.md（不继承
        // 环境上下文），cwd 落 PF 数据目录（不继承委派者目录），只读工具集。
        // 适用本地/廉价模型跑系统日志摘要等批量总结。
        AgentProfile {
            id: SUMMARIZER_ID.into(),
            name: Some("摘要机".into()),
            description: "无状态摘要 · 阅读文件或内容按要求总结".into(),
            model: None,
            tools: Some("read-only".into()),
            thinking_level: None,
            // 不继承环境（044-0「no cwd」）：落在 PF 数据目录里跑
            cwd: Some(CWD_PF_DATA.into()),
            // 轮次固定 5（044-0 内置清单）
            max_turns: Some(5),
            inject_agents_md: Some(false),
            system_prompt: None,
            append_system_prompt: Some(
                "你是摘要员：阅读指定文件或内容，按要求总结并返回摘要，勿写文件".into(),
            ),
            unknown: Vec::new(),
            extensions: Vec::new(),
            mcp_servers: Vec::new(),
            // skills 关闭（044-0）：键存在但空清单
            skills: Some(Vec::new()),
            origin: AgentOrigin::Builtin,
            path: None,
            error: None,
        },
    ]
}
