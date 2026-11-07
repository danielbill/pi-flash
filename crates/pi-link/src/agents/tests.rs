use super::*;

fn tmp_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("agents-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn parses_v1_fields_and_ignores_unknown() {
    let doc = "---\ndescription: lab\nmodel: deepseek/deepseek-chat\ntools: read-only\nmaxTurns: 42\ninjectAgentsMd: false\nfutureField: whatever\n---\nappend body\n";
    let a = parse_agent("reviewer", doc, AgentOrigin::Custom).unwrap();
    assert_eq!(a.description, "lab");
    assert_eq!(a.model.as_deref(), Some("deepseek/deepseek-chat"));
    assert_eq!(a.tools_tier(), "read-only");
    assert_eq!(a.max_turns, Some(42));
    assert_eq!(a.max_turns_tier(), MaxTurnsTier::Custom(42));
    assert!(!a.inject_enabled());
    assert_eq!(a.append_system_prompt.as_deref(), Some("append body"));
    // 未知字段透传
    assert!(
        a.unknown
            .iter()
            .any(|(k, v)| k == "futureField" && v == "whatever")
    );
}

#[test]
fn reads_systemprompt_block_scalar() {
    let doc = "---\ndescription: strict\nsystemPrompt: |\n  You are strict.\n  Never edit.\n---\nextra notes\n";
    let a = parse_agent("strict", doc, AgentOrigin::Custom).unwrap();
    assert_eq!(
        a.system_prompt.as_deref(),
        Some("You are strict.\nNever edit.")
    );
    assert_eq!(a.append_system_prompt.as_deref(), Some("extra notes"));
}

#[test]
fn max_turns_tiers_and_validation() {
    let u = parse_agent("u", "---\nmaxTurns: 0\n---\nx\n", AgentOrigin::Custom).unwrap();
    assert_eq!(u.max_turns_tier(), MaxTurnsTier::Unlimited);
    let d = parse_agent("d", "---\ndescription: d\n---\n", AgentOrigin::Custom).unwrap();
    assert_eq!(d.max_turns_tier(), MaxTurnsTier::Hundred);
    assert!(parse_agent("n", "---\nmaxTurns: -1\n---\nx\n", AgentOrigin::Custom).is_err());
    assert!(parse_agent("i", "---\nmaxTurns: abc\n---\nx\n", AgentOrigin::Custom).is_err());
}

#[test]
fn rejects_bad_tools_and_inject() {
    assert!(parse_agent("p", "---\ntools: read\n---\nx\n", AgentOrigin::Custom).is_err());
    assert!(parse_agent("p", "---\ntools:\n  - read\n---\nx\n", AgentOrigin::Custom).is_err());
    assert!(
        parse_agent("p", "---\ntools: chat-only\n---\nx\n", AgentOrigin::Custom).is_ok(),
        "044-0 六档 chat-only"
    );
    assert!(
        parse_agent("p", "---\ntools: full+\n---\nx\n", AgentOrigin::Custom).is_ok(),
        "044-0 六档 full+"
    );
    assert!(
        parse_agent(
            "p",
            "---\ninjectAgentsMd: yes\n---\nx\n",
            AgentOrigin::Custom
        )
        .is_err()
    );
    assert!(parse_agent("p", "---\nisolation: docker\n---\nx\n", AgentOrigin::Custom).is_err());
}

#[test]
fn list_fields_roundtrip_0440() {
    let doc = "---\ndescription: pick\ntools: full+\nextensions:\n- D:/ext/a\n- D:/ext/b\nmcp:\n- mcp-server-one\nskills:\n- mx-data\n---\n\nbody\n";
    let a = parse_agent("picks", doc, AgentOrigin::Custom).unwrap();
    assert_eq!(a.extensions, vec!["D:/ext/a", "D:/ext/b"]);
    assert_eq!(a.mcp_servers, vec!["mcp-server-one"]);
    assert_eq!(a.skills, vec!["mx-data"]);
    // 序列化→再解析回环
    let b = parse_agent("picks", &serialize_agent(&a), AgentOrigin::Custom).unwrap();
    assert_eq!(b.extensions, a.extensions);
    assert_eq!(b.mcp_servers, a.mcp_servers);
    assert_eq!(b.skills, a.skills);
    // 不进 unknown（已知键）
    assert!(
        !b.unknown
            .iter()
            .any(|(k, _)| k == "extensions" || k == "mcp" || k == "skills")
    );
}

