//! Transcript system prompt + tool declarations.
//!
//! pi 0.86+ moved both out of `get_state`: every run appends a
//! `role:"system"` message to the transcript carrying `content` (appended to
//! the base prompt), `sections` (patched by name, `null` deletes),
//! `toolsAdded` / `toolsRemoved` (declaration deltas). pi's own state getter
//! replays those messages (`@earendil-works/pi-ai`
//! `utils/transcript.js` + `utils/text.js`), and that replay is the only
//! supported read — the CLI RPC exposes neither `systemPrompt` nor
//! `get_tools`.
//!
//! Both `get_messages` payloads and session-file entries feed this module:
//! entries may be raw messages or whole session entries (`{message: …}`).

use serde_json::Value;

/// One tool declaration as it appears in the transcript (`toolsAdded`).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolDecl {
    pub name: String,
    pub description: String,
    /// JSON Schema as declared to the model (may be `Null` when absent).
    pub parameters: Value,
}

/// Current system prompt text + declared tools of one transcript.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TranscriptSystem {
    pub prompt: String,
    pub tools: Vec<ToolDecl>,
    /// 分类明细：`(label, text)`，label = `"content"`（基础 content 段）或
    /// section 名（preamble / tools / rules / skills / 扩展名…），text 为该类
    /// 的现值（sections 按 replay 后最新值）。`prompt` = 全部 text 按序
    /// `"\n\n"` 连接 —— token 分类统计（app 面板的比例条）以此为准。
    pub parts: Vec<(String, String)>,
}

