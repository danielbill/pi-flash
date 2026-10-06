//! Live transcript replay probe (machine-dependent, guarded).
//!
//! Dumps `transcript_system()` for one real session file so it can be diffed
//! against pi-ai's own reference rendering:
//!
//! ```text
//! PI_FLASH_PROBE_SESSION=/path/to/session.jsonl \
//!   cargo test -p pi-link --test live_transcript_probe -- --ignored --nocapture
//! ```
//!
//! Reference side (same messages, pi-ai `getCurrentSystemPrompt`):
//!
//! ```js
//! import {getCurrentSystemPrompt, getCurrentTools} from
//!   "…/@earendil-works/pi-ai/dist/utils/transcript.js";
//! ```
//!
//! Writes `<tmp>/pi-flash-{prompt,tools}.txt`. Parity was confirmed byte-for-byte
//! on a 560-message session (62,522-char prompt, 26 tools).

use std::io::Write;

#[test]
#[ignore]
fn dump_replay() {
    let path = std::env::var("PI_FLASH_PROBE_SESSION").expect("PI_FLASH_PROBE_SESSION");
    let text = std::fs::read_to_string(&path).expect("read session");
    let mut msgs: Vec<serde_json::Value> = Vec::new();
    for line in text.lines() {
        let Ok(v) = pi_link::json::parse_value(line) else {
            continue;
        };
        if v["type"].as_str() == Some("message") {
            if let Some(m) = v.get("message") {
                msgs.push(m.clone());
            }
        }
    }
    let sys = pi_link::transcript::transcript_system(&msgs).expect("system message");
    let dir = std::env::temp_dir();
    let mut f = std::fs::File::create(dir.join("pi-flash-prompt.txt")).unwrap();
    f.write_all(sys.prompt.as_bytes()).unwrap();
    let tools: Vec<String> = sys
        .tools
        .iter()
        .map(|t| format!("{}\t{}", t.name, t.description))
        .collect();
    std::fs::write(dir.join("pi-flash-tools.txt"), tools.join("\n")).unwrap();
    eprintln!(
        "prompt chars {} tools {} -> {}",
        sys.prompt.chars().count(),
        tools.len(),
        dir.display()
    );
}
