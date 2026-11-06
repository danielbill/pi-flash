//! apply patch 解析：parse_apply_patch_input / preview→files / 工具名
//! 判别，含内嵌测试。

use super::*;

/// pi-web parseApplyPatchInput: V4A patch document → split files. Tolerant of
/// truncated input (streaming) — complete operations parsed so far return.
pub(crate) fn parse_apply_patch_input(patch_text: &str) -> Option<Vec<DiffFile>> {
    if !patch_text.contains("*** Begin Patch")
        && !["Add File: ", "Delete File: ", "Update File: "]
            .iter()
            .any(|op| {
                patch_text
                    .lines()
                    .any(|l| l.starts_with(&format!("*** {op}")))
            })
    {
        return None;
    }

    let mut files: Vec<DiffFile> = Vec::new();
    let mut sink: Option<RowSink> = None;
    // (file index, operation) — "add"/"delete" bodies carry bare lines,
    // "update" bodies carry prefixed ones
    let mut current: Option<(usize, &'static str)> = None;

    for raw_line in patch_text.lines() {
        if let Some((op, path)) = file_header(raw_line) {
            if let Some(s) = sink.take() {
                let rows = s.finish();
                if let Some((ix, _)) = current {
                    files[ix].rows = rows;
                }
            }
            let op_s: &'static str = match op {
                Op::Add => "add",
                Op::Delete => "delete",
                Op::Update => "update",
            };
            current = Some((files.len(), op_s));
            files.push(DiffFile {
                old_path: (op != Op::Add).then(|| path.clone()),
                new_path: (op != Op::Delete).then(|| path.clone()),
                rows: Vec::new(),
            });
            sink = Some(RowSink::new());
            continue;
        }

        if let Some(move_path) = raw_line.strip_prefix("*** Move to: ") {
            let move_path = move_path.trim();
            if let Some((ix, "update")) = current {
                files[ix].new_path = Some(move_path.to_string());
            }
            continue;
        }

        let Some(s) = sink.as_mut() else { continue };
        let Some((ix, op)) = current else { continue };
        if raw_line.starts_with("*** ") {
            continue; // Begin/End Patch markers
        }
        if op == "update" && raw_line.starts_with("@@") {
            continue; // hunk context markers carry no line numbers here
        }

        if op == "update" {
            let mut chars = raw_line.chars();
            let prefix = chars.next();
            let content = chars.as_str();
            match prefix {
                Some('+') => s.added(content, None),
                Some('-') => s.removed(content, None),
                Some(' ') => s.context(content, None),
                _ if !raw_line.is_empty() => s.context(raw_line, None),
                _ => {}
            }
        } else if op == "add" {
            if raw_line.is_empty() {
                continue;
            }
            let body = raw_line.strip_prefix('+').unwrap_or(raw_line);
            s.added(body, None);
        } else {
            // delete
            if raw_line.is_empty() {
                continue;
            }
            let body = raw_line.strip_prefix('-').unwrap_or(raw_line);
            s.removed(body, None);
        }
        let _ = ix;
    }
    if let Some(s) = sink.take() {
        let rows = s.finish();
        if let Some((ix, _)) = current {
            files[ix].rows = rows;
        }
    }

    files.retain(|f| !f.rows.is_empty());
    (!files.is_empty()).then_some(files)
}

#[derive(PartialEq)]
enum Op {
    Add,
    Delete,
    Update,
}

fn file_header(line: &str) -> Option<(Op, String)> {
    let rest = line.strip_prefix("*** ")?;
    for (prefix, op) in [
        ("Add File: ", Op::Add),
        ("Delete File: ", Op::Delete),
        ("Update File: ", Op::Update),
    ] {
        if let Some(p) = rest.strip_prefix(prefix) {
            let p = p.trim();
            if !p.is_empty() {
                return Some((op, p.to_string()));
            }
        }
    }
    None
}

