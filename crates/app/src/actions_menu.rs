//! Slash/@ completion menu.

//! Split out of main.rs for the file-size budget. Child module of the
//! crate root: Chat's root-private fields stay accessible here.

use crate::*;

impl Chat {
    pub(crate) fn active_menu(&self) -> Option<MenuKind> {
        let input = &self.input;
        if input.starts_with('/') && !input[1..].contains(char::is_whitespace) {
            return Some(MenuKind::Slash);
        }
        if let Some(at) = input.rfind('@') {
            if !input[at..].contains(char::is_whitespace) && input[at + 1..].len() < 64 {
                return Some(MenuKind::At);
            }
        }
        None
    }

    pub(crate) fn menu_items(&self, cx: &gpui::App) -> Vec<MenuItem> {
        match self.active_menu() {
            Some(MenuKind::Slash) => {
                let q = self.input[1..].to_lowercase();
                self.rt()
                    .read(cx)
                    .commands
                    .iter()
                    .filter(|c| q.is_empty() || c.name.to_lowercase().starts_with(&q))
                    .take(60)
                    .map(|c| MenuItem {
                        insert: c.name.clone(),
                        desc: c.description.clone(),
                    })
                    .collect()
            }
            Some(MenuKind::At) => {
                let at = self.input.rfind('@').unwrap_or(0);
                let q = self.input[at + 1..].to_lowercase();
                self.project_files
                    .iter()
                    .filter(|f| q.is_empty() || f.to_lowercase().contains(&q))
                    .take(60)
                    .map(|f| MenuItem {
                        insert: f.clone(),
                        desc: String::new(),
                    })
                    .collect()
            }
            None => Vec::new(),
        }
    }

    pub(crate) fn accept_menu(&mut self, insert: String, cx: &mut Context<Self>) {
        match self.active_menu() {
            Some(MenuKind::Slash) => self.set_input(format!("/{insert} "), cx),
            Some(MenuKind::At) => {
                if let Some(at) = self.input.rfind('@') {
                    let v = format!("{}{} ", &self.input[..=at], insert);
                    self.set_input(v, cx);
                }
            }
            None => {}
        }
        self.menu_ix = 0;
        cx.notify();
    }
}

/// pi-web「/」大菜单（ChatInput.tsx slash 菜单 parity）：挂在胶囊正上方，
/// 头部 = 命令计数 + Tab/Enter 提示，主体 = 滚动区 + 双列卡片网格。
/// v58：布局从「双独立列」改为行主序 `chunks(2)`——同一行两张卡 stretch
/// 等宽等高，左右列永远对齐（旧布局两列各自 flex_1，行高随内容漂移）。
/// 行 = 滚动容器的直接子元素，`menu_scroll.scroll_to_item(行号)` 即按键
/// 选中时的最小滚动跟随；右缘常显滚动条 = psp_scrollbar 菜单变体。
/// 卡片 = mono 命令名 13px + 描述 11px 两行截断，选中态 accent 边框 +
/// bg_selected。
pub(crate) fn slash_menu_view(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::theme::Theme,
    cx: &gpui::App,
) -> gpui::AnyElement {
    let items = chat.menu_items(cx);
    let n = items.len();

    let card = |ix: usize, c: &MenuItem| -> gpui::AnyElement {
        let active = ix == chat.menu_ix;
        let weak = weak.clone();
        let name = c.insert.clone();
        let display_name = name.strip_prefix("skill:").unwrap_or(&name).to_string();
        let desc = c.desc.clone();
        div()
            .id(SharedString::from(format!("slash-{ix}")))
            .flex_1()
            .min_w_0()
            .min_h(px(58.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .justify_center()
            .px(px(10.))
            .py(px(9.))
            .rounded(px(7.))
            .border_1()
            .border_color(rgb(if active { t.accent } else { t.border }))
            .bg(rgb(if active { t.bg_selected } else { t.bg_panel }))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                let _ = weak.update(cx, |chat, cx| {
                    let items = chat.menu_items(cx);
                    if let Some(c) = items.get(ix) {
                        let name = c.insert.clone();
                        chat.accept_menu(name, cx);
                    }
                });
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .child(crate::ui::icon(
                        if name.starts_with("skill:") { "wand" } else { "terminal" },
                        13.,
                        if active { t.accent } else { t.text_dim },
                    ))
                    .child(
                        div()
                            .font_family("Consolas")
                            .text_size(px(13.))
                            .text_color(rgb(t.text))
                            .child(SharedString::from(display_name)),
                    ),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .line_clamp(2)
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(desc)),
            )
            .into_any_element()
    };

    // 行主序：每行两张卡（偶数下标左、奇数右），行内 stretch 等高对齐。
    // 行必须是滚动容器的「直接子元素」——scroll_to_item 按 child_bounds
    // 索引，中间再包一层 body 就跟踪不到了
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    for (row_ix, pair) in items.chunks(2).enumerate() {
        let mut row = div().flex().gap(px(8.)).w_full();
        for (off, c) in pair.iter().enumerate() {
            let ix = row_ix * 2 + off;
            row = row.child(card(ix, c));
        }
        if pair.len() == 1 {
            // 奇数收尾行补空位，保持卡片恒为半宽
            row = row.child(div().flex_1());
        }
        rows.push(row.into_any_element());
    }

    div()
        .occlude()
        .w_full()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(8.))
        .shadow_lg()
        .max_h(px(420.))
        .overflow_hidden()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px(px(10.))
                .py(px(8.))
                .border_b_1()
                .border_color(rgb(t.border))
                .text_size(px(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(format!("斜杠命令 · {n}")))
                .child(
                    div()
                        .font_family("Consolas")
                        .child(tr("Tab / Enter 插入")),
                ),
        )
        .child(
            // 滚动区包 relative 壳：滚动条贴壳右缘（psp 面板同构），不随
            // 内容滚走；行直接挂在滚动容器下（scroll_to_item 依赖）
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child({
                    let mut scroll = div()
                        .id("slash-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&chat.menu_scroll)
                        .p(px(10.));
                    if n == 0 {
                        scroll = scroll.child(
                            div()
                                .text_size(px(12.))
                                .text_color(rgb(t.text_dim))
                                .child(tr("未找到扩展、提示词或技能命令")),
                        );
                    } else {
                        scroll = scroll
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .children(rows);
                    }
                    scroll
                })
                .when(n > 0, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(2.))
                            .bottom(px(2.))
                            .right(px(2.))
                            .w(px(10.))
                            .child(crate::ui::psp_scrollbar::menu_scrollbar(
                                &chat.menu_scroll,
                            )),
                    )
                }),
        )
        .into_any_element()
}
