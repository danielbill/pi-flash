import io

p = r'crates/app/src/main.rs'
s = io.open(p, encoding='utf-8').read()

# A) struct fields (after menu_ix: usize,)
if 'hovered_session' not in s:
    a = '    menu_ix: usize,\n    epoch: u64,\n}'
    assert a in s, 'fields'
    n = (
        '    menu_ix: usize,\n'
        '    /// sidebar row under the mouse (hover reveals actions)\n'
        '    hovered_session: Option<usize>,\n'
        '    /// open rename dialog once the freshly opened session reports state\n'
        '    pending_rename: bool,\n'
        '    epoch: u64,\n'
        '}'
    )
    s = s.replace(a, n, 1)
if 'pending_rename: false' not in s:
    a = '            menu_ix: 0,\n'
    assert a in s, 'init menu_ix'
    n = (
        '            menu_ix: 0,\n'
        '            hovered_session: None,\n'
        '            pending_rename: false,\n'
    )
    s = s.replace(a, n, 1)

# B) fix open_session signature (rename param)
a = '    fn open_session(&mut self, path: PathBuf, cx: &mut Context<Self>) {'
n = '    fn open_session(&mut self, path: PathBuf, rename: bool, cx: &mut Context<Self>) {'
assert a in s, 'open_session sig'
s = s.replace(a, n, 1)

# C) get_state arm: pending rename dialog pop (insert before the closing brace
#    of the get_state arm — after the state parse block)
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

# D) call sites of open_session get rename flag
a = 'let _ = weak.update(cx, |c, cx| c.open_session(p, cx));'
assert a in s, 'row open call'
s = s.replace(
    a,
    'let _ = weak.update(cx, |c, cx| c.open_session(p, false, cx));',
    1,
)

io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print('open_session fixed + fields + get_state')