/// pi-web applyPatchPreviewToFiles: the extension's applied-result preview —
/// per-file diff lines embed their line number (`+12 text` / `-3 text`).
pub(crate) fn apply_patch_preview_to_files(preview: &Value) -> Option<Vec<DiffFile>> {
    let raw_files = preview.get("files")?.as_array()?;
    let mut files = Vec::new();
    for raw in raw_files {
        let Some(path) = raw.get("filePath").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(diff) = raw.get("diff").and_then(|v| v.as_str()) else {
            continue;
        };
        let op = raw.get("operation").and_then(|v| v.as_str()).unwrap_or("");
        let move_path = raw.get("movePath").and_then(|v| v.as_str());
        let mut sink = RowSink::new();
        for line in diff.lines() {
            let Some((marker, num, body)) = split_numbered_line(line) else {
                continue;
            };
            match marker {
                '+' => sink.added(body, Some(num)),
                '-' => sink.removed(body, Some(num)),
                _ => sink.context(body, Some(num)),
            }
        }
        let rows = sink.finish();
        if rows.is_empty() {
            continue;
        }
        files.push(DiffFile {
            old_path: (op != "add").then(|| path.to_string()),
            new_path: (op != "delete").then(|| move_path.unwrap_or(path).to_string()),
            rows,
        });
    }
    (!files.is_empty()).then_some(files)
}

/// `+12 text` / `-3 text` / `␣7 text` → (marker, line_no, text)
fn split_numbered_line(line: &str) -> Option<(char, u64, &str)> {
    let mut chars = line.char_indices();
    let (_, marker) = chars.next()?;
    if !matches!(marker, '+' | '-' | ' ') {
        return None;
    }
    let rest = &line[1..];
    let rest_t = rest.trim_start_matches(' ');
    let (num, body) = rest_t.split_once(' ')?;
    let num: u64 = num.parse().ok()?;
    Some((marker, num, body))
}

/// pi-web tool-names.ts predicates (MCP servers expose decorated names).
pub(crate) fn is_edit_tool_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n == "edit"
        || n.starts_with("edit_")
        || n.ends_with(".edit")
        || n.ends_with("_edit")
        || n.contains("str_replace")
        || n.contains("replace_editor")
}

pub(crate) fn is_apply_patch_tool_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n == "apply_patch"
        || n.starts_with("apply_patch_")
        || n.ends_with(".apply_patch")
        || n.ends_with("_apply_patch")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unified_patch_splits_added_removed_runs() {
        let diff = "--- a/f.rs\t2024\n+++ b/f.rs\t2024\n@@ -1,3 +1,3 @@\n ctx\n-old\n+new\n keep\n";
        let files = parse_unified_patch(diff).expect("parses");
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].old_path.as_deref(), Some("a/f.rs"));
        assert_eq!(files[0].new_path.as_deref(), Some("b/f.rs"));
        let lines: Vec<&Row> = files[0]
            .rows
            .iter()
            .filter(|r| matches!(r, Row::Line { .. }))
            .collect();
        assert_eq!(lines.len(), 3);
        // pairing: removed old / added new share one row
        if let Row::Line { left, right } = lines[1] {
            assert_eq!(left.text, "old");
            assert_eq!(left.kind, CellKind::Removed);
            assert_eq!(right.text, "new");
            assert_eq!(right.kind, CellKind::Added);
        } else {
            panic!("expected line row");
        }
    }

    #[test]
    fn v4a_patch_parses_add_update_delete() {
        let patch = "*** Begin Patch\n*** Add File: new.rs\n+fn a() {}\n*** Update File: old.rs\n*** Move to: renamed.rs\n@@ context\n ctx\n-old\n+new\n*** Delete File: gone.rs\n-old body\n*** End Patch\n";
        let files = parse_apply_patch_input(patch).expect("parses");
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].new_path.as_deref(), Some("new.rs"));
        assert!(files[0].old_path.is_none());
        // update with move
        assert_eq!(files[1].old_path.as_deref(), Some("old.rs"));
        assert_eq!(files[1].new_path.as_deref(), Some("renamed.rs"));
        // delete keeps old path only
        assert_eq!(files[2].old_path.as_deref(), Some("gone.rs"));
        assert!(files[2].new_path.is_none());
    }

    #[test]
    fn extract_paths_dedupes_in_order() {
        let p = "*** Update File: a.rs\n*** Add File: b.rs\n*** Update File: a.rs\n";
        assert_eq!(extract_apply_patch_paths(p), vec!["a.rs", "b.rs"]);
    }
}
