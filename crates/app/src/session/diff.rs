//! Diff parsing for tool-call cards (v56-2 c10; pi-web lib/patch.ts +
//! lib/apply-patch.ts parity). Pure parsing; rendering lives in messages.rs.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CellKind {
    Context,
    Removed,
    Added,
    Empty,
}

#[derive(Debug, Clone)]
pub(crate) struct Cell {
    pub(crate) line_no: Option<u64>,
    pub(crate) text: String,
    pub(crate) kind: CellKind,
}

#[derive(Debug, Clone)]
pub(crate) enum Row {
    /// @@ header / "\ No newline" marker — rendered as a band, no cells
    Hunk(String),
    Line { left: Cell, right: Cell },
}

#[derive(Debug, Clone)]
pub(crate) struct DiffFile {
    pub(crate) old_path: Option<String>,
    pub(crate) new_path: Option<String>,
    pub(crate) rows: Vec<Row>,
}

fn empty_cell() -> Cell {
    Cell { line_no: None, text: String::new(), kind: CellKind::Empty }
}

/// Pending removed/added runs pair up into split rows on the next flush.
struct RowSink {
    rows: Vec<Row>,
    pending_removed: Vec<Cell>,
    pending_added: Vec<Cell>,
}

impl RowSink {
    fn new() -> RowSink {
        RowSink { rows: Vec::new(), pending_removed: Vec::new(), pending_added: Vec::new() }
    }

    fn flush(&mut self) {
        let count = self.pending_removed.len().max(self.pending_added.len());
        for i in 0..count {
            let left = self.pending_removed.get(i).cloned().unwrap_or_else(empty_cell);
            let right = self.pending_added.get(i).cloned().unwrap_or_else(empty_cell);
            self.rows.push(Row::Line { left, right });
        }
        self.pending_removed.clear();
        self.pending_added.clear();
    }

    fn context(&mut self, text: &str, line_no: Option<u64>) {
        self.flush();
        self.rows.push(Row::Line {
            left: Cell { line_no, text: text.to_string(), kind: CellKind::Context },
            right: Cell { line_no, text: text.to_string(), kind: CellKind::Context },
        });
    }

    fn removed(&mut self, text: &str, line_no: Option<u64>) {
        self.pending_removed
            .push(Cell { line_no, text: text.to_string(), kind: CellKind::Removed });
    }

    fn added(&mut self, text: &str, line_no: Option<u64>) {
        self.pending_added
            .push(Cell { line_no, text: text.to_string(), kind: CellKind::Added });
    }

    /// finish(): flush + drop non-line rows (hunk bands carry no data here)
    fn finish(mut self) -> Vec<Row> {
        self.flush();
        self.rows.retain(|r| matches!(r, Row::Line { .. }));
        self.rows
    }
}

fn clean_path(p: &str) -> String {
    p.split('\t').next().unwrap_or("").trim().to_string()
}

