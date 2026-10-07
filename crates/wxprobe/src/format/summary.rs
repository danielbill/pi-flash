//! `getCompactToolCallSummary` 的移植 —— **PC 端与微信端共享的摘要原语**。
//!
//! 对应 ZCode `shared/tool-call-summary.ts:207`，PC 端
//! `ui/lib/toolCallSummary.ts` / `PermissionDialog.tsx` 与微信端
//! `bots/replyFormatter.ts:246` 消费的是同一份。这就是「微信界面对齐 PC」的根：
//! 两个渲染器拿同一份摘要，差异只在排版。

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChangeStat {
    pub added: u32,
    pub removed: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolCallSummary {
    pub primary_text: String,
    pub secondary_text: Option<String>,
    pub change_stat: Option<ChangeStat>,
}

pub struct ToolCallSummarySource<'a> {
    pub title: Option<&'a str>,
    pub kind: &'a str,
    pub input: &'a Value,
    pub output: Option<&'a Value>,
    pub raw: Option<&'a Value>,
}

/// ZCode `normalizeDisplayText`：trim + 连续空白压成单空格。
pub fn normalize_display_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// ZCode `countLines`：数换行；空串 0；**末尾换行不算一行**。
fn count_lines(value: &str) -> u32 {
    if value.is_empty() {
        return 0;
    }
    let mut count = value.matches('\n').count() as u32 + 1;
    if value.ends_with('\n') {
        count -= 1;
    }
    count
}

fn is_record(value: &Value) -> bool {
    value.is_object()
}

/// ZCode `readFirstStringField`：**只认字符串**，数字/布尔一概跳过。
fn read_first_string_field(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| value.get(*k).and_then(Value::as_str).map(str::to_string))
}

const BEFORE_KEYS: [&str; 4] = ["old_string", "oldString", "oldText", "before"];
const AFTER_KEYS: [&str; 5] = ["new_string", "newString", "newText", "after", "content"];

/// ZCode `extractBeforeAfterText`：三层取 before/after
/// （自身 → `metadata.filediff` → `content[]` 块）。
fn extract_before_after_text(source: &Value) -> Option<(String, String)> {
    if !is_record(source) {
        return None;
    }
    if let (Some(before), Some(after)) = (
        read_first_string_field(source, &BEFORE_KEYS),
        read_first_string_field(source, &AFTER_KEYS),
    ) {
        return Some((before, after));
    }
    if let Some(file_diff) = source.get("metadata").and_then(|m| m.get("filediff")) {
        if let (Some(before), Some(after)) = (
            read_first_string_field(file_diff, &BEFORE_KEYS),
            read_first_string_field(file_diff, &AFTER_KEYS),
        ) {
            return Some((before, after));
        }
    }
    if let Some(blocks) = source.get("content").and_then(Value::as_array) {
        for block in blocks {
            if !is_record(block) {
                continue;
            }
            if let (Some(before), Some(after)) = (
                read_first_string_field(block, &BEFORE_KEYS),
                read_first_string_field(block, &AFTER_KEYS),
            ) {
                return Some((before, after));
            }
        }
    }
    None
}

/// ZCode `/(edit|patch|replace|multi.?edit)/i` 的等价实现。
///
/// 第四个分支 `multi.?edit` **必然包含子串 `edit`**，所以三个 `contains`
/// 已完全覆盖整条正则 —— 不需要手写 `multi` + 可选字符 + `edit` 的扫描
/// （那样反而要小心 UTF-8 字节边界）。
fn kind_matches_change(kind: &str) -> bool {
    let hay = kind.to_lowercase();
    hay.contains("edit") || hay.contains("patch") || hay.contains("replace")
}

/// ZCode `getChangeStat`：只有 edit 族 kind 才统计行数增删。
fn get_change_stat(kind: &str, input: &Value, output: Option<&Value>, raw: Option<&Value>) -> Option<ChangeStat> {
    if !kind_matches_change(kind) {
        return None;
    }
    let source = extract_before_after_text(input)
        .or_else(|| output.and_then(extract_before_after_text))
        .or_else(|| raw.and_then(extract_before_after_text))?;
    let removed = count_lines(&source.0);
    let added = count_lines(&source.1);
    if added == 0 && removed == 0 {
        return None;
    }
    Some(ChangeStat { added, removed })
}

