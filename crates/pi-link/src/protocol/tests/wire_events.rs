//! 轮生命周期 / 重试 / 压缩 / 扩展 UI / ANSI 剥除等 wire 事件类型锁。

use crate::protocol::*;

// zi2s.8：扩展文本剥 ANSI（pi-web PlainTextTheme parity）
#[test]
fn strip_ansi_removes_sgr_and_osc_keeps_text() {
    assert_eq!(strip_ansi("\x1b[31m█ RED\x1b[0m tail"), "█ RED tail");
    assert_eq!(
        strip_ansi("\x1b[38;5;5m🔌 MCP: 0 servers\x1b[39m"),
        "🔌 MCP: 0 servers"
    );
    // OSC（标题序列）整段剥
    assert_eq!(strip_ansi("\x1b]0;title\x07rest"), "rest");
    assert_eq!(strip_ansi("\x1b]8;;url\x1b\\link"), "link");
    // 多行 widget 保留换行；无序列原样
    assert_eq!(strip_ansi("line1\nline2"), "line1\nline2");
    assert_eq!(strip_ansi("plain"), "plain");
}

// zi2s.2：未知 extension UI method 不丢条——parse 必须回 Unknown（带原
// 方法名），app 侧据此立刻回 cancelled。否则 pi 升级新增 UI 方法即整条
// 请求被丢弃、扩展 handler await 永久阻塞且零痕迹（#7 同型卡死）。
#[test]
fn unknown_ext_ui_method_parses_as_unknown() {
    let e = parse_line(
        r#"{"type":"extension_ui_request","id":"u9","method":"futureWidget","someFutureField":true}"#,
    )
    .expect("unknown method must still parse to ExtensionUi");
    match e {
        Event::ExtensionUi(ExtensionUiRequest { id, method }) => {
            assert_eq!(id, "u9");
            match method {
                ExtUiMethod::Unknown { method } => assert_eq!(method, "futureWidget"),
                other => panic!("expected Unknown, got {other:?}"),
            }
        }
        other => panic!("expected ExtensionUi, got {other:?}"),
    }
    // 已知方法行为不变
    let e = parse_line(
        r#"{"type":"extension_ui_request","id":"s1","method":"select","title":"t","options":["a"]}"#,
    )
    .unwrap();
    assert!(matches!(
        e,
        Event::ExtensionUi(ExtensionUiRequest {
            method: ExtUiMethod::Select { .. },
            ..
        })
    ));
}

#[test]
fn prompt_disposition_values() {
    // 实录（issue #7 复现，vendored pi 1.5.0 rpc-mode）：扩展命令吃掉 prompt
    assert_eq!(
        prompt_disposition(&serde_json::json!({"disposition": "handled"})),
        Some("handled")
    );
    assert_eq!(
        prompt_disposition(&serde_json::json!({"disposition": "queued"})),
        Some("queued")
    );
    assert_eq!(
        prompt_disposition(&serde_json::json!({"disposition": "started"})),
        Some("started")
    );
    // 其他命令的 response 没有 disposition；未知值一律 None
    assert_eq!(
        prompt_disposition(&serde_json::json!({"messages": []})),
        None
    );
    assert_eq!(
        prompt_disposition(&serde_json::json!({"disposition": "bogus"})),
        None
    );
    assert_eq!(prompt_disposition(&serde_json::Value::Null), None);
}

#[test]
fn slash_commands_parse_list() {
    let data = serde_json::json!({"commands":[
        {"name":"fix-tests","description":"Fix failing tests","source":"prompt"},
        {"name":"review","description":"","source":"skill"}
    ]});
    let cmds = SlashCommand::parse_list(&data);
    assert_eq!(cmds.len(), 2);
    assert_eq!(cmds[0].name, "fix-tests");
    assert_eq!(cmds[0].description, "Fix failing tests");
}

