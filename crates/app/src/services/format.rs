//! Pure formatting + file-sampling helpers (pi-web lib/format-level utils).

use std::path::Path;

pub fn status_line(connected: bool, state: &str) -> String {
    if connected {
        format!("pi {} | {state}", pi_link::PI_VENDOR_VERSION)
    } else {
        "pi not available (vendor missing)".to_string()
    }
}



pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Message send/reply time (pi-web formatTime): today -> "HH:MM",
/// otherwise "MM-DD HH:MM".
pub fn fmt_msg_time(ms: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_millis_opt(ms) {
        chrono::LocalResult::Single(t) => {
            let today = chrono::Local::now().date_naive() == t.date_naive();
            if today {
                t.format("%H:%M").to_string()
            } else {
                t.format("%m-%d %H:%M").to_string()
            }
        }
        _ => String::new(),
    }
}

/// Wall-clock turn duration (032 回复用时): "3.2s" / "1m24s" / "12m".
pub fn fmt_duration_ms(ms: i64) -> String {
    let secs = (ms / 1000).max(0);
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m{}", secs / 60, if secs % 60 > 0 { format!("{}s", secs % 60) } else { String::new() })
    }
}

pub fn time_ago(modified: std::time::SystemTime) -> String {
    let secs = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()?;
            Some(now.as_secs() as i64 - d.as_secs() as i64)
        })
        .unwrap_or(0)
        .max(0);
    match secs {
        0..=59 => crate::i18n::tf("{secs}秒前", &[("secs", secs.to_string())]),
        60..=3599 => crate::i18n::tf("{n}分钟前", &[("n", (secs / 60).to_string())]),
        3600..=86399 => crate::i18n::tf("{n}小时前", &[("n", (secs / 3600).to_string())]),
        _ => crate::i18n::tf("{n}天前", &[("n", (secs / 86400).to_string())]),
    }
}

pub fn read_branch(cwd: &Path) -> String {
    let Ok(head) = std::fs::read_to_string(cwd.join(".git").join("HEAD")) else {
        return String::new();
    };
    head.trim()
        .strip_prefix("ref: refs/heads/")
        .unwrap_or(head.trim())
        .to_string()
}


pub fn walk_files(cwd: &Path, depth: usize, cap: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    fn rec(dir: &Path, rel: &str, depth: usize, cap: usize, out: &mut Vec<String>) {
        if depth == 0 || out.len() >= cap {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= cap {
                return;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.')
                || name == "node_modules"
                || name == "target"
                || name == "dist"
            {
                continue;
            }
            let rel_path =
                if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
            let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
            if is_dir {
                rec(&e.path(), &rel_path, depth - 1, cap, out);
            } else {
                out.push(rel_path);
            }
        }
    }
    rec(cwd, "", depth, cap, &mut out);
    out
}

pub fn mime_from_ext(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => "image/png".to_string(),
        Some("jpg") | Some("jpeg") => "image/jpeg".to_string(),
        Some("gif") => "image/gif".to_string(),
        Some("webp") => "image/webp".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

pub fn pretty_args(args: &str) -> String {
    serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
        .unwrap_or_else(|| args.to_string())
}


/// pi-web estimateTokens: CJK chars ~1 token each, others ~4 chars/token.
pub fn estimate_tokens(text: &str) -> u64 {
    let mut cjk: u64 = 0;
    let mut rest: u64 = 0;
    for ch in text.chars() {
        let c = ch as u32;
        let is_cjk = (0x3000..=0x30ff).contains(&c)
            || (0x3400..=0x9fff).contains(&c)
            || (0xf900..=0xfaff).contains(&c)
            || (0x20000..=0x2fa1f).contains(&c)
            || (0xac00..=0xd7af).contains(&c);
        if is_cjk {
            cjk += 1;
        } else {
            rest += 1;
        }
    }
    cjk + rest / 4
}

