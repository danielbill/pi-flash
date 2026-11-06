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


pub(crate) mod applypatch;
pub(crate) use applypatch::{
    apply_patch_preview_to_files, is_apply_patch_tool_name, is_edit_tool_name,
    parse_apply_patch_input,
};
#[derive(Debug, Clone)]
pub(crate) struct Cell {
    pub(crate) line_no: Option<u64>,
    pub(crate) text: String,
    pub(crate) kind: CellKind,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Hunk text 未渲染（pi-web hunk 行返回 null）但保留数据
pub(crate) enum Row {
    /// @@ header / "\ No newline" marker — rendered as a band, no cells
    Hunk(String),
    Line {
        left: Cell,
        right: Cell,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct DiffFile {
    pub(crate) old_path: Option<String>,
    pub(crate) new_path: Option<String>,
    pub(crate) rows: Vec<Row>,
}

fn empty_cell() -> Cell {
    Cell {
        line_no: None,
        text: String::new(),
        kind: CellKind::Empty,
    }
}

/// Pending removed/added runs pair up into split rows on the next flush.
struct RowSink {
    rows: Vec<Row>,
    pending_removed: Vec<Cell>,
    pending_added: Vec<Cell>,
}

impl RowSink {
    fn new() -> RowSink {
        RowSink {
            rows: Vec::new(),
            pending_removed: Vec::new(),
            pending_added: Vec::new(),
        }
    }

    fn flush(&mut self) {
        let count = self.pending_removed.len().max(self.pending_added.len());
        for i in 0..count {
            let left = self
                .pending_removed
                .get(i)
                .cloned()
                .unwrap_or_else(empty_cell);
            let right = self
                .pending_added
                .get(i)
                .cloned()
                .unwrap_or_else(empty_cell);
            self.rows.push(Row::Line { left, right });
        }
        self.pending_removed.clear();
        self.pending_added.clear();
    }

    fn context(&mut self, text: &str, line_no: Option<u64>) {
        self.flush();
        self.rows.push(Row::Line {
            left: Cell {
                line_no,
                text: text.to_string(),
                kind: CellKind::Context,
            },
            right: Cell {
                line_no,
                text: text.to_string(),
                kind: CellKind::Context,
            },
        });
    }

    fn removed(&mut self, text: &str, line_no: Option<u64>) {
        self.pending_removed.push(Cell {
            line_no,
            text: text.to_string(),
            kind: CellKind::Removed,
        });
    }

    fn added(&mut self, text: &str, line_no: Option<u64>) {
        self.pending_added.push(Cell {
            line_no,
            text: text.to_string(),
            kind: CellKind::Added,
        });
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
                files.push(DiffFile {
                    old_path: None,
                    new_path: None,
                    rows: Vec::new(),
                });
            }
            flush_changes!();
            old_no = h.0;
            new_no = h.2;
            hunk_old_left = h.1.unwrap_or(1);
            hunk_new_left = h.3.unwrap_or(1);
            files[current.unwrap()]
                .rows
                .push(Row::Hunk(line.to_string()));
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
