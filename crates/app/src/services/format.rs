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

/// Message send/reply time（主界面UI设计-2.html 操作栏 .when）：
/// "8月25日 06:02"；跨年补年份 "2024年12月3日 09:15"。
pub fn fmt_msg_time(ms: i64) -> String {
    use chrono::{Datelike, TimeZone};
    match chrono::Local.timestamp_millis_opt(ms) {
        chrono::LocalResult::Single(t) => {
            if t.year() == chrono::Local::now().year() {
                t.format("%-m月%-d日 %H:%M").to_string()
            } else {
                t.format("%Y年%-m月%-d日 %H:%M").to_string()
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
        format!(
            "{}m{}",
            secs / 60,
            if secs % 60 > 0 {
                format!("{}s", secs % 60)
            } else {
                String::new()
            }
        )
    }
}

/// 详情卡相对时间（带「前」后缀，走 i18n）。
///
/// **不是 [`fmt_ago`] 的重复**：那个是会话行内紧凑形态（无「前」、刚过阈值
/// 5 分钟、设计稿 fmtAgo parity 故意不走 i18n）。两处可见文本不同，合并
/// 必然改变其中一处的 UI——勿合（2026-10-31 重构盘查结论）。
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
    // 不显示秒（2026-11 用户定夺：详情卡/搜索弹窗时间不刷新不数秒），
    // <1 分钟并入「刚刚」，最小粒度到分钟——秒级跳动从源头消失
    let mins = secs / 60;
    match mins {
        0 => "刚刚".to_string(),
        1..=59 => crate::i18n::tf("time.m_ago", &[("n", mins.to_string())]),
        60..=1439 => crate::i18n::tf("time.h_ago", &[("n", (mins / 60).to_string())]),
        _ => crate::i18n::tf("time.d_ago", &[("n", (mins / 1440).to_string())]),
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
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            if out.len() >= cap {
                return;
            }
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "dist"
            {
                continue;
            }
            let rel_path = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
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

/// pi-web estimateTokens（CJK ≈1 token/字，其余 4 字符/token）——实现在
/// `pi_link::estimate`（独立工具脚本 token-count 同源），这里转出口维持
/// 既有调用面（session/mod.rs 的 glob use）。
pub use pi_link::estimate::estimate_tokens;

/// 千分位（pi-web usage 行的 toLocaleString parity）：785536 → "785,536"。
pub fn fmt_thousand(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// 会话行相对时间（设计稿 fmtAgo parity）：距今
/// <5 分钟=「刚刚」；<60 分钟=N分钟；<24 小时=N小时；否则=N天。
///
/// 与 [`time_ago`] 是**刻意变体**（行内紧凑无「前」/阈值 5 分钟/不走 i18n），
/// 勿合并——见彼处注释。
pub fn fmt_ago(t: std::time::SystemTime) -> String {
    let mins = t.elapsed().map(|d| d.as_secs() / 60).unwrap_or(0) as i64;
    if mins < 5 {
        "刚刚".to_string()
    } else if mins < 60 {
        format!("{mins}分钟")
    } else if mins < 60 * 24 {
        format!("{}小时", mins / 60)
    } else {
        format!("{}天", mins / (60 * 24))
    }
}
