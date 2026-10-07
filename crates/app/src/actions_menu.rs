//! Slash/@ completion menu.

//! Split out of main.rs for the file-size budget. Child module of the
//! crate root: Chat's root-private fields stay accessible here.
//!
//! @ 菜单（031 对齐 pi-web）：触发/token 提取在 services::at_file
//! （extract_at_query 正则语义），候选用 TUI scoreEntry 阶梯打分，数据源
//! 是 Chat::at_index 的 git ls-files 清单（services::file_index，后台构建
//! + TTL 缓存）。确认插入 `@path `（目录 `@dir/` 不闭合，钻取继续）。

use crate::*;

impl Chat {
    pub(crate) fn active_menu(&self, cx: &gpui::App) -> Option<MenuKind> {
        // 点外收起后保持关闭，直到输入再次变化（set_input 复位）
        if self.menu_dismissed {
            return None;
        }
        let input = &self.input;
        if input.starts_with('/') && !input[1..].contains(char::is_whitespace) {
            return Some(MenuKind::Slash);
        }
        if self.at_token(cx).is_some() {
            return Some(MenuKind::At);
        }
        None
    }

    /// 光标前的 @ token（含字节起点/查询词/引号形态）。与 active_menu 不同，
    /// 这里不看 menu_dismissed——确认补全需要 token 位置本身。
    pub(crate) fn at_token(&self, cx: &gpui::App) -> Option<crate::services::at_file::AtQueryMatch> {
        let input = &self.input;
        let mut cursor = self
            .composer
            .as_ref()
            .map(|c| c.read(cx).cursor(cx))
            .unwrap_or(input.len())
            .min(input.len());
        while cursor > 0 && !input.is_char_boundary(cursor) {
            cursor -= 1;
        }
        crate::services::at_file::extract_at_query(&input[..cursor])
    }

    pub(crate) fn menu_items(&self, cx: &gpui::App) -> Vec<MenuItem> {
        match self.active_menu(cx) {
            Some(MenuKind::Slash) => {
                let q = self.input[1..].to_lowercase();
                // 命令清单 = 会话进程答案优先，否则启动装载的（项目 skill + 扩展缓存）
                self.slash_commands(cx).iter()
                    .filter(|c| q.is_empty() || c.name.to_lowercase().starts_with(&q))
                    .take(60)
                    .map(|c| MenuItem {
                        insert: c.name.clone(),
                        desc: c.description.clone(),
                        is_dir: false,
                    })
                    .collect()
            }
            Some(MenuKind::At) => {
                // 打分走完整派生条目（目录 + 文件；pi-web 对全量打分，仓库
                // 超过索引上限时深层文件仍可搜到）。索引没就绪 → 空列表，
                // 后台构建完成的通知会把菜单刷出来
                let key = self.cwd.to_string_lossy().to_string();
                let Some(entry) = self.at_index.get(&key) else {
                    return Vec::new();
                };
                let query = self.at_token(cx).map(|m| m.query).unwrap_or_default();
                crate::services::at_file::filter_file_entries(&entry.entries, &query)
                    .into_iter()
                    .map(|e| MenuItem {
                        insert: e.path,
                        desc: String::new(),
                        is_dir: e.is_dir,
                    })
                    .collect()
            }
            None => Vec::new(),
        }
    }

    pub(crate) fn accept_menu(&mut self, insert: String, cx: &mut Context<Self>) {
        match self.active_menu(cx) {
            Some(MenuKind::Slash) => self.set_input(format!("/{insert} "), cx),
            Some(MenuKind::At) => {
                let is_dir = self
                    .menu_items(cx)
                    .iter()
                    .find(|i| i.insert == insert)
                    .map(|i| i.is_dir)
                    .unwrap_or(false);
                let Some(m) = self.at_token(cx) else { return };
                // pi-web applyAtCompletion：只替换 @token（保留光标后的文本），
                // 插入形态见 build_at_insert_text（目录不闭合 → 菜单继续钻取）
                let (text, cur_off) = crate::services::at_file::build_at_insert_text(&insert, is_dir);
                let mut v = String::new();
                v.push_str(&self.input[..m.start]);
                v.push_str(&text);
                let cursor_byte = m.start + cur_off;
                let tail = self.composer.as_ref().map(|c| c.read(cx).cursor(cx)).unwrap_or(self.input.len());
                let mut tail = tail.min(self.input.len());
                while tail > 0 && !self.input.is_char_boundary(tail) {
                    tail -= 1;
                }
                if tail > m.start {
                    v.push_str(&self.input[tail..]);
                }
                self.set_input_with_cursor(v, cursor_byte, cx);
            }
            None => {}
        }
        self.menu_ix = 0;
        self.menu_scroll.set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }
}

