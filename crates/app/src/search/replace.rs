//! 搜索替换：全部替换 / 逐个替换段。

use gpui::Context;

use super::*;

use super::{build_query, replace_in_text, split_globs};

impl Chat {
    // -------------------------------------------------------------------
    // 替换
    // -------------------------------------------------------------------

    /// 替换全部命中（Zed ReplaceAll）。活跃文件有未保存修改时跳过并计数。
    pub(crate) fn search_replace_all(&mut self, cx: &mut Context<Self>) {
        self.search_replace(cx, false);
    }

    /// 替换下一处（Zed ReplaceNext）：只动当前活跃命中所在文件的第一个命中，
    /// 然后重搜并推进选中。
    pub(crate) fn search_replace_next(&mut self, cx: &mut Context<Self>) {
        self.search_replace(cx, true);
    }

    fn search_replace(&mut self, cx: &mut Context<Self>, next_only: bool) {
        let text = self.ws.search.query_text.clone();
        let replacement = self.ws.search.replace_text.clone();
        if text.is_empty() {
            return;
        }
        let include = split_globs(
            self.ws
                .search
                .include
                .as_ref()
                .map(|e| e.read(cx).value().to_string())
                .unwrap_or_default(),
        );
        let exclude = split_globs(
            self.ws
                .search
                .exclude
                .as_ref()
                .map(|e| e.read(cx).value().to_string())
                .unwrap_or_default(),
        );
        let Ok(q) = build_query(&text, self.ws.search.opts, include, exclude) else {
            return;
        };
        let q = q.with_replacement(replacement.clone());
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        // 待改文件：ReplaceAll = 每个内容命中文件；ReplaceNext = 活跃命中所在文件
        let active_block: Option<usize> = if next_only {
            let active = self.ws.search.active;
            let mut found = None;
            for (block_ix, block) in self.ws.search.blocks.iter().enumerate() {
                for segment in &block.segments {
                    for line in segment {
                        if line.ordinal.is_some() && line.ordinal == active {
                            found = Some(block_ix);
                        }
                    }
                }
            }
            found
        } else {
            None
        };
        let mut files: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (block_ix, block) in self.ws.search.blocks.iter().enumerate() {
            if block.path_only {
                continue;
            }
            if let Some(target) = active_block
                && block_ix != target
            {
                continue;
            }
            if seen.insert(block.rel.clone()) {
                files.push(block.rel.clone());
            }
        }
        if files.is_empty() {
            self.ws.search.notice = Some((tr("search.replace_none").to_string(), true));
            cx.notify();
            return;
        }
        let mut changed_files = 0usize;
        let mut changed_hits = 0usize;
        let mut skipped_dirty = 0usize;
        for rel in &files {
            let abs = cwd.join(rel);
            // 有未保存修改的文件不动（写盘会触发冲突横幅，改动归属不明）
            if self
                .ws
                .file_cache
                .get(&abs)
                .is_some_and(|f| f.dirty || f.conflict.is_some())
            {
                skipped_dirty += 1;
                continue;
            }
            let Ok(bytes) = std::fs::read(&abs) else {
                continue;
            };
            let Ok(decoded) = crate::editor::file_content::decode_text(bytes) else {
                continue;
            };
            let src = decoded.text;
            let (out, hits) = replace_in_text(&q, &src, next_only);
            if hits == 0 {
                continue;
            }
            let encoded = crate::editor::file_content::encode_text(
                out.clone(),
                decoded.encoding,
                decoded.has_bom,
            );
            if std::fs::write(&abs, encoded).is_err() {
                continue;
            }
            changed_files += 1;
            changed_hits += hits;
            // 已开 tab 同步（内容 + 磁盘基准），避免 watcher 把它判成外部改动；
            // 编辑器值由渲染帧经 reload_pending 灌入（set_value 要 &mut Window）
            if let Some(ft) = self.ws.file_cache.get_mut(&abs) {
                ft.content = out;
                ft.conflict = None;
                ft.dirty = false;
                ft.disk_sig = crate::services::workspace::file_sig(&abs);
                if ft.editor.is_some() {
                    ft.reload_pending = true;
                }
            }
            if next_only {
                break;
            }
        }
        let mut msg = crate::i18n::tf(
            "search.replaced",
            &[
                ("files", changed_files.to_string()),
                ("hits", changed_hits.to_string()),
            ],
        );
        if skipped_dirty > 0 {
            msg.push_str(&crate::i18n::tf(
                "search.replace_skipped_dirty",
                &[("n", skipped_dirty.to_string())],
            ));
        }
        let failed = changed_files == 0;
        self.ws.search.notice = Some((msg, failed));
        let advance = next_only && !failed;
        self.search_start(cx);
        if advance {
            self.search_step(true, cx);
        }
        cx.notify();
    }
}
