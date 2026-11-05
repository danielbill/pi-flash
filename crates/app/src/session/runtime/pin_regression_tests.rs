//! 发言钉顶回归锁：空闲普通发送 = 发送帧 + 回显双钉顶；
//! 打断投递（立即/Ctrl+Enter）全程不钉（上屏与回显同决）；
//! stream_cut 残包并入旧 assistant，不得在打断气泡后长出新壳。

use super::*;
use gpui::TestAppContext;

fn runtime(cx: &mut TestAppContext) -> gpui::Entity<SessionRuntime> {
    cx.new(|_| {
        let mut r = SessionRuntime::new("pin-t".into(), PathBuf::from("C:/t"), None);
        r.sound_on = false;
        r
    })
}

fn feed(rt: &gpui::Entity<SessionRuntime>, cx: &mut TestAppContext, ev: Event) {
    rt.update(cx, |r, cx| r.on_event(ev, cx));
}

fn msg_start(role: &str, text: &str) -> Event {
    Event::MessageStart {
        role: role.into(),
        blocks: vec![pi_link::protocol::Block::Text {
            content_index: 0,
            text: text.into(),
        }],
        timestamp: None,
        is_error: false,
        tool_call_id: None,
        custom_type: None,
        custom_display: true,
        details: None,
        raw_system: None,
    }
}

fn text_delta(delta: &str) -> Event {
    Event::MessageUpdate(pi_link::protocol::AssistantEvent::TextDelta {
        content_index: 0,
        delta: delta.into(),
    })
}

/// 普通发送：发送帧 page_turn，回显按发送帧的 pin=true 再钉（不回退）。
#[gpui::test]
fn normal_send_echo_keeps_pin(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    rt.update(cx, |r, cx| {
        r.optimistic_send("你好".into(), true, cx);
        assert_eq!(r.pager.anchor_ix(), Some(0), "发送帧钉顶");
        assert!(
            r.pending_echoes.last().is_some_and(|p| p.pin),
            "普通发送 pin=true 随条目入列"
        );
    });
    feed(&rt, cx, msg_start("user", "你好"));
    rt.read_with(cx, |r, _| {
        assert_eq!(r.messages.len(), 1, "回显就地升级，不重复推气泡");
        assert!(r.pending_echoes.is_empty());
        assert_eq!(r.pager.anchor_ix(), Some(0), "回显按 pin=true 继续钉顶");
    });
}

/// 空闲投递（steer 档 Enter / 空闲点立即）= **普通发送**：agent 没在
/// 说话无所谓插话，照常发言钉顶，回显继续钉。
#[gpui::test]
fn idle_delivery_pins_like_normal_send(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    rt.update(cx, |r, cx| {
        r.queued_inputs.push(QueuedInput {
            text: "插队".into(),
            images: Vec::new(),
            remote: false,
        });
        r.queue_send_now(0, cx);
        assert_eq!(
            r.pager.anchor_ix(),
            Some(0),
            "空闲投递 = 普通发送，照常钉顶"
        );
        assert_eq!(r.messages.len(), 1);
        assert!(r.interrupt_pending.is_some());
        assert!(!r.stream_cut, "空闲打断无 abort，不置切断标记");
    });
    feed(&rt, cx, msg_start("user", "插队"));
    rt.read_with(cx, |r, _| {
        assert_eq!(r.messages.len(), 1, "回显就地升级");
        assert_eq!(r.pager.anchor_ix(), Some(0), "回显继续钉顶");
    });
}

/// 运行中打断 + abort 在途残包：残包并入旧 assistant（气泡上方收尾），
/// 不得在气泡后长新壳；回显升级不钉顶。
#[gpui::test]
fn stale_deltas_merge_into_old_assistant(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    rt.update(cx, |r, cx| {
        r.on_event(Event::AgentStart, cx);
        r.on_event(msg_start("assistant", ""), cx);
        r.on_event(text_delta("旧"), cx);
    });
    rt.update(cx, |r, cx| {
        r.queued_inputs.push(QueuedInput {
            text: "插".into(),
            images: Vec::new(),
            remote: false,
        });
        r.queue_send_now(0, cx);
        assert!(r.stream_cut, "运行中打断置切断标记");
        assert_eq!(r.pager.anchor_ix(), None, "运行中打断不钉顶（跟流不刷屏）");
        assert_eq!(r.messages.len(), 2, "assistant + 打断气泡");
        assert_eq!(
            r.messages[1].role,
            crate::session::messages::Role::User,
            "气泡在末尾"
        );
    });
    // abort 前入管道的残包此刻才到
    rt.update(cx, |r, cx| {
        r.on_event(text_delta("残包"), cx);
    });
    rt.read_with(cx, |r, _| {
        assert_eq!(r.messages.len(), 2, "残包不得在气泡后长出新 assistant");
        let merged = match r.messages[0].blocks.first() {
            Some(pi_link::protocol::Block::Text { text, .. }) => text.clone(),
            other => panic!("expected text block, got {other:?}"),
        };
        assert!(
            merged.contains("旧") && merged.contains("残包"),
            "残包并入旧 assistant：{merged}"
        );
        assert_eq!(
            r.messages[1].role,
            crate::session::messages::Role::User,
            "气泡保持在末尾"
        );
    });
    // abort 收尾 → AgentSettled（清切断标记 + flush 槽内条目）→ 新轮回显
    feed(&rt, cx, Event::AgentSettled);
    rt.read_with(cx, |r, _| {
        assert!(!r.stream_cut, "结算清除切断标记");
    });
    feed(&rt, cx, msg_start("user", "插"));
    rt.read_with(cx, |r, _| {
        assert_eq!(r.messages.len(), 2, "回显升级打断气泡");
        assert_eq!(r.pager.anchor_ix(), None, "打断回显不钉顶");
    });
}

/// 用户报障回归锁：首条消息已钉顶、agent 思考中再发一条
/// steer——打断投递不得退役钉顶锚点。钉顶期（内容还短于一屏）退役 = 摘
/// 垫片，Bottom 胶水把整块内容拽到屏底（「所有消息落到屏幕下半部分」）。
/// 几何锁见 chat_list.rs `interrupt_while_pinned_keeps_position`。
#[gpui::test]
fn steer_while_pinned_keeps_anchor(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    rt.update(cx, |r, cx| {
        r.optimistic_send("第一条".into(), true, cx);
        assert_eq!(r.pager.anchor_ix(), Some(0), "首条照常钉顶");
        r.agent_running = true;
        r.input = "第二条".into();
        r.steer_input(cx);
    });
    rt.read_with(cx, |r, _| {
        assert_eq!(
            r.pager.anchor_ix(),
            Some(0),
            "钉顶期打断不退役锚点（昵了就落到屏幕下半部）"
        );
        assert_eq!(r.messages.len(), 2, "插话气泡接在下面");
        assert_eq!(r.messages[1].role, crate::session::messages::Role::User);
        assert!(r.interrupt_pending.is_some(), "打断槽持有本条");
    });
}
