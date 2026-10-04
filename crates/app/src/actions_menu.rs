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

/// pi-web「/」大菜单：挂在胶囊正上方，头部 = 命令计数 + Tab/Enter 提示，
/// 主体 = 滚动区 + 单列单行条目（v58 用户终裁：命令名 + 描述同行，描述
/// 超出右缘省略号截断，不占第二行）。desc 含 \n 硬行先拍平成空格（gpui
/// line_clamp 只限软换行）。滚动区右侧留白比左侧多 4px 给滚动条让位；
/// 滚动条贴壳右缘（right 0）。条目 = 滚动容器直接子元素，
/// `menu_scroll.scroll_to_item(menu_ix)` 按键跟随。
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
        let desc = c.desc.replace('\n', " ");
        div()
            .id(SharedString::from(format!("slash-{ix}")))
            .w_full()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(8.))
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
            .child(crate::ui::icon(
                if name.starts_with("skill:") { "wand" } else { "terminal" },
                13.,
                if active { t.accent } else { t.text_dim },
            ))
            .child(
                div()
                    .font_family(crate::markdown::MONO_FAMILY)
                    .text_size(px(13.))
                    .text_color(rgb(t.text))
                    .whitespace_nowrap()
                    .child(SharedString::from(display_name)),
            )
            .when(!desc.is_empty(), |d| {
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(11.))
                        .text_color(rgb(t.text_dim))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(SharedString::from(desc)),
                )
            })
            .into_any_element()
    };

    let cards: Vec<gpui::AnyElement> = items
        .iter()
        .enumerate()
        .map(|(ix, c)| card(ix, c))
        .collect();

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
                        .font_family(crate::markdown::MONO_FAMILY)
                        .child(tr("Tab / Enter 插入")),
                ),
        )
        .child(
            // 滚动区包 relative 壳：滚动条贴壳右缘（psp 面板同构），不随
            // 内容滚走；卡片直接挂在滚动容器下（scroll_to_item 依赖）
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
                        // 右侧比左侧多 4px：给滚动条让位（用户定稿）
                        .pt(px(10.))
                        .pb(px(10.))
                        .pl(px(10.))
                        .pr(px(14.));
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
                            .children(cards);
                    }
                    scroll
                })
                .when(n > 0, |d| {
                    d.child(
                        div()
                            .absolute()
                            .top(px(2.))
                            .bottom(px(2.))
                            .right(px(0.))
                            .w(px(10.))
                            .child(crate::ui::psp_scrollbar::menu_scrollbar(
                                &chat.menu_scroll,
                            )),
                    )
                }),
        )
        .into_any_element()
}
