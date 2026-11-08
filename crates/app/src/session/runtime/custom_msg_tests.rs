//! custom 消息 live 事件回归锁（pi-flash-9eck）：`message_start` 的
//! customType/display/details 必须落表——background 子代理终态回灌
//! （subagent_panel::sync_cards_from_completions）按
//! `custom_type=="subagent-completion"` + `details.childId` 找 patch，
//! live 事件臂此前丢弃这两个字段，卡片转圈到永远（重开会话走磁盘
//! 重放才自愈，见 stream.rs on_message_start custom 臂）。

use super::*;
use gpui::TestAppContext;

fn feed_custom(
    rt: &gpui::Entity<SessionRuntime>,
    cx: &mut TestAppContext,
    custom_type: Option<&str>,
    display: bool,
    details: Option<serde_json::Value>,
) {
    let ev = Event::MessageStart {
        role: "custom".into(),
        blocks: vec![pi_link::protocol::Block::Text {
            content_index: 0,
            text: "Background subagent sub-1 finished.".into(),
        }],
        timestamp: None,
        is_error: false,
        tool_call_id: None,
        custom_type: custom_type.map(str::to_string),
        custom_display: display,
        details,
        raw_system: None,
    };
    rt.update(cx, |r, cx| r.on_event(ev, cx));
}

/// subagent-completion 终态消息：customType + details 原样落表。
#[gpui::test]
fn completion_custom_msg_keeps_type_and_details(cx: &mut TestAppContext) {
    let rt = cx.new(|_| {
        let mut r = SessionRuntime::new("custom-t".into(), PathBuf::from("C:/t"), None);
        r.sound_on = false;
        r
    });
    feed_custom(
        &rt,
        cx,
        Some("subagent-completion"),
        true,
        Some(serde_json::json!({
            "childId": "sub-1",
            "stopReason": "completed",
            "session_file": "C:/t/sub-1.jsonl"
        })),
    );
    rt.read_with(cx, |r, _| {
        assert_eq!(r.messages.len(), 1);
        let m = &r.messages[0];
        assert_eq!(m.role, Role::Custom);
        assert_eq!(
            m.custom_type.as_deref(),
            Some("subagent-completion"),
            "customType 必须落表（回灌按它筛选 patch）"
        );
        let details = m.details.as_ref().expect("details 必须落表");
        assert_eq!(details["childId"], "sub-1");
        assert_eq!(details["stopReason"], "completed");
    });
}

/// display:false 的扩展消息照原样落表（渲染层折叠暗卡口径）。
#[gpui::test]
fn hidden_custom_msg_keeps_display_flag(cx: &mut TestAppContext) {
    let rt = cx.new(|_| {
        let mut r = SessionRuntime::new("custom-t".into(), PathBuf::from("C:/t"), None);
        r.sound_on = false;
        r
    });
    feed_custom(&rt, cx, Some("some-ext"), false, None);
    rt.read_with(cx, |r, _| {
        let m = &r.messages[0];
        assert!(!m.custom_display, "display:false 不得被硬编码成 true");
        assert_eq!(m.custom_type.as_deref(), Some("some-ext"));
    });
}