/// pi-web parseUnifiedPatch: unified diff → split files. Returns None when no
/// renderable line rows came out.
pub(crate) fn parse_unified_patch(text: &str) -> Option<Vec<DiffFile>> {
    let mut files: Vec<DiffFile> = Vec::new();
    let mut pending_old_path: Option<String> = None;
    let mut old_no: u64 = 0;
    let mut new_no: u64 = 0;
    // @@ header counts: while either is positive we're inside a hunk body,
    // where "--- "/"+++ " are content lines, not file headers
    let mut hunk_old_left = 0u64;
    let mut hunk_new_left = 0u64;
    let mut removed: Vec<Cell> = Vec::new();
    let mut added: Vec<Cell> = Vec::new();

    // index of the current file in `files` (Option avoids borrow tangles)
    let mut current: Option<usize> = None;

    macro_rules! flush_changes {
        () => {
            if let Some(ix) = current {
                let count = removed.len().max(added.len());
                for i in 0..count {
                    let left = removed.get(i).cloned().unwrap_or_else(empty_cell);
                    let right = added.get(i).cloned().unwrap_or_else(empty_cell);
                    files[ix].rows.push(Row::Line { left, right });
                }
            }
            removed.clear();
            added.clear();
        };
    }

    for line in text.lines() {
        let inside_hunk = hunk_old_left > 0 || hunk_new_left > 0;

        if !inside_hunk {
            if let Some(p) = line.strip_prefix("--- ") {
                flush_changes!();
                pending_old_path = Some(clean_path(p));
                continue;
            }
            if let Some(p) = line.strip_prefix("+++ ") {
                flush_changes!();
                current = Some(files.len());
                files.push(DiffFile {
                    old_path: pending_old_path.take(),
                    new_path: Some(clean_path(p)),
                    rows: Vec::new(),
                });
                continue;
            }
        }

        if let Some(h) = hunk_header(line) {
            if current.is_none() {
                current = Some(files.len());
                files.push(DiffFile { old_path: None, new_path: None, rows: Vec::new() });
            }
            flush_changes!();
            old_no = h.0;
            new_no = h.2;
            hunk_old_left = h.1.unwrap_or(1);
            hunk_new_left = h.3.unwrap_or(1);
            files[current.unwrap()].rows.push(Row::Hunk(line.to_string()));
            continue;
        }

        let Some(ix) = current else { continue };

        if line.starts_with("\\ ") {
            flush_changes!();
            files[ix].rows.push(Row::Hunk(line.to_string()));
            continue;
        }

        let mut chars = line.chars();
        let prefix = chars.next();
        let content: &str = chars.as_str();

        match prefix {
            Some(' ') => {
                flush_changes!();
                files[ix].rows.push(Row::Line {
                    left: Cell {
                        line_no: Some(old_no),
                        text: content.to_string(),
                        kind: CellKind::Context,
                    },
                    right: Cell {
                        line_no: Some(new_no),
                        text: content.to_string(),
                        kind: CellKind::Context,
                    },
                });
                old_no += 1;
                new_no += 1;
                hunk_old_left = hunk_old_left.saturating_sub(1);
                hunk_new_left = hunk_new_left.saturating_sub(1);
            }
            Some('-') => {
                removed.push(Cell {
                    line_no: Some(old_no),
                    text: content.to_string(),
                    kind: CellKind::Removed,
                });
                old_no += 1;
                hunk_old_left = hunk_old_left.saturating_sub(1);
            }
            Some('+') => {
                added.push(Cell {
                    line_no: Some(new_no),
                    text: content.to_string(),
                    kind: CellKind::Added,
                });
                new_no += 1;
                hunk_new_left = hunk_new_left.saturating_sub(1);
            }
            _ if !line.is_empty() => {
                flush_changes!();
                files[ix].rows.push(Row::Hunk(line.to_string()));
            }
            _ => {}
        }
    }
    flush_changes!();

    files.retain(|f| f.rows.iter().any(|r| matches!(r, Row::Line { .. })));
    (!files.is_empty()).then_some(files)
}

/// `@@ -a[,b] +c[,d] @@` → (a, b, c, d)
fn hunk_header(line: &str) -> Option<(u64, Option<u64>, u64, Option<u64>)> {
    let rest = line.strip_prefix("@@ -")?;
    let sp = rest.find(" +")?;
    let (old_part, new_part) = rest.split_at(sp);
    let new_part = &new_part[2..];
    let new_part = new_part.split(" @@").next()?;
    let mut old_it = old_part.split(',');
    let mut new_it = new_part.split(',');
    let a = old_it.next()?.parse().ok()?;
    let b = old_it.next().and_then(|x| x.parse().ok());
    let c = new_it.next()?.parse().ok()?;
    let d = new_it.next().and_then(|x| x.parse().ok());
    Some((a, b, c, d))
}

/// Paths targeted by a V4A patch document, in order (pi-web
/// extractApplyPatchPaths).
pub(crate) fn extract_apply_patch_paths(patch_text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in patch_text.lines() {
        if let Some(rest) = line.strip_prefix("*** ") {
            for op in ["Add File: ", "Delete File: ", "Update File: "] {
                if let Some(p) = rest.strip_prefix(op) {
                    let p = p.trim();
                    if !p.is_empty() && !paths.iter().any(|x| x == p) {
                        paths.push(p.to_string());
                    }
                }
            }
        }
    }
    paths
}

/// pi-web parseApplyPatchInput: V4A patch document → split files. Tolerant of
/// truncated input (streaming) — complete operations parsed so far return.
pub(crate) fn parse_apply_patch_input(patch_text: &str) -> Option<Vec<DiffFile>> {
    if !patch_text.contains("*** Begin Patch")
        && !["Add File: ", "Delete File: ", "Update File: "]
            .iter()
            .any(|op| patch_text.lines().any(|l| l.starts_with(&format!("*** {op}"))))
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
            current = Some((
                files.len(),
                op_s,
            ));
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
        let Some(path) = raw.get("filePath").and_then(|v| v.as_str()) else { continue };
        let Some(diff) = raw.get("diff").and_then(|v| v.as_str()) else { continue };
        let op = raw.get("operation").and_then(|v| v.as_str()).unwrap_or("");
        let move_path = raw.get("movePath").and_then(|v| v.as_str());
        let mut sink = RowSink::new();
        for line in diff.lines() {
            let Some((marker, num, body)) = split_numbered_line(line) else { continue };
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
        let lines: Vec<&Row> = files[0].rows.iter().filter(|r| matches!(r, Row::Line { .. })).collect();
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
