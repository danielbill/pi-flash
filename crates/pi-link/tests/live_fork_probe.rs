//! Live fork probe (machine-dependent, ignored): open a real persisted
//! session in vendored pi rpc mode, read the fork anchors, optionally fork,
//! and report what fork + post-fork get_state say.
//!
//!   cargo test -p pi-link --test live_fork_probe -- --ignored --nocapture
//!
//! Env:
//!   PROBE_SESSION=<path to a session .jsonl>  (required)
//!   PROBE_FORK=1                              (optional; actually forks, which
//!                                              writes a real branch session file)

use futures::{FutureExt, StreamExt};
use std::time::{Duration, Instant};

fn drain(
    rx: &mut futures::channel::mpsc::UnboundedReceiver<pi_link::protocol::Event>,
    want: &str,
    secs: u64,
) -> Option<pi_link::protocol::Event> {
    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut found = None;
    while Instant::now() < deadline {
        let ev = futures::executor::block_on(async {
            let d = Duration::from_millis(400);
            futures::select_biased! {
                e = rx.next() => e,
                _ = futures_timer::Delay::new(d).fuse() => None,
            }
        });
        match ev {
            Some(e) => {
                if let pi_link::protocol::Event::Response {
                    command, data, success, error, ..
                } = &e
                {
                    let head = data
                        .as_ref()
                        .map(|d| d.to_string())
                        .unwrap_or_default()
                        .chars()
                        .take(120)
                        .collect::<String>();
                    eprintln!("   << {command} ok={success} err={error:?} data={head}");
                    if command == want {
                        found = Some(e);
                        break;
                    }
                }
            }
            // quiet period after a match, or keep waiting
            None => {
                if found.is_some() {
                    break;
                }
            }
        }
    }
    found
}

fn field(data: &Option<serde_json::Value>, key: &str) -> String {
    data.as_ref()
        .and_then(|d| d[key].as_str())
        .unwrap_or("?")
        .to_string()
}

#[test]
#[ignore]
fn probe_fork() {
    let path = std::env::var("PROBE_SESSION").expect("PROBE_SESSION=<session.jsonl>");
    let cwd = std::env::current_dir().unwrap();
    let (session, mut rx) =
        pi_link::client::spawn(&cwd, &["--session", &path]).expect("spawn pi");
    eprintln!("spawned pi pid={} session={path}", session.id());

    session.send(&pi_link::protocol::Command::GetState).unwrap();
    let st = drain(&mut rx, "get_state", 30).expect("no get_state");
    if let pi_link::protocol::Event::Response { data, .. } = &st {
        eprintln!(
            "BEFORE sessionFile={} sessionId={}",
            field(data, "sessionFile"),
            field(data, "sessionId"),
        );
    }

    // get_entries is the authoritative fork-anchor source: flat, never nests,
    // so it survives sessions where pi's own get_tree dies with
    // "Maximum call stack size exceeded".
    session
        .send(&pi_link::protocol::Command::GetEntries)
        .unwrap();
    let entries_ev = drain(&mut rx, "get_entries", 30).expect("no get_entries");
    let mut anchors: Vec<String> = Vec::new();
    if let pi_link::protocol::Event::Response { data, .. } = &entries_ev {
        let d = data.as_ref().expect("get_entries data");
        let (entries, leaf) = pi_link::protocol::parse_entries(d);
        anchors = pi_link::protocol::active_user_entry_ids(&entries, leaf.as_deref());
        eprintln!(
            "GET_ENTRIES entries={} leaf={:?} user_anchors={}",
            entries.len(),
            leaf,
            anchors.len()
        );
    }
    assert!(
        !anchors.is_empty(),
        "no user anchors on the active chain — 「新分支」 would be inert"
    );

    // nav panel source; failure here is expected on deep sessions and must not
    // affect forkability
    session.send(&pi_link::protocol::Command::GetTree).unwrap();
    match drain(&mut rx, "get_tree", 30) {
        Some(pi_link::protocol::Event::Response { data, .. }) => {
            eprintln!("GET_TREE payload_bytes={}", data.map(|d| d.to_string().len()).unwrap_or(0));
        }
        _ => eprintln!("GET_TREE: no response at all"),
    }

    if std::env::var("PROBE_FORK").as_deref() != Ok("1") {
        eprintln!("(read-only probe: set PROBE_FORK=1 to actually fork)");
        return;
    }

    // PROBE_MODE=clone exercises the rpc `clone` command (branch at the leaf,
    // i.e. keep everything) instead of fork-before-a-user-message.
    let clone_mode = std::env::var("PROBE_MODE").as_deref() == Ok("clone");
    if clone_mode {
        eprintln!("=== clone (branch at the leaf)");
        session.send(&pi_link::protocol::Command::Clone).unwrap();
    } else {
        let target = anchors[anchors.len() / 2].clone();
        eprintln!("=== fork at {target}");
        session
            .send(&pi_link::protocol::Command::Fork {
                entry_id: target.clone(),
            })
            .unwrap();
    }
    let want = if clone_mode { "clone" } else { "fork" };
    let forked = drain(&mut rx, want, 30).expect("no fork/clone response");
    if let pi_link::protocol::Event::Response {
        success,
        error,
        data,
        ..
    } = &forked
    {
        eprintln!("FORK success={success} error={error:?} data={data:?}");
    }

    session.send(&pi_link::protocol::Command::GetState).unwrap();
    let st2 = drain(&mut rx, "get_state", 30).expect("no get_state after fork");
    if let pi_link::protocol::Event::Response { data, .. } = &st2 {
        eprintln!(
            "AFTER sessionFile={} sessionId={} messageCount={}",
            field(data, "sessionFile"),
            field(data, "sessionId"),
            data.as_ref()
                .map(|d| d["messageCount"].to_string())
                .unwrap_or_else(|| "?".into()),
        );
    }

    // 分支自动改名（用户要求：原 title 前 15 字 + "2"）
    let rename = std::env::var("PROBE_RENAME").unwrap_or_else(|_| "test-name-2".into());
    session
        .send(&pi_link::protocol::Command::SetSessionName {
            name: rename.clone(),
        })
        .unwrap();
    match drain(&mut rx, "set_session_name", 30) {
        Some(pi_link::protocol::Event::Response { success, error, .. }) => {
            eprintln!("SET_SESSION_NAME success={success} error={error:?}");
        }
        _ => eprintln!("SET_SESSION_NAME: no response at all"),
    }
    session.send(&pi_link::protocol::Command::GetState).unwrap();
    if let Some(pi_link::protocol::Event::Response { data, .. }) =
        drain(&mut rx, "get_state", 30)
    {
        eprintln!("AFTER RENAME sessionName={:?}", data.as_ref().map(|d| d["sessionName"].clone()));
        if let Some(f) = data.as_ref().and_then(|d| d["sessionFile"].as_str()) {
            let txt = std::fs::read_to_string(f).unwrap_or_default();
            for line in txt.lines().rev().take(40) {
                if line.contains("session_info") {
                    eprintln!("   file session_info: {}", &line[..line.len().min(200)]);
                    break;
                }
            }
        }
    }

    session.send(&pi_link::protocol::Command::GetMessages).unwrap();
    if let Some(pi_link::protocol::Event::Response { data, .. }) =
        drain(&mut rx, "get_messages", 30)
    {
        eprintln!(
            "AFTER messages={}",
            data.as_ref()
                .and_then(|d| d["messages"].as_array())
                .map(|a| a.len())
                .unwrap_or(0)
        );
    }
}
