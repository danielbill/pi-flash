//! 回复排版（P2 切片 3）—— ZCode `replyFormatter.ts` 的移植。
//!
//! 纯函数、无 IO：输入是已解析的工具调用 / 变更摘要，输出是发给微信的文本块。
//!
//! **暂缺**：`formatBotPermissionRequestSummary` / `formatBotPermissionRequestTitle`
//! 依赖 `getPermissionRequestPreview`（`shared/permission-request-preview.ts`，
//! 366 行，PC 端 `PermissionDialog` 同源），留到切片 4 与其余文案一起搬。

use serde_json::{json, Value};

use super::messages::{t, tf, Lang};
use super::summary::{
    get_compact_tool_call_summary, normalize_display_text, ToolCallSummary,
    ToolCallSummarySource,
};
use super::permission::{
    get_permission_request_preview, ChangeKind, PermissionRequest, PermissionRequestPreview, Scope,
};

/// ZCode `replyFormatter.ts:48-52` 的四个上限。
pub const MAX_TOOL_SUMMARY_ITEMS: usize = 10;
pub const MAX_FIELD_LENGTH: usize = 160;
pub const MAX_COMMAND_FIELD_LENGTH: usize = 96;
pub const MAX_REPLY_MESSAGE_LENGTH: usize = 3500;

const DIFF_ADDED_MARKER: &str = "🟢";
const DIFF_REMOVED_MARKER: &str = "🔴";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Denied,
    Stopped,
}

#[derive(Debug, Clone)]
pub struct BotReplyToolCallState {
    pub tool_id: String,
    pub title: Option<String>,
    pub kind: Option<String>,
    pub input: Value,
    pub output: Option<Value>,
    pub status: Option<ToolStatus>,
    pub error: Option<String>,
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiffStat {
    pub added: u32,
    pub removed: u32,
}

#[derive(Debug, Clone)]
pub struct FileChange {
    pub path: String,
    pub added: u32,
    pub removed: u32,
}

#[derive(Debug, Clone, Default)]
pub struct ChangeSummary {
    pub file_count: u32,
    pub added: u32,
    pub removed: u32,
    pub files: Vec<FileChange>,
}

#[derive(Debug, Clone)]
pub enum BotAssistantReplyBlock {
    Content { content: String },
    ToolCall { tool_call: BotReplyToolCallState },
    ChangeSummary { change_summary: ChangeSummary },
}

// ── 文本原语 ──────────────────────────────────────────────────────

/// ZCode `truncateText`（默认 160 字）：超长取前 `max-3` 字 + `...`。
///
/// ⚠️ 差异说明：JS 用 UTF-16 code unit 计数与切片，这里用 `char`。
/// 对 BMP 字符（中文、emoji 之外的常见文本）两者等价；**含代理对的 emoji
/// 会与 JS 差一截**——但 JS 会在代理对中间切断产生乱码，按 char 切更安全。
pub fn truncate_text(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let head: String = value.chars().take(max - 3).collect();
    format!("{head}...")
}

/// ZCode `truncateMiddleText`（默认 96 字）：头 60 + `" ... "` + 尾 31。
///
/// `head = ceil((96-5) * 0.65) = ceil(59.15) = 60`，`tail = 96 - 5 - 60 = 31`。
pub fn truncate_middle_text(value: &str, max: usize) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() <= max {
        return value.to_string();
    }
    let head_len = (((max - 5) as f64) * 0.65).ceil() as usize;
    let tail_len = max.saturating_sub(5).saturating_sub(head_len);
    let head: String = chars.iter().take(head_len).collect();
    let tail: String = chars.iter().skip(chars.len() - tail_len).collect();
    format!("{head} ... {tail}")
}

/// ZCode `formatMarkdownInlineCode`：转义反斜杠与反引号（Telegram 退回裸显示的坑）。
pub fn format_markdown_inline_code(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('`', "\\`");
    format!("`{escaped}`")
}

fn normalize_path_separators(value: &str) -> String {
    value.replace('\\', "/")
}

fn strip_trailing_slash(value: &str) -> String {
    if value.chars().count() > 1 {
        value.trim_end_matches('/').to_string()
    } else {
        value.to_string()
    }
}

