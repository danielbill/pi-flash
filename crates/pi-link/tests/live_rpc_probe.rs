//! Live RPC probe (machine-dependent, guarded): spawn the vendored pi in
//! rpc mode, issue the session-handshake commands the app sends on open,
//! and print every event that comes back. Diagnostic for "model list
//! empty / no state" regressions — run with:
//!   cargo test -p pi-link --test live_rpc_probe -- --ignored --nocapture

use futures::FutureExt;
use futures::stream::StreamExt;
use std::time::{Duration, Instant};

#[test]
#[ignore]
fn probe_vendored_pi_handshake() {
    let cwd = std::env::current_dir().unwrap();
    let (session, mut rx) = match pi_link::client::spawn(&cwd, &[]) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("SPAWN FAILED: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("spawned pi pid={}", session.id());

    session
        .send(&pi_link::protocol::Command::GetState)
        .unwrap();
    session
        .send(&pi_link::protocol::Command::GetAvailableModels)
        .unwrap();
    session
        .send(&pi_link::protocol::Command::GetCommands)
        .unwrap();

    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = 0usize;
    let mut quiet = 0u32;
    while Instant::now() < deadline {
        let ev = futures::executor::block_on(async {
            let d = Duration::from_millis(500);
            futures::select_biased! {
                e = rx.next() => e,
                _ = futures_timer::Delay::new(d).fuse() => None,
            }
        });
        match ev {
            Some(e) => {
                seen += 1;
                quiet = 0;
                match &e {
                    pi_link::protocol::Event::Response { command, success, data, .. } => {
                        eprintln!(
                            "[{seen}] Response command={command} success={success} data={}",
                            data
                                .as_ref()
                                .map(|d| d.to_string())
                                .unwrap_or_default()
                                .chars()
                                .take(400)
                                .collect::<String>()
                        );
                        if command == "get_available_models" {
                            let models = data
                                .as_ref()
                                .map(|d| pi_link::protocol::parse_model_list(d))
                                .unwrap_or_default();
                            eprintln!("    parsed models: {}", models.len());
                            for m in models.iter().take(5) {
                                eprintln!("    - {}/{}", m.provider, m.id);
                            }
                        }
                    }
                    other => eprintln!("[{seen}] {other:?}"),
                }
            }
            None => {
                quiet += 1;
                if seen > 0 && quiet >= 6 {
                    break; // 3s of silence after at least one event
                }
            }
        }
    }
    eprintln!("total events: {seen}");
    assert!(
        seen > 0,
        "no events received from vendored pi — RPC flow broken"
    );
}