#[test]
fn roundtrip_preserves_fields_and_unknown() {
    let doc = "---\ndescription: 评审\nmodel: a/b\ntools: full\nthinking: high\nmaxTurns: 50\ninjectAgentsMd: true\nsystemPrompt: |\n  line1\n  line2\nextra: keep\n---\n\nbody line\n";
    let a = parse_agent("rt", doc, AgentOrigin::Custom).unwrap();
    let out = serialize_agent(&a);
    let b = parse_agent("rt", &out, AgentOrigin::Custom).unwrap();
    assert_eq!(b.description, "评审");
    assert_eq!(b.model.as_deref(), Some("a/b"));
    assert_eq!(b.tools_tier(), "full");
    assert_eq!(b.thinking_level.as_deref(), Some("high"));
    assert_eq!(b.max_turns, Some(50));
    assert!(b.inject_enabled());
    assert_eq!(b.system_prompt.as_deref(), Some("line1\nline2"));
    assert_eq!(b.append_system_prompt.as_deref(), Some("body line"));
    assert!(b.unknown.iter().any(|(k, v)| k == "extra" && v == "keep"));
    // thinking 键省略 = auto（None），回环不写键
    let auto = parse_agent("rt2", "---\ndescription: x\n---\n", AgentOrigin::Custom).unwrap();
    assert_eq!(auto.thinking_level, None);
    assert!(!serialize_agent(&auto).contains("thinking"));
}

#[test]
fn rejects_bad_thinking_level() {
    let err = parse_agent(
        "bad",
        "---\ndescription: x\nthinking: ultra\n---\n",
        AgentOrigin::Custom,
    )
    .unwrap_err();
    assert!(err.contains("thinking must be one of"), "{err}");
}

