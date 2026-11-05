//! `Command::to_record` 出站序列化形状锁（wire 字段名/可选键）。

use crate::protocol::wire::string_list;
use crate::protocol::*;
use serde_json::json;

#[test]
fn prompt_record_shape() {
    let c = Command::Prompt {
        message: "hi".into(),
        images: vec![],
        streaming_behavior: None,
    };
    assert_eq!(
        c.to_record("req-1"),
        json!({"id":"req-1","type":"prompt","message":"hi"})
    );
}

#[test]
fn prompt_streaming_behavior_wire() {
    // pi-web parity：排队发送恒走 prompt + streamingBehavior（pi 以自身
    // isStreaming 原子判定排队/开跑，杜绝裸 follow_up 的滞留队列竞态）
    let c = Command::Prompt {
        message: "hi".into(),
        images: vec![],
        streaming_behavior: Some("followUp".into()),
    };
    assert_eq!(
        c.to_record("q1"),
        json!({"id":"q1","type":"prompt","message":"hi","streamingBehavior":"followUp"})
    );
}

#[test]
fn set_model_record_shape() {
    let c = Command::SetModel {
        provider: "glm".into(),
        model: "glm-5.3-flash".into(),
    };
    assert_eq!(
        c.to_record("m1"),
        json!({"id":"m1","type":"set_model","provider":"glm","modelId":"glm-5.3-flash"})
    );
}

#[test]
fn steer_record_shape() {
    let c = Command::Steer {
        message: "stop".into(),
        images: vec![],
    };
    assert_eq!(
        c.to_record("s1"),
        json!({"id":"s1","type":"steer","message":"stop"})
    );
}

#[test]
fn prompt_with_images_omits_empty_and_includes_present() {
    let c = Command::Prompt {
        message: "hi".into(),
        images: vec![],
        streaming_behavior: None,
    };
    assert_eq!(
        c.to_record("a"),
        json!({"id":"a","type":"prompt","message":"hi"})
    );
    let img = json!({"type":"image","data":"QUJD","mimeType":"image/png"});
    let c = Command::Prompt {
        message: "hi".into(),
        images: vec![img.clone()],
        streaming_behavior: None,
    };
    assert_eq!(
        c.to_record("b"),
        json!({"id":"b","type":"prompt","message":"hi","images":[img]})
    );
}

#[test]
fn set_session_name_record_shape() {
    let c = Command::SetSessionName {
        name: "my-feature".into(),
    };
    assert_eq!(
        c.to_record("n1"),
        json!({"id":"n1","type":"set_session_name","name":"my-feature"})
    );
}

#[test]
fn follow_up_record_shape_with_images() {
    // pi 1.0 rpc-mode: session.followUp(command.message, command.images)
    let c = Command::FollowUp {
        message: "queued".into(),
        images: vec![],
    };
    assert_eq!(
        c.to_record("u1"),
        json!({"id":"u1","type":"follow_up","message":"queued"})
    );
    let img = json!({"type":"image","data":"QUJD","mimeType":"image/png"});
    let c = Command::FollowUp {
        message: "queued".into(),
        images: vec![img.clone()],
    };
    assert_eq!(
        c.to_record("u2"),
        json!({"id":"u2","type":"follow_up","message":"queued","images":[img]})
    );
}

#[test]
fn fork_record_shape() {
    // wire field is entryId (rpc-types.d.ts fork command)
    let c = Command::Fork {
        entry_id: "e-42".into(),
    };
    assert_eq!(
        c.to_record("f1"),
        json!({"id":"f1","type":"fork","entryId":"e-42"})
    );
    assert_eq!(
        Command::GetTree.to_record("t1"),
        json!({"id":"t1","type":"get_tree"})
    );
    assert_eq!(
        Command::Clone.to_record("c1"),
        json!({"id":"c1","type":"clone"})
    );
}

#[test]
fn model_list_parses_and_thinking_record_shape() {
    let data = serde_json::json!({"models":[
        {"id":"glm-5.3-flash","name":"GLM 5.3 Flash","provider":"glm","contextWindow":200000},
        {"id":"m2","name":"M2","provider":"p"}
    ]});
    let models = parse_model_list(&data);
    assert_eq!(models.len(), 2);
    assert_eq!(models[0].provider, "glm");
    let c = Command::SetThinkingLevel {
        level: "high".into(),
    };
    assert_eq!(
        c.to_record("t1"),
        json!({"id":"t1","type":"set_thinking_level","level":"high"})
    );
}

#[test]
fn extension_ui_response_record_shape() {
    let c = Command::ExtensionUiResponse {
        id: "r1".into(),
        value: Some("a".into()),
        confirmed: None,
        cancelled: false,
    };
    let v = c.to_record("ignored");
    assert_eq!(v["type"], "extension_ui_response");
    assert_eq!(v["id"], "r1");
    assert_eq!(v["value"], "a");
    assert!(v.get("confirmed").is_none());
    assert!(v.get("cancelled").is_none());
    let cancel = Command::ExtensionUiResponse {
        id: "r2".into(),
        value: None,
        confirmed: Some(true),
        cancelled: true,
    };
    let v = cancel.to_record("ignored");
    assert_eq!(v["confirmed"], true);
    assert_eq!(v["cancelled"], true);
}

// rpc `bash`：excludeFromContext=true 才带字段（pi 侧 ?? false 默认）
#[test]
fn bash_record_shape() {
    let c = Command::Bash {
        command: "ls -la".into(),
        exclude_from_context: false,
    };
    assert_eq!(
        c.to_record("b1"),
        json!({"id":"b1","type":"bash","command":"ls -la"})
    );
    let c = Command::Bash {
        command: "ls".into(),
        exclude_from_context: true,
    };
    assert_eq!(
        c.to_record("b2"),
        json!({"id":"b2","type":"bash","command":"ls","excludeFromContext":true})
    );
    assert_eq!(
        Command::AbortBash.to_record("b3"),
        json!({"id":"b3","type":"abort_bash"})
    );
}

// clear_queue：abort 配对命令（撤回滞留队列）；response data 携带被撤回
// 原文 {steering, followUp}（rpc-mode success(id,"clear_queue",clearQueue())）
#[test]
fn clear_queue_record_shape_and_response() {
    assert_eq!(
        Command::ClearQueue.to_record("q1"),
        json!({"id":"q1","type":"clear_queue"})
    );
    let e = parse_line(
        r#"{"id":"q1","type":"response","command":"clear_queue","success":true,"data":{"steering":[],"followUp":["改设计：单列布局"]}}"#,
    )
    .unwrap();
    match e {
        Event::Response { command, data, .. } => {
            assert_eq!(command, "clear_queue");
            let d = data.expect("data");
            assert_eq!(string_list(&d["steering"]), Vec::<String>::new());
            assert_eq!(string_list(&d["followUp"]), vec!["改设计：单列布局"]);
        }
        other => panic!("expected Response, got {other:?}"),
    }
}
