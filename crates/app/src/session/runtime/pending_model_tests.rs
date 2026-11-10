//! 草稿手选模型（pending_model）生命周期锁：
//!
//! 1. spawn 携带 `--model` 用 **clone 不 take**——pi 冷启动 1-3s+，首个
//!    get_state 到达前标签三段链（pending → state → 新会话默认）靠手选
//!    顶住。此前 take() 在 spawn 当下清账，标签先跌进默认模型、首个
//!    state 一到又跳回手选（2026-11-10 真机割裂）。
//! 2. 清账点 = **首个 get_state 响应**：state 即真值源；留着旧手选会在
//!    三段链里遮蔽 pi 的真值（--model 解析失败时 pi 回落默认模型）。
use super::*;
use gpui::TestAppContext;

fn runtime(cx: &mut TestAppContext) -> gpui::Entity<SessionRuntime> {
    cx.new(|_| {
        let mut r = SessionRuntime::new("t".into(), PathBuf::from("C:\\t"), None);
        r.sound_on = false;
        r
    })
}

fn feed(rt: &gpui::Entity<SessionRuntime>, cx: &mut TestAppContext, ev: Event) {
    rt.update(cx, |r, cx| r.on_event(ev, cx));
}

fn get_state_response(id: &str, provider: &str, mid: &str, name: &str) -> Event {
    Event::Response {
        id: id.into(),
        command: "get_state".into(),
        success: true,
        error: None,
        data: Some(serde_json::json!({
            "model": {"id": mid, "name": name, "provider": provider},
            "thinkingLevel": "high",
            "isStreaming": false,
            "messageCount": 0,
        })),
    }
}

/// 回归 1：首个 get_state 到达清账手选，state 接管标签真值。
#[gpui::test]
fn pending_model_cleared_by_first_state(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    rt.update(cx, |r, _| {
        r.pending_model = Some(("zai".into(), "glm-5.3-flash".into()))
    });
    feed(
        &rt,
        cx,
        get_state_response("s1", "zai", "glm-5.3-flash", "GLM-5.3-Flash"),
    );
    rt.read_with(cx, |r, _| {
        assert!(
            r.pending_model.is_none(),
            "state 已就位，手选必须清账（否则遮蔽 pi 真值）"
        );
        assert_eq!(
            r.state.as_ref().and_then(|s| s.model_label()).as_deref(),
            Some("GLM-5.3-Flash")
        );
    });
}

/// 回归 2（副症状面）：pi 解析失败回落默认模型时，清账让标签跟 pi 真值，
/// 不被落空的手选钉死。
#[gpui::test]
fn state_wins_over_stale_pick_on_fallback(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    rt.update(cx, |r, _| {
        r.pending_model = Some(("zai".into(), "glm-5.3-flash".into()))
    });
    // pi 端实际落了默认商 deepseek（--model 未解析成功的回落）
    feed(
        &rt,
        cx,
        get_state_response("s1", "deepseek", "deepseek-flash", "DeepSeek V4.1 Flash"),
    );
    rt.read_with(cx, |r, _| {
        assert!(r.pending_model.is_none());
        assert_eq!(
            r.state.as_ref().and_then(|s| s.model_label()).as_deref(),
            Some("DeepSeek V4.1 Flash")
        );
    });
}

/// 回归 3：手选撑过 spawn 窗口（无 state 时不清账）——只有 get_state
/// 响应臂清账，其他响应/事件不得碰它。
#[gpui::test]
fn pending_model_survives_non_state_traffic(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    rt.update(cx, |r, _| {
        r.pending_model = Some(("zai".into(), "glm-5.3-flash".into()))
    });
    feed(
        &rt,
        cx,
        Event::Response {
            id: "c1".into(),
            command: "get_commands".into(),
            success: true,
            error: None,
            data: Some(serde_json::json!({"commands": []})),
        },
    );
    feed(&rt, cx, Event::AgentStart);
    feed(&rt, cx, Event::AgentSettled);
    rt.read_with(cx, |r, _| {
        assert!(
            r.pending_model.is_some(),
            "无 state 前手选必须撑住（pi 冷启动窗口的标签真值）"
        );
    });
}