/// ZCode `toWorkspaceRelativePath`：Windows 分隔符归一 + 大小写不敏感前缀剥离。
pub fn to_workspace_relative_path(path_value: &str, workspace_path: Option<&str>) -> String {
    let normalized_path = strip_trailing_slash(&normalize_path_separators(path_value.trim()));
    let Some(ws) = workspace_path.map(str::trim).filter(|s| !s.is_empty()) else {
        return normalized_path;
    };
    let normalized_workspace = strip_trailing_slash(&normalize_path_separators(ws));
    let comparable_path = normalized_path.to_lowercase();
    let comparable_workspace = normalized_workspace.to_lowercase();
    if comparable_path == comparable_workspace {
        return ".".to_string();
    }
    if comparable_path.starts_with(&format!("{comparable_workspace}/")) {
        // ZCode 用 `normalizedWorkspace.length + 1` 做 slice 下标（UTF-16 长度），
        // 这里按 char 数等价切。
        let take = normalized_workspace.chars().count() + 1;
        return normalized_path.chars().skip(take).collect();
    }
    normalized_path
}

/// ZCode `formatBotDiffCount`：`🟢`+增量 / `🔴`-减量，只输出非零项。
pub fn format_bot_diff_count(stat: &DiffStat) -> String {
    let mut parts: Vec<String> = Vec::new();
    if stat.added > 0 {
        parts.push(format!(
            "{DIFF_ADDED_MARKER} {}",
            format_markdown_inline_code(&format!("+{}", stat.added))
        ));
    }
    if stat.removed > 0 {
        parts.push(format!(
            "{DIFF_REMOVED_MARKER} {}",
            format_markdown_inline_code(&format!("-{}", stat.removed))
        ));
    }
    parts.join(" ")
}

// ── 摘要行 ────────────────────────────────────────────────────────

/// ZCode `normalizeToolCallSummaryInput`：只有 `kind === "edit"` 才把路径相对化。
fn normalize_tool_call_summary_input(tool_call: &BotReplyToolCallState, workspace: Option<&str>) -> Value {
    let Some(obj) = tool_call.input.as_object() else {
        return tool_call.input.clone();
    };
    if tool_call.kind.as_deref() != Some("edit") {
        return tool_call.input.clone();
    }
    let mut next = obj.clone();
    for key in ["path", "file_path", "filePath"] {
        if let Some(value) = next.get(key).and_then(Value::as_str) {
            next.insert(key.to_string(), json!(to_workspace_relative_path(value, workspace)));
        }
    }
    Value::Object(next)
}

