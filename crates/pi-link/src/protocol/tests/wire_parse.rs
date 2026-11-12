//! `parse_line`/`parse_record` 消息流解析：response / message_start / streaming
//! / message_end / queue & bash 事件（fixtures 为 pi 真机实录）。
//!
//! Fixtures below are REAL records captured from pi 0.87.1 runs.

use crate::protocol::*;

#[test]
fn response_success() {
    let e =
        parse_line(r#"{"id":"1","type":"response","command":"prompt","success":true}"#).unwrap();
    match e {
        Event::Response {
            id,
            command,
            success,
            error,
            data,
        } => {
            assert_eq!(
                (
                    id.as_str(),
                    command.as_str(),
                    success,
                    error.is_none(),
                    data.is_none()
                ),
                ("1", "prompt", true, true, true)
            );
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn get_messages_response_carries_data() {
    let e = parse_line(
        r#"{"type":"response","command":"get_messages","success":true,"data":{"messages":[{"role":"user","content":"hi"}]}}"#,
    )
    .unwrap();
    match e {
        Event::Response { command, data, .. } => {
            assert_eq!(command, "get_messages");
            let data = data.expect("data");
            assert_eq!(data["messages"][0]["role"], "user");
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn user_message_start_with_block_array() {
    // user content is a block array, not a string (wire format, pi 0.87)
    let e = parse_line(
        r#"{"type":"message_start","message":{"role":"user","content":[{"type":"text","text":"say OK"}],"timestamp":1790206311858}}"#,
    )
    .unwrap();
    match e {
        Event::MessageStart {
            role,
            blocks,
            timestamp,
            ..
        } => {
            assert_eq!(role, "user");
            assert_eq!(timestamp, Some(1790206311858));
            assert_eq!(
                blocks,
                vec![Block::Text {
                    content_index: 0,
                    text: "say OK".into()
                }]
            );
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn text_delta_uses_camel_case_fields() {
    let e = parse_line(
        r#"{"type":"message_update","usage":{"totalTokens":101},"assistantMessageEvent":{"type":"text_delta","contentIndex":0,"delta":"Hello "}}"#,
    )
    .unwrap();
    match e {
        Event::MessageUpdate(AssistantEvent::TextDelta {
            content_index,
            delta,
        }) => {
            assert_eq!(content_index, 0);
            assert_eq!(delta, "Hello ");
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn toolcall_events_carry_tool_name_and_content() {
    let start = parse_line(
        r#"{"type":"message_update","assistantMessageEvent":{"type":"toolcall_start","contentIndex":1,"id":"tc_1","toolName":"bash","partial":{}}}"#,
    )
    .unwrap();
    assert!(matches!(
        start,
        Event::MessageUpdate(AssistantEvent::ToolCallStart { content_index: 1, tool_name, .. })
            if tool_name == "bash"
    ));

    let end = parse_line(
        r#"{"type":"message_update","assistantMessageEvent":{"type":"toolcall_end","contentIndex":1,"toolCall":{"id":"tc_1","name":"bash","arguments":{"command":"ls"}}}}"#,
    )
    .unwrap();
    match end {
        Event::MessageUpdate(AssistantEvent::ToolCallEnd {
            content_index,
            tool_call,
        }) => {
            assert_eq!(content_index, 1);
            assert_eq!(tool_call["name"], "bash");
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn thinking_events() {
    let e = parse_line(
        r#"{"type":"message_update","assistantMessageEvent":{"type":"thinking_delta","contentIndex":0,"delta":"hmm"}}"#,
    )
    .unwrap();
    assert!(matches!(
        e,
        Event::MessageUpdate(AssistantEvent::ThinkingDelta { delta, .. }) if delta == "hmm"
    ));
}

#[test]
fn message_end_blocks_extract_toolcall_with_tool_name() {
    // assistant final content uses toolName; index comes from the block itself
    let e = parse_line(
        r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"thinking","thinking":"","index":0},{"type":"toolCall","id":"tc_1","name":"read","arguments":{"path":"a.rs"},"index":1},{"type":"text","text":"done","index":2}]}}"#,
    )
    .unwrap();
    match e {
        Event::MessageEnd { role, blocks, .. } => {
            assert_eq!(role, "assistant");
            assert_eq!(blocks.len(), 3);
            assert!(
                matches!(&blocks[0], Block::Thinking { content_index: 0, text } if text.is_empty())
            );
            assert!(
                matches!(&blocks[1], Block::ToolCall { content_index: 1, name, .. } if name == "read")
            );
            assert!(matches!(&blocks[2], Block::Text { content_index: 2, text } if text == "done"));
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn message_end_carries_stop_reason_and_error() {
    let e = parse_line(
        r#"{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"x","index":0}],"stopReason":"error","errorMessage":"boom"}}"#,
    )
    .unwrap();
    match e {
        Event::MessageEnd {
            stop_reason,
            error_message,
            ..
        } => {
            assert_eq!(stop_reason.as_deref(), Some("error"));
            assert_eq!(error_message.as_deref(), Some("boom"));
        }
        other => panic!("wrong event: {other:?}"),
    }
    // non-error messages: stopReason present, errorMessage absent
    let e = parse_line(
        r#"{"type":"message_end","message":{"role":"assistant","content":[],"stopReason":"stop"}}"#,
    )
    .unwrap();
    match e {
        Event::MessageEnd {
            stop_reason,
            error_message,
            ..
        } => {
            assert_eq!(stop_reason.as_deref(), Some("stop"));
            assert!(error_message.is_none());
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn image_blocks_parse_with_mime_and_data() {
    let e = parse_line(
        r#"{"type":"message_start","message":{"role":"user","content":[{"type":"text","text":"look","index":0},{"type":"image","data":"QUJD","mimeType":"image/png","index":1}]}}"#,
    )
    .unwrap();
    match e {
        Event::MessageStart { blocks, .. } => {
            assert_eq!(blocks.len(), 2);
            assert!(
                matches!(&blocks[1], Block::Image { mime, data, .. } if mime == "image/png" && data == "QUJD")
            );
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn custom_message_carries_custom_type_and_display() {
    let e = parse_line(
        r#"{"type":"message_start","message":{"role":"custom","customType":"compaction","content":"summary text","display":true,"timestamp":1790206311858}}"#,
    )
    .unwrap();
    match e {
        Event::MessageStart {
            role,
            blocks,
            custom_type,
            custom_display,
            ..
        } => {
            assert_eq!(role, "custom");
            assert_eq!(custom_type.as_deref(), Some("compaction"));
            assert!(custom_display);
            assert_eq!(
                blocks,
                vec![Block::Text {
                    content_index: 0,
                    text: "summary text".into()
                }]
            );
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn tool_result_message_carries_is_error_and_tool_call_id() {
    let e = parse_line(
        r#"{"type":"message_start","message":{"role":"toolResult","toolCallId":"tc_1","toolName":"bash","content":[{"type":"text","text":"nope","index":0}],"isError":true,"timestamp":1790206311858}}"#,
    )
    .unwrap();
    match e {
        Event::MessageStart {
            is_error,
            tool_call_id,
            ..
        } => {
            assert!(is_error);
            assert_eq!(tool_call_id.as_deref(), Some("tc_1"));
        }
        other => panic!("wrong event: {other:?}"),
    }
}

#[test]
fn assistant_message_start_partial_toolcall_uses_partial_json() {
    // REAL record: message_start carries a partial toolCall block
    let e = parse_line(
        r#"{"type":"message_start","message":{"role":"assistant","content":[{"type":"toolCall","id":"call_x","name":"bash","arguments":{},"partialJson":"","index":0}]}}"#,
    )
    .unwrap();
    match e {
        Event::MessageStart { role, blocks, .. } => {
            assert_eq!(role, "assistant");
            assert!(
                matches!(&blocks[0], Block::ToolCall { name, args, .. } if name == "bash" && args.is_empty())
            );
        }
        other => panic!("wrong event: {other:?}"),
    }
}

// 实测（pi 1.0.0/1.1.0 RPC，tmp/rpc_probe.js）：每个 run 的 turn_start 后 pi 都
// 会发 role:"system" 的 message_start/message_end，全量携带 transcript 补丁
// ——app 靠 raw_system 做 live 重放（新会话第一轮点亮系统提示词面板）。
#[test]
fn system_message_start_carries_raw_message() {
    let e = parse_line(
        r#"{"type":"message_start","message":{"role":"system","content":"base",
"sections":{"rules":"R2"},"timestamp":1790000000000,
"toolsAdded":[{"name":"read","description":"d","parameters":{"type":"object"}}]}}"#,
    )
    .unwrap();
    match e {
        Event::MessageStart {
            role, raw_system, ..
        } => {
            assert_eq!(role, "system");
            let raw = raw_system.expect("system start carries raw message");
            assert_eq!(raw["sections"]["rules"], "R2");
            assert_eq!(raw["toolsAdded"][0]["name"], "read");
        }
        other => panic!("wrong event: {other:?}"),
    }
    // 非 system 角色不带（省掉每条 user/assistant 消息的整包 clone）
    let e = parse_line(
        r#"{"type":"message_start","message":{"role":"user","content":"hi","timestamp":1}}"#,
    )
    .unwrap();
    match e {
        Event::MessageStart {
            role, raw_system, ..
        } => {
            assert_eq!(role, "user");
            assert!(raw_system.is_none());
        }
        other => panic!("wrong event: {other:?}"),
    }
}

// queue_update：pi 队列全量快照（_emitQueueUpdate：steering/followUp 文本数组）
#[test]
fn queue_update_parses_text_arrays() {
    let e = parse_line(
        r#"{"type":"queue_update","steering":["插一句"],"followUp":["排队甲","排队乙"]}"#,
    )
    .unwrap();
    match e {
        Event::QueueUpdate {
            steering,
            follow_up,
        } => {
            assert_eq!(steering, vec!["插一句"]);
            assert_eq!(follow_up, vec!["排队甲", "排队乙"]);
        }
        other => panic!("expected QueueUpdate, got {other:?}"),
    }
    // 空队列/缺字段 → 空数组，不 panic
    let e = parse_line(r#"{"type":"queue_update","steering":[],"followUp":[]}"#).unwrap();
    assert_eq!(
        e,
        Event::QueueUpdate {
            steering: vec![],
            follow_up: vec![]
        }
    );
}

// bash response data = BashResult（rpc-commands.md bash 节实测形态）
#[test]
fn bash_response_parses_bash_result() {
    let e = parse_line(
        r#"{"id":"b1","type":"response","command":"bash","success":true,"data":{"output":"file1\nfile2\n","exitCode":0,"cancelled":false,"truncated":false}}"#,
    )
    .unwrap();
    match e {
        Event::Response { command, data, .. } => {
            assert_eq!(command, "bash");
            let r = BashResult::parse(&data.expect("data"));
            assert_eq!(r.output, "file1\nfile2\n");
            assert_eq!(r.exit_code, Some(0));
            assert!(!r.cancelled && !r.truncated);
            assert!(r.full_output_path.is_none());
        }
        other => panic!("wrong event: {other:?}"),
    }
    let e = parse_line(
        r#"{"type":"response","command":"bash","success":true,"data":{"output":"partial","exitCode":null,"cancelled":true,"truncated":true,"fullOutputPath":"/tmp/pi-bash-x.log"}}"#,
    )
    .unwrap();
    match e {
        Event::Response { data, .. } => {
            let r = BashResult::parse(&data.expect("data"));
            assert_eq!(r.exit_code, None);
            assert!(r.cancelled && r.truncated);
            assert_eq!(r.full_output_path.as_deref(), Some("/tmp/pi-bash-x.log"));
        }
        other => panic!("wrong event: {other:?}"),
    }
}

// 执行中的流式输出增量（agent_session._emit 原样转发，json-event 不改写）
#[test]
fn bash_execution_update_event() {
    let e =
        parse_line(r#"{"type":"bash_execution_update","id":"req-9","delta":"hello "}"#).unwrap();
    match e {
        Event::BashExecutionUpdate { id, delta } => {
            assert_eq!(id.as_deref(), Some("req-9"));
            assert_eq!(delta, "hello ");
        }
        other => panic!("wrong event: {other:?}"),
    }
}

// ---- 047-P1-1 move 语义等价锁：大字段 null/missing 区分 + 全值保留 ----

/// Response.data：缺失 → None；null → Some(Null)（旧 get().cloned() 口径，
/// 不能把 null 折成 missing——调用方据此区分「无数据」与「空数据」）。
#[test]
fn response_data_null_vs_missing() {
    let missing = parse_line(r#"{"type":"response","id":"1","success":true}"#).unwrap();
    match missing {
        Event::Response { data, .. } => assert!(data.is_none()),
        other => panic!("wrong event: {other:?}"),
    }
    let null = parse_line(r#"{"type":"response","id":"1","success":true,"data":null}"#).unwrap();
    match null {
        Event::Response { data, .. } => assert_eq!(data, Some(serde_json::Value::Null)),
        other => panic!("wrong event: {other:?}"),
    }
}

/// tool_execution_start/end 的 args/result：缺失或 null → None；存在 →
/// Some 且值完整（move 不得改写内容）。
#[test]
fn tool_execution_args_result_null_vs_missing() {
    let e = parse_line(r#"{"type":"tool_execution_start","toolCallId":"t1"}"#).unwrap();
    assert!(matches!(e, Event::ToolExecutionStart { args: None, .. }));
    let e = parse_line(r#"{"type":"tool_execution_start","toolCallId":"t1","args":null}"#).unwrap();
    assert!(matches!(e, Event::ToolExecutionStart { args: None, .. }));
    let e = parse_line(
        r#"{"type":"tool_execution_end","toolCallId":"t1","result":{"output":"big text","nested":{"a":[1,2]}}}"#,
    )
    .unwrap();
    match e {
        Event::ToolExecutionEnd { result, .. } => {
            let r = result.expect("result preserved");
            assert_eq!(r["output"], "big text");
            assert_eq!(r["nested"]["a"][1], 2);
        }
        other => panic!("wrong event: {other:?}"),
    }
}

/// message_start details：非 object（字符串）→ None；object → Some。
/// system 消息的 raw_system 必须携带完整 message 本体（含 details 与
/// toolsAdded——app live 重放的数据源）。
#[test]
fn message_start_details_and_raw_system_move_semantics() {
    // details 非 object → None（system 角色也在内）
    let e = parse_line(
        r#"{"type":"message_start","message":{"role":"assistant","content":[],"details":"not-an-object"}}"#,
    )
    .unwrap();
    match e {
        Event::MessageStart { details, .. } => assert!(details.is_none()),
        other => panic!("wrong event: {other:?}"),
    }
    // 非 system：details move 直出
    let e = parse_line(
        r#"{"type":"message_start","message":{"role":"custom","customType":"x","content":[],"details":{"k":1}}}"#,
    )
    .unwrap();
    match e {
        Event::MessageStart {
            details,
            raw_system,
            ..
        } => {
            assert_eq!(details.expect("details"), serde_json::json!({"k":1}));
            assert!(raw_system.is_none());
        }
        other => panic!("wrong event: {other:?}"),
    }
    // system：raw_system 全值保留（含 details），details 同时独立存在
    let e = parse_line(
        r#"{"type":"message_start","message":{"role":"system","content":"base","details":{"toolCount":3},"toolsAdded":[{"name":"read"}]}}"#,
    )
    .unwrap();
    match e {
        Event::MessageStart {
            details,
            raw_system,
            ..
        } => {
            assert_eq!(details.expect("details")["toolCount"], 3);
            let raw = raw_system.expect("raw_system");
            assert_eq!(raw["details"]["toolCount"], 3, "raw_system 含 details");
            assert_eq!(raw["content"], "base");
            assert_eq!(raw["toolsAdded"][0]["name"], "read");
        }
        other => panic!("wrong event: {other:?}"),
    }
}

/// WireEvent.value：整行原值保留（entry_appended 的 app 消费方按字段挖取）。
#[test]
fn wire_event_value_preserved_whole() {
    let e = parse_line(
        r#"{"type":"entry_appended","entry":{"type":"pfctx-manager-op","customType":"x","data":{"k":"v"}},"seq":7}"#,
    )
    .unwrap();
    match e {
        Event::WireEvent { kind, value } => {
            assert_eq!(kind, "entry_appended");
            assert_eq!(value["entry"]["type"], "pfctx-manager-op");
            assert_eq!(value["entry"]["data"]["k"], "v");
            assert_eq!(value["seq"], 7);
        }
        other => panic!("wrong event: {other:?}"),
    }
}

/// 未知 type / 非 object 顶级行落 Unparsed 且原值保留（按 type 去重留痕的
/// 数据源）。
#[test]
fn unparsed_keeps_whole_value() {
    let e = parse_line(r#"{"type":"something_new","payload":{"x":1}}"#).unwrap();
    match e {
        Event::Unparsed(v) => {
            assert_eq!(v["type"], "something_new");
            assert_eq!(v["payload"]["x"], 1);
        }
        other => panic!("wrong event: {other:?}"),
    }
    // 非 object 顶级行
    let e = parse_line(r#"[1,2,3]"#).unwrap();
    assert!(matches!(e, Event::Unparsed(v) if v.is_array()));
}
