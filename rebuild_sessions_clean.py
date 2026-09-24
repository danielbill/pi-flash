import io

p = r'crates/app/src/main.rs'
lines = io.open(p, encoding='utf-8').read().splitlines(keepends=True)

# Region boundaries:
# start: the `.child(` that opens the sessions list (line with list( follows)
start = next(i for i, l in enumerate(lines) if l.strip() == '.child(' and 'sessions_list' in lines[i + 1])
# end: the `),` that closes it — the first line after start whose strip() == '),'
# AND whose line index >= start (the list block's closing)
# Walk with depth to find it precisely.
depth = 0
end = None
for j in range(start, len(lines)):
    l = lines[j]
    depth += l.count('(') - l.count(')')
    if depth == 0 and j > start:
        end = j
        break
assert end is not None, 'no closing found'

bs = chr(92)
block = (
"            .child(\n"
"                list(self.sessions_list.clone(), move |ix, _window, cx| {\n"
"                    let chat = sessions_entity.read(cx);\n"
"                    let Some(info) = chat.sessions.get(ix) else {\n"
"                        return div().into_any_element();\n"
"                    };\n"
"                    let is_active =\n"
"                        chat.active_session_file.as_deref() == Some(info.path.as_path());\n"
"                    let path = info.path.clone();\n"
"                    let preview: SharedString = if info.preview.is_empty() {\n"
"                        \"(empty)\".into()\n"
"                    } else {\n"
"                        info.preview.clone().into()\n"
"                    };\n"
"                    let meta: SharedString = format!(\n"
"                        \"{} {} 条消息\",\n"
"                        time_ago(info.modified),\n"
"                        info.message_count\n"
"                    )\n"
"                    .into();\n"
"                    let time_text = time_ago(info.modified);\n"
"                    let streaming = is_active\n"
"                        && chat\n"
"                            .state\n"
"                            .as_ref()\n"
"                            .is_some_and(|s| s.is_streaming);\n"
"                    let hovered = chat.hovered_session == Some(ix);\n"
"                    let weak = weak_for_sessions.clone();\n"
"                    let weak_del = weak_for_sessions.clone();\n"
"                    let weak_ren = weak_for_sessions.clone();\n"
"                    let weak_hover = weak_for_sessions.clone();\n"
"                    let p_del = info.path.clone();\n"
"                    div()\n"
"                        .w_full()\n"
"                        .flex()\n"
"                        .items_start()\n"
"                        .on_hover(move |hovered, _, cx| {\n"
"                            let h = *hovered;\n"
"                            let _ = weak_hover.update(cx, |c, cx| {\n"
"                                c.hovered_session = if h { Some(ix) } else { None };\n"
"                                cx.notify();\n"
"                            });\n"
"                        })\n"
"                        .when(is_active, |d| {\n"
"                            d.bg(rgb(t.bg_selected))\n"
"                                .border_l_2()\n"
"                                .border_color(rgb(t.accent))\n"
"                        })\n"
"                        .when(!is_active, |d| d.border_l_2().border_color(rgb(t.bg)))\n"
"                        .child(\n"
"                            div()\n"
"                                .id(SharedString::from(format!(\"sess-{ix}\")))\n"
"                                .flex_1()\n"
"                                .min_w_0()\n"
"                                .pl_3p5()\n"
"                                .pr_2()\n"
"                                .py_2()\n"
"                                .cursor_pointer()\n"
"                                .hover(|s| s.bg(rgb(t.bg_hover)))\n"
"                                .on_mouse_down(MouseButton::Left, move |_, _, cx| {\n"
"                                    let p = path.clone();\n"
"                                    let _ = weak.update(cx, |c, cx| c.open_session(p, false, cx));\n"
"                                })\n"
"                                .flex()\n"
"                                .flex_col()\n"
"                                .gap_0p5()\n"
"                                .child(\n"
"                                    div()\n"
"                                        .w_full()\n"
"                                        .overflow_hidden()\n"
"                                        .whitespace_nowrap()\n"
"                                        .text_ellipsis()\n"
"                                        .text_xs()\n"
"                                        .font_weight(if is_active {\n"
"                                            gpui::FontWeight::MEDIUM\n"
"                                        } else {\n"
"                                            gpui::FontWeight::NORMAL\n"
"                                        })\n"
"                                        .text_color(rgb(t.text))\n"
"                                        .child(preview),\n"
"                                )\n"
"                                .child(\n"
"                                    div()\n"
"                                        .flex()\n"
"                                        .items_center()\n"
"                                        .gap_2()\n"
"                                        .text_xs()\n"
"                                        .text_color(rgb(t.text_dim))\n"
"                                        .child(if is_active && streaming {\n"
"                                            icon(\"loader\", 11., t.accent)\n"
"                                                .into_any_element()\n"
"                                        } else {\n"
"                                            SharedString::from(time_text.clone())\n"
"                                                .into_any_element()\n"
"                                        })\n"
"                                        .child(SharedString::from(format!(\n"
"                                            \"{} 条消息\",\n"
"                                            info.message_count\n"
"                                        ))),\n"
"                                ),\n"
"                        )\n"
"                        .children(if hovered {\n"
"                            Some(\n"
"                                div()\n"
"                                    .flex()\n"
"                                    .gap_1()\n"
"                                    .child(\n"
"                                        div()\n"
"                                            .id(SharedString::from(format!(\"ren-{ix}\")))\n"
"                                            .w(px(24.))\n"
"                                            .flex()\n"
"                                            .items_center()\n"
"                                            .justify_center()\n"
"                                            .cursor_pointer()\n"
"                                            .text_color(rgb(t.accent))\n"
"                                            .hover(|s| s.text_color(rgb(t.text)))\n"
"                                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {\n"
"                                                let p = path.clone();\n"
"                                                let _ = weak_ren.update(cx, |c, cx| {\n"
"                                                    c.open_session(p, true, cx);\n"
"                                                });\n"
"                                            })\n"
"                                            .child(icon(\"pencil\", 11., t.accent)),\n"
"                                    )\n"
"                                    .child(\n"
"                                        div()\n"
"                                            .id(SharedString::from(format!(\"del-{ix}\")))\n"
"                                            .w(px(24.))\n"
"                                            .flex()\n"
"                                            .items_center()\n"
"                                            .justify_center()\n"
"                                            .cursor_pointer()\n"
"                                            .hover(|s| s.text_color(rgb(0xd9534f)))\n"
"                                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {\n"
"                                                let p = p_del.clone();\n"
"                                                let _ = weak_del.update(cx, |c, cx| {\n"
"                                                    c.delete_session(p, cx);\n"
"                                                });\n"
"                                            })\n"
"                                            .child(icon(\"x\", 12., t.text_dim)),\n"
"                                    ),\n"
"                            )\n"
"                            .into_any_element()\n"
"                        } else {\n"
"                            None\n"
"                        }),\n"
"                })\n"
"                .flex_1()\n"
"                .min_h_0(),\n"
"            )\n"
)

# verify balance of the new block
d = 0
for chx in block:
    if chx == '(':
        d += 1
    elif chx == ')':
        d -= 1
assert d == 0, f'block parens unbalanced: {d}'

lines[start:end + 1] = [block]
io.open(p, 'w', encoding='utf-8', newline='\n').write(''.join(lines))
print('sessions list replaced cleanly:', start + 1, '->', end + 1)