/// ZCode `formatCompactSummaryDetail`：`内联代码 · 增删统计`，两段用 ` · ` 连接。
fn format_compact_summary_detail(summary: &ToolCallSummary) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    if let Some(secondary) = &summary.secondary_text {
        parts.push(format_markdown_inline_code(&truncate_middle_text(
            &normalize_display_text(secondary),
            MAX_COMMAND_FIELD_LENGTH,
        )));
    }
    if let Some(stat) = summary.change_stat {
        let text = format_bot_diff_count(&DiffStat {
            added: stat.added,
            removed: stat.removed,
        });
        if !text.is_empty() {
            parts.push(text);
        }
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// ZCode `formatToolStatus`。
///
/// ⚠️ `stopped` **不被处理**，落到默认的「等待中 / Pending」——这是 ZCode 的原样行为，
/// 对拍保持一致（`isBotToolCallReplyTerminal` 却把 stopped 当终态，两处不对称是原设计）。
pub fn format_tool_status(status: Option<ToolStatus>, error: Option<&str>, lang: Lang) -> String {
    match status {
        Some(ToolStatus::Completed) => t(lang, "完成"),
        Some(ToolStatus::Failed) => {
            let mut text = t(lang, "失败");
            if let Some(error) = error {
                text.push_str(&format!(
                    ": {}",
                    truncate_text(&normalize_display_text(error), MAX_FIELD_LENGTH)
                ));
            }
            text
        }
        Some(ToolStatus::Denied) => t(lang, "已拒绝"),
        Some(ToolStatus::InProgress) => t(lang, "⏳ 运行中"),
        _ => t(lang, "等待中"),
    }
}

/// ZCode `formatBotToolCallSummaryLine`（`replyFormatter.ts:240`）。
///
/// 形如：`- 完成 · Bash · \`npm test\` · 🟢 \`+3\``
pub fn format_bot_tool_call_summary_line(
    tool_call: &BotReplyToolCallState,
    lang: Lang,
    workspace: Option<&str>,
) -> String {
    let input = normalize_tool_call_summary_input(tool_call, workspace);
    let summary = get_compact_tool_call_summary(ToolCallSummarySource {
        title: tool_call.title.as_deref(),
        kind: tool_call.kind.as_deref().unwrap_or("tool"),
        input: &input,
        output: tool_call.output.as_ref(),
        raw: tool_call.raw.as_ref(),
    });
    let status = format_tool_status(tool_call.status, tool_call.error.as_deref(), lang);
    let mut line = format!("- {status} · {}", summary.primary_text);
    if let Some(detail) = format_compact_summary_detail(&summary) {
        line.push_str(&format!(" · {detail}"));
    }
    line
}

/// ZCode `formatBotToolCallReply`：`工具调用：` 头 + 一行摘要。
pub fn format_bot_tool_call_reply(
    tool_call: &BotReplyToolCallState,
    lang: Lang,
    workspace: Option<&str>,
) -> String {
    format!(
        "{}\n{}",
        t(lang, "工具调用："),
        format_bot_tool_call_summary_line(tool_call, lang, workspace)
    )
}

/// ZCode `isBotToolCallReplyTerminal`：完成/失败/拒绝/停止 —— **不含 running**。
pub fn is_bot_tool_call_reply_terminal(status: Option<ToolStatus>) -> bool {
    matches!(
        status,
        Some(ToolStatus::Completed)
            | Some(ToolStatus::Failed)
            | Some(ToolStatus::Denied)
            | Some(ToolStatus::Stopped)
    )
}

// ── 变更摘要 ──────────────────────────────────────────────────────

/// ZCode `formatBotChangeSummary`（`replyFormatter.ts:341`）。
///
/// ⚠️ zh / en 的头行是**两套硬编码模板**，不是同一句 + 译文：
/// zh `变更摘要：{n} 个文件，{diff}`，en `Change summary: {n} files, {diff}`。
pub fn format_bot_change_summary(change_summary: &ChangeSummary, lang: Lang) -> String {
    if change_summary.file_count == 0 || change_summary.files.is_empty() {
        return String::new();
    }
    let diff = format_bot_diff_count(&DiffStat {
        added: change_summary.added,
        removed: change_summary.removed,
    });
    let header = if lang == Lang::En {
        format!(
            "{}: {} files, {diff}",
            t(lang, "变更摘要"),
            change_summary.file_count
        )
    } else {
        format!(
            "{}：{} 个文件，{diff}",
            t(lang, "变更摘要"),
            change_summary.file_count
        )
    };

    let mut lines = vec![header];
    for file in change_summary.files.iter().take(MAX_TOOL_SUMMARY_ITEMS) {
        lines.push(format!(
            "- {} ({})",
            format_markdown_inline_code(&file.path),
            format_bot_diff_count(&DiffStat {
                added: file.added,
                removed: file.removed
            })
        ));
    }
    if change_summary.files.len() > MAX_TOOL_SUMMARY_ITEMS {
        // 在 if 内求值：外层先算会在 files.len() < 10 时 usize underflow
        let overflow = change_summary.files.len() - MAX_TOOL_SUMMARY_ITEMS;
        lines.push(format!(
            "- {}",
            tf(lang, "还有 {count} 个文件", &[("count", &overflow.to_string())])
        ));
    }
    lines.join("\n")
}

// ── 分块 ──────────────────────────────────────────────────────────

/// ZCode `splitLongReplyText`（`replyFormatter.ts:266`）。
///
/// 断点取 `max(最后一个换行, 最后一个空格)`，**两者都没有才硬切 3500**；
/// 切出的块与剩余部分都会 `trim`。断点优先级是 `Math.max` —— 意味着
/// 只要窗口内有空格就优先在空格处断，哪怕换行更靠前。
pub fn split_long_reply_text(text: &str) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut remaining = text.trim().to_string();
    let mut chars: Vec<char> = remaining.chars().collect();

    while chars.len() > MAX_REPLY_MESSAGE_LENGTH {
        // ZCode 的 fromIndex 是 3500（含），这里取 0..=3500 这段找断点。
        let window: Vec<char> = chars.iter().take(MAX_REPLY_MESSAGE_LENGTH + 1).copied().collect();
        let newline = window.iter().rposition(|c| *c == '\n');
        let space = window.iter().rposition(|c| *c == ' ');
        let breakpoint: isize = match (newline, space) {
            (Some(a), Some(b)) => a.max(b) as isize,
            (Some(a), None) => a as isize,
            (None, Some(b)) => b as isize,
            (None, None) => -1,
        };
        let end = if breakpoint > 0 {
            breakpoint as usize
        } else {
            MAX_REPLY_MESSAGE_LENGTH
        };
        let head: String = chars[..end].iter().collect();
        chunks.push(head.trim().to_string());
        chars = chars[end..].iter().copied().collect();
        remaining = chars.iter().collect::<String>().trim().to_string();
        chars = remaining.chars().collect();
    }

    if !remaining.is_empty() {
        chunks.push(remaining);
    }
    chunks
}

/// ZCode `extractBotAssistantResponseMessages`：非终态不 flush。
pub fn extract_bot_assistant_response_messages(buffer: &str, force: bool) -> (Vec<String>, String) {
    let normalized = buffer.replace("\r\n", "\n");
    if !force {
        return (Vec::new(), normalized);
    }
    (split_long_reply_text(&normalized), String::new())
}

