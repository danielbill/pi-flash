//! 内置终端的 Chat 侧接线（053）：开终端 tab / spawn / 复制粘贴 /
//! 键盘派发 / on_term_event 事件泵。Element 绘制在 [`super::element`]。

use super::*;

impl Chat {
    /// 打开一个终端；`cwd_override` 为 psp 项目菜单定向打开时
    /// 的项目目录，None = 当前工作区。v54: 终端在内容区 tab 显示。
    ///
    /// 不按 cwd 复用：每次调用都新开一个终端 tab（同一项目可开任意
    /// 多个，tab 标题按目录名显示、允许重名），不跳回已有终端。
    pub(crate) fn open_terminal(
        &mut self,
        cwd_override: Option<PathBuf>,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        // 无项目态且无显式目录：没有可落终端的工作区，不开（防进程 cwd 泄入）
        let term_cwd = match cwd_override {
            Some(c) => c,
            None => match self.ws.cwd.clone() {
                Some(c) => c,
                None => return,
            },
        };
        // 生 tab（PTY + alacritty 事件泵），不激活不聚焦——启动 tab 恢复
        //（pi-flash-weia）共用这条路，回放后按存档统一复位激活位
        if !self.spawn_terminal_tab(term_cwd, window, cx) {
            cx.notify();
            return;
        }
        let tix = self.ws.panel_tabs.len() - 1;
        self.activate_panel_tab(tix, cx);
        self.set_content_view(ContentView::Term);
        let focus = self.ws.terminals[self.ws.terminals.len() - 1].focus.clone();
        window.focus(&focus);
        cx.notify();
    }

    /// 终端 tab 装载核心：spawn PTY + 入 terminals + 带尾挂 `PanelTab::Term`
    ///（open_terminal 与启动恢复共用；**不激活、不切视图、不聚焦**，调用
    /// 方自行接续）。失败落状态条返回 false。
    pub(crate) fn spawn_terminal_tab(
        &mut self,
        term_cwd: PathBuf,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(tx) = self.ws.term_events.clone() else {
            return false;
        };
        let (cell_w, line_h) = terminal::measure_cell(window);
        let font_size = terminal::font_size();
        self.ws.term_seq += 1;
        let id = self.ws.term_seq;
        let focus = cx.focus_handle();
        let proxy = terminal::Proxy { tab: id, tx };
        match terminal::spawn_terminal(id, term_cwd, cell_w, line_h, font_size, focus, proxy, None)
        {
            Ok(tab) => {
                self.ws.terminals.push(tab);
                self.ws.active_terminal = Some(self.ws.terminals.len() - 1);
                self.ws.panel_tabs.push(PanelTab::Term(id));
                true
            }
            Err(e) => {
                self.set_status(format!("terminal spawn failed: {e}"), cx);
                false
            }
        }
    }

