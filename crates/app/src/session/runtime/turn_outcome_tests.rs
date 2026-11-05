//! 轮末结局（三色通知点）的定义锁：结局 = **最后一条 assistant 消息的
//! stopReason**，由用户消息归零、由每条 assistant 消息覆盖。
//!
//! 两个真机回归：
//! 1. pi 的 `agent_end.willRetry` 是乐观预判（先报 true，再看
//! `_prepareRetry` 决定是否真重试）。原先在这里清旗 → 退避期间被
//! 中止/未重试时，错误结局被清掉 → 错误的轮次报青蓝完成点。
//! 2. `toolResult.isError`（grep 未命中、命令非零退出）被当成轮末错误 →
//! 正常完成的轮次报红点。
use super::*;
use gpui::TestAppContext;

fn ev_start(role: &str, is_error: bool) -> Event {
    Event::MessageStart {
        role: role.into(),
        blocks: Vec::new(),
        timestamp: None,
        is_error,
        tool_call_id: None,
        custom_type: None,
        custom_display: true,
        details: None,
        raw_system: None,
    }
}

fn ev_end(role: &str, stop: &str, err: Option<&str>) -> Event {
    Event::MessageEnd {
        role: role.into(),
        blocks: Vec::new(),
        usage: None,
        timestamp: None,
        stop_reason: Some(stop.into()),
        error_message: err.map(str::to_string),
        is_error: false,
        tool_call_id: None,
        custom_type: None,
        custom_display: true,
        model: Some("m".into()),
    }
}

fn runtime(cx: &mut TestAppContext) -> gpui::Entity<SessionRuntime> {
    cx.new(|_| {
        let mut r = SessionRuntime::new("t".into(), PathBuf::from("C:\t"), None);
        r.sound_on = false;
        r
    })
}

fn feed(rt: &gpui::Entity<SessionRuntime>, cx: &mut TestAppContext, ev: Event) {
    rt.update(cx, |r, cx| r.on_event(ev, cx));
}

/// 真机回归 1：错误尝试之后 willRetry=true，但重试没跑起来（退避期间被
/// 中止/新消息打断）→ 结算必须仍是「出错」，不能被乐观预判清成完成。
#[gpui::test]
fn error_survives_optimistic_will_retry(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    feed(&rt, cx, ev_start("user", false));
    feed(&rt, cx, ev_start("assistant", false));
    feed(
        &rt,
        cx,
        ev_end("assistant", "error", Some("Connection error.")),
    );
    assert!(rt.read_with(cx, |r, _| r.turn_error), "error 消息当即置红");
    feed(&rt, cx, Event::AgentEnd { will_retry: true });
    assert!(
        rt.read_with(cx, |r, _| r.turn_error),
        "乐观 willRetry 不得清掉已发生的错误结局"
    );
    feed(&rt, cx, Event::AgentSettled);
    assert!(rt.read_with(cx, |r, _| r.turn_error), "结算时仍是红档");
}

/// 重试真的成功：最后一条 assistant 的 stop 覆盖错误结局 → 青蓝。
#[gpui::test]
fn retry_success_overwrites_error(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    feed(&rt, cx, ev_start("user", false));
    feed(&rt, cx, ev_start("assistant", false));
    feed(
        &rt,
        cx,
        ev_end("assistant", "error", Some("Connection error.")),
    );
    // 重试：pi 会再发一次 agent_start + 一条成功的 assistant 消息
    feed(&rt, cx, Event::AgentStart);
    feed(&rt, cx, ev_start("assistant", false));
    feed(&rt, cx, ev_end("assistant", "stop", None));
    feed(&rt, cx, Event::AgentEnd { will_retry: false });
    feed(&rt, cx, Event::AgentSettled);
    assert!(!rt.read_with(cx, |r, _| r.turn_error));
    assert!(!rt.read_with(cx, |r, _| r.turn_interrupted));
}

/// pi-flash-7ep 回归：退避窗口轮**未**结束——agent_end{willRetry:true}
/// 不得复位 agent_running（composer 停止按钮/输入框提示/等待行同源，
/// 就地复位会在退避的 2-8s 里把「停止」翻回「发送」）；auto_retry_start
/// 报倒计时；真轮末（willRetry=false + settled）才回落 idle。
#[gpui::test]
fn retry_backoff_keeps_running_state(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    feed(&rt, cx, Event::AgentStart);
    feed(&rt, cx, ev_start("assistant", false));
    feed(
        &rt,
        cx,
        ev_end("assistant", "error", Some("429: too many requests")),
    );
    feed(&rt, cx, Event::AgentEnd { will_retry: true });
    assert!(
        rt.read_with(cx, |r, _| r.agent_running),
        "退避窗口必须保持运行态，否则停止按钮翻回发送"
    );
    feed(
        &rt,
        cx,
        Event::AutoRetryStart {
            attempt: 1,
            max_attempts: 3,
            delay_ms: 2000,
            error_message: Some("429".into()),
        },
    );
    assert!(rt.read_with(cx, |r, _| r.agent_running));
    assert!(
        rt.read_with(cx, |r, _| r.status.contains("2s")),
        "状态行报退避倒计时，实际：{}",
        rt.read_with(cx, |r, _| r.status.clone())
    );
    // 重试跑起来并成功收尾
    feed(&rt, cx, Event::AgentStart);
    feed(&rt, cx, ev_start("assistant", false));
    feed(&rt, cx, ev_end("assistant", "stop", None));
    feed(
        &rt,
        cx,
        Event::AutoRetryEnd {
            success: true,
            attempt: 1,
            final_error: None,
        },
    );
    feed(&rt, cx, Event::AgentEnd { will_retry: false });
    feed(&rt, cx, Event::AgentSettled);
    assert!(!rt.read_with(cx, |r, _| r.agent_running), "真轮末回落 idle");
}