/// ZCode `formatBotAssistantReplyBlocks`：按块类型分别分块后顺序拼接。
pub fn format_bot_assistant_reply_blocks(
    blocks: &[BotAssistantReplyBlock],
    lang: Lang,
    workspace: Option<&str>,
) -> Vec<String> {
    let mut messages: Vec<String> = Vec::new();
    for block in blocks {
        match block {
            BotAssistantReplyBlock::Content { content } => {
                messages.extend(split_long_reply_text(&content.replace("\r\n", "\n")));
            }
            BotAssistantReplyBlock::ToolCall { tool_call } => {
                let text = format_bot_tool_call_reply(tool_call, lang, workspace);
                messages.extend(split_long_reply_text(&text));
            }
            BotAssistantReplyBlock::ChangeSummary { change_summary } => {
                let text = format_bot_change_summary(change_summary, lang);
                if !text.is_empty() {
                    messages.extend(split_long_reply_text(&text));
                }
            }
        }
    }
    messages
}

// ── 权限请求排版 ────────────────────────────────────────────────

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// JS `\b(word)\b` 的等价判断：词的两侧必须落在 `\w`（ASCII 字母数字下划线）之外。
///
/// 直接在**原始字节**上做大小写不敏感比较，不做 `to_lowercase()` ——
/// 后者会改变非 ASCII 字符的字节长度（如 `İ` → 2 字符），把下标搞乱。
fn contains_word_case_insensitive(haystack: &str, word: &str) -> bool {
    let hb = haystack.as_bytes();
    let nb = word.as_bytes();
    if nb.is_empty() || hb.len() < nb.len() {
        return false;
    }
    let mut start = 0;
    while start + nb.len() <= hb.len() {
        if hb[start..start + nb.len()].eq_ignore_ascii_case(nb) {
            let before_ok = start == 0 || !is_word_byte(hb[start - 1]);
            let after = start + nb.len();
            let after_ok = after >= hb.len() || !is_word_byte(hb[after]);
            if before_ok && after_ok {
                return true;
            }
        }
        start += 1;
    }
    false
}

pub(crate) fn contains_any_word(haystack: &str, words: &[&str]) -> bool {
    words.iter().any(|word| contains_case_insensitive_any(haystack, word))
}

fn contains_case_insensitive_any(haystack: &str, word: &str) -> bool {
    contains_word_case_insensitive(haystack, word)
}

/// ZCode `preview.title.replace(/^edit\b[:：]?\s*/iu, "").trim()`。
fn strip_leading_edit(title: &str) -> String {
    if title.len() >= 4 && title.as_bytes()[..4].eq_ignore_ascii_case(b"edit") {
        let after = title.as_bytes().get(4).copied();
        if !after.map(is_word_byte).unwrap_or(false) {
            let mut idx = 4;
            if title[idx..].starts_with(':') {
                idx += 1;
            } else if title[idx..].starts_with('：') {
                idx += '：'.len_utf8();
            }
            return title[idx..].trim().to_string();
        }
    }
    title.to_string()
}

