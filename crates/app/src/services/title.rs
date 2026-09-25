//! LLM session-title generation (pi-web lib/session-title.ts parity):
//! prompt constants, transcript compaction and reply sanitization, plus the
//! export-html parsers used for the system-prompt/tools panels.

const TITLE_SYSTEM_PROMPT: &str = "You name chat sessions from a transcript. Reply with the title only.";

const TITLE_PROMPT: &str = "Create a concise title for this session based on the conversation above.\n\
Requirements:\n\
- Match the primary language used by the user.\n\
- Describe the user's concrete goal or the outcome, not the act of chatting.\n\
- Use 4-12 words for space-separated languages, or 8-24 characters for CJK text when practical.\n\
- Do not call any tools.\n\
- Return only the title as plain text, with no quotes, label, markdown, or explanation.";

const TITLE_USER_CHARS: usize = 800;
const TITLE_ASSISTANT_CHARS: usize = 300;
const TITLE_LAST_ASSISTANT_CHARS: usize = 600;
const TITLE_TRANSCRIPT_CHARS: usize = 6000;
const TITLE_HEAD_CHARS: usize = TITLE_TRANSCRIPT_CHARS * 40 / 100;
const TITLE_MAX_LEN: usize = 80;
const TITLE_ELISION: &str = "\u{2026}";

/// One turn fed to [`build_title_transcript`] (decoupled from the chat
/// message model so this service stays UI-free).
pub struct TitleTurn {
    pub user: bool,
    pub text: String,
}

pub fn title_prompts() -> (&'static str, &'static str) {
    (TITLE_SYSTEM_PROMPT, TITLE_PROMPT)
}

fn clip_chars(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push_str(TITLE_ELISION);
    }
    out
}

/// Compact transcript for the title request: every user turn (what was
/// asked), the last reply (what came out), middle replies as openers only.
/// Total budget with head priority keeps the session's opening goal.
pub fn build_title_transcript(messages: &[TitleTurn]) -> String {
    let n = messages.len();
    let mut lines: Vec<String> = Vec::new();
    for (ix, m) in messages.iter().enumerate() {
        let raw = m.text.as_str();
        if raw.trim().is_empty() {
            continue;
        }
        let (role, cap) = match m.user {
            true => ("User", TITLE_USER_CHARS),
            false if ix + 1 == n => ("Assistant", TITLE_LAST_ASSISTANT_CHARS),
            false => ("Assistant", TITLE_ASSISTANT_CHARS),
        };
        lines.push(format!("{role}: {}", clip_chars(raw.trim(), cap)));
    }
    let total: usize = lines.iter().map(|l| l.chars().count()).sum();
    if total <= TITLE_TRANSCRIPT_CHARS {
        return lines.join("\n");
    }
    // head 40%, tail keeps the newest turns, middle elided
    let mut head: Vec<String> = Vec::new();
    let mut used = 0usize;
    let mut ix = 0usize;
    while ix < lines.len() && used < TITLE_HEAD_CHARS {
        used += lines[ix].chars().count();
        head.push(lines[ix].clone());
        ix += 1;
    }
    let mut tail: Vec<String> = Vec::new();
    let mut tused = 0usize;
    let mut j = lines.len();
    while j > ix && tused < TITLE_TRANSCRIPT_CHARS.saturating_sub(used + 40) {
        j -= 1;
        tused += lines[j].chars().count();
        tail.push(lines[j].clone());
    }
    tail.reverse();
    head.push(TITLE_ELISION.to_string());
    head.extend(tail);
    head.join("\n")
}

/// Clean the model's reply into a session title: first non-empty line, strip
/// wrapping quotes/markdown, clamp to the pi-web title length.
pub fn sanitize_title(raw: &str) -> String {
    let mut title = raw
        .lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string();
    loop {
        let before = title.clone();
        for mark in ["#", "*", "`", "\"", "'", "\u{201c}", "\u{201d}"] {
            if title.starts_with(mark) {
                title = title[mark.len()..].trim_start().to_string();
            }
            if title.ends_with(mark) && title.chars().count() > mark.chars().count() {
                title = title[..title.len() - mark.len()].trim_end().to_string();
            }
        }
        if title == before {
            break;
        }
    }
    clip_chars(title.trim(), TITLE_MAX_LEN).trim_end_matches(TITLE_ELISION).trim_end().to_string()
}

