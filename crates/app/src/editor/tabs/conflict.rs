//! 外部改动冲突族（自 tabs.rs 外移）：磁盘回读、忽略外部改动、
//! 周期性外部变化检测。

use super::*;

impl Chat {
    /// 冲突 banner「重新加载」：磁盘为准，丢弃本地未保存修改。
    pub(crate) fn file_reload_from_disk(&mut self, path: &Path, cx: &mut Context<Self>) {
        let decoded = match std::fs::read(path).map(crate::editor::file_content::decode_text) {
            Ok(Ok(d)) => d,
            _ => {
                // 读不出来/二进制化：视同消失（沿用既有口径）
                self.set_status(crate::i18n::tr("conflict.deleted_on_disk").to_string(), cx);
                return;
            }
        };
        if let Some(ft) = self.ws.file_cache.get_mut(path) {
            ft.content = decoded.text;
            ft.encoding = decoded.encoding;
            ft.has_bom = decoded.has_bom;
            ft.dirty = false;
            ft.reload_pending = true;
            ft.conflict = None;
            ft.disk_sig = file_sig(path);
        }
        cx.notify();
    }

    /// 冲突 banner「忽略」：保留本地缓冲；以当前磁盘态为新基准，磁盘再变
    /// 才会再次提示。
    pub(crate) fn file_ignore_conflict(&mut self, path: &Path, cx: &mut Context<Self>) {
        if let Some(ft) = self.ws.file_cache.get_mut(path) {
            ft.conflict = None;
            ft.disk_sig = file_sig(path);
        }
        cx.notify();
    }

    /// 023 外部改动检测（对齐 Zed）：fs 监听泵的每个合批信号跑一遍。
    /// 无未保存修改 → 自动重载（reload_pending 由渲染帧灌进编辑器）；
    /// 有修改 → 标冲突，等用户在 banner 上裁决。自己的保存已把基准刷到
    /// 写后元数据，本轮信号比较为空转。
    pub(crate) fn check_external_file_changes(&mut self, cx: &mut Context<Self>) {
        self.ws.ext_probe.0 += 1;
        let mut changed = false;
        let paths: Vec<PathBuf> = self
            .ws
            .panel_tabs
            .iter()
            .filter_map(|t| match t {
                PanelTab::File(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        for path in paths {
            let Some(ft) = self.ws.file_cache.get_mut(&path) else {
                continue;
            };
            if ft.binary {
                continue; // 二进制占位 tab 无缓冲，磁盘变化不参与冲突/重载裁决
            }
            match std::fs::metadata(&path) {
                Err(_) => {
                    // 文件已从磁盘消失：标 Deleted（zed DiskState::Deleted 同构
                    // ——缓冲保留可继续编辑、tab 删除线、保存即重建；不弹横幅）。
                    // 已在 Deleted 态不重复置位（watcher 泵 150ms 一轮，避免
                    // 每轮空 notify）
                    if ft.conflict != Some(crate::FileConflict::Deleted) {
                        ft.conflict = Some(crate::FileConflict::Deleted);
                        changed = true;
                    }
                }
                Ok(md) => {
                    let sig = md.modified().ok().map(|m| (m, md.len()));
                    if sig.is_none() || sig == ft.disk_sig {
                        continue;
                    }
                    match std::fs::read(&path).map(crate::editor::file_content::decode_text) {
                        Ok(Ok(decoded)) => {
                            self.ws.ext_probe.1 += 1;
                            if !ft.dirty {
                                ft.content = decoded.text;
                                ft.encoding = decoded.encoding;
                                ft.has_bom = decoded.has_bom;
                                ft.reload_pending = true;
                                ft.conflict = None;
                            } else if ft.conflict != Some(crate::FileConflict::Changed) {
                                // Deleted→Changed 覆盖：文件被外部重建而本地
                                // 仍有未保存修改（zed：has_conflict 弹裁决）
                                ft.conflict = Some(crate::FileConflict::Changed);
                            }
                            ft.disk_sig = sig;
                            changed = true;
                        }
                        _ => {
                            // 二进制化/读不出来：视同消失（同样只置一次）
                            if ft.conflict != Some(crate::FileConflict::Deleted) {
                                ft.conflict = Some(crate::FileConflict::Deleted);
                                changed = true;
                            }
                        }
                    }
                }
            }
        }
        if changed {
            cx.notify();
        }
    }
}
