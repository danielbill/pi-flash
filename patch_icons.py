import io

p = r'crates/app/src/main.rs'
s = io.open(p, encoding='utf-8').read()
bs = chr(92)
NL = chr(10)

count = 0

def rep(old, new, tag):
    global s, count
    assert old in s, f'{tag} not found'
    s = s.replace(old, new, 1)
    count += 1

# 1) image chip (line-precise)
old = (
    '                        .child(SharedString::from(format!(' + NL +
    '                            "' + bs + 'u{1f5bc} {name} ' + bs + 'u{00d7}"' + NL +
    '                        )))' + NL +
    '                        .into_any_element()' + NL
)
new = (
    '                        .child(' + NL +
    '                            div()' + NL +
    '                                .flex()' + NL +
    '                                .items_center()' + NL +
    '                                .gap_1()' + NL +
    '                                .child(icon("image", 10, t.text_muted))' + NL +
    '                                .child(SharedString::from(name.clone()))' + NL +
    '                                .child(icon("x", 10, t.text_muted)),' + NL +
    '                        )' + NL +
    '                        .into_any_element()' + NL
)
rep(old, new, 'image chip')

# 2) thinking chevrons
old = (
    '                        .child(SharedString::from(if is_collapsed {' + NL +
    '                            "thinking ' + bs + 'u{25b8}".to_string()' + NL +
    '                        } else {' + NL +
    '                            "thinking ' + bs + 'u{25be}".to_string()' + NL +
    '                        })),' + NL
)
new = (
    '                        .child(if is_collapsed {' + NL +
    '                            icon("chevron-right", 10, t.text_dim)' + NL +
    '                        } else {' + NL +
    '                            icon("chevron-down", 10, t.text_dim)' + NL +
    '                        }),' + NL
)
rep(old, new, 'thinking chevrons')

# 3) branch label
rep('            format!("' + bs + 'u{2387} {}", self.branch).into()',
    '            self.branch.clone().into()', 'branch label')

# 4) branch box icon
old = (
    '                    .child(' + NL +
    '                        div()' + NL +
    '                            .text_color(rgb(t.text))' + NL +
    '                            .child(branch_label),' + NL +
    '                    )'
)
new = (
    '                    .child(' + NL +
    '                        div()' + NL +
    '                            .flex()' + NL +
    '                            .items_center()' + NL +
    '                            .gap_1()' + NL +
    '                            .child(icon("git-branch", 12, t.text))' + NL +
    '                            .child(' + NL +
    '                                div()' + NL +
    '                                    .text_color(rgb(t.text))' + NL +
    '                                    .child(branch_label),' + NL +
    '                            ),' + NL +
    '                    )'
)
rep(old, new, 'branch box')

# 5) search button
rep('                                    .child("' + bs + 'u{1f50d}"),',
    '                                    .child(icon("search", 12, t.text)),',
    'search btn')

# 6) branch chevron
rep('                            .child("' + bs + 'u{25be}"),',
    '                            .child(icon("chevron-down", 10, t.text_muted)),',
    'branch chevron')

# 7) session delete
rep('                                .child(if is_active { "' + bs + 'u{25cf}" } else { "' + bs + 'u{00d7}" }),',
    '                                .child(if is_active {' + NL +
    '                                    div()' + NL +
    '                                        .text_color(rgb(t.accent))' + NL +
    '                                        .child("' + bs + 'u{25cf}")' + NL +
    '                                        .into_any_element()' + NL +
    '                                } else {' + NL +
    '                                    icon("x", 12, t.text_muted)' + NL +
    '                                }),',
    'session delete')

# 8) explorer header
rep('                                    .child("' + bs + 'u{25be} 文件浏览器"),',
    '                                    .child(' + NL +
    '                                        div()' + NL +
    '                                            .flex()' + NL +
    '                                            .items_center()' + NL +
    '                                            .gap_1()' + NL +
    '                                            .child(icon("chevron-down", 10, t.text))' + NL +
    '                                            .child(SharedString::from("文件浏览器")),' + NL +
    '                                    ),',
    'explorer header')

# 9) explorer icon row
old = (
    '                                    .child("' + bs + 'u{1f5a5}")' + NL +
    '                                    .child("' + bs + 'u{1f50d}")' + NL +
    '                                    .child("' + bs + 'u{2191}")' + NL +
    '                                    .child("' + bs + 'u{21bb}"),'
)
new = (
    '                                    .child(icon("monitor", 12, t.text_muted))' + NL +
    '                                    .child(icon("search", 12, t.text_muted))' + NL +
    '                                    .child(icon("upload", 12, t.text_muted))' + NL +
    '                                    .child(icon("refresh", 12, t.text_muted)),'
)
rep(old, new, 'explorer icons')

# 11) bottom nav
rep('                            .child("' + bs + 'u{2699} 模型"),',
    '                            .child(' + NL +
    '                                div()' + NL +
    '                                    .flex()' + NL +
    '                                    .items_center()' + NL +
    '                                    .gap_1p5()' + NL +
    '                                    .child(icon("settings", 12, t.text_muted))' + NL +
    '                                    .child(SharedString::from("模型")),' + NL +
    '                            ),',
    'nav models')
