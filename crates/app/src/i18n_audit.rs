// i18n 审计（语义键版）——三道防线，全部源码级静态检查。
//
// 迁移后（2026-10-12）键 = `<域>.<主语>[.<限定>]` 语义标识，TABLE 行为
// `(key, zh-CN, zh-TW, en)`。风险面随之改变：
//
// ① **键必须在表里**：`tr("拼错的键")` 编译期零检查，运行期会把裸键名
//    直接显示在界面上（比旧方案的"显示中文"更显眼也更难看）。扫源码里
//    所有 `tr("…")`/`tf("…")` 字面量，断言都能命中 TABLE 首列。
// ② **空译文挂账**：zh-TW/en 单元格为空 = 尚未翻译，tr() 回落 zh-CN。
//    存量挂进 BACKLOG（只减不增）；补齐后必须从 BACKLOG 删掉，否则红——
//    防止挂账清单变成死账。
// ③ **死键**：表里有、但源码再无字面量调用的键。只打印不判定（常量/变量
//    传参、预留文案都会误报）。
//
// 扫描实现按**整段文本**跑正则（`\s*` 吞换行）：旧版逐行匹配漏掉了
// `tr(\n    "…",\n)` 这种跨行调用（实测 97 条表项因此被误判"无调用点"）。
// 注释行先剔除（含 tr( 字样的注释会被当成调用点）。

