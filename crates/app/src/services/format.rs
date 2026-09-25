//! Pure formatting + file-sampling helpers (pi-web lib/format-level utils).

use std::path::Path;

pub fn status_line(connected: bool, state: &str) -> String {
    if connected {
        format!("pi {} | {state}", pi_link::PI_VENDOR_VERSION)
    } else {
        "pi not available (vendor missing)".to_string()
    }
}

/// Usage footer line (pi-web message footer) over raw usage fields.
pub fn usage_footer(input: u64, output: u64, cache_read: u64, cost: f64) -> String {
    let mut s = format!(
        "{} in · {} out",
        fmt_thousands(input),
        fmt_thousands(output)
    );
    if cache_read > 0 {
        s.push_str(&format!(" · {} cache R", fmt_thousands(cache_read)));
    }
    s.push_str(&format!(" · ${:.4}", cost));
    s
}

pub fn fmt_thousands(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::new();
    for (i, ch) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*ch as char);
    }
    out
}

pub fn fmt_hhmm(ms: i64) -> String {
    use chrono::TimeZone;
    match chrono::Local.timestamp_millis_opt(ms) {
        chrono::LocalResult::Single(t) => t.format("%H:%M").to_string(),
        _ => String::new(),
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

pub fn cwd_tail(cwd: &str) -> String {
    cwd.rsplit(['/', '\\']).next().unwrap_or(cwd).to_string()
}

pub fn top_level_entries(cwd: &Path) -> Vec<(bool, String)> {
    let Ok(rd) = std::fs::read_dir(cwd) else { return Vec::new() };
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            dirs.push(name);
        } else {
            files.push(name);
        }
    }
    dirs.sort();
    files.sort();
    dirs.iter()
        .map(|n| (true, n.clone()))
        .chain(files.iter().map(|n| (false, n.clone())))
        .take(12)
        .collect()
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

pub fn fmt_compact(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.0}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
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

/// Speed badge color (pi-web: >=50 cyan, >=30 green, >=15 yellow, else red).
pub fn tps_color(tps: f32) -> u32 {
    if tps >= 50. {
        0x53b3cb
    } else if tps >= 30. {
        0x9bc53d
    } else if tps >= 15. {
        0xf9c22e
    } else {
        0xe01a4f
    }
}
