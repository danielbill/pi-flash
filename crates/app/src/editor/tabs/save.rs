//! 保存族（自 tabs.rs 外移）：显式保存 / 写盘回写（编码与换行保真）/
//! 防抖自动保存 / 活动页保存。

use super::*;

impl Chat {
    /// 保存文件 tab（Ctrl+S / 关闭确认「保存并关闭」共用）：编辑器在则取
    /// 编辑器值，写盘成功后刷新磁盘真值缓存与外部改动基准。
    pub(crate) fn save_file(&mut self, path: &Path, cx: &mut Context<Self>) {
        // 二进制占位 tab 无缓冲可存，直接短路（不弹「已保存」假提示）
        if self.ws.file_cache.get(path).is_some_and(|f| f.binary) {
            return;
        }
        let err = self.write_file_back(path, cx);
        if let Some(e) = err {
            self.set_status(
                format!("{}: {e}", crate::i18n::tr("file_view.save_failed")),
                cx,
            );
            return;
        }
        self.set_status(crate::i18n::tr("status.saved").to_string(), cx);
        cx.notify();
    }

    /// 写盘 + 刷新磁盘真值/基准/脏位。Err = 写盘失败的错误串（调用方决定
    /// 提示方式：手动保存 toast，自动保存静默）。
    pub(super) fn write_file_back(
        &mut self,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let Some(ft) = self.ws.file_cache.get(path) else {
            return None;
        };
        let text = match ft.editor.as_ref() {
            Some(ed) => ed.read(cx).value().to_string(),
            None => ft.content.clone(),
        };
        // 按 tab 记录的磁盘编码编码回盘（zed encode_text 同款：UTF-8 无 BOM
        // 原样直写；UTF-16/遗留编码/BOM 文件往返保持，不静默转 UTF-8）
        let bytes = crate::editor::file_content::encode_text(text.clone(), ft.encoding, ft.has_bom);
        // zed fs.save 同款：先建父目录再写——已删文件保存即重建（目录被
        // 一并删除时也能恢复）；目录健在时 create_dir_all 是空操作
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return Some(e.to_string());
            }
        }
        if let Err(e) = std::fs::write(path, &bytes) {
            return Some(e.to_string());
        }
        if let Some(ft) = self.ws.file_cache.get_mut(path) {
            ft.content = text;
            ft.dirty = false;
            ft.conflict = None;
            ft.disk_sig = file_sig(path);
        }
        cx.notify();
        None
    }

    /// 023 自动保存（Zed after-timeout 同构）：编辑停顿约 1 秒写盘。每次
    /// 脏变起一个定时器；静默期内的后续编辑各自再起定时器，先到的发现
    /// 「最后编辑 < 1s」就让位退出，最晚的那个落盘。冲突挂着不自动写
    /// （等用户在横幅裁决），写盘静默（不抢状态栏）。
    /// （2026-10-10 用户定夺：恒开，无开关——默认行为即唯一行为）
    pub(crate) fn autosave_later(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(AUTOSAVE_DEBOUNCE_MS))
                    .await;
                let proceed = this.update(cx, |c, _| {
                    c.ws.file_cache.get(&path).is_some_and(|f| {
                        f.dirty
                            && f.conflict.is_none()
                            && f.last_edit.is_some_and(|t| {
                                t.elapsed().as_millis() as u64 >= AUTOSAVE_DEBOUNCE_MS
                            })
                    })
                });
                if !proceed.unwrap_or(false) {
                    return; // 已保存/被丢弃/冲突/仍在输入（后续定时器接管）
                }
                let _ = this.update(cx, |c, cx| {
                    let _ = c.write_file_back(&path, cx);
                });
                return;
            }
        })
        .detach();
    }

    /// Ctrl+S（FileSave action，"Input" 上下文绑定）：保存当前文件 tab。
    pub(crate) fn save_active_file(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.active_file_path() {
            self.save_file(&path, cx);
        }
    }
}