/// pi-web「/」大菜单：挂在胶囊正上方，头部 = 命令计数 + Tab/Enter 提示，
/// 主体 = 滚动区 + 单列单行条目（v58 用户终裁：命令名 + 描述同行，描述
/// 超出右缘省略号截断，不占第二行）。desc 含 \n 硬行先拍平成空格（gpui
/// line_clamp 只限软换行）。滚动区右侧留白比左侧多 4px 给滚动条让位；
/// 滚动条贴壳右缘（right 0）。条目 = 滚动容器直接子元素，
/// `menu_scroll.scroll_to_item(menu_ix)` 按键跟随。
///
/// @ 菜单同构：头部「文件 · n」；条目 = 目录/文件图标 + 目录前缀暗色 +
/// 文件名亮色 + 目录尾随 `/`；悬停更新高亮（pi-web onMouseEnter parity）。
pub(crate) fn slash_menu_view(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::theme::Theme,
    cx: &gpui::App,
) -> gpui::AnyElement {
    let is_at = chat.active_menu(cx) == Some(MenuKind::At);
    let items = chat.menu_items(cx);
    let n = items.len();

    let card = |ix: usize, c: &MenuItem| -> gpui::AnyElement {
        let active = ix == chat.menu_ix;
        let weak = weak.clone();
        let name = c.insert.clone();
        let desc = c.desc.replace('\n', " ");
        // @ 条目：目录前缀暗色 + 文件名亮色，目录尾随 `/`（pi-web 渲染 parity）
        let (dir_part, name_part) = if is_at {
            match name.rfind('/') {
                Some(ix) => (Some(name[..=ix].to_string()), name[ix + 1..].to_string()),
                None => (None, name.clone()),
            }
        } else {
            (None, name.clone())
        };
        let display_name = name_part.strip_prefix("skill:").unwrap_or(&name_part).to_string();
        let dir_el = dir_part.map(|dir| {
            div()
                .font_family(crate::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(13.))
                .text_color(rgb(t.text_dim))
                .whitespace_nowrap()
                .child(SharedString::from(dir))
                .into_any_element()
        });
        let weak_hover = weak.clone();
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
            .on_hover(move |h: &bool, _w, cx| {
                // 悬停跟随高亮（pi-web onMouseEnter 更新 highlight parity）
                if *h {
                    let _ = weak_hover.update(cx, |chat, cx| {
                        if chat.menu_ix != ix {
                            chat.menu_ix = ix;
                            chat.menu_scroll.scroll_to_item(ix);
                            cx.notify();
                        }
                    });
                }
            })
            .child(if is_at {
                crate::ui::icon(
                    if c.is_dir { "folder" } else { "file" },
                    13.,
                    if active { t.accent } else { t.text_dim },
                )
                .into_any_element()
            } else {
                crate::ui::icon(
                    if name.starts_with("skill:") { "wand" } else { "terminal" },
                    13.,
                    if active { t.accent } else { t.text_dim },
                )
                .into_any_element()
            })
            .children(dir_el)
            .child(
                div()
                    .font_family(crate::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(13.))
                    .text_color(rgb(t.text))
                    .whitespace_nowrap()
                    .child(SharedString::from(display_name)),
            )
            .when(is_at && c.is_dir, |d| {
                d.child(
                    div()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(13.))
                        .text_color(rgb(t.text_dim))
                        .child("/"),
                )
            })
            .when(!desc.is_empty(), |d| {
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(crate::appearance::ui_size(11.))
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
        // 点菜单外任意处收起（capture 阶段监听）；不 stop_propagation，
        // 这次点击继续落到下层界面（如直接点发送仍然发送）
        .on_mouse_down_out({
            let weak = weak.clone();
            move |_, _, cx| {
                let _ = weak.update(cx, |c, cx| {
                    c.menu_dismissed = true;
                    cx.notify();
                });
            }
        })
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
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(if is_at {
                    format!("{} · {n}", tr("文件"))
                } else {
                    format!("{} · {n}", tr("斜杠命令"))
                }))
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
                                .text_size(crate::appearance::ui_size(12.))
                                .text_color(rgb(t.text_dim))
                                .child(if is_at {
                                    tr("没有匹配的文件")
                                } else {
                                    tr("未找到扩展、提示词或技能命令")
                                }),
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