rep('                            .child("' + bs + 'u{2637} 技能"),',
    '                            .child(' + NL +
    '                                div()' + NL +
    '                                    .flex()' + NL +
    '                                    .items_center()' + NL +
    '                                    .gap_1p5()' + NL +
    '                                    .child(icon("layers", 12, t.text_muted))' + NL +
    '                                    .child(SharedString::from("技能")),' + NL +
    '                            ),',
    'nav skills')
rep('                            .child("' + bs + 'u{2699} 设置"),',
    '                            .child(' + NL +
    '                                div()' + NL +
    '                                    .flex()' + NL +
    '                                    .items_center()' + NL +
    '                                    .gap_1p5()' + NL +
    '                                    .child(icon("settings", 12, t.text_muted))' + NL +
    '                                    .child(SharedString::from("设置")),' + NL +
    '                            ),',
    'nav settings')

# 12) pill fn + call sites
rep('fn pill(id: &\'static str, label: SharedString) -> gpui::AnyElement {',
    'fn pill(id: &\'static str, icon_name: &\'static str, label: SharedString) -> gpui::AnyElement {',
    'pill signature')
rep('        .child(label)\n        .into_any_element()\n}',
    '        .child(icon(icon_name, 12, T().text_muted))\n        .child(label)\n        .into_any_element()\n}',
    'pill body')
rep('pill("tb-sidebar", SharedString::from("' + bs + 'u{2630}"))',
    'pill("tb-sidebar", "panel-left", SharedString::from(""))',
    'pill sidebar')
rep('pill("tb-history", SharedString::from("' + bs + 'u{1f550} 完整历史"))',
    'pill("tb-history", "history", SharedString::from("完整历史"))',
    'pill history')
rep('pill("tb-system", SharedString::from("' + bs + 'u{1f4c4} 系统"))',
    'pill("tb-system", "file-text", SharedString::from("系统"))',
    'pill system')
rep('pill("tb-tools", SharedString::from("' + bs + 'u{1f527} 工具"))',
    'pill("tb-tools", "wrench", SharedString::from("工具"))',
    'pill tools')

# 13) title pill
rep('                            .child("' + bs + 'u{270e} 生成标题"),',
    '                                    .child(' + NL +
    '                                        div()' + NL +
    '                                            .flex()' + NL +
    '                                            .items_center()' + NL +
    '                                            .gap_1p5()' + NL +
    '                                            .child(icon("pencil", 12, t.text_muted))' + NL +
    '                                            .child(SharedString::from("生成标题")),' + NL +
    '                                    ),',
    'title pill')

# 14) export pill
rep('                            .child("' + bs + 'u{2913} 导出"),',
    '                                    .child(' + NL +
    '                                        div()' + NL +
    '                                            .flex()' + NL +
    '                                            .items_center()' + NL +
    '                                            .gap_1p5()' + NL +
    '                                            .child(icon("download", 12, t.text_muted))' + NL +
    '                                            .child(SharedString::from("导出")),' + NL +
    '                                    ),',
    'export pill')

# 15) attach image
rep('                                    .child("' + bs + 'u{1f5bc}")',
    '                                    .child(icon("image", 12, t.text_muted)),',
    'attach image')

# 16) model gear
rep('                                            .child("' + bs + 'u{2699}")\n                                            .child(model_label),',
    '                                            .child(icon("settings", 12, t.text_muted)),\n                                            .child(model_label),',
    'model gear')

# 17) thinking label
rep('                                            .child(format!("' + bs + 'u{1f4a1} {thinking_label}")),',
    '                                            .child(' + NL +
    '                                                div()' + NL +
    '                                                    .flex()' + NL +
    '                                                    .items_center()' + NL +
    '                                                    .gap_1()' + NL +
    '                                                    .child(icon("lightbulb", 12, t.text_muted))' + NL +
    '                                                    .child(SharedString::from(' + NL +
    '                                                        thinking_label.clone(),' + NL +
    '                                                    )),' + NL +
    '                                            ),',
    'thinking label')

# 18) scissors + volume
rep('                                    .child("' + bs + 'u{2702} 压缩")\n                                    .child("' + bs + 'u{1f50a}"),',
    '                                    .child(' + NL +
    '                                        div()' + NL +
    '                                            .flex()' + NL +
    '                                            .items_center()' + NL +
    '                                            .gap_1()' + NL +
    '                                            .child(icon("scissors", 12, t.text_muted))' + NL +
    '                                            .child(SharedString::from("压缩")),' + NL +
    '                                    )' + NL +
    '                                    .child(icon("volume", 12, t.text_muted)),',
    'scissors volume')

# 19) send button
rep('                                    .child("' + bs + 'u{2192} 发送"),',
    '                                    .child(' + NL +
    '                                        div()' + NL +
    '                                            .flex()' + NL +
    '                                            .items_center()' + NL +
    '                                            .gap_1p5()' + NL +
    '                                            .child(icon("send", 12, t.text))' + NL +
    '                                            .child(SharedString::from("发送")),' + NL +
    '                                    ),',
    'send button')

# 20) model dialog close x (first)
rep('                                            .child("' + bs + 'u{00d7}"),',
    '                                            .child(icon("x", 12, t.text_muted)),',
    'model dialog close')

# 21) file preview close x (second occurrence)
needle = '                                            .child("' + bs + 'u{00d7}"),'
if needle in s:
    s = s.replace(needle, '                                            .child(icon("x", 12, t.text_muted)),', 1)
    count += 1

io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print(f'{count} replacements ok')