/// 工具声明按发往模型的 wire 形态（`{name, description, input_schema}` JSON）
/// 串接 —— 「工具声明」分类的 token 估算输入。
pub fn tools_wire_text(tools: &[ToolDecl]) -> String {
    tools
        .iter()
        .map(|t| {
            serde_json::to_string(&serde_json::json!({
                "name": t.name,
                "description": t.description,
                "input_schema": t.parameters,
            }))
            .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 系统提示词 token 消耗的 7 个大类（app 面板「分析块」的固定口径，顺序 =
/// 展示顺序）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemBucket {
    /// PI 内置提示 + 全局 AGENTS.md
    GlobalPrompt,
    /// 工具说明文本（`tools` section）+ 核心内置工具声明
    SystemTools,
    Skills,
    /// 扩展/包注入的 section + 扩展注册的工具声明
    Plugins,
    /// MCP 网关工具声明（pi 1.0 的 MCP 工具描述不进提示词，走网关动态调用）
    Mcp,
    /// 项目 AGENTS.md（`project_context` 里非全局的 `<project_instructions>` 块）
    ProjectPrompt,
    /// `cwd` 等剩余 section
    Other,
}

/// 展示顺序 = 用户口径 1-7。
pub const SYSTEM_BUCKETS: [SystemBucket; 7] = [
    SystemBucket::GlobalPrompt,
    SystemBucket::SystemTools,
    SystemBucket::Skills,
    SystemBucket::Plugins,
    SystemBucket::Mcp,
    SystemBucket::ProjectPrompt,
    SystemBucket::Other,
];

/// pi 核心内置工具名（dist/core/tools，钉 1.0.0）。声明无来源字段，以此
/// 名单差集分桶：核心 → 系统工具，`mcp` → MCP，其余 → 插件。vendor bump
/// 时由协议符合性测试把关。
const CORE_TOOLS: &[&str] = &["bash", "read", "write", "edit", "find", "grep", "ls", "powershell"];

/// 提示词文本的有序分类分段：`(桶, 文本)`。顺序 = prompt 里的出现顺序；
/// 同一 part 内的碎片段（project_context 切块）已合并，part 之间保持独立
/// （跨 part 合并会改变 token 估算口径，且分段本就对应不同 section）。token
/// 统计（[`breakdown`]）与文本区按类铺背景色（app 面板）共用这一份切分。
///
/// `agent_dir` = pi 全局目录（`~/.pi/agent`）：`project_context` 里
/// `<project_instructions path="...">` 块的 path 落在该目录下 → 全局
/// AGENTS.md，否则 → 项目提示词；传 `None` 时全部按项目算。
pub fn segments(
    sys: &TranscriptSystem,
    agent_dir: Option<&std::path::Path>,
) -> Vec<(SystemBucket, String)> {
    let mut out: Vec<(SystemBucket, String)> = Vec::new();
    for (label, text) in &sys.parts {
        if text.is_empty() {
            continue;
        }
        let mut frags: Vec<(SystemBucket, &str)> = match label.as_str() {
            "content" | "preamble" | "rules" | "docs" => {
                vec![(SystemBucket::GlobalPrompt, text)]
            }
            "tools" => vec![(SystemBucket::SystemTools, text)],
            "skills" => vec![(SystemBucket::Skills, text)],
            "cwd" => vec![(SystemBucket::Other, text)],
            "project_context" => project_context_segments(text, agent_dir),
            // 未知名 section = 扩展/包注入（如 agent_browser）
            _ => vec![(SystemBucket::Plugins, text)],
        };
        // 同 part 内相邻同桶合并（切块产生的碎片拼回连续文本）；part 之间
        // 不合并 —— 分段保持 section 边界，估算口径与逐段一致
        let mut merged: Vec<(SystemBucket, String)> = Vec::new();
        for (b, seg) in frags.drain(..) {
            match merged.last_mut() {
                Some((b0, s0)) if *b0 == b => s0.push_str(seg),
                _ => merged.push((b, seg.to_string())),
            }
        }
        out.extend(merged);
    }
    out
}

/// 各桶的工具声明（只含有声明的桶，按 SYSTEM_BUCKETS 顺序）：文本区
/// 「调用声明」折叠块与 [`breakdown`] 的声明 token 共用同一分桶。
pub fn declarations(sys: &TranscriptSystem) -> Vec<(SystemBucket, Vec<ToolDecl>)> {
    let mut core = Vec::new();
    let mut mcp = Vec::new();
    let mut plugins = Vec::new();
    for t in &sys.tools {
        if t.name == "mcp" {
            mcp.push(t.clone());
        } else if CORE_TOOLS.contains(&t.name.as_str()) {
            core.push(t.clone());
        } else {
            plugins.push(t.clone());
        }
    }
    [
        (SystemBucket::SystemTools, core),
        (SystemBucket::Mcp, mcp),
        (SystemBucket::Plugins, plugins),
    ]
    .into_iter()
    .filter(|(_, v)| !v.is_empty())
    .collect()
}

/// 把 transcript 系统态按 7 大类估算 token（[`estimate_tokens`] 口径）：
/// 文本走 [`segments`] 汇总，工具声明按名单分桶（核心 → 系统工具，`mcp` →
/// MCP，其余 → 插件）经 [`tools_wire_text`] 估算。
pub fn breakdown(sys: &TranscriptSystem, agent_dir: Option<&std::path::Path>) -> [u64; 7] {
    let mut out = [0u64; 7];
    let idx = |b: SystemBucket| SYSTEM_BUCKETS.iter().position(|x| *x == b).unwrap();

    for (bucket, text) in segments(sys, agent_dir) {
        out[idx(bucket)] += crate::estimate::estimate_tokens(&text);
    }

    for (bucket, tools) in declarations(sys) {
        out[idx(bucket)] += crate::estimate::estimate_tokens(&tools_wire_text(&tools));
    }
    out
}

/// 切 `project_context` 为有序分段：`<project_instructions path="...">…</
/// project_instructions>` 逐块按 path 归全局/项目，标签外的包装文字归项目桶。
fn project_context_segments<'a>(
    text: &'a str,
    agent_dir: Option<&std::path::Path>,
) -> Vec<(SystemBucket, &'a str)> {
    const OPEN: &str = "<project_instructions path=\"";
    const CLOSE: &str = "</project_instructions>";
    let norm = |s: &str| s.replace('/', "\\").to_ascii_lowercase();
    let global_prefix = agent_dir.map(|p: &std::path::Path| norm(p.to_string_lossy().as_ref()));

    let mut out: Vec<(SystemBucket, &str)> = Vec::new();
    let mut tail = text;
    while let Some(at) = tail.find(OPEN) {
        if at > 0 {
            // 标签前的包装文字归项目桶
            out.push((SystemBucket::ProjectPrompt, &tail[..at]));
        }
        let rest = &tail[at + OPEN.len()..];
        let Some(q) = rest.find('"') else { break };
        let path = &rest[..q];
        let Some(close) = rest[q..].find(CLOSE) else { break };
        let body = &rest[q + 2..q + close]; // 路径引号后的 `>` 不算正文
        let is_global = global_prefix
            .as_ref()
            .is_some_and(|p: &String| norm(path).starts_with(p.as_str()));
        out.push((
            if is_global {
                SystemBucket::GlobalPrompt
            } else {
                SystemBucket::ProjectPrompt
            },
            body,
        ));
        tail = &rest[q + close + CLOSE.len()..];
    }
    if !tail.is_empty() {
        out.push((SystemBucket::ProjectPrompt, tail));
    }
    out
}

/// Text of a `content` value: a string, or the concatenated text blocks of an
/// array (pi-ai `contentText`).
fn content_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter(|b| b["type"].as_str() == Some("text"))
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Replay a transcript's system messages into the current system prompt and
/// tool set.
///
/// Mirrors pi-ai's `getCurrentSystemMessage` / `getSystemMessageText`:
///
/// - prompt = non-empty `content` parts followed by every live section value,
///   joined `\n\n` (section order = first-insert order, hence serde_json's
///   `preserve_order`)
/// - tools = replay `toolsRemoved` then `toolsAdded` over an insertion-ordered
///   map (re-declaring a name keeps its slot, exactly like a JS `Map.set`)
///
/// Returns `None` when the transcript carries no system message at all, so
/// callers can tell "not loaded" from "empty prompt".
pub fn transcript_system(entries: &[Value]) -> Option<TranscriptSystem> {
    let mut parts: Vec<(String, String)> = Vec::new();
    let mut sections: Vec<(String, String)> = Vec::new();
    let mut tools: Vec<ToolDecl> = Vec::new();
    let mut seen_system = false;

    let tool_of = |v: &Value| -> Option<ToolDecl> {
        let name = v["name"].as_str()?.to_string();
        Some(ToolDecl {
            name,
            description: v["description"].as_str().unwrap_or("").to_string(),
            parameters: v.get("parameters").cloned().unwrap_or(Value::Null),
        })
    };

    for entry in entries {
        let m = entry.get("message").unwrap_or(entry);
        if m["role"].as_str() != Some("system") {
            continue;
        }
        seen_system = true;

        let text = content_text(&m["content"]);
        if !text.is_empty() {
            parts.push(("content".to_string(), text));
        }
        if let Some(patch) = m["sections"].as_object() {
            for (name, value) in patch {
                if value.is_null() {
                    sections.retain(|(n, _)| n != name);
                    continue;
                }
                let text = content_text(value);
                match sections.iter_mut().find(|(n, _)| n == name) {
                    Some(slot) => slot.1 = text,
                    None => sections.push((name.clone(), text)),
                }
            }
        }
        for removed in m["toolsRemoved"].as_array().into_iter().flatten() {
            if let Some(name) = removed["name"].as_str() {
                tools.retain(|t| t.name != name);
            }
        }
        for added in m["toolsAdded"].as_array().into_iter().flatten() {
            let Some(decl) = tool_of(added) else { continue };
            match tools.iter_mut().find(|t| t.name == decl.name) {
                Some(slot) => *slot = decl,
                None => tools.push(decl),
            }
        }
    }

    if !seen_system {
        return None;
    }
    parts.extend(sections);
    Some(TranscriptSystem {
        prompt: parts
            .iter()
            .map(|(_, text)| text.as_str())
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join("\n\n"),
        tools,
        parts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// pi-ai transcript.js/text.js parity: sections patch by name, prompts
    /// append, tools replay add/remove, order follows first insertion.
    #[test]
    fn replays_sections_and_tools() {
        let entries = vec![
            json!({"role":"user","content":"hi"}),
            json!({"role":"system","content":"base",
                "sections":{"preamble":"P1","rules":"R1","tools":"T1"},
                "toolsAdded":[
                    {"name":"read","description":"read a file","parameters":{"type":"object"}},
                    {"name":"bash","description":"run","parameters":{"type":"object"}}
                ]}),
            json!({"role":"system","sections":{"rules":"R2","cwd":"/x","tools":null},
                "toolsRemoved":[{"name":"bash"}],
                "toolsAdded":[{"name":"edit","description":"edit","parameters":{"type":"object"}}]}),
        ];
        let sys = transcript_system(&entries).expect("has system message");
        // content + live sections in first-insert order; "tools" section deleted,
        // "cwd" appended, "rules" patched in place.
        assert_eq!(sys.prompt, "base\n\nP1\n\nR2\n\n/x");
        let names: Vec<&str> = sys.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["read", "edit"]);
        assert_eq!(sys.tools[0].description, "read a file");
    }

    /// Re-declaring a name replaces the definition but keeps its slot (JS Map.set).
    #[test]
    fn tool_redeclaration_keeps_slot() {
        let entries = vec![
            json!({"role":"system","toolsAdded":[{"name":"a","description":"1"},{"name":"b","description":"2"}]}),
            json!({"role":"system","toolsAdded":[{"name":"a","description":"1b"}]}),
        ];
        let sys = transcript_system(&entries).expect("has system message");
        let names: Vec<&str> = sys.tools.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(sys.tools[0].description, "1b");
        assert_eq!(sys.tools[1].description, "2");
    }

    /// Whole session entries (`{message:{…}}`) parse the same as raw messages,
    /// and a transcript without any system message reports None (≠ empty prompt).
    #[test]
    fn accepts_entries_and_reports_absent() {
        let entries = vec![
            json!({"type":"message","id":"a","message":{"role":"system","sections":{"a":"A"}}}),
            json!({"type":"message","id":"b","message":{"role":"user","content":"x"}}),
        ];
        assert_eq!(transcript_system(&entries).expect("sys").prompt, "A");
        assert!(transcript_system(&[json!({"role":"user","content":"x"})]).is_none());
    }

    /// breakdown：7 桶归属 + project_context 按路径切全局/项目。
    #[test]
    fn breakdown_buckets_sections_and_tools() {
        use super::super::estimate::estimate_tokens;
        use SystemBucket::*;
        let entries = vec![json!({
            "role": "system",
            "content": "base prompt",
            "sections": {
                "preamble": "you are pi",
                "tools": "<tools>- read</tools>",
                "rules": "<rules>- be careful</rules>",
                "skills": "<skills>- mx-data</skills>",
                "cwd": "<cwd>D:/x</cwd>",
                "ext_thing": "<ext_thing>extension rules</ext_thing>",
                "project_context": concat!(
                    "<project_context>Project-specific:\n",
                    "<project_instructions path=\"C:\\Users\\u\\.pi\\agent\\AGENTS.md\">global rules</project_instructions>\n",
                    "<project_instructions path=\"D:/repo/AGENTS.md\">project rules</project_instructions>",
                ),
            },
            "toolsAdded": [
                {"name":"read","description":"read a file"},
                {"name":"mcp","description":"mcp gateway"},
                {"name":"computer_use_click","description":"click"},
            ],
        })];
        let sys = transcript_system(&entries).expect("sys");
        let b = breakdown(&sys, Some(std::path::Path::new("C:/Users/u/.pi/agent")));

        let at = |bucket: SystemBucket| SYSTEM_BUCKETS.iter().position(|x| *x == bucket).unwrap();
        let t = |s: &str| estimate_tokens(s);
        let tool_est = |names: &[&str]| {
            let subset: Vec<ToolDecl> = sys
                .tools
                .iter()
                .filter(|d| names.contains(&d.name.as_str()))
                .cloned()
                .collect();
            t(&tools_wire_text(&subset))
        };
        // 全局 = content + preamble + rules + 全局 AGENTS 块
        assert_eq!(b[at(GlobalPrompt)], t("base prompt") + t("you are pi") + t("<rules>- be careful</rules>") + t("global rules"));
        assert_eq!(b[at(Skills)], t("<skills>- mx-data</skills>"));
        assert_eq!(b[at(SystemTools)], t("<tools>- read</tools>") + tool_est(&["read"]));
        assert_eq!(b[at(Mcp)], tool_est(&["mcp"]));
        assert_eq!(b[at(Plugins)], t("<ext_thing>extension rules</ext_thing>") + tool_est(&["computer_use_click"]));
        assert_eq!(b[at(Other)], t("<cwd>D:/x</cwd>"));
        // declarations：分桶与 breakdown 的声明部分一致
        let decls = declarations(&sys);
        assert_eq!(
            decls,
            vec![
                (SystemTools, vec![sys.tools[0].clone()]),
                (Mcp, vec![sys.tools[1].clone()]),
                (Plugins, vec![sys.tools[2].clone()]),
            ]
        );
        // 项目 = 包装句 + 块间换行 + 项目 AGENTS 块（分段逐段估算）
        assert_eq!(b[at(ProjectPrompt)], t("<project_context>Project-specific:\n") + t("\nproject rules"));

        // agent_dir 传 None：所有块归项目，全局只剩非 project_context 部分
        let b = breakdown(&sys, None);
        assert_eq!(b[at(GlobalPrompt)], t("base prompt") + t("you are pi") + t("<rules>- be careful</rules>"));
        assert!(b[at(ProjectPrompt)] > t("project rules"));
    }

    /// segments：分段顺序 = prompt 顺序；project_context 切块产生全局/项目
    /// 交替段；同 part 内相邻同桶合并、part 之间不合并。
    #[test]
    fn segments_follow_prompt_order() {
        use SystemBucket::*;
        let entries = vec![json!({
            "role": "system",
            "content": "base",
            "sections": {
                "skills": "S",
                // json! 按 JSON 原文解析字面量：\\\\ 在值里是双反斜杠；
                // 这里源码写 \\u → JSON \u… 值即单反斜杠路径
                "project_context": "<pc>wrap <project_instructions path=\"C:\\u\\.pi\\agent\\AGENTS.md\">G</project_instructions>|<project_instructions path=\"D:/p/AGENTS.md\">P</project_instructions>",
            },
        })];
        let sys = transcript_system(&entries).expect("sys");
        let segs = segments(&sys, Some(std::path::Path::new("C:/u/.pi/agent")));
        assert_eq!(
            segs,
            vec![
                (GlobalPrompt, "base".to_string()),
                (Skills, "S".to_string()),
                // 前缀 "wrap " 与分隔符 "|" 各自归项目，但中间隔着全局块，
                // 三个项目段被全局块隔开：wrap / G / |P
                (ProjectPrompt, "<pc>wrap ".to_string()),
                (GlobalPrompt, "G".to_string()),
                (ProjectPrompt, "|P".to_string()),
            ]
        );
    }

    /// project_context_segments：原始切分（未合并）；标签外文字归项目、
    /// 路径归一化（/ 与 \ 等价、大小写不敏感）。
    #[test]
    fn split_project_context_by_path() {
        let text = "head <project_instructions path=\"C:/Users/u/.pi/agent/AGENTS.md\">G</project_instructions> mid <project_instructions path=\"D:\\repo\\AGENTS.md\">P</project_instructions> tail";
        let segs = project_context_segments(text, Some(std::path::Path::new("c:\\users\\u\\.pi\\agent")));
        assert_eq!(
            segs,
            vec![
                (SystemBucket::ProjectPrompt, "head "),
                (SystemBucket::GlobalPrompt, "G"),
                (SystemBucket::ProjectPrompt, " mid "),
                (SystemBucket::ProjectPrompt, "P"),
                (SystemBucket::ProjectPrompt, " tail"),
            ]
        );
    }
}