#[test]
fn agent_lifecycle() {
    assert!(matches!(
        parse_line(r#"{"type":"agent_start"}"#),
        Some(Event::AgentStart)
    ));
    assert!(matches!(
        parse_line(r#"{"type":"agent_end","messages":[],"willRetry":false}"#),
        Some(Event::AgentEnd { will_retry: false })
    ));
    assert!(matches!(
        parse_line(r#"{"type":"agent_settled"}"#),
        Some(Event::AgentSettled)
    ));
}

#[test]
fn auto_retry_events_are_typed() {
    let e = parse_line(
        r#"{"type":"auto_retry_start","attempt":2,"maxAttempts":3,"delayMs":4000,"errorMessage":"429: too many requests"}"#,
    )
    .unwrap();
    assert_eq!(
        e,
        Event::AutoRetryStart {
            attempt: 2,
            max_attempts: 3,
            delay_ms: 4000,
            error_message: Some("429: too many requests".into()),
        }
    );
    // success=true 的收尾不带 finalError（重试后的尝试已通过）
    let e = parse_line(r#"{"type":"auto_retry_end","success":true,"attempt":1}"#).unwrap();
    assert_eq!(
        e,
        Event::AutoRetryEnd {
            success: true,
            attempt: 1,
            final_error: None
        }
    );
    let e = parse_line(
        r#"{"type":"auto_retry_end","success":false,"attempt":3,"finalError":"Retry cancelled"}"#,
    )
    .unwrap();
    assert_eq!(
        e,
        Event::AutoRetryEnd {
            success: false,
            attempt: 3,
            final_error: Some("Retry cancelled".into()),
        }
    );
}

#[test]
fn compaction_and_summarization_retry_events_are_typed() {
    // compaction_start/end（agent-session.d.ts 形态；end 带失败原因）
    let e = parse_line(r#"{"type":"compaction_start","reason":"overflow"}"#).unwrap();
    assert_eq!(
        e,
        Event::CompactionStart {
            reason: Some("overflow".into())
        }
    );
    let e = parse_line(
        r#"{"type":"compaction_end","reason":"threshold","aborted":false,"willRetry":true,"errorMessage":"boom"}"#,
    )
    .unwrap();
    assert_eq!(
        e,
        Event::CompactionEnd {
            reason: Some("threshold".into()),
            aborted: false,
            will_retry: true,
            error_message: Some("boom".into()),
        }
    );
    // summarization_retry_*（与 auto_retry 同构的退避窗口）
    let e = parse_line(
        r#"{"type":"summarization_retry_scheduled","attempt":1,"maxAttempts":3,"delayMs":2000,"errorMessage":"ctx overflow"}"#,
    )
    .unwrap();
    assert_eq!(
        e,
        Event::SummarizationRetryScheduled {
            attempt: 1,
            max_attempts: 3,
            delay_ms: 2000,
            error_message: Some("ctx overflow".into()),
        }
    );
    assert!(matches!(
        parse_line(r#"{"type":"summarization_retry_attempt_start","source":"compaction"}"#),
        Some(Event::SummarizationRetryAttemptStart { .. })
    ));
    assert!(matches!(
        parse_line(r#"{"type":"summarization_retry_finished"}"#),
        Some(Event::SummarizationRetryFinished)
    ));
}

#[test]
fn extension_error_and_misc_wire_events_are_typed() {
    // rpc-mode 对扩展错误的输出（error 可能是任意 JSON，这里按 string）
    let e = parse_line(
        r#"{"type":"extension_error","extensionPath":"/x/e.ts","event":"session_start","error":"boom"}"#,
    )
    .unwrap();
    assert_eq!(
        e,
        Event::ExtensionError {
            extension_path: Some("/x/e.ts".into()),
            event_name: Some("session_start".into()),
            error: Some("boom".into()),
        }
    );
    // 工具执行起收：立正式事件（嵌套调用的完整 result 只在这里出现），
    // parentToolCallId = 嵌套调用的父工具调用（codemode 里的 tools.agent）
    let e = parse_line(
        r#"{"type":"tool_execution_start","toolCallId":"p/1","toolName":"agent","args":{"prompt":"x"},"parentToolCallId":"p"}"#,
    )
    .unwrap();
    assert_eq!(
        e,
        Event::ToolExecutionStart {
            tool_call_id: "p/1".into(),
            tool_name: "agent".into(),
            parent_tool_call_id: Some("p".into()),
            args: Some(serde_json::json!({"prompt": "x"})),
        }
    );
    // 顶层调用无 parentToolCallId（直调 agent / 其他工具）
    let e = parse_line(r#"{"type":"tool_execution_start","toolCallId":"t1","toolName":"bash"}"#)
        .unwrap();
    assert_eq!(
        e,
        Event::ToolExecutionStart {
            tool_call_id: "t1".into(),
            tool_name: "bash".into(),
            parent_tool_call_id: None,
            args: None,
        }
    );
    let e = parse_line(
        r#"{"type":"tool_execution_end","toolCallId":"p/1","toolName":"agent","isError":false,"durationMs":761,"parentToolCallId":"p","result":{"content":[],"details":{"child_id":"c1","stop_reason":"completed"}}}"#,
    )
    .unwrap();
    let Event::ToolExecutionEnd {
        tool_call_id,
        parent_tool_call_id,
        is_error,
        result,
        duration_ms,
        ..
    } = e
    else {
        panic!("not tool_execution_end")
    };
    assert_eq!(tool_call_id, "p/1");
    assert_eq!(parent_tool_call_id.as_deref(), Some("p"));
    assert!(!is_error);
    assert_eq!(duration_ms, Some(761));
    assert_eq!(
        result.unwrap()["details"]["stop_reason"],
        serde_json::json!("completed")
    );
    for kind in [
        "tool_execution_update",
        "entry_appended",
        "session_info_changed",
    ] {
        let line = format!(r#"{{"type":"{kind}"}}"#);
        let e = parse_line(&line).unwrap();
        let Event::WireEvent { kind: k, .. } = e else {
            panic!("not wire event")
        };
        assert_eq!(k, kind);
    }
    // thinking_level_changed：pi 截断后的**真值**（请求 minimal → low）
    assert_eq!(
        parse_line(r#"{"type":"thinking_level_changed","level":"low"}"#).unwrap(),
        Event::ThinkingLevelChanged {
            level: "low".into()
        }
    );
    // 真正未知的 type 仍落 Unparsed
    assert!(matches!(
        parse_line(r#"{"type":"wat"}"#),
        Some(Event::Unparsed(_))
    ));
}

#[test]
fn extension_ui_requests_are_typed() {
    // select
    let e = parse_line(
        r#"{"type":"extension_ui_request","id":"r1","method":"select","title":"Pick","options":["a","b"],"timeout":5000}"#,
    )
    .unwrap();
    let Event::ExtensionUi(req) = e else {
        panic!("not extension ui")
    };
    assert_eq!(req.id, "r1");
    match req.method {
        ExtUiMethod::Select { title, options } => {
            assert_eq!(title, "Pick");
            assert_eq!(options, vec!["a", "b"]);
        }
        _ => panic!("wrong method"),
    }
    // setStatus / setWidget / notify / set_editor_text / confirm / editor / setTitle
    let e = parse_line(
        r#"{"type":"extension_ui_request","id":"r2","method":"setStatus","statusKey":"goal","statusText":"42%"}"#,
    )
    .unwrap();
    assert!(matches!(
        e,
        Event::ExtensionUi(ExtensionUiRequest {
            method: ExtUiMethod::SetStatus { status_key, status_text },
            ..
        }) if status_key == "goal" && status_text.as_deref() == Some("42%")
    ));
    let e = parse_line(
        r#"{"type":"extension_ui_request","id":"r3","method":"setWidget","widgetKey":"plan","widgetLines":["step 1"],"widgetPlacement":"belowEditor"}"#,
    )
    .unwrap();
    assert!(matches!(
        e,
        Event::ExtensionUi(ExtensionUiRequest {
            method: ExtUiMethod::SetWidget { placement, .. },
            ..
        }) if placement.as_deref() == Some("belowEditor")
    ));
    let e = parse_line(
        r#"{"type":"extension_ui_request","id":"r4","method":"notify","message":"done","notifyType":"warning"}"#,
    )
    .unwrap();
    assert!(matches!(
        e,
        Event::ExtensionUi(ExtensionUiRequest {
            method: ExtUiMethod::Notify { notify_type, .. },
            ..
        }) if notify_type.as_deref() == Some("warning")
    ));
    let e = parse_line(
        r#"{"type":"extension_ui_request","id":"r5","method":"input","title":"Name","placeholder":"x"}"#,
    )
    .unwrap();
    assert!(matches!(
        e,
        Event::ExtensionUi(ExtensionUiRequest {
            method: ExtUiMethod::Input { .. },
            ..
        })
    ));
    let e = parse_line(
        r#"{"type":"extension_ui_request","id":"r6","method":"confirm","title":"Sure?","message":"go"}"#,
    )
    .unwrap();
    assert!(matches!(
        e,
        Event::ExtensionUi(ExtensionUiRequest {
            method: ExtUiMethod::Confirm { .. },
            ..
        })
    ));
    let e = parse_line(
        r#"{"type":"extension_ui_request","id":"r7","method":"set_editor_text","text":"hi"}"#,
    )
    .unwrap();
    assert!(matches!(
        e,
        Event::ExtensionUi(ExtensionUiRequest {
            method: ExtUiMethod::SetEditorText { .. },
            ..
        })
    ));
}

/// 阻塞语义判定（黄点链路共用）：select/confirm/input/editor 挂住 pi 等
/// 答案；notify/setStatus/setWidget/setTitle/setEditorText 是
/// fire-and-forget。unknown 不得误判为阻塞（zi2s.2：回 cancelled 即落地）。
#[test]
fn ext_ui_blocking_classification() {
    let blocking = [
        r#"{"type":"extension_ui_request","id":"b1","method":"select","title":"t","options":["a"]}"#,
        r#"{"type":"extension_ui_request","id":"b2","method":"confirm","title":"t","message":"m"}"#,
        r#"{"type":"extension_ui_request","id":"b3","method":"input","title":"t"}"#,
        r#"{"type":"extension_ui_request","id":"b4","method":"editor","title":"t"}"#,
    ];
    for line in blocking {
        match parse_line(line).unwrap() {
            Event::ExtensionUi(req) => assert!(req.method.is_blocking(), "{line} 应判为阻塞"),
            _ => panic!("wrong event"),
        }
    }
    let fire_and_forget = [
        r#"{"type":"extension_ui_request","id":"f1","method":"notify","message":"m"}"#,
        r#"{"type":"extension_ui_request","id":"f2","method":"setStatus","statusKey":"k"}"#,
        r#"{"type":"extension_ui_request","id":"f3","method":"setWidget","widgetKey":"w"}"#,
        r#"{"type":"extension_ui_request","id":"f4","method":"setTitle"}"#,
        r#"{"type":"extension_ui_request","id":"f5","method":"set_editor_text","text":"t"}"#,
        r#"{"type":"extension_ui_request","id":"f6","method":"bogus"}"#,
    ];
    for line in fire_and_forget {
        match parse_line(line).unwrap() {
            Event::ExtensionUi(req) => {
                assert!(!req.method.is_blocking(), "{line} 应判为 fire-and-forget")
            }
            _ => panic!("wrong event"),
        }
    }
}

#[test]
fn unknown_record_is_unparsed_not_dropped() {
    let e = parse_line(r#"{"type":"future_thing","a":1}"#).unwrap();
    assert!(matches!(e, Event::Unparsed(_)));
}

#[test]
fn garbage_line_is_none() {
    assert!(parse_line("\x1b]0;pi title\x07").is_none());
}