/// Unescape the handful of entities escapeHtml produces.
pub fn html_unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
}

/// Extract (systemPrompt, [(tool name, description)]) from an exported
/// session HTML (core/export-html/template.js markers).
pub fn parse_export_html(html: &str) -> (Option<String>, Vec<(String, String)>) {
    let mut prompt = None;
    if let Some(pos) = html.find("class=\"system-prompt-full\"") {
        if let Some(gt) = html[pos..].find('>') {
            let from = pos + gt + 1;
            if let Some(close) = html[from..].find("</div>") {
                let raw = &html[from..from + close];
                let text = html_unescape(raw).trim().to_string();
                if !text.is_empty() {
                    prompt = Some(text);
                }
            }
        }
    }
    let mut tools = Vec::new();
    let needle = "<span class=\"tool-item-name\">";
    let mut search_from = 0usize;
    while let Some(rel) = html[search_from..].find(needle) {
        let name_from = search_from + rel + needle.len();
        let Some(name_end) = html[name_from..].find("</span>") else { break };
        let name = html_unescape(&html[name_from..name_from + name_end]);
        let after_name = name_from + name_end + "</span>".len();
        let desc_needle = " - <span class=\"tool-item-desc\">";
        let Some(drel) = html[after_name..].find(desc_needle) else { break };
        let desc_from = after_name + drel + desc_needle.len();
        let Some(desc_end) = html[desc_from..].find("</span>") else { break };
        let desc = html_unescape(&html[desc_from..desc_from + desc_end]);
        tools.push((name, desc));
        search_from = desc_from + desc_end;
    }
    (prompt, tools)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(user: bool, text: &str) -> TitleTurn {
        TitleTurn { user, text: text.to_string() }
    }

    #[test]
    fn transcript_clips_long_turns_and_keeps_order() {
        let long = "x".repeat(2000);
        let messages = vec![turn(true, &long), turn(false, &long)];
        let t = build_title_transcript(&messages);
        assert!(t.starts_with("User: "));
        assert!(t.contains("Assistant: "));
        assert!(t.chars().count() < 2 * 2000);
        // per-turn caps applied
        let user_line = t.lines().next().unwrap();
        assert!(user_line.chars().count() <= TITLE_USER_CHARS + TITLE_ELISION.len() + "User: ".len());
    }

    #[test]
    fn transcript_budget_elides_middle() {
        let mut messages = Vec::new();
        for i in 0..40 {
            messages.push(turn(true, &format!("turn {i}: {}", "y".repeat(300))));
            messages.push(turn(false, &format!("reply {i}: {}", "z".repeat(200))));
        }
        let t = build_title_transcript(&messages);
        // single-line overshoot past the soft budget is fine (the prompt is
        // small either way); it must stay far below the raw transcript
        assert!(t.chars().count() < TITLE_TRANSCRIPT_CHARS + 600, "budget respected: {}", t.chars().count());
        assert!(t.contains(TITLE_ELISION));
        // head keeps the opening goal, tail keeps the newest turns
        assert!(t.contains("turn 0"));
        assert!(t.contains("turn 39"));
    }

    #[test]
    fn sanitize_title_strips_markdown_and_quotes() {
        assert_eq!(sanitize_title("\"Fix login bug\"\n"), "Fix login bug");
        assert_eq!(sanitize_title("`重构主题模块`"), "重构主题模块");
        assert_eq!(sanitize_title("## A Title"), "A Title");
        assert_eq!(sanitize_title("\n\n  \n"), "");
        let long = "w".repeat(200);
        let got = sanitize_title(&long);
        assert!(got.chars().count() <= TITLE_MAX_LEN);
    }
}
