//! 会话内容搜索（013；pi-web lib/session-search.ts parity）。全量口径：
//! 所有项目 group 一起搜，mtime 倒序，预算/上限多级截断。

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::info::{group_name_for_cwd, sessions_root};

// ---------------------------------------------------------------------------
// session content search (013; pi-web lib/session-search.ts parity)
// ---------------------------------------------------------------------------

/// Total hit cap across the scan.
pub const SEARCH_MAX_RESULTS: usize = 30;
const SEARCH_MAX_FILES: usize = 500;
const SEARCH_MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
const SEARCH_MAX_LINE_BYTES: usize = 1024 * 1024;
const SEARCH_BUDGET_MS: u64 = 3000;
const SEARCH_CONTEXT_CHARS: usize = 80;
const SEARCH_MAX_PER_SESSION: usize = 5;

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub session_path: PathBuf,
    /// 该会话所属项目的 cwd（会话文件首行 `type=="session"` 的 `cwd`；
    /// 老文件缺失时回落 group 目录名解码）。跨项目全量搜索时上屏用。
    pub project: Option<String>,
    pub session_name: Option<String>,
    pub preview: String,
    pub modified: SystemTime,
    /// matched message's payload timestamp (epoch ms) — row locator
    pub ts: Option<i64>,
    pub role: String,
    pub before: String,
    pub match_text: String,
    pub after: String,
}

#[derive(Debug, Clone, Default)]
pub struct SearchResponse {
    pub hits: Vec<SearchHit>,
    /// budget/result/file cap reached — the UI hints at partial results
    pub truncated: bool,
}

/// Literal case-insensitive content search across one project's session
/// files, newest first. Hits are capped per session so the grouped view
/// stays readable; user/assistant text blocks only (pi-web scope).
///
/// **全量**：所有项目 group 目录一起搜（2026-10-21 用户定案——会话查询不能
/// 只看当前项目），结果按会话文件 mtime 倒序排（跨项目混排、最近会话靠前），
/// 同一会话的命中仍然连在一起。
pub fn search_sessions_all(query: &str) -> SearchResponse {
    match sessions_root() {
        Some(root) => search_all_in_root(&root, query),
        None => SearchResponse {
            hits: Vec::new(),
            truncated: true,
        },
    }
}

/// 全量搜索的可测入口（root 由调用方给，测试不碰真实 sessions 目录）。
pub fn search_all_in_root(root: &Path, query: &str) -> SearchResponse {
    let Ok(rd) = std::fs::read_dir(root) else {
        return SearchResponse::default();
    };
    let groups: Vec<PathBuf> = rd
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.path())
        .collect();
    search_in_groups(&groups, query)
}

/// 单项目（按 cwd 找 group 目录）——pi-web 原范围，保留给调用方/测试。
pub fn search_sessions_for_cwd(cwd: &str, query: &str) -> SearchResponse {
    match sessions_root() {
        Some(root) => search_sessions_in_dir(&root.join(group_name_for_cwd(cwd)), query),
        None => SearchResponse {
            hits: Vec::new(),
            truncated: true,
        },
    }
}

/// The scan itself, over one project group directory (testable without
/// touching the real sessions root).
pub fn search_sessions_in_dir(group: &Path, query: &str) -> SearchResponse {
    search_in_groups(std::slice::from_ref(&group.to_path_buf()), query)
}