    /// 021 文件树右键【执行】：脚本进新终端 tab——`cmd /k cd /d <目录> &&
    /// <命令>`，脚本退出留提示符，交互/续敲都行。PTY working_directory 落
    /// 文件目录；命令里的 cd 另一半是 cmd 引号规则护盾（script_exec 模块头）。
    pub(crate) fn open_script_terminal(
        &mut self,
        file: PathBuf,
        plan: crate::services::script_exec::ExecPlan,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dir) = file.parent().map(|p| p.to_path_buf()) else {
            return;
        };
        let Some(tx) = self.ws.term_events.clone() else {
            return;
        };
        let (cell_w, line_h) = terminal::measure_cell(window);
        let font_size = terminal::font_size();
        self.ws.term_seq += 1;
        let id = self.ws.term_seq;
        let focus = cx.focus_handle();
        let proxy = terminal::Proxy { tab: id, tx };
        let shell = Shell::new(
            std::env::var("ComSpec").unwrap_or_else(|_| "cmd.exe".to_string()),
            vec!["/k".into(), plan.command],
        );
        match terminal::spawn_terminal(
            id,
            dir,
            cell_w,
            line_h,
            font_size,
            focus,
            proxy,
            Some(shell),
        ) {
            Ok(tab) => {
                self.ws.terminals.push(tab);
                self.ws.active_terminal = Some(self.ws.terminals.len() - 1);
                self.ws.panel_tabs.push(PanelTab::Term(id));
                let tix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(tix, cx);
                self.set_content_view(ContentView::Term);
                let focus = self.ws.terminals[self.ws.terminals.len() - 1].focus.clone();
                window.focus(&focus);
                cx.notify();
            }
            Err(e) => {
                self.set_status(format!("terminal spawn failed: {e}"), cx);
                cx.notify();
            }
        }
    }

    /// 内容区**当前显示**的终端在 `terminals` 里的下标（None = 激活的
    /// panel tab 不是终端）。`term_view` 就是按 `active_panel_tab` 渲染的，
    /// 所以这条是「渲染面」的唯一口径。
    pub(crate) fn shown_terminal_index(&self) -> Option<usize> {
        let id = match self
            .ws
            .active_panel_tab
            .and_then(|ix| self.ws.panel_tabs.get(ix))?
        {
            PanelTab::Term(id) => *id,
            _ => return None,
        };
        self.ws.terminals.iter().position(|t| t.id == id)
    }

    /// 当前显示终端的焦点句柄（渲染帧焦点仲裁用）。
    pub(crate) fn shown_terminal_focus(&self) -> Option<gpui::FocusHandle> {
        self.shown_terminal_index()
            .and_then(|ix| self.ws.terminals.get(ix))
            .map(|t| t.focus.clone())
    }

    /// 终端 tab 的选区删除（Backspace/Delete，以及「粘贴替换」先删选区）：
    /// 按 [`deletion_plan`] 算出的键数给 shell 发「左移 + 前向删除」。返回
    /// 是否真的删了（false = 这个选区删不了，调用方按原语义处理）。
    pub(crate) fn term_delete_selection(&mut self, tab_id: usize, cx: &mut Context<Self>) -> bool {
        use alacritty_terminal::event_loop::Msg;
        let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id) else {
            return false;
        };
        let Some(sel) = tab.selection else {
            return false;
        };
        let Some((left, del)) = deletion_plan(&tab.term.lock(), sel) else {
            return false;
        };
        let mode = *tab.term.lock().mode();
        let left_seq = special_key("left", 1, &mode).unwrap_or_else(|| b"[D".to_vec());
        let del_seq = special_key("delete", 1, &mode).unwrap_or_else(|| b"[3~".to_vec());
        let mut bytes = Vec::with_capacity((left + del) * 4);
        for _ in 0..left {
            bytes.extend_from_slice(&left_seq);
        }
        for _ in 0..del {
            bytes.extend_from_slice(&del_seq);
        }
        let _ = tab.pty.send(Msg::Input(bytes.into()));
        tab.selection = None;
        tab.sel_anchor = None;
        cx.notify();
        true
    }

    /// 该终端 tab 的选区文本 → 系统剪贴板。返回是否真的复制了东西
    /// （无选区 = false，调用方据此决定 Ctrl+C 是否回落为 ^C）。
    /// 选区**不**清掉（xterm/浏览器语义：复制后还能看到选中的东西）。
    pub(crate) fn term_copy(&mut self, tab_id: usize, cx: &mut Context<Self>) -> bool {
        let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id) else {
            return false;
        };
        let Some(sel) = tab.selection else {
            return false;
        };
        let text = terminal::selection_text(&tab.term.lock(), sel);
        if text.is_empty() {
            return false;
        }
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        true
    }

    /// 系统剪贴板文本 → 该终端 PTY（换行归一 + bracketed paste 交给
    /// [`terminal::paste_bytes`]）。返回是否读到了剪贴板文本。
    pub(crate) fn term_paste(&mut self, tab_id: usize, cx: &mut Context<Self>) -> bool {
        use alacritty_terminal::event_loop::Msg;
        let Some(text) = cx.read_from_clipboard().and_then(|i| i.text()) else {
            return false;
        };
        // 有选区 = 粘贴替换：先删选区（删不掉就只清选区），再写入
        let had_sel = self
            .ws
            .terminals
            .iter()
            .find(|t| t.id == tab_id)
            .is_some_and(|t| t.selection.is_some());
        if had_sel && !self.term_delete_selection(tab_id, cx) {
            if let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id) {
                tab.selection = None;
                tab.sel_anchor = None;
                cx.notify();
            }
        }
        let Some(tab) = self.ws.terminals.iter().find(|t| t.id == tab_id) else {
            return false;
        };
        let mode = *tab.term.lock().mode();
        let bytes = terminal::paste_bytes(&text, &mode);
        let _ = tab.pty.send(Msg::Input(bytes.into()));
        true
    }

    /// 全选：可见屏 + 回滚缓冲（选区坐标与 [`terminal::selection_text`]
    /// 的钳位口径一致，-history..screen）。
    pub(crate) fn term_select_all(&mut self, tab_id: usize, cx: &mut Context<Self>) {
        let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        let (screen, total, cols) = {
            let term = tab.term.lock();
            (
                term.grid().screen_lines() as i32,
                term.grid().total_lines() as i32,
                term.grid().columns(),
            )
        };
        let min_line = -(total - screen).max(0);
        tab.sel_anchor = None;
        tab.selection = Some((
            SelPt {
                line: min_line,
                col: 0,
            },
            SelPt {
                line: screen - 1,
                col: cols.saturating_sub(1),
            },
        ));
        cx.notify();
    }

    /// Terminal keyboard input: clipboard shortcuts first (pi-web
    /// attachCustomKeyEventHandler parity: Ctrl+C with selection copies and
    /// never sends ^C; Ctrl+V goes to the PTY, never the browser), plus the
    /// Windows 习惯键（Ctrl+Insert / Shift+Insert / Ctrl+Shift+C / Ctrl+Shift+A），
    /// then the keystroke → escape-sequence table. 目标恒为**当前显示**的终端。
    pub(crate) fn terminal_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        use alacritty_terminal::event_loop::Msg;
        let Some(tix) = self.shown_terminal_index().or(self.ws.active_terminal) else {
            return;
        };
        let Some(tab) = self.ws.terminals.get(tix) else {
            return;
        };
        let tab_id = tab.id;
        let k = &ev.keystroke;
        let ctrl = k.modifiers.control;
        let shift = k.modifiers.shift;
        // 右键菜单开着时 ESC 先收菜单（否则这一下会变成 PTY 的 ^[）
        if k.key == "escape" && self.dlg.term_menu.is_some() {
            self.dlg.term_menu = None;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let mode = *tab.term.lock().mode();

        // 有选区时 Backspace/Delete = 删掉选中的内容（GUI 直觉）。删不了
        // （跨屏幕行/在滚回区/alt screen）就只把选区清掉，键照旧交给 shell。
        if k.key == "backspace" || k.key == "delete" {
            if self.term_delete_selection(tab_id, cx) {
                cx.stop_propagation();
                return;
            }
            if let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id)
                && tab.selection.take().is_some()
            {
                tab.sel_anchor = None;
                cx.notify();
            }
        }

        // 粘贴：Ctrl+V（pi-web parity）/ Shift+Insert（Windows 习惯）
        if (ctrl && k.key == "v") || (shift && k.key == "insert") {
            self.term_paste(tab_id, cx);
            cx.stop_propagation();
            return;
        }
        // 复制：Ctrl+C 有选区才算复制（无选区落到下面的 ^C 中断）；
        //       Ctrl+Insert / Ctrl+Shift+C 即使无选区也吞掉（不把 ^C 送进 PTY）
        if ctrl && (k.key == "c" || k.key == "insert") {
            let copied = self.term_copy(tab_id, cx);
            if copied || shift || k.key == "insert" {
                cx.stop_propagation();
                return;
            }
        }
        // 全选（Windows Terminal 同键：Ctrl+Shift+A）
        if ctrl && shift && k.key == "a" {
            self.term_select_all(tab_id, cx);
            cx.stop_propagation();
            return;
        }
        let Some(tab) = self.ws.terminals.get(tix) else {
            return;
        };
        // 裸 `exit` 拦截：内嵌终端里 shell 退出只会留下一块「Process
        // exited」死面，没有再进去的路。Enter 时看一眼光标行，命令就是
        // `exit` 就当作用户要关这个终端——直接关掉 tab（同点 tab 的 ×，
        // PTY Shutdown 一并走）。`exit 0` 等带参数的不算，照常执行（出
        // 横幅不崩）；全屏应用不掺和。
        if k.key == "enter" && !mode.contains(TermMode::ALT_SCREEN) {
            let bare_exit = is_bare_exit(&tab.term.lock());
            let tab_id = tab.id;
            if bare_exit
                && let Some(ix) = self
                    .ws
                    .panel_tabs
                    .iter()
                    .position(|t| matches!(t, crate::PanelTab::Term(id) if *id == tab_id))
            {
                self.close_tab_at(ix, cx);
                cx.stop_propagation();
                return;
            }
        }
        if let Some(bytes) = terminal::keystroke_to_pty(k, &mode) {
            let _ = tab.pty.send(Msg::Input(bytes.into()));
            cx.stop_propagation();
        }
    }
}