#[cfg(test)]
mod audit {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};

    /// 存量未翻译键（zh-TW 或 en 为空的表行）。补齐译文后：TABLE 里填上
    /// 译文 + 从这里删掉该键——测试会校验清单没有死账。
    const BACKLOG: &[&str] = &[
        "branch.detour_note",
        "composer.compacting_wait2",
        "composer.hint_esc",
        "composer.hint_shortcuts",
        "file_view.image_read_failed",
        "git.commit_placeholder",
        "git.no_commits",
        "git.pushed",
        "git.pushing",
        "git.staged",
        "mcp.closed",
        "mcp.failed_count",
        "menu.no_command_found",
        "messages.before",
        "messages.extended_hidden",
        "messages.generating_args",
        "messages.too_large",
        "messages.truncated",
        "psp.confirm_delete",
        "psp.list_mode",
        "psp.messages_count",
        "psp.no_sessions",
        "psp.open_in_file_browser",
        "psp.project_deleted",
        "psp.project_groups",
        "psp.recent_sessions",
        "psp.sort_by_time",
        "psp.sort_manual",
        "psp.sort_mode",
    ];

    fn src_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
    }

    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().and_then(|s| s.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }

    /// 去掉纯注释行后拼回整段文本（跨行调用才能被整段正则扫到）
    fn code_of(p: &Path) -> String {
        let mut out = Vec::new();
        let text = read(p);
        for line in text.lines() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with("/*") || t.starts_with('*') {
                continue;
            }
            out.push(line);
        }
        out.join("\n")
    }

    /// TABLE 行 → (key, zh, tw, en)
    fn table_rows(i18n_src: &str) -> Vec<(String, String, String, String)> {
        let Some((_, after)) = i18n_src.split_once("const TABLE") else {
            panic!("i18n/table.rs 里找不到 TABLE");
        };
        let mut rows = Vec::new();
        let started = after.contains("&[");
        assert!(started, "i18n/table.rs 的 TABLE 声明格式变了");
        for line in after.lines() {
            let t = line.trim();
            if !(t.starts_with('(')) {
                continue;
            }
            let inner = t
                .trim_start_matches('(')
                .trim_end_matches("),")
                .trim_end_matches(')');
            let parts: Vec<String> = split_args(inner);
            if parts.len() == 4 {
                rows.push((
                    parts[0].clone(),
                    parts[1].clone(),
                    parts[2].clone(),
                    parts[3].clone(),
                ));
            }
        }
        rows
    }

    /// 把一行里的顶层逗号拆开，返回**去引号后**的字符串字面量
    fn split_args(s: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut cur = String::new();
        let mut in_str = false;
        let mut esc = false;
        for ch in s.chars() {
            if in_str {
                cur.push(ch);
                if esc {
                    esc = false;
                } else if ch == '\\' {
                    esc = true;
                } else if ch == '"' {
                    in_str = false;
                }
                continue;
            }
            match ch {
                '"' => {
                    in_str = true;
                    cur.push(ch);
                }
                ',' => {
                    out.push(cur.trim().to_string());
                    cur.clear();
                }
                _ => cur.push(ch),
            }
        }
        if !cur.trim().is_empty() {
            out.push(cur.trim().to_string());
        }
        out.into_iter()
            .map(|a| {
                a.trim()
                    .trim_start_matches('"')
                    .trim_end_matches('"')
                    .to_string()
            })
            .collect()
    }

    /// 源码里所有 `tr("…")` / `tf("…")` 的字面量参数（跨行、尾逗号都算）
    fn literal_calls(root: &Path) -> BTreeSet<String> {
        let mut files = Vec::new();
        walk(root, &mut files);
        let mut found = BTreeSet::new();
        for f in files {
            match f.file_name().and_then(|s| s.to_str()) {
                // i18n.rs/table.rs = 元数据与数据表；本文件源码里有 `["tr(", "tf("]`
                Some("i18n.rs") | Some("i18n_audit.rs") | Some("table.rs") => continue,
                _ => {}
            }
            let txt = code_of(&f);
            let mut from = 0usize;
            while let Some(at) = txt[from..].find("tr(") {
                let abs = from + at;
                from = abs + 3;
                let prev = txt[..abs].chars().next_back();
                if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let rest = &txt[from..];
                let Some(q) = rest.find('"') else { continue };
                let head = rest[..q].trim();
                if !head.is_empty() && !head.chars().all(|c| c.is_whitespace() || c == ',') {
                    // tr( 之后到第一个引号之间还有别的实参 → 不是单键调用
                    continue;
                }
                if let Some(lit) = first_string_literal(&rest[q..]) {
                    found.insert(lit);
                }
            }
            // tf( 同口径
            let mut from = 0usize;
            while let Some(at) = txt[from..].find("tf(") {
                let abs = from + at;
                from = abs + 3;
                let prev = txt[..abs].chars().next_back();
                if prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                let rest = &txt[from..];
                let Some(q) = rest.find('"') else { continue };
                let head = rest[..q].trim();
                if !head.is_empty() && !head.chars().all(|c| c.is_whitespace() || c == ',') {
                    continue;
                }
                if let Some(lit) = first_string_literal(&rest[q..]) {
                    found.insert(lit);
                }
            }
        }
        found
    }

    fn first_string_literal(s: &str) -> Option<String> {
        let b = s.as_bytes();
        let start = b.first().copied()?;
        if start != b'"' {
            return None;
        }
        let mut out = String::new();
        let mut i = 1usize;
        while i < b.len() {
            match b[i] {
                b'\\' if i + 1 < b.len() => {
                    out.push('\\');
                    out.push(b[i + 1] as char);
                    i += 2;
                }
                b'"' => return Some(out),
                _ => {
                    let ch = s[i..].chars().next()?;
                    out.push(ch);
                    i += ch.len_utf8();
                }
            }
        }
        None
    }

    #[test]
    fn keys_and_translations_are_complete() {
        let root = src_root();
        let rows = table_rows(&read(&root.join("i18n/table.rs")));
        assert!(rows.len() > 300, "TABLE 解析异常，只读到 {} 行", rows.len());

        // 键唯一（撞键 = find 永远取第一条，另一条成死数据）
        let mut seen = std::collections::BTreeSet::new();
        let mut dup = Vec::new();
        for (k, _, _, _) in &rows {
            if !seen.insert(k.clone()) {
                dup.push(k.clone());
            }
        }
        assert!(dup.is_empty(), "TABLE 有重复键：{dup:?}");

        let keys: BTreeSet<&str> = rows.iter().map(|r| r.0.as_str()).collect();
        let calls = literal_calls(&root);

        // ① 调用点用的键必须在表里（否则界面显示裸键名）
        let unknown: Vec<&String> = calls
            .iter()
            .filter(|c| !keys.contains(c.as_str()))
            .collect();
        assert!(
            unknown.is_empty(),
            "⛔ {} 个调用点的键不在 TABLE 里（界面会显示裸键名）：{unknown:?}",
            unknown.len()
        );

        // ② 空译文挂账：BACKLOG 必须与「当前空译文的键」完全一致
        let untranslated: BTreeSet<&str> = rows
            .iter()
            .filter(|r| r.2.is_empty() || r.3.is_empty())
            .map(|r| r.0.as_str())
            .collect();
        let backlog: BTreeSet<&str> = BACKLOG.iter().copied().collect();

        let fixed: Vec<&&str> = backlog.difference(&untranslated).collect();
        assert!(
            fixed.is_empty(),
            "BACKLOG 有 {} 条已补齐译文，请从 BACKLOG 删掉：{fixed:?}",
            fixed.len()
        );
        let fresh: Vec<&&str> = untranslated.difference(&backlog).collect();
        assert!(
            fresh.is_empty(),
            "⛔ 有 {} 条表行繁/英为空但不在 BACKLOG 里（新文案忘了配译文，\
             或改了旧文案的翻译状态）：{fresh:?}",
            fresh.len()
        );

        // ③ 死键：只报告，不判定（常量/变量传参、预留文案都会误报）
        let dead: Vec<&str> = keys
            .iter()
            .copied()
            .filter(|k| !calls.contains(*k))
            .collect();
        eprintln!(
            "i18n 审计：{} 行 / {} 键，{} 个调用点键，未翻译 {} 条（BACKLOG {}），死键 {} 条",
            rows.len(),
            keys.len(),
            calls.len(),
            untranslated.len(),
            backlog.len(),
            dead.len()
        );
        if !dead.is_empty() {
            eprintln!("死键（表里有、无字面量调用点，常量/变量传参也会计入）：");
            for k in &dead {
                eprintln!("    {k}");
            }
        }
    }
}