/// 真机回归 2：工具报错（isError）是工作过程，不是轮末结局。
#[gpui::test]
fn tool_error_is_not_a_turn_error(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    feed(&rt, cx, ev_start("user", false));
    feed(&rt, cx, ev_start("assistant", false));
    feed(&rt, cx, ev_start("toolResult", true));
    feed(&rt, cx, ev_end("toolResult", "stop", None));
    feed(&rt, cx, ev_start("assistant", false));
    feed(&rt, cx, ev_end("assistant", "stop", None));
    feed(&rt, cx, Event::AgentSettled);
    assert!(!rt.read_with(cx, |r, _| r.turn_error));
}

/// 黄色档：length/aborted 是内部中断，非外部错误。
#[gpui::test]
fn length_stop_is_warning_not_error(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    feed(&rt, cx, ev_start("user", false));
    feed(&rt, cx, ev_start("assistant", false));
    feed(&rt, cx, ev_end("assistant", "length", None));
    feed(&rt, cx, Event::AgentSettled);
    assert!(!rt.read_with(cx, |r, _| r.turn_error));
    assert!(rt.read_with(cx, |r, _| r.turn_interrupted));
}

/// 新一轮用户消息把上轮结局归零（上一轮的黄/红不得泄漏进下一轮）。
#[gpui::test]
fn new_user_turn_resets_outcome(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    feed(&rt, cx, ev_start("user", false));
    feed(&rt, cx, ev_start("assistant", false));
    feed(&rt, cx, ev_end("assistant", "error", Some("boom")));
    assert!(rt.read_with(cx, |r, _| r.turn_error));
    // 用户再发一条 → 归零（重试不发用户消息，所以重试不会误清）
    feed(&rt, cx, ev_start("user", false));
    assert!(!rt.read_with(cx, |r, _| r.turn_error));
    assert!(!rt.read_with(cx, |r, _| r.turn_interrupted));
}

/// 进程死亡收尾（pi-flash-7r0）：半截工具卡就地落错误态
/// （红框 + 中断文案），已回结果的卡不动。
#[gpui::test]
fn process_death_closes_dangling_tool_calls(cx: &mut TestAppContext) {
    use pi_link::protocol::{AssistantEvent, Block};
    let rt = runtime(cx);
    feed(&rt, cx, ev_start("assistant", false));
    feed(
        &rt,
        cx,
        Event::MessageUpdate(AssistantEvent::ToolCallStart {
            content_index: 0,
            id: "a".into(),
            tool_name: "bash".into(),
        }),
    );
    feed(
        &rt,
        cx,
        Event::MessageUpdate(AssistantEvent::ToolCallStart {
            content_index: 1,
            id: "b".into(),
            tool_name: "read".into(),
        }),
    );
    // 卡 B 先回了结果：进程死时不得被改写
    rt.update(cx, |r, _| {
        if let Block::ToolCall {
            result,
            result_arrived,
            ..
        } = &mut r.messages[0].blocks[1]
        {
            *result = "ok".into();
            *result_arrived = true;
        }
    });
    rt.update(cx, |r, _| r.fail_dangling_tool_calls());
    rt.read_with(cx, |r, _| {
        let blocks = &r.messages[0].blocks;
        let Block::ToolCall {
            result: ra,
            is_error: ea,
            args_partial: pa,
            result_arrived: va,
            ..
        } = &blocks[0]
        else {
            panic!("block 0 not toolcall")
        };
        assert!(*va && *ea && !*pa, "半截卡落错误态");
        assert!(!ra.is_empty(), "中断文案已填");
        let Block::ToolCall {
            result: rb,
            is_error: eb,
            result_arrived: vb,
            ..
        } = &blocks[1]
        else {
            panic!("block 1 not toolcall")
        };
        assert_eq!(rb, "ok", "已回结果的卡不动");
        assert!(*vb && !*eb);
    });
}

