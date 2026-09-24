import io

p = r'crates/app/src/main.rs'
s = io.open(p, encoding='utf-8').read()
n_changes = 0


def rep(old, new, tag):
    global s, n_changes
    assert old in s, f'{tag} not found'
    s = s.replace(old, new, 1)
    n_changes += 1


# ---- 1) title: single line, ellipsis, wraps in nowrap (pi-web sidebar) ----
a = (
    '                                .child(\n'
    '                                    div()\n'
    '                                        .text_xs()\n'
    '                                        .font_weight(if is_active {\n'
    '                                            gpui::FontWeight::MEDIUM\n'
    '                                        } else {\n'
    '                                            gpui::FontWeight::NORMAL\n'
    '                                        })\n'
    '                                        .text_color(rgb(t.text))\n'
    '                                        .child(preview),\n'
    '                                )\n'
    '                                .child(\n'
    '                                    div()\n'
    '                                        .text_xs()\n'
    '                                        .text_color(rgb(t.text_dim))\n'
    '                                        .child(meta),\n'
    '                                ),'
)
n = (
    '                                .child(\n'
    '                                    div()\n'
    '                                        .w_full()\n'
    '                                        .overflow_hidden()\n'
    '                                        .whitespace_nowrap()\n'
    '                                        .text_ellipsis()\n'
    '                                        .text_xs()\n'
    '                                        .font_weight(if is_active {\n'
    '                                            gpui::FontWeight::MEDIUM\n'
    '                                        } else {\n'
    '                                            gpui::FontWeight::NORMAL\n'
    '                                        })\n'
    '                                        .text_color(rgb(t.text))\n'
    '                                        .child(preview),\n'
    '                                )\n'
    '                                .child(\n'
    '                                    div()\n'
    '                                        .flex()\n'
    '                                        .items_center()\n'
    '                                        .gap_2()\n'
    '                                        .text_xs()\n'
    '                                        .text_color(rgb(t.text_dim))\n'
    '                                        .child(if is_active && streaming {\n'
    '                                            icon("loader", 11., t.accent)\n'
    '                                        } else {\n'
    '                                            SharedString::from(time_text.clone())\n'
    '                                                .into_any_element()\n'
    '                                        })\n'
    '                                        .child(SharedString::from(format!(\n'
    '                                            "{} 条消息",\n'
    '                                            info.message_count\n'
    '                                        ))),\n'
    '                                ),'
)
rep(a, n, 'title/meta rows')

# ---- 2) locals: time_text + streaming + hover handling; actions on hover ----
a = (
    '                    let weak = weak_for_sessions.clone();\n'
    '                    let weak_del = weak_for_sessions.clone();\n'
    '                    let weak_ren = weak_for_sessions.clone();\n'
    '                    let p_del = info.path.clone();\n'
    '                    div()\n'
    '                        .w_full()\n'
    '                        .flex()\n'
    '                        .items_start()'
)
n = (
    '                    let weak = weak_for_sessions.clone();\n'
    '                    let weak_del = weak_for_sessions.clone();\n'
    '                    let weak_ren = weak_for_sessions.clone();\n'
    '                    let weak_hover = weak_for_sessions.clone();\n'
    '                    let p_del = info.path.clone();\n'
    '                    let time_text = time_ago(info.modified);\n'
    '                    let streaming = is_active\n'
    '                        && chat\n'
    '                            .state\n'
    '                            .as_ref()\n'
    '                            .is_some_and(|s| s.is_streaming);\n'
    '                    let hovered = chat.hovered_session == Some(ix);\n'
    '                    div()\n'
    '                        .w_full()\n'
    '                        .flex()\n'
    '                        .items_start()\n'
    '                        .on_hover(move |hovered, _, cx| {\n'
    '                            let h = *hovered;\n'
    '                            let _ = weak_hover.update(cx, |c, cx| {\n'
    '                                c.hovered_session = if h { Some(ix) } else { None };\n'
    '                                cx.notify();\n'
    '                            });\n'
    '                        })'
)
rep(a, n, 'row locals + on_hover')

# ---- 3) trailing buttons render only on hover ----
a = '                        .child(if is_active {\n                            div()\n                                .id(SharedString::from(format!("ren-{ix}")))'
n = '                        .children(if hovered {\n                            Some({\n                            div()\n                                .id(SharedString::from(format!("ren-{ix}")))'
rep(a, n, 'hover render head')

a = (
    '                                .child(icon("x", 12., t.text_dim))\n'
    '                                .into_any_element()\n'
    '                        }),\n'
    '                        .into_any_element()\n'
    '                })\n'
    '                .flex_1()\n'
    '                .min_h_0(),'
)
n = (
    '                                .child(icon("x", 12., t.text_dim))\n'
    '                                .into_any_element()\n'
    '                            })\n'
    '                        } else {\n'
    '                            None\n'
    '                        })\n'
    '                        .into_any_element()\n'
    '                })\n'
    '                .flex_1()\n'
    '                .min_h_0(),'
)
rep(a, n, 'hover render tail')

# ---- 4) pencil click: open session then rename; delete stays ----
a = '''                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let _ = weak_ren.update(cx, |c, cx| {
                                        c.dialog = Some(Dialog::RenameSession {
                                            value: c
                                                .state
                                                .as_ref()
                                                .and_then(|s| s.session_name.clone())
                                                .unwrap_or_default(),
                                        });
                                        cx.notify();
                                    });
                                })
                                .child("''' + bs + '''u{270e}"),'''
n = '''                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                    let p = path.clone();
                                    let _ = weak_ren.update(cx, |c, cx| {
                                        c.open_session(p, true, cx);
                                    });
                                })
                                .child(icon("pencil", 11., t.accent)),'''
rep(a, n, 'pencil action')

io.open(p, 'w', encoding='utf-8', newline='\n').write(s)
print(f'{n_changes} ok')