/// 多 group 扫描：收集所有会话文件 → 全局按 mtime 倒序 → 逐文件扫。
fn search_in_groups(groups: &[PathBuf], query: &str) -> SearchResponse {
    let mut resp = SearchResponse::default();
    let needle = query.trim();
    if needle.is_empty() {
        return resp;
    }
    let mut files: Vec<(SystemTime, PathBuf)> = Vec::new();
    for group in groups {
        let Ok(rd) = std::fs::read_dir(group) else {
            continue;
        };
        for f in rd.flatten() {
            let path = f.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = f.metadata() else { continue };
            files.push((meta.modified().unwrap_or(SystemTime::UNIX_EPOCH), path));
        }
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(SEARCH_BUDGET_MS);
    let needle_lc: Vec<char> = needle.to_lowercase().chars().collect();
    let mut scanned = 0usize;
    'files: for (mtime, path) in files {
        if resp.hits.len() >= SEARCH_MAX_RESULTS
            || scanned >= SEARCH_MAX_FILES
            || std::time::Instant::now() >= deadline
        {
            resp.truncated = true;
            break;
        }
        scanned += 1;
        let Ok(bytes) = std::fs::read(&path) else {
            resp.truncated = true;
            continue;
        };
        if bytes.len() as u64 > SEARCH_MAX_FILE_BYTES {
            resp.truncated = true;
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let mut session_name: Option<String> = None;
        // 项目 cwd：会话文件首行 `{"type":"session","cwd":...}`；老文件/截断
        // 时回落 group 目录名（`--D--ai_workspace-pi-flash--` → 去掉 `--` 外壳）
        let mut project: Option<String> = path
            .parent()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().trim_matches('-').to_string())
            .filter(|s| !s.is_empty());
        let mut preview = String::new();
        // (ts, role, before, match, after) gathered per file, assembled below
        let mut local: Vec<(Option<i64>, String, String, String, String)> = Vec::new();
        for line in text.lines() {
            if std::time::Instant::now() >= deadline {
                resp.truncated = true;
                break 'files;
            }
            if line.len() > SEARCH_MAX_LINE_BYTES {
                resp.truncated = true;
                continue;
            }
            let Ok(entry) = crate::json::parse_value(line) else {
                continue;
            };
            // pi 落盘的 session_info：name 在顶层（不是 content.name——
            // 旧写法永远取不到，搜索结果标题从来只显示预览）
            if entry["type"] == "session" {
                if let Some(cwd) = entry["cwd"].as_str() {
                    project = Some(cwd.to_string());
                }
                continue;
            }
            if entry["type"] == "session_info" {
                session_name = entry["name"].as_str().map(str::to_string).or(session_name);
                continue;
            }
            if entry["type"] != "message" {
                continue;
            }
            let role = entry["message"]["role"].as_str().unwrap_or("");
            if role != "user" && role != "assistant" {
                continue;
            }
            let blocks: Vec<String> = entry["message"]["content"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter(|b| b["type"] == "text")
                        .filter_map(|b| b["text"].as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            if blocks.is_empty() {
                continue;
            }
            if preview.is_empty() && role == "user" {
                let t: String = blocks
                    .first()
                    .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
                    .unwrap_or_default();
                preview = t.chars().take(120).collect();
            }
            let joined = blocks.join("\n");
            let orig: Vec<char> = joined.chars().collect();
            let lower: Vec<char> = orig
                .iter()
                .map(|c| c.to_lowercase().next().unwrap_or(*c))
                .collect();
            let Some(pos) = find_sub(&lower, &needle_lc) else {
                continue;
            };
            let ts = entry["message"]["timestamp"]
                .as_i64()
                .or_else(|| entry["timestamp"].as_str().and_then(parse_iso_ts));
            let squash = |range: std::ops::Range<usize>| -> String {
                orig[range]
                    .iter()
                    .collect::<String>()
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let start = pos.saturating_sub(SEARCH_CONTEXT_CHARS);
            let end = (pos + needle_lc.len() + SEARCH_CONTEXT_CHARS).min(orig.len());
            let mut before = squash(start..pos);
            if pos > SEARCH_CONTEXT_CHARS {
                before = format!("...{before}");
            }
            let match_text = orig[pos..pos + needle_lc.len()].iter().collect::<String>();
            let mut after = squash(end..orig.len().min(end + SEARCH_CONTEXT_CHARS));
            if end + SEARCH_CONTEXT_CHARS < orig.len() {
                after.push_str("...");
            }
            local.push((ts, role.to_string(), before, match_text, after));
            if local.len() >= SEARCH_MAX_PER_SESSION {
                break;
            }
        }
        for (ts, role, before, match_text, after) in local {
            if resp.hits.len() >= SEARCH_MAX_RESULTS {
                resp.truncated = true;
                break 'files;
            }
            resp.hits.push(SearchHit {
                session_path: path.clone(),
                project: project.clone(),
                session_name: session_name.clone(),
                preview: preview.clone(),
                modified: mtime,
                ts,
                role,
                before,
                match_text,
                after,
            });
        }
    }
    resp
}

fn find_sub(hay: &[char], needle: &[char]) -> Option<usize> {
    if needle.is_empty() || hay.len() < needle.len() {
        return None;
    }
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Fixed-format RFC3339 UTC -> epoch ms ("2026-09-25T08:09:47.902Z", the pi
/// writer's shape). Hand-parsed: pi-link stays date-crate-free.
fn parse_iso_ts(v: &str) -> Option<i64> {
    let b = v.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || (b[10] != b'T' && b[10] != b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> { v.get(r)?.parse().ok() };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, se) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let ms = if b.len() >= 23 && b[19] == b'.' {
        num(20..23)?
    } else {
        0
    };
    // days since 1970-01-01 (Howard Hinnant's days_from_civil)
    let y = if mo <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    Some(((days * 24 + h) * 60 + mi) * 60 * 1000 + se * 1000 + ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::testing::{temp_root, write_session};

    #[test]
    fn content_search_finds_matches_with_context() {
        let root = temp_root("search");
        let body = concat!(
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"timestamp\":1790454123456,\"content\":[{\"type\":\"text\",\"text\":\"help me refactor the parser code\"}]}}\n",
            "{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"timestamp\":1790454125000,\"content\":[{\"type\":\"text\",\"text\":\"sure, the PARSER module is ready\"}]}}\n",
            "{\"type\":\"message\",\"message\":{\"role\":\"toolResult\",\"content\":\"ignored\"}}\n"
        );
        write_session(&root, "D:\\proj-s", "s1", body);
        let group = root.join(group_name_for_cwd("D:\\proj-s"));
        let resp = search_sessions_in_dir(&group, "PARSER");
        assert!(!resp.truncated);
        assert_eq!(resp.hits.len(), 2, "user + assistant both match");
        assert_eq!(resp.hits[0].ts, Some(1790454123456));
        assert_eq!(resp.hits[0].match_text, "parser"); // lowercased needle len
        assert!(resp.hits[0].before.starts_with("help me"));
        assert!(resp.hits[1].match_text.eq_ignore_ascii_case("parser"));
        // empty query: no hits, no truncation
        let empty = search_sessions_in_dir(&group, "   ");
        assert!(empty.hits.is_empty() && !empty.truncated);
        // no match: clean empty response
        let miss = search_sessions_in_dir(&group, "quantum");
        assert!(miss.hits.is_empty() && !miss.truncated);
    }

    #[test]
    fn all_projects_search_spans_groups() {
        // 2026-10-21 用户定案：会话查询搜**全部**会话，不只看当前项目
        let root = temp_root("all");
        let body = concat!(
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"timestamp\":1790454123456,",
            "\"content\":[{\"type\":\"text\",\"text\":\"跨项目 hello 世界\"}]}}\n"
        );
        let a = write_session(&root, r"D:\proj-a", "a1", body);
        let b = write_session(&root, r"D:\proj-b", "b1", body);
        // 让 proj-b 的会话更新一些，验证跨项目按 mtime 倒序
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&a)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let resp = search_all_in_root(&root, "世界");
        assert!(!resp.truncated, "两个 group 都在预算内");
        assert_eq!(resp.hits.len(), 2, "两个项目的会话都要命中");
        assert_eq!(resp.hits[0].session_path, b, "更新的会话（proj-b）排在前");
        assert_eq!(resp.hits[0].project.as_deref(), Some(r"D:\proj-b"));
        assert_eq!(resp.hits[1].session_path, a);
        assert_eq!(resp.hits[1].project.as_deref(), Some(r"D:\proj-a"));
        // 单项目入口仍然只搜自己的 group
        let only_a = search_sessions_in_dir(&root.join(group_name_for_cwd(r"D:\proj-a")), "世界");
        assert_eq!(only_a.hits.len(), 1);
    }
}