impl Chat {
    pub(crate) fn on_term_event(
        &mut self,
        tab_id: usize,
        event: alacritty_terminal::event::Event,
        cx: &mut Context<Self>,
    ) {
        use alacritty_terminal::event_loop::Msg;
        let Some(tab) = self.ws.terminals.iter_mut().find(|t| t.id == tab_id) else {
            return;
        };
        match event {
            alacritty_terminal::event::Event::Wakeup => {
                cx.notify();
            }
            alacritty_terminal::event::Event::PtyWrite(s) => {
                let _ = tab.pty.send(Msg::Input(s.into_bytes().into()));
            }
            alacritty_terminal::event::Event::ClipboardStore(_, text) => {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
            alacritty_terminal::event::Event::ClipboardLoad(_, fmt) => {
                let text = cx
                    .read_from_clipboard()
                    .and_then(|i| i.text())
                    .unwrap_or_default();
                let _ = tab.pty.send(Msg::Input(fmt(&text).into_bytes().into()));
            }
            alacritty_terminal::event::Event::ColorRequest(i, fmt) => {
                let _ = tab
                    .pty
                    .send(Msg::Input(fmt(terminal::term_rgb(i)).into_bytes().into()));
            }
            alacritty_terminal::event::Event::TextAreaSizeRequest(fmt) => {
                let ws = alacritty_terminal::event::WindowSize {
                    num_cols: tab.cols as u16,
                    num_lines: tab.rows as u16,
                    cell_width: tab.cell_w as u16,
                    cell_height: tab.line_h as u16,
                };
                let _ = tab.pty.send(Msg::Input(fmt(ws).into_bytes().into()));
            }
            alacritty_terminal::event::Event::ChildExit(status) => {
                tab.status = TermStatus::Exited(status.code());
                cx.notify();
            }
            alacritty_terminal::event::Event::Exit => {
                if tab.status == TermStatus::Ready {
                    tab.status = TermStatus::Exited(None);
                }
                cx.notify();
            }
            _ => {}
        }
    }
}