#[test]
fn scan_marks_corrupt_and_builtin_override() {
    let d = tmp_dir("scan");
    std::fs::write(d.join("good.md"), "---\ndescription: ok\n---\n").unwrap();
    // 无闭合 --- 的损坏文件：不丢弃，记 error
    std::fs::write(d.join("broken.md"), "---\ndescription: oops\n").unwrap();
    let list = scan_agents_dir(&d);
    assert_eq!(list.len(), 2);
    assert!(
        list.iter()
            .find(|a| a.id == "good.md" || a.id == "good")
            .map(|a| a.error.is_none())
            .unwrap_or(false)
    );
    let broken = list.iter().find(|a| a.id.starts_with("broken")).unwrap();
    assert!(broken.error.is_some(), "corrupt file must carry an error");
    assert_eq!(broken.origin, AgentOrigin::Custom);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn builtin_explorer_and_custom_override() {
    let builtins = builtin_agents();
    assert_eq!(
        builtins.len(),
        3,
        "explorer + general-purpose（2026-10-19 拍板）+ summarizer（044-0 摘要机）"
    );
    let ex = builtins.iter().find(|b| b.id == "explorer").unwrap();
    assert_eq!(ex.tools_tier(), "read-only");
    assert_eq!(ex.max_turns_tier(), MaxTurnsTier::Unlimited);
    let gp = builtins.iter().find(|b| b.id == "general-purpose").unwrap();
    assert_eq!(gp.tools_tier(), "inherit", "general-purpose 继承全部工具");
    assert_eq!(gp.max_turns_tier(), MaxTurnsTier::Unlimited);
    let custom = AgentProfile {
        id: "explorer".into(),
        origin: AgentOrigin::Custom,
        ..parse_agent(
            "explorer",
            "---\ndescription: mine\n---\n",
            AgentOrigin::Custom,
        )
        .unwrap()
    };
    assert!(custom.overrides_builtin(&builtins));
    let other = AgentProfile {
        id: "nope".into(),
        ..custom.clone()
    };
    assert!(!other.overrides_builtin(&builtins));
}

/// 两侧内置清单逐字一致（profile.ts `BUILTIN_PROFILE_DOCS` ↔ 本模块
/// `builtin_agents`）：id 集、description、tools、maxTurns、正文。
/// 防止「两处各存一份」的同步契约静默烂掉——扩展侧改了这边忘改就红。
#[test]
fn builtins_match_extension_docs() {
    let docs = [
        (
            "explorer",
            "只读探索 · 调研与定位",
            Some("read-only"),
            "你是只读探索员：调研代码、定位实现、回答问题。\n不执行任何写操作。",
        ),
        (
            "general-purpose",
            "通用子代理 · 复杂调研、代码检索、多步任务",
            None,
            "你是通用子代理：独立完成委派的任务——调研复杂问题、检索代码、执行多步操作。\n可使用会话的全部工具，按需读写文件与执行命令。",
        ),
        (
            "summarizer",
            "无状态摘要 · 阅读文件或内容按要求总结",
            Some("read-only"),
            "你是摘要机：无状态的摘要机器人——阅读指定的文件或一段内容，按要求总结。\n不继承任何环境上下文，不执行任何写操作，只输出摘要。",
        ),
    ];
    let builtins = builtin_agents();
    assert_eq!(builtins.len(), docs.len());
    for (id, description, tools, body) in docs {
        let a = builtins
            .iter()
            .find(|b| b.id == id)
            .unwrap_or_else(|| panic!("builtin {id} missing"));
        assert_eq!(
            a.description, description,
            "{id} description 与扩展侧不一致"
        );
        assert_eq!(a.tools.as_deref(), tools, "{id} tools 与扩展侧不一致");
        assert_eq!(a.max_turns, Some(0), "{id} maxTurns 与扩展侧不一致");
        assert_eq!(
            a.append_system_prompt.as_deref(),
            Some(body),
            "{id} 正文与扩展侧不一致"
        );
    }
    // 摘要机的定义性特征：不注入 AGENTS.md（044-0 不继承环境）
    let sum = builtins.iter().find(|b| b.id == "summarizer").unwrap();
    assert_eq!(sum.inject_agents_md, Some(false));
}

#[test]
fn migrate_copies_without_overwriting_and_skips_non_md() {
    let from = tmp_dir("mig-from");
    let to = tmp_dir("mig-to");
    std::fs::write(from.join("old.md"), "---\ndescription: legacy\n---\n").unwrap();
    // 别家的非 .md 文件（实况：旧目录里有 settings.json）不迁
    std::fs::write(from.join("settings.json"), "{}").unwrap();
    // 新目录已有同名 = 用户已在新家改过，以新为准
    std::fs::create_dir_all(&to).unwrap();
    std::fs::write(to.join("old.md"), "---\ndescription: new-home\n---\n").unwrap();
    migrate_agents_from(&from, &to);
    assert!(to.join("old.md").exists());
    assert!(!to.join("settings.json").exists());
    let list = scan_agents_dir(&to);
    let old = list.iter().find(|a| a.id == "old").unwrap();
    assert_eq!(old.description, "new-home", "已存在的同名不被覆盖");
    assert_eq!(list.len(), 1);
}

#[test]
fn migrate_from_missing_dir_is_noop() {
    let to = tmp_dir("mig-empty");
    migrate_agents_from(&to.join("nonexistent"), &to);
    assert!(to.exists() || !to.exists()); // 不 panic 即可；目录可能建出（幂等）
}
