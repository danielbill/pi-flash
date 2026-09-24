import io

p = r'crates/app/src/main.rs'
s = io.open(p, encoding='utf-8').read()

if 'hovered_session' in s:
    print('already present')
else:
    # 1) fields
    a = '    menu_ix: usize,\n    epoch: u64,\n}'
    n = (
        '    menu_ix: usize,\n'
        '    /// sidebar row currently under the mouse (hover reveals actions)\n'
        '    hovered_session: Option<usize>,\n'
        '    /// open the rename dialog once the freshly-opened session reports state\n'
        '    pending_rename: bool,\n'
        '    epoch: u64,\n'
        '}'
    )
    assert a in s, 'fields'
    s = s.replace(a, n, 1)

    a = '            menu_ix: 0,\n    epoch: 1,\n        };'
    if a not in s:
        a = '            menu_ix: 0,\n            epoch: 1,\n        };'
    assert a in s, 'init'
    s = s.replace(a, '            menu_ix: 0,\n            hovered_session: None,\n            pending_rename: false,\n            epoch: 1,\n        };', 1)

    # 2) get_state handler: after refresh, open pending rename dialog
    a = (
        '                if command == "get_state" && success {\n'
        '                    if let Some(data) = &data {\n'
        '                        self.state = Some(SessionState::parse(data));\n'
        '                    }\n'
        '                }'
    )
    n = (
        '                if command == "get_state" && success {\n'
        '                    if let Some(data) = &data {\n'
        '                        self.state = Some(SessionState::parse(data));\n'
        '                    }\n'
        '                    if self.pending_rename {\n'
        '                        self.pending_rename = false;\n'
        '                        self.dialog = Some(Dialog::RenameSession {\n'
        '                            value: self\n'
        '                                .state\n'
        '                                .as_ref()\n'
        '                                .and_then(|s| s.session_name.clone())\n'
        '                                .unwrap_or_default(),\n'
        '                        });\n'
        '                    }\n'
        '                }'
    )
    assert a in s, 'get_state arm'
    s = s.replace(a, n, 1)

    # 3) open_session sets pending_rename when requested by the pencil
    a = (
        '    fn open_session(&mut self, path: PathBuf, cx: &mut Context<Self>) {\n'
        '        let cwd = self'
    )
    n = (
        '    /// Open a session; with `rename` the rename dialog pops after state arrives.\n'
        '    fn open_session_inner(&mut self, path: PathBuf, rename: bool, cx: &mut Context<Self>) {\n'
        '        let cwd = self'
    )
    assert a in s, 'open_session head'
    s = s.replace(a, n, 1)
    a = '        self.status = status_line(self.session.is_some(), "resuming");'
    n = '        self.pending_rename = rename;\n        self.status = status_line(self.session.is_some(), "resuming");'
    assert a in s
    s = s.replace(a, n, 1)

    # keep old call name working: wrapper
    a = '    /// Delete a stored session file (pi-web parity). The active session\'s'
    n = (
        '    fn open_session(&mut self, path: PathBuf, cx: &mut Context<Self>) {\n'
        '        self.open_session_inner(path, false, cx);\n'
        '    }\n'
        '\n'
        '    fn open_session_for_rename(&mut self, path: PathBuf, cx: &mut Context<Self>) {\n'
        '        self.pending_rename = true;\n'
        '        self.open_session_inner(path, true, cx);\n'
        '    }\n'
        '\n'
        '    /// Delete a stored session file (pi-web parity). The active session\'s'
    )
    assert a in s
    s = s.replace(a, n, 1)

    io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
    print('state ok')
