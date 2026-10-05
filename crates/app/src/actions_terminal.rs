//! Terminal create/close/restart/key input + PTY event pump.

//! Split out of main.rs for the file-size budget. Child module of the
//! crate root: Chat's root-private fields stay accessible here.

use crate::*;

impl Chat {
    /// 打开（或聚焦）一个终端；`cwd_override` 为 psp 项目菜单定向打开时
    /// 的项目目录，None = 当前工作区。v54: 终端在内容区 tab 显示。
    pub(crate) fn open_terminal(
        &mut self,
        cwd_override: Option<PathBuf>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let term_cwd = cwd_override.unwrap_or_else(|| self.cwd.clone());
        if let Some(ix) = self.terminals.iter().position(|t| same_path(&t.cwd, &term_cwd)) {
            self.active_terminal = Some(ix);
            if let Some(tix) = self.panel_tabs.iter().position(|tab| matches!(tab, PanelTab::Term(id) if *id == self.terminals[ix].id)) {
                self.active_panel_tab = Some(tix);
            }
            self.set_content_view(ContentView::Term);
            let focus = self.terminals[ix].focus.clone();
            window.focus(&focus);
            cx.notify();
            return;
        }
        let Some(tx) = self.term_events.clone() else { return };
        let (cell_w, line_h) = terminal::measure_cell(window);
        self.term_seq += 1;
        let id = self.term_seq;
        let focus = cx.focus_handle();
        let proxy = terminal::Proxy { tab: id, tx };
        match terminal::spawn_terminal(id, term_cwd.clone(), cell_w, line_h, focus, proxy) {
            Ok(tab) => {
                self.terminals.push(tab);
                self.active_terminal = Some(self.terminals.len() - 1);
                self.panel_tabs.push(PanelTab::Term(id));
                self.active_panel_tab = Some(self.panel_tabs.len() - 1);
                self.set_content_view(ContentView::Term);
                let focus = self.terminals[self.terminals.len() - 1].focus.clone();
                window.focus(&focus);
                cx.notify();
            }
            Err(e) => {
                self.set_status(format!("terminal spawn failed: {e}"), cx);
                cx.notify();
            }
        }
    }

    /// Close a tab: shutdown the PTY, drop state (DELETE /api/terminal/:id).
    pub(crate) fn restart_terminal(&mut self, ix: usize, cx: &mut Context<Self>) {
        let Some(tx) = self.term_events.clone() else { return };
        let Some(old) = self.terminals.get(ix) else { return };
        if old.status == TermStatus::Ready {
            let _ = old.pty.send(alacritty_terminal::event_loop::Msg::Shutdown);
        }
        let (cols, rows, cell_w, line_h) = (old.cols, old.rows, old.cell_w, old.line_h);
        let cwd = old.cwd.clone();
        self.term_seq += 1;
        let id = self.term_seq;
        let focus = cx.focus_handle();
        let proxy = terminal::Proxy { tab: id, tx };
        match terminal::spawn_terminal(id, cwd, cell_w, line_h, focus, proxy) {
            Ok(mut tab) => {
                tab.cols = cols;
                tab.rows = rows;
                self.terminals[ix] = tab;
                self.active_terminal = Some(ix);
            }
            Err(e) => {
                self.terminals[ix].status = TermStatus::Failed(e);
            }
        }
        cx.notify();
    }

    /// Active terminal index helper.
    pub(crate) fn active_term(&mut self) -> Option<&mut TerminalTab> {
        self.active_terminal
            .and_then(|ix| self.terminals.get_mut(ix))
    }

    /// Terminal keyboard input: copy/paste shortcuts first (pi-web
    /// attachCustomKeyEventHandler parity: Ctrl+C with selection copies and
    /// never sends ^C; Ctrl+V goes to the PTY, never the browser), then the
    /// keystroke → escape-sequence table.
    pub(crate) fn terminal_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        use alacritty_terminal::event_loop::Msg;
        let Some(tab) = self.active_term() else { return };
        let k = &ev.keystroke;
        let ctrl = k.modifiers.control;
        let shift = k.modifiers.shift;
        let mode = *tab.term.lock().mode();

        if ctrl && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) {
                let bytes = terminal::paste_bytes(&text, &mode);
                let _ = tab.pty.send(Msg::Input(bytes.into()));
            }
            cx.stop_propagation();
            return;
        }
        if ctrl && k.key == "c" && (shift || tab.selection.is_some()) {
            if let Some(sel) = tab.selection {
                let text = terminal::selection_text(&tab.term.lock(), sel);
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                tab.selection = None;
                tab.sel_anchor = None;
            }
            cx.stop_propagation();
            return;
        }
        if let Some(bytes) = terminal::keystroke_to_pty(k, &mode) {
            let _ = tab.pty.send(Msg::Input(bytes.into()));
            cx.stop_propagation();
        }
    }

    // -----------------------------------------------------------------------
    // models panel (pi-web ModelsConfig parity: enabledModels + API keys)
    // -----------------------------------------------------------------------

    /// `provider/modelId` refs of every available model, display order.
    /// Reads the shared catalog of the current project (models_by_cwd).
    pub(crate) fn mc_refs(&self) -> Vec<String> {
        self.models_for(&self.cwd)
            .iter()
            .map(|m| format!("{}/{}", m.provider, m.id))
            .collect()
    }
}