/// ZCode `formatEditPermissionKindLabel`（`replyFormatter.ts:213`）。
///
/// 三个词表依次判，命中即返回；`fileChange.type === "add"` 可直接判「写入」。
pub fn format_edit_permission_kind_label(
    request: &PermissionRequest,
    preview: &PermissionRequestPreview,
    lang: Lang,
) -> String {
    let raw_kind = request
        .raw
        .as_object()
        .and_then(|m| m.get("kind"))
        .and_then(Value::as_str);
    let raw_title = request
        .raw
        .as_object()
        .and_then(|m| m.get("title"))
        .and_then(Value::as_str);
    let raw_text = [
        request.title.as_deref(),
        Some(request.description.as_str()),
        raw_kind,
        raw_title,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    let normalized = raw_text.trim().to_lowercase();
    let file_change_type = preview.file_change.as_ref().map(|c| c.kind);

    if contains_any_word(
        &normalized,
        &["delete", "deleted", "remove", "removed", "erase", "erased", "unlink", "rm"],
    ) {
        return t(lang, "删除中");
    }
    if file_change_type == Some(ChangeKind::Add)
        || contains_any_word(
            &normalized,
            &["write", "wrote", "create", "created", "add", "added", "save", "saved", "new"],
        )
    {
        return t(lang, "写入中");
    }
    if contains_any_word(&normalized, &["update", "updating", "updated"]) {
        return t(lang, "更新中");
    }
    t(lang, "编辑中")
}

/// ZCode `formatPermissionRequestTitle`（`replyFormatter.ts:194`）。
fn format_permission_request_title(
    request: &PermissionRequest,
    preview: &PermissionRequestPreview,
    lang: Lang,
    workspace: Option<&str>,
) -> String {
    if request.kind != "edit" || (preview.scope != Scope::File && preview.file_changes.is_empty())
    {
        return preview.title.clone();
    }
    let label = format_edit_permission_kind_label(request, preview, lang);
    let title_without_edit = strip_leading_edit(&preview.title);
    let target_text = if !title_without_edit.is_empty() && title_without_edit != preview.title {
        title_without_edit
    } else if preview.file_paths.len() == 1 || preview.file_changes.len() == 1 {
        let first = preview
            .file_paths
            .first()
            .map(String::as_str)
            .or_else(|| preview.file_changes.first().map(|c| c.path.as_str()))
            .unwrap_or_default();
        to_workspace_relative_path(first, workspace)
    } else {
        String::new()
    };
    if target_text.is_empty() {
        label
    } else {
        format!("{label} {target_text}")
    }
}

/// ZCode `formatPermissionRequestHeader`（`replyFormatter.ts:186`）。
pub fn format_permission_request_header(
    request: &PermissionRequest,
    lang: Lang,
    workspace: Option<&str>,
) -> String {
    let preview = get_permission_request_preview(request);
    format!(
        "{}\n{}",
        t(lang, "需要权限："),
        format_permission_request_title(request, &preview, lang, workspace)
    )
}

/// ZCode `formatBotPermissionRequestSummary`（`replyFormatter.ts:312`）。
///
/// 命令 → 行内代码（中截断 96）；否则最多 3 个路径（相对化 + 行内代码 + 整体截断 160）。
pub fn format_bot_permission_request_summary(
    request: &PermissionRequest,
    lang: Lang,
    workspace: Option<&str>,
) -> String {
    let preview = get_permission_request_preview(request);
    // ZCode 在这里经 `formatPermissionRequestHeader` **再解析一次** preview ——
    // 两处各自解析、结果一致（解析无副作用），保持同构而不是复用上面那次。
    let header = format_permission_request_header(request, lang, workspace);
    if let Some(command) = &preview.command {
        return format!(
            "{header}\n{}",
            format_markdown_inline_code(&truncate_middle_text(
                &normalize_display_text(command),
                MAX_COMMAND_FIELD_LENGTH
            ))
        );
    }
    let list = if preview.file_paths.is_empty() {
        preview
            .file_changes
            .iter()
            .map(|c| c.path.clone())
            .collect::<Vec<_>>()
    } else {
        preview.file_paths.clone()
    };
    if !list.is_empty() {
        let paths = list
            .iter()
            .take(3)
            .map(|path| format_markdown_inline_code(&to_workspace_relative_path(path, workspace)))
            .collect::<Vec<_>>()
            .join(", ");
        return format!("{header}\n{}", truncate_text(&paths, MAX_FIELD_LENGTH));
    }
    header
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_call(kind: &str, input: Value) -> BotReplyToolCallState {
        BotReplyToolCallState {
            tool_id: "t1".into(),
            title: Some("Bash".into()),
            kind: Some(kind.into()),
            input,
            output: None,
            status: None,
            error: None,
            raw: None,
        }
    }

    #[test]
    fn truncate_text_splits_at_max_minus_three() {
        assert_eq!(truncate_text("abc", 160), "abc");
        let long = "x".repeat(160);
        assert_eq!(truncate_text(&long, 160), long, "恰好等于上限时原样返回（ZCode 是 > 才切）");
        assert_eq!(truncate_text(&"x".repeat(161), 160), "x".repeat(157) + "...");
        assert_eq!(truncate_text(&"x".repeat(161), 160).chars().count(), 160);
    }

    #[test]
    fn truncate_middle_uses_head60_tail31() {
        // max=96 → head = ceil(91*0.65) = 60，tail = 96-5-60 = 31
        let value: String = (0..120).map(|i| char::from(b'a' + (i % 26))).collect();
        let out = truncate_middle_text(&value, 96);
        assert_eq!(out.chars().count(), 60 + 5 + 31, "60 + ' ... ' + 31");
        assert!(value.starts_with(&out.chars().take(60).collect::<String>()));
        assert!(value.ends_with(&out.chars().skip(65).collect::<String>()));
        assert_eq!(truncate_middle_text("short", 96), "short", "不超长原样返回");
    }

    #[test]
    fn markdown_inline_code_escapes_backslash_and_backtick() {
        assert_eq!(format_markdown_inline_code("npm test"), "`npm test`");
        assert_eq!(format_markdown_inline_code("a`b"), "`a\\`b`");
        assert_eq!(format_markdown_inline_code("a\\b"), "`a\\\\b`");
    }

    #[test]
    fn workspace_relative_path_variants() {
        let ws = Some("C:\\proj\\pi-flash");
        assert_eq!(
            to_workspace_relative_path("C:\\proj\\pi-flash\\src\\a.rs", ws),
            "src/a.rs",
            "Windows 分隔符归一 + 前缀剥离"
        );
        assert_eq!(
            to_workspace_relative_path("C:/PROJ/pi-flash/", ws),
            ".",
            "大小写不敏感且等价于根"
        );
        assert_eq!(
            to_workspace_relative_path("D:\\other\\b.rs", ws),
            "D:/other/b.rs",
            "非前缀路径只归一分隔符"
        );
        assert_eq!(
            to_workspace_relative_path("  /tmp/x.rs  ", None),
            "/tmp/x.rs",
            "无 workspace 时只 trim + 归一"
        );
    }

    #[test]
    fn diff_count_only_non_zero_sides() {
        assert_eq!(format_bot_diff_count(&DiffStat { added: 8, removed: 0 }), "🟢 `+8`");
        assert_eq!(format_bot_diff_count(&DiffStat { added: 0, removed: 3 }), "🔴 `-3`");
        assert_eq!(
            format_bot_diff_count(&DiffStat { added: 8, removed: 3 }),
            "🟢 `+8` 🔴 `-3`"
        );
        assert_eq!(format_bot_diff_count(&DiffStat { added: 0, removed: 0 }), "");
    }

    #[test]
    fn tool_status_mapping_including_stopped_falling_through() {
        assert_eq!(format_tool_status(Some(ToolStatus::Completed), None, Lang::ZhCn), "完成");
        assert_eq!(format_tool_status(Some(ToolStatus::Denied), None, Lang::ZhCn), "已拒绝");
        assert_eq!(format_tool_status(Some(ToolStatus::InProgress), None, Lang::En), "Running");
        assert_eq!(
            format_tool_status(Some(ToolStatus::Failed), Some("boom  "), Lang::ZhCn),
            "失败: boom",
            "错误信息归一空白并截断"
        );
        assert_eq!(
            format_tool_status(Some(ToolStatus::Stopped), None, Lang::ZhCn),
            "等待中",
            "ZCode 的 formatToolStatus 不处理 stopped，落默认 pending"
        );
        assert_eq!(format_tool_status(None, None, Lang::ZhCn), "等待中");
    }

    #[test]
    fn summary_line_shape() {
        let line = format_bot_tool_call_summary_line(
            &tool_call("bash", json!({ "command": "npm test" })),
            Lang::ZhCn,
            None,
        );
        assert_eq!(line, "- 等待中 · Bash · `npm test`");
    }

    #[test]
    fn summary_line_includes_diff_for_edit_kind() {
        let mut tc = tool_call(
            "edit",
            json!({ "path": "/ws/a.rs", "old_string": "1\n2\n", "new_string": "1\n2\n3\n" }),
        );
        tc.title = Some("Edit".into());
        tc.status = Some(ToolStatus::Completed);
        let line = format_bot_tool_call_summary_line(&tc, Lang::ZhCn, Some("/ws"));
        assert_eq!(
            line,
            "- 完成 · Edit · `a.rs` · 🟢 `+3` 🔴 `-2`",
            "edit kind 才做路径相对化与增删统计"
        );
    }

    #[test]
    fn terminal_statuses() {
        for s in [
            ToolStatus::Completed,
            ToolStatus::Failed,
            ToolStatus::Denied,
            ToolStatus::Stopped,
        ] {
            assert!(is_bot_tool_call_reply_terminal(Some(s)), "{s:?} 应是终态");
        }
        assert!(!is_bot_tool_call_reply_terminal(Some(ToolStatus::InProgress)));
        assert!(!is_bot_tool_call_reply_terminal(None));
    }

    #[test]
    fn change_summary_headers_differ_by_locale() {
        let summary = ChangeSummary {
            file_count: 2,
            added: 5,
            removed: 1,
            files: vec![
                FileChange { path: "src/a.rs".into(), added: 3, removed: 1 },
                FileChange { path: "src/b.rs".into(), added: 2, removed: 0 },
            ],
        };
        assert_eq!(
            format_bot_change_summary(&summary, Lang::ZhCn),
            "变更摘要：2 个文件，🟢 `+5` 🔴 `-1`\n- `src/a.rs` (🟢 `+3` 🔴 `-1`)\n- `src/b.rs` (🟢 `+2`)"
        );
        assert_eq!(
            format_bot_change_summary(&summary, Lang::En),
            "Change summary: 2 files, 🟢 `+5` 🔴 `-1`\n- `src/a.rs` (🟢 `+3` 🔴 `-1`)\n- `src/b.rs` (🟢 `+2`)",
            "en 头行是硬编码模板，不是 zh 的译文"
        );
        assert_eq!(format_bot_change_summary(&ChangeSummary::default(), Lang::ZhCn), "");
    }

    #[test]
    fn change_summary_caps_at_ten_files_with_overflow_line() {
        let files: Vec<FileChange> = (0..12)
            .map(|i| FileChange {
                path: format!("f{i}.rs"),
                added: 1,
                removed: 0,
            })
            .collect();
        let summary = ChangeSummary { file_count: 12, added: 12, removed: 0, files };
        let text = format_bot_change_summary(&summary, Lang::ZhCn);
        assert_eq!(text.lines().count(), 1 + 10 + 1, "头行 + 10 个文件 + 溢出行");
        let overflow = format!("- {}", tf(Lang::ZhCn, "还有 {count} 个文件", &[("count", "2")]));
        assert!(text.ends_with(&overflow), "{text}");
    }

    #[test]
    fn split_prefers_space_break_over_earlier_newline() {
        // 窗口内既有换行又有空格时取 max → 空格更靠后就断在空格
        let mut text = "line1\n".to_string();
        text.push_str(&"b".repeat(40));
        text.push(' ');
        text.push_str(&"c".repeat(3600));
        let chunks = split_long_reply_text(&text);
        // 首块在空格处断（index 46），剩余 3600 个 c 还要再硬切一次 → 共 3 块。
        assert_eq!(chunks.len(), 3, "应切成三块：{chunks:?}");
        assert!(chunks[0].ends_with(&"b".repeat(40)), "{:?}", chunks[0]);
        assert!(chunks[0].starts_with("line1"), "{:?}", chunks[0]);
        assert_eq!(chunks[1].chars().count(), 3500, "第二次无断点硬切");
    }

    #[test]
    fn split_hard_cuts_when_no_breakpoint_in_window() {
        let text = "x".repeat(7000);
        let chunks = split_long_reply_text(&text);
        assert_eq!(chunks.len(), 2, "无断点硬切 3500");
        assert_eq!(chunks[0].chars().count(), 3500);
        assert_eq!(chunks[1].chars().count(), 3500);
    }

    #[test]
    fn split_keeps_short_text_whole_and_trims() {
        assert_eq!(split_long_reply_text("  hello  "), vec!["hello".to_string()]);
        assert!(split_long_reply_text("").is_empty());
        let near = "a".repeat(3500);
        assert_eq!(split_long_reply_text(&near).len(), 1, "等于上限不切");
    }

    #[test]
    fn flush_only_on_terminal_state() {
        let (messages, rest) = extract_bot_assistant_response_messages(" partial ", false);
        assert!(messages.is_empty(), "非终态不 flush");
        assert_eq!(rest, " partial ", "原文保留在 buffer");

        let (messages, rest) = extract_bot_assistant_response_messages(" done ", true);
        assert_eq!(messages, vec!["done".to_string()]);
        assert_eq!(rest, "", "flush 后 buffer 清空");
    }

    #[test]
    fn blocks_are_chunked_in_order() {
        let blocks = vec![
            BotAssistantReplyBlock::Content { content: "第一段".into() },
            BotAssistantReplyBlock::ToolCall {
                tool_call: {
                    let mut tc = tool_call("bash", json!({ "command": "ls" }));
                    tc.status = Some(ToolStatus::Completed);
                    tc
                },
            },
            BotAssistantReplyBlock::ChangeSummary {
                change_summary: ChangeSummary {
                    file_count: 1,
                    added: 2,
                    removed: 0,
                    files: vec![FileChange { path: "a.rs".into(), added: 2, removed: 0 }],
                },
            },
        ];
        let messages = format_bot_assistant_reply_blocks(&blocks, Lang::ZhCn, None);
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0], "第一段");
        assert_eq!(messages[1], "工具调用：\n- 完成 · Bash · `ls`");
        assert_eq!(messages[2], "变更摘要：1 个文件，🟢 `+2`\n- `a.rs` (🟢 `+2`)");
    }

    #[test]
    fn word_boundary_matches_js_b_semantics() {
        // \brm\b 不命中 hardware，命中独立的 rm；\bupdate\b 不命中 updates
        assert!(contains_word_case_insensitive("rm -rf /tmp", "rm"));
        assert!(!contains_word_case_insensitive("hardware", "rm"));
        // JS `\bupdate\b` 不命中 "Updated"（后随 d 属 \w）；ZCode 词表里另有 `updated` 项
        assert!(contains_word_case_insensitive("Updated files", "updated"));
        assert!(!contains_word_case_insensitive("Updated files", "update"));
        assert!(!contains_word_case_insensitive("updates files", "update"));
        assert!(!contains_word_case_insensitive("newer", "new"));
    }

    #[test]
    fn edit_kind_label_priority_delete_then_add_then_update_then_generic() {
        let mk = |desc: &str, raw: Value| PermissionRequest {
            title: None,
            description: desc.into(),
            kind: "edit".into(),
            raw,
        };
        let label = |request: &PermissionRequest| {
            let preview = get_permission_request_preview(request);
            format_edit_permission_kind_label(request, &preview, Lang::ZhCn)
        };
        // delete 词表排在最前，压过 fileChange.add
        assert_eq!(
            label(&mk(
                "please delete a.rs",
                json!({ "changes": { "a.rs": { "type": "add" } } })
            )),
            "删除中"
        );
        // fileChange.type = add 直接判写入
        assert_eq!(
            label(&mk("edit a.rs", json!({ "changes": { "a.rs": { "type": "add" } } }))),
            "写入中"
        );
        // update 词表
        assert_eq!(label(&mk("updated config", json!({}))), "更新中");
        // 兜底：ZCode 的「泛化 Edit」注释说的就是这条
        assert_eq!(label(&mk("edit a.rs", json!({}))), "编辑中");
    }

    #[test]
    fn edit_title_strips_prefix_then_falls_back_to_relative_path() {
        // title 就是 "Edit" 时，剥完为空 → 落到 file_paths 做相对化
        let request = PermissionRequest {
            title: Some("Edit".into()),
            description: "d".into(),
            kind: "edit".into(),
            raw: json!({ "input": { "path": "/proj/pi-flash/src/a.rs" } }),
        };
        let preview = get_permission_request_preview(&request);
        assert_eq!(
            format_permission_request_title(&request, &preview, Lang::ZhCn, Some("/proj/pi-flash")),
            "编辑中 src/a.rs"
        );
        // title 带 "Edit " 前缀时，剥掉前缀后的剩余文本直接当目标，**不再相对化**
        let request = PermissionRequest {
            title: Some("Edit src/b.rs".into()),
            description: "d".into(),
            kind: "edit".into(),
            raw: json!({ "input": { "path": "/proj/pi-flash/src/b.rs" } }),
        };
        let preview = get_permission_request_preview(&request);
        assert_eq!(
            format_permission_request_title(&request, &preview, Lang::ZhCn, Some("/proj/pi-flash")),
            "编辑中 src/b.rs"
        );
    }

    #[test]
    fn permission_summary_branches_on_command_then_paths_then_header_only() {
        // 有命令 → header + 行内代码
        let request = PermissionRequest {
            title: None,
            description: "Run npm test".into(),
            kind: "bash".into(),
            raw: json!({ "input": { "command": "npm test" } }),
        };
        assert_eq!(
            format_bot_permission_request_summary(&request, Lang::ZhCn, None),
            format!("{}\nRun npm test\n`npm test`", t(Lang::ZhCn, "需要权限："))
        );

        // 无命令但有路径 → 最多 3 个，相对化 + 行内代码
        let request = PermissionRequest {
            title: None,
            description: "Write files".into(),
            kind: "edit".into(),
            raw: json!({ "paths": ["/proj/pi-flash/a.rs", "/proj/pi-flash/b.rs"] }),
        };
        assert_eq!(
            format_bot_permission_request_summary(&request, Lang::ZhCn, Some("/proj/pi-flash")),
            format!(
                // ZCode `filePaths.length===1 || fileChanges.length===1`；
                // 本 case 有 2 个 path、0 条变更 → 两边都不满足 → target 为空。
                "{}\n写入中\n`a.rs`, `b.rs`",
                t(Lang::ZhCn, "需要权限：")
            )
        );

        // 什么都没有 → 只有 header
        let request = PermissionRequest {
            title: None,
            description: "Proceed?".into(),
            kind: "generic".into(),
            raw: json!({}),
        };
        assert_eq!(
            format_bot_permission_request_summary(&request, Lang::En, None),
            format!("Permission required:\nProceed?")
        );
    }

    #[test]
    fn permission_path_list_is_capped_at_three() {
        let request = PermissionRequest {
            title: None,
            description: "Write files".into(),
            kind: "edit".into(),
            raw: json!({ "paths": ["/p/1.rs", "/p/2.rs", "/p/3.rs", "/p/4.rs"] }),
        };
        let text = format_bot_permission_request_summary(&request, Lang::En, Some("/p"));
        assert!(text.contains("`1.rs`, `2.rs`, `3.rs`"), "{text}");
        assert!(!text.contains("4.rs"), "第 4 个不该出现：{text}");
    }
}