/// ProcessExited 存退出码；Unparsed 按 type 去重留痕。
#[gpui::test]
fn exit_code_and_unparsed_dedup(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    feed(&rt, cx, Event::ProcessExited { code: Some(1) });
    feed(
        &rt,
        cx,
        Event::Unparsed(serde_json::json!({"type":"mystery"})),
    );
    feed(
        &rt,
        cx,
        Event::Unparsed(serde_json::json!({"type":"mystery"})),
    );
    feed(
        &rt,
        cx,
        Event::Unparsed(serde_json::json!({"type":"other"})),
    );
    rt.read_with(cx, |r, _| {
        assert_eq!(r.exit_code, Some(1));
        assert_eq!(r.unparsed_types.len(), 2, "同 type 只记一次");
    });
}

/// 扩展命令收场按 pi-web `prompt_done → loadSession` 的磁盘真相语义：
/// 会话已落盘 → 盘上没有这条命令消息，摘乐观气泡；**未落盘** → pi-web
/// 的 loadSession 走 404 分支不动消息列表，气泡与侧栏 transient 行（PF
/// 伪标签）都保留。真机回归：/usage 一关，屏幕与侧栏都像没执行过。
#[gpui::test]
fn command_phase_settle_follows_disk_truth(cx: &mut TestAppContext) {
    // 未落盘：气泡 / 伪标签保留
    let rt = runtime(cx);
    rt.update(cx, |r, cx| r.optimistic_send("/usage".into(), true, cx));
    rt.update(cx, |r, cx| r.settle_command_phase(cx));
    rt.read_with(cx, |r, _| {
        assert_eq!(r.messages.len(), 1, "未落盘：命令气泡保留");
        assert_eq!(r.pending_echoes.len(), 1);
        assert!(!r.phase_waiting && !r.phase_command, "相位已收场");
    });

    // 已落盘：磁盘真相赢，摘除气泡
    let real = std::env::temp_dir().join(format!(
        "pf-cmd-settle-{}-{}.jsonl",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::write(&real, "{}\n").unwrap();
    let rt2 = runtime(cx);
    rt2.update(cx, |r, cx| {
        r.state = Some(SessionState {
            session_file: Some(real.to_string_lossy().into_owned()),
            ..Default::default()
        });
        r.optimistic_send("/usage".into(), true, cx);
        r.settle_command_phase(cx);
        assert!(r.messages.is_empty(), "已落盘：气泡随磁盘真相摘除");
        assert!(r.pending_echoes.is_empty());
        assert!(r.file.is_some(), "收场时补上文件绑定");
    });
    let _ = std::fs::remove_file(&real);
}

/// 运行中发扩展命令（agent 还在上一轮里）：handled 回执必须收场命令相位。
/// 真机回归：`agent_running` 来自**上一轮**，旧守卫 `if !agent_running`
/// 直接跳过 → 等待行永远卡在「执行扩展命令中...」。
#[gpui::test]
fn handled_settles_command_phase_while_previous_run_active(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    feed(&rt, cx, Event::AgentStart);
    rt.update(cx, |r, cx| r.optimistic_send("/usage".into(), true, cx));
    assert!(rt.read_with(cx, |r, _| r.phase_command && r.phase_waiting));
    feed(
        &rt,
        cx,
        Event::Response {
            id: "p1".into(),
            command: "prompt".into(),
            success: true,
            error: None,
            data: Some(serde_json::json!({"disposition": "handled"})),
        },
    );
    rt.read_with(cx, |r, _| {
        assert!(!r.phase_command, "命令相位无条件收场");
        assert!(r.phase_waiting, "上一轮仍在跑：等待行让给真轮");
        assert!(r.agent_running);
    });
}

/// flush 链上的扩展命令：排队发出的 /usage（agent 跑着时用户发的），
/// 回执也是 disposition=handled（pi 对扩展命令不开 run）。此前被 flush
/// 回执分支吞掉 → 等待行永久「执行扩展命令中」，弹窗关掉也不退。
#[gpui::test]
fn handled_on_flush_path_settles_command_phase(cx: &mut TestAppContext) {
    let rt = runtime(cx);
    rt.update(cx, |r, cx| {
        r.optimistic_send("/usage".into(), true, cx);
        r.flush_in_flight = Some(FlushInFlight {
            id: "cmd-9".into(),
            interrupt: None,
        });
    });
    assert!(rt.read_with(cx, |r, _| r.phase_command && r.phase_waiting));
    feed(
        &rt,
        cx,
        Event::Response {
            id: "cmd-9".into(),
            command: "prompt".into(),
            success: true,
            error: None,
            data: Some(serde_json::json!({"disposition": "handled"})),
        },
    );
    rt.read_with(cx, |r, _| {
        assert!(
            !r.phase_command && !r.phase_waiting,
            "flush 路径 handled 也必须收场"
        );
        assert!(r.flush_in_flight.is_none(), "flush 关联已消化");
    });
}