/// ZCode `getInputSummary`：字符串直接归一；对象按
/// `command → path → file_path → filePath → prompt` 取第一个**字符串**。
fn get_input_summary(input: &Value) -> Option<String> {
    if let Some(text) = input.as_str() {
        let summary = normalize_display_text(text);
        return (!summary.is_empty()).then_some(summary);
    }
    if !is_record(input) {
        return None;
    }
    for key in ["command", "path", "file_path", "filePath", "prompt"] {
        if let Some(candidate) = input.get(key).and_then(Value::as_str) {
            let summary = normalize_display_text(candidate);
            if !summary.is_empty() {
                return Some(summary);
            }
        }
    }
    None
}

/// ZCode `getCompactToolCallSummary`（`tool-call-summary.ts:207`）。
pub fn get_compact_tool_call_summary(source: ToolCallSummarySource<'_>) -> ToolCallSummary {
    let change_stat = get_change_stat(
        source.kind,
        source.input,
        source.output,
        source.raw,
    );
    let primary_text = source
        .title
        .map(normalize_display_text)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "tool".to_string());
    ToolCallSummary {
        primary_text,
        secondary_text: get_input_summary(source.input),
        change_stat,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn summary(kind: &str, input: Value) -> ToolCallSummary {
        get_compact_tool_call_summary(ToolCallSummarySource {
            title: None,
            kind,
            input: &input,
            output: None,
            raw: None,
        })
    }

    #[test]
    fn primary_text_falls_back_to_tool() {
        assert_eq!(summary("bash", json!({})).primary_text, "tool");
        assert_eq!(
            get_compact_tool_call_summary(ToolCallSummarySource {
                title: Some("   "),
                kind: "bash",
                input: &json!({}),
                output: None,
                raw: None,
            })
            .primary_text,
            "tool",
            "纯空白 title 也回退"
        );
        assert_eq!(
            get_compact_tool_call_summary(ToolCallSummarySource {
                title: Some("  Bash  "),
                kind: "bash",
                input: &json!({}),
                output: None,
                raw: None,
            })
            .primary_text,
            "Bash"
        );
    }

    #[test]
    fn input_summary_prefers_command_then_path() {
        assert_eq!(
            summary("bash", json!({ "command": "  npm   test  " })).secondary_text.as_deref(),
            Some("npm test"),
            "连续空白压成单空格"
        );
        assert_eq!(
            summary("edit", json!({ "path": "/a/b.rs", "command": "x" })).secondary_text.as_deref(),
            Some("x"),
            "字段优先级：command 排在 path 之前"
        )
    }

    #[test]
    fn input_summary_ignores_non_string_fields() {
        assert_eq!(
            summary("bash", json!({ "command": 42 })).secondary_text,
            None,
            "ZCode 只认 typeof === 'string'"
        );
        assert_eq!(summary("bash", json!(42)).secondary_text, None);
    }

    #[test]
    fn change_stat_only_for_edit_family() {
        let before_after = json!({ "old_string": "a\nb\n", "new_string": "a\nb\nc\n" });
        let s = summary("edit", before_after.clone());
        assert_eq!(
            s.change_stat,
            Some(ChangeStat { added: 3, removed: 2 }),
            "末尾换行不计行：'a\\nb\\n' 是 2 行，'a\\nb\\nc\\n' 是 3 行"
        );
        assert_eq!(
            summary("multi_edit", before_after.clone()).change_stat,
            Some(ChangeStat { added: 3, removed: 2 }),
            "multi_edit 命中 multi.?edit"
        );
        assert_eq!(summary("read", before_after).change_stat, None, "非 edit 族不统计");
    }

    #[test]
    fn change_stat_reads_before_after_from_metadata_filediff() {
        let s = summary(
            "patch",
            json!({ "metadata": { "filediff": { "old_string": "x\n", "new_string": "y\nz\n" } } }),
        );
        assert_eq!(s.change_stat, Some(ChangeStat { added: 2, removed: 1 }));
    }

    #[test]
    fn change_stat_zero_when_both_sides_empty() {
        assert_eq!(
            summary("edit", json!({ "old_string": "", "new_string": "" })).change_stat,
            None
        );
    }

    #[test]
    fn count_lines_edge_cases() {
        assert_eq!(count_lines(""), 0);
        assert_eq!(count_lines("a"), 1);
        assert_eq!(count_lines("a\nb"), 2);
        assert_eq!(count_lines("a\n"), 1, "末尾换行减一");
        assert_eq!(count_lines("\n"), 1, "只有换行：1+1 再减末尾换行 = 1");
    }
}
