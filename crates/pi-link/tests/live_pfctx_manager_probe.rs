//! Live probe (machine-dependent, `#[ignore]`-guarded): spawn the vendored pi
//! with the **pfctx-manager** extension mounted and `--tools pfctx_manager`,
//! then give the real agent an instruction to call the tool — and assert the
//! host contract lands: an `entry_appended` event with
//! `customType == "pfctx-manager-op"` carrying the ops (001-4).
//!
//! 这是「agent 能否调用新工具」的端到端自动化（无需 UI / 无需 PF 宿主在场）：
//! 条目即宿主消费契约（on_event → pfctx_ops → 设置页同链）。宿主侧执行另由
//! gpui 桥单测与设置页同链单测覆盖（test/001-PFContext/pfctx-manager/case.md）。
//!
//! 前置：pi 已配置可用模型（auth.json/models.json 与日常使用同源）。
//! 跑法：
//!   cargo test -p pi-link --test live_pfctx_manager_probe -- --ignored --nocapture
//! 可用 env：
//!   PFCTX_PROBE_CWD=<dir>   agent 工作目录（默认 temp 子目录，防误动真项目）
//!   PFCTX_PROBE_TIMEOUT=<s> 等待上限（默认 180；LLM 出牌有延迟）

use std::time::{Duration, Instant};

use futures::{FutureExt, StreamExt};
use pi_link::protocol::{Command, Event};
use serde_json::json;

#[test]
#[ignore]
fn agent_calls_pfctx_manager_and_entry_reaches_host() {
    let cwd = std::env::var("PFCTX_PROBE_CWD")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| {
            let d = std::env::temp_dir().join("pfctx-manager-probe");
            std::fs::create_dir_all(&d).unwrap();
            d
        });
    // 模型：机器默认档（mimo-free）常配额耗尽 → 显式指定验证过可用的模型，
    // env 可覆盖。该选择只影响本探针，不改用户设置。
    let model = std::env::var("PFCTX_PROBE_MODEL").unwrap_or_else(|_| "deepseek-flash".into());
    // 数据目录（app spawn.rs 同款注入）：扩展 query 定位 app-settings.json、
    // 说明里指 pfctx.log 都靠它。探针读真目录（只读 + 条目投递，无宿主执行）。
    let envs: Vec<(String, String)> = match pi_link::paths::dir() {
        Some(d) => vec![("PI_FLASH_DIR".into(), d.to_string_lossy().into_owned())],
        None => vec![],
    };
    // 双开：扩展挂载 + allowlist 点名（与 app spawn 同参口径）
    let (session, mut rx) = match pi_link::client::spawn(
        &cwd,
        &["--tools", "pfctx_manager", "--model", &model],
        &envs,
        true,
        true,
    ) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("SPAWN FAILED: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("spawned pi pid={}", session.id().unwrap_or(0));

    let timeout_secs: u64 = std::env::var("PFCTX_PROBE_TIMEOUT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(180);
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    // 会话握手（app 打开会话必发序列，live_rpc_probe 同款）：缺了它 prompt
    // 被受理却零事件（首跑 90s 无事件实证——session 未初始化不跑轮次）
    session.send(&Command::GetState).unwrap();
    session.send(&Command::GetAvailableModels).unwrap();
    session.send(&Command::GetCommands).unwrap();

    // 明确指令，最大化模型出牌确定性（仍属真实 agent 调用，非注入）
    let prompt = "调用 pfctx_manager 工具：{action:\"set\", ops:[{\"setting\":\"subagents.capacity\",\"value\":8}]}。只做这一次调用，然后结束。";
    session
        .send(&Command::Prompt {
            message: prompt.to_string(),
            images: vec![],
            streaming_behavior: None,
        })
        .unwrap();

    let mut tool_called = false;
    let mut entry_ops: Option<serde_json::Value> = None;
    let mut saw_any_event = false;

    while Instant::now() < deadline {
        // 500ms 有界等待（live_rpc_probe 同款 block_on 模式）
        let ev = futures::executor::block_on(async {
            let d = Duration::from_millis(500);
            futures::select_biased! {
                e = rx.next() => e,
                _ = futures_timer::Delay::new(d).fuse() => None,
            }
        });
        let Some(event) = ev else { continue };
        saw_any_event = true;
        eprintln!(
            "[probe:all] {:?}",
            format!("{event:?}").chars().take(200).collect::<String>()
        );
        if let Event::WireEvent { kind, value } = &event {
            eprintln!("[probe:evt] {kind} {}", {
                let t = value.to_string();
                t.chars().take(160).collect::<String>()
            });
            match kind.as_str() {
                // 工具调用证据（模型真的出了这张牌）
                "tool_execution_update" | "tool_execution_start" => {
                    // 附加证据（字段名随 pi 版本可能漂移，不作硬断言——
                    // entry 才是宿主契约：只有本工具会 append 该 customType）
                    if value.to_string().contains("pfctx_manager") {
                        tool_called = true;
                        eprintln!("[probe] 事件流出现 pfctx_manager 工具痕迹");
                    }
                }
                // 宿主契约本体：op 条目广播
                "entry_appended" => {
                    let entry = &value["entry"];
                    if entry["type"].as_str() == Some("custom")
                        && entry["customType"].as_str() == Some("pfctx-manager-op")
                    {
                        let ops = entry["data"]["ops"].clone();
                        eprintln!("[probe] entry_appended pfctx-manager-op: {ops}");
                        entry_ops = Some(ops);
                    }
                }
                _ => {}
            }
        } else if let Event::Response {
            command,
            success,
            error,
            ..
        } = &event
        {
            eprintln!("[probe:resp] {command} success={success} error={error:?}");
        }
        if entry_ops.is_some() {
            break; // 宿主契约已上流：agent 调用 → appendEntry 全链打通
        }
    }

    assert!(saw_any_event, "无任何事件——RPC 流未建立");
    session.send(&Command::Abort).ok();

    // tool_called 仅为诊断（工具事件字段随 pi 版本可能不落在 WireEvent 上）；
    // 硬断言 = entry：只有 pfctx_manager 工具会 append 该 customType。
    let _ = tool_called;
    let ops =
        entry_ops.expect("超时内未见 pfctx-manager-op 条目——agent 未调用或 appendEntry 契约断裂");
    let first = &ops[0];
    assert_eq!(first["setting"], json!("subagents.capacity"));
    assert_eq!(first["value"], json!(8));
    eprintln!("[probe] PASS：agent 真实调用了 pfctx_manager 且宿主契约条目在流上");
}
