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
    assert_eq!(
        b[at(GlobalPrompt)],
        t("base prompt") + t("you are pi") + t("<rules>- be careful</rules>") + t("global rules")
    );
    assert_eq!(b[at(Skills)], t("<skills>- mx-data</skills>"));
    assert_eq!(
        b[at(SystemTools)],
        t("<tools>- read</tools>") + tool_est(&["read"])
    );
    assert_eq!(b[at(Mcp)], tool_est(&["mcp"]));
    assert_eq!(
        b[at(Plugins)],
        t("<ext_thing>extension rules</ext_thing>") + tool_est(&["computer_use_click"])
    );
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
    assert_eq!(
        b[at(ProjectPrompt)],
        t("<project_context>Project-specific:\n") + t("\nproject rules")
    );

    // agent_dir 传 None：所有块归项目，全局只剩非 project_context 部分
    let b = breakdown(&sys, None);
    assert_eq!(
        b[at(GlobalPrompt)],
        t("base prompt") + t("you are pi") + t("<rules>- be careful</rules>")
    );
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
    let segs =
        project_context_segments(text, Some(std::path::Path::new("c:\\users\\u\\.pi\\agent")));
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
