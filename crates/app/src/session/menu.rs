//! 统一输入菜单（031-2；Slash/@ 两菜单合并为一个弹层）。
//!
//! 弹层 = 搜索栏 + tab 条（技能 `/`、命令 `$`、项目文件 `@`、会话 `&`、
//! 「打开文件」钮）+ 单列条目区。tab 样式复用 topbar `tab_body`（连体态：
//! 填列表底色、只画左右边线、上圆角，条上不画横线）。
//!
//! - 触发：`/` `$` 仅输入开头且其后无空白；`@` `&` 是光标前 token
//!   （services::at_file::extract_trigger_query 同构语法，行首/空白之后）。
//!   `!` bash 模式不开 `&`（shell 里 `&` 太常见）。
//! - 过滤沿用各自口径：技能/命令 = 前缀匹配（技能按剥前缀名，`/skill:x`
//!   全名仍通）；文件 = TUI scoreEntry 打分（数据源 Chat::at_index 的
//!   git ls-files 清单，记忆化见 at_filter_cache）；会话 = 标题包含（psp
//!   项目清单，按修改时间倒序）。
//! - 搜索栏非空 = 万能搜索结果视图：四类**仅按名称**子串匹配、分组罗列
//!   （组头 = 类名 · 计数），行样式与子菜单一致；清空即回当前 tab。
//! - 确认只认 Tab / 点击：技能·命令 → `/name `；文件 → `@path `（目录
//!   `@dir/` 不闭合，钻取继续）；会话 → 会话文件全路径纯文本（chip 化由
//!   session_token_for_input 派生）。Enter 不再接受补全（2026-11-11 用户令）。

use crate::*;

use crate::ui::WithTip;

/// 子菜单条目上限（技能/命令/会话；文件走 at_file::AT_RESULT_LIMIT）
const TAB_LIMIT: usize = 60;
/// 万能搜索每类上限（文件名过滤同值）
const SEARCH_LIMIT: usize = 20;
/// tab 条高度；条内 tab 高 = 条高 −4（规范 §5.3「bar 内 tab 高」）
const STRIP_H: f32 = 34.;
const SKILL_PREFIX: &str = "skill:";

/// 当前触发 token：类目 + 输入里的字节区间 `[start, end)` + 查询词。
/// `start == 0 && end == input.len()` = 整段输入型触发（`/` `$`）。
pub(crate) struct MenuTrigger {
    pub(crate) kind: MenuKind,
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) query: String,
}

impl Chat {
    pub(crate) fn active_menu(&self, cx: &gpui::App) -> Option<MenuKind> {
        // 点外收起后保持关闭，直到输入再次变化（set_input 复位）
        if self.composer.menu_dismissed {
            return None;
        }
        self.menu_trigger(cx).map(|t| t.kind)
    }

    /// 当前触发 token（不看 menu_dismissed——确认补全需要区间本身）。
    fn menu_trigger(&self, cx: &gpui::App) -> Option<MenuTrigger> {
        let input = &self.composer.input;
        // 廉价门：输入里没有 `@`/`&` 字节就跳过两次 token 扫描
        let has_token = input.as_bytes().iter().any(|b| *b == b'@' || *b == b'&');
        for (prefix, kind) in [('/', MenuKind::Skill), ('$', MenuKind::Command)] {
            if input.starts_with(prefix) && !input[1..].contains(char::is_whitespace) {
                return Some(MenuTrigger {
                    kind,
                    start: 0,
                    end: input.len(),
                    query: input[1..].to_string(),
                });
            }
        }
        if !has_token {
            return None;
        }
        let cursor = self.composer_cursor(cx);
        let before = &input[..cursor];
        if let Some(m) = crate::services::at_file::extract_trigger_query(before, b'@') {
            return Some(MenuTrigger {
                kind: MenuKind::File,
                start: m.start,
                end: cursor,
                query: m.query,
            });
        }
        // `&` 会话 token：`!` bash 模式不触发（shell 里 `&&` 常见）；查询
        // 词以 `&` 开头（`&&` 起手）也不算
        if input.trim_start().starts_with('!') {
            return None;
        }
        let m = crate::services::at_file::extract_trigger_query(before, b'&')?;
        if m.query.starts_with('&') {
            return None;
        }
        Some(MenuTrigger {
            kind: MenuKind::Session,
            start: m.start,
            end: cursor,
            query: m.query,
        })
    }

    /// composer 光标字节位（取整到字符边界；组件未建 = 值末尾）。
    fn composer_cursor(&self, cx: &gpui::App) -> usize {
        let input = &self.composer.input;
        let mut cursor = self
            .composer
            .composer
            .as_ref()
            .map(|c| c.read(cx).cursor(cx))
            .unwrap_or(input.len())
            .min(input.len());
        while cursor > 0 && !input.is_char_boundary(cursor) {
            cursor -= 1;
        }
        cursor
    }

    /// 搜索栏非空 = 万能搜索结果视图（菜单开着才有意义）。
    pub(crate) fn menu_searching(&self) -> bool {
        !self.composer.menu_query.trim().is_empty()
    }

    pub(crate) fn menu_items(&self, cx: &gpui::App) -> Vec<MenuItem> {
        if self.composer.menu_dismissed {
            return Vec::new();
        }
        let Some(tr) = self.menu_trigger(cx) else {
            return Vec::new();
        };
        if self.menu_searching() {
            // 万能搜索：固定类目序分组（menu.rs 视图按 kind 变化插组头）
            let q = self.composer.menu_query.trim().to_lowercase();
            return MenuKind::ALL
                .iter()
                .flat_map(|k| self.menu_kind_items(*k, &q, true, cx))
                .collect();
        }
        self.menu_kind_items(tr.kind, &tr.query, false, cx)
    }

    fn menu_kind_items(
        &self,
        kind: MenuKind,
        q: &str,
        search: bool,
        cx: &gpui::App,
    ) -> Vec<MenuItem> {
        match kind {
            MenuKind::Skill | MenuKind::Command => {
                let want_skill = kind == MenuKind::Skill;
                let ql = q.to_lowercase();
                let limit = if search { SEARCH_LIMIT } else { TAB_LIMIT };
                self.slash_commands(cx)
                    .iter()
                    .filter(|c| c.name.starts_with(SKILL_PREFIX) == want_skill)
                    .filter(|c| {
                        if ql.is_empty() {
                            return true;
                        }
                        let full = c.name.to_lowercase();
                        let short = full.strip_prefix(SKILL_PREFIX).unwrap_or(&full);
                        if search {
                            short.contains(&ql)
                        } else {
                            // 子菜单按剥前缀名前缀匹配；`/skill:pi` 全名形态仍通
                            short.starts_with(&ql) || full.starts_with(&ql)
                        }
                    })
                    .take(limit)
                    .map(|c| MenuItem {
                        kind,
                        insert: c.name.clone(),
                        label: c
                            .name
                            .strip_prefix(SKILL_PREFIX)
                            .unwrap_or(&c.name)
                            .to_string(),
                        desc: c.description.clone(),
                        is_dir: false,
                    })
                    .collect()
            }
            MenuKind::File => self.menu_file_items(q, search),
            MenuKind::Session => self.menu_session_items(q, search),
        }
    }

    /// 文件候选：打分走完整派生条目（目录 + 文件；pi-web 对全量打分，仓库
    /// 超过索引上限时深层文件仍可搜到）。索引没就绪 → 空列表，后台构建完
    /// 成的通知会把菜单刷出来。万能搜索 = 仅 basename 子串。渲染期 &self，
    /// 写缓存走 RefCell（记忆化：同一份 entries（Arc 身份）+ 同查询词直接
    /// 复用上次过滤——菜单打开期间每次渲染都会走到这里，全量打分只该在
    /// 输入或索引变化时发生）。
    fn menu_file_items(&self, q: &str, search: bool) -> Vec<MenuItem> {
        let key = self.project_key();
        let Some(entry) = self.composer.at_index.get(&key) else {
            return Vec::new();
        };
        let cache = if search {
            &self.composer.menu_search_cache
        } else {
            &self.composer.at_filter_cache
        };
        let hit = cache
            .borrow()
            .as_ref()
            .is_some_and(|(arc, cq, _)| std::sync::Arc::ptr_eq(arc, &entry.entries) && cq == q);
        if !hit {
            let hits = if search {
                crate::services::at_file::filter_file_names(&entry.entries, q, SEARCH_LIMIT)
            } else {
                crate::services::at_file::filter_file_entries(&entry.entries, q)
            };
            *cache.borrow_mut() = Some((entry.entries.clone(), q.to_string(), hits));
        }
        cache
            .borrow()
            .as_ref()
            .map(|(_, _, v)| v.clone())
            .unwrap_or_default()
            .into_iter()
            .map(|e| MenuItem {
                kind: MenuKind::File,
                label: e.path.clone(),
                insert: e.path,
                desc: String::new(),
                is_dir: e.is_dir,
            })
            .collect()
    }

    /// 会话候选（031-2 新增数据源）：psp 项目清单全量，标题包含过滤，
    /// 按修改时间倒序（跨项目混排，desc = 项目名）。
    fn menu_session_items(&self, q: &str, search: bool) -> Vec<MenuItem> {
        let ql = q.to_lowercase();
        let limit = if search { SEARCH_LIMIT } else { TAB_LIMIT };
        let mut hits: Vec<(
            std::time::SystemTime,
            &crate::ProjectGroup,
            &pi_link::sessions::SessionInfo,
        )> = Vec::new();
        for g in &self.psp.projects {
            for s in &g.sessions {
                if !ql.is_empty() && !session_title(s).to_lowercase().contains(&ql) {
                    continue;
                }
                hits.push((s.modified, g, s));
            }
        }
        hits.sort_by(|a, b| b.0.cmp(&a.0));
        hits.truncate(limit);
        hits.into_iter()
            .map(|(_, g, s)| MenuItem {
                kind: MenuKind::Session,
                insert: s.path.to_string_lossy().replace('\\', "/"),
                label: session_title(s).to_string(),
                desc: g.name.clone(),
                is_dir: false,
            })
            .collect()
    }

    /// 菜单高亮移动。返回 false = 菜单未开/无条目（调用方走各自兜底）。
    pub(crate) fn menu_move(&mut self, delta: i32, cx: &mut Context<Self>) -> bool {
        let items = self.menu_items(cx);
        if self.active_menu(cx).is_none() || items.is_empty() {
            return false;
        }
        self.composer.menu_ix = if delta < 0 {
            self.composer.menu_ix.saturating_sub(1)
        } else {
            (self.composer.menu_ix + 1).min(items.len() - 1)
        };
        self.menu_scroll_to(&items, self.composer.menu_ix);
        cx.notify();
        true
    }

    /// Tab / 点击确认当前高亮条目。返回 false = 菜单未开/无条目。
    pub(crate) fn menu_accept_current(&mut self, cx: &mut Context<Self>) -> bool {
        let items = self.menu_items(cx);
        if self.active_menu(cx).is_none() || items.is_empty() {
            return false;
        }
        let ix = self.composer.menu_ix.min(items.len() - 1);
        let item = items[ix].clone();
        self.accept_menu(item, cx);
        true
    }

    pub(crate) fn accept_menu(&mut self, item: MenuItem, cx: &mut Context<Self>) {
        let Some(tr) = self.menu_trigger(cx) else {
            return;
        };
        let input = self.composer.input.clone();
        let before = &input[..tr.start];
        let after = &input[tr.end.min(input.len())..];
        let (text, cursor) = match item.kind {
            // 命令形态 `/name `：技能/命令仅输入开头合法，token 前若有正文
            // 则命令置顶、原文作参数跟在后面（不丢字）
            MenuKind::Skill | MenuKind::Command => {
                let head = format!("/{} ", item.insert);
                let cursor = head.len();
                let v = format!("{head}{before}{after}");
                (v, cursor)
            }
            // pi-web applyAtCompletion：只替换触发 token（保留光标后的文本），
            // 插入形态见 build_at_insert_text（目录不闭合 → 菜单继续钻取）
            MenuKind::File => {
                let (t, off) =
                    crate::services::at_file::build_at_insert_text(&item.insert, item.is_dir);
                let v = format!("{before}{t}{after}");
                (v, tr.start + off)
            }
            // 会话 = 全路径纯文本（hover_card_at_insert 同语义，后续空格收尾；
            // chip 化由 session_token_for_input 派生）
            MenuKind::Session => {
                let t = format!("{} ", item.insert);
                let cursor = tr.start + t.len();
                let v = format!("{before}{t}{after}");
                (v, cursor)
            }
        };
        self.set_input_with_cursor(text, cursor, cx);
        self.close_menu_common(cx);
    }

    /// 点 tab 切子菜单：改写输入里的触发符（前缀即状态，无需额外覆盖字段）。
    /// 技能/命令前缀只在整段输入型触发（开头 + 无空白）下可切，否则禁用。
    /// 搜索视图里点 tab = 清搜索词、落到该子菜单。
    pub(crate) fn menu_switch_tab(&mut self, kind: MenuKind, cx: &mut Context<Self>) {
        let Some(tr) = self.menu_trigger(cx) else {
            return;
        };
        if tr.kind != kind {
            if !self.menu_tab_available(kind, &tr) {
                return;
            }
            let cursor = self.composer_cursor(cx);
            let mut v = self.composer.input.clone();
            v.replace_range(tr.start..tr.start + 1, &kind.prefix().to_string());
            self.set_input_with_cursor(v, cursor, cx);
        }
        self.close_menu_common(cx);
    }

    /// tab 是否可点：`@`/`&` 恒可；`/`/`$` 需要整段输入型触发（token 在
    /// 开头且改写后无空白/引号残留——`@"..` 引号形态换前缀会破语法）。
    fn menu_tab_available(&self, kind: MenuKind, tr: &MenuTrigger) -> bool {
        if matches!(kind, MenuKind::File | MenuKind::Session) {
            return true;
        }
        tr.start == 0 && !tr.query.contains(char::is_whitespace) && !tr.query.contains('"')
    }

    /// 「打开文件」钮：系统文件选择器 → 选中文件以 `@path ` 插入，替换
    /// 当前触发 token（项目外文件 = 绝对路径；tree_at_insert 同款剥前缀）。
    pub(crate) fn menu_open_file(&mut self, cx: &mut Context<Self>) {
        let opts = gpui::PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: None,
        };
        let rx = cx.prompt_for_paths(opts);
        cx.spawn(async move |this, cx| {
            let picked = rx.await.ok().and_then(|r| r.ok()).flatten();
            let Some(paths) = picked else {
                return;
            };
            let _ = this.update(cx, |chat, cx| chat.menu_insert_files(paths, cx));
        })
        .detach();
    }

    fn menu_insert_files(&mut self, paths: Vec<std::path::PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let cwd = self.ws.cwd.clone().unwrap_or_default();
        let mut text = String::new();
        for p in &paths {
            let rel = p
                .strip_prefix(&cwd)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/");
            text.push_str(&crate::services::at_file::build_at_insert_text(&rel, false).0);
        }
        // 替换触发 token；菜单已收起/输入已变 = 追加到输入末尾
        let (start, end) = match self.menu_trigger(cx) {
            Some(tr) => (tr.start, tr.end.min(self.composer.input.len())),
            None => {
                let l = self.composer.input.len();
                (l, l)
            }
        };
        let mut v = String::new();
        v.push_str(&self.composer.input[..start]);
        if start == end && !v.is_empty() && !v.ends_with(' ') && !v.ends_with('\n') {
            v.push(' ');
        }
        let cursor = v.len() + text.len();
        v.push_str(&text);
        v.push_str(&self.composer.input[end..]);
        self.set_input_with_cursor(v, cursor, cx);
        self.close_menu_common(cx);
    }

    /// 收起菜单（Esc / 点外）：清搜索词、复位选中，下一帧焦点仲裁把焦点
    /// 还给 composer（menu_refocus 放行白名单）。
    pub(crate) fn menu_dismiss(&mut self, cx: &mut Context<Self>) {
        self.composer.menu_dismissed = true;
        self.close_menu_common(cx);
    }

    /// accept/切 tab/收起共用的善后：清搜索、复位选中与滚动。
    fn close_menu_common(&mut self, cx: &mut Context<Self>) {
        self.clear_menu_search(cx);
        self.composer.menu_refocus = true;
        self.composer.menu_ix = 0;
        self.composer
            .menu_scroll
            .set_offset(gpui::point(px(0.), px(0.)));
        cx.notify();
    }

    /// 清空万能搜索：镜像值与输入实体同清（实体回调只镜像值，不会回环）。
    pub(crate) fn clear_menu_search(&mut self, cx: &mut Context<Self>) {
        if self.composer.menu_query.is_empty() {
            return;
        }
        self.composer.menu_query.clear();
        if let Some(i) = &self.composer.menu_search {
            i.update(cx, |ti, cx| ti.set_value(String::new(), cx));
        }
    }

    /// 菜单搜索栏实体惰性创建（菜单首帧调用；模型菜单过滤框同款接线：
    /// on_change 只镜像值 + 复位选中，Esc 延迟到派发周期外收起——回调发生在
    /// TextInput 自身 update 内，同步摸实体会撞双租约）。
    pub(crate) fn ensure_menu_search(&mut self, cx: &mut Context<Self>) {
        if self.composer.menu_search.is_some() {
            return;
        }
        let weak = cx.entity().downgrade();
        let weak_esc = weak.clone();
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .flat()
                .placeholder(tr("menu.search_placeholder"))
        });
        input.update(cx, |ti, _| {
            ti.set_on_change(Box::new(move |v, cx| {
                let _ = weak.update(cx, |c, cx| {
                    if c.composer.menu_query != v {
                        c.composer.menu_query.clear();
                        c.composer.menu_query.push_str(v);
                        c.composer.menu_ix = 0;
                        c.composer
                            .menu_scroll
                            .set_offset(gpui::point(px(0.), px(0.)));
                        cx.notify();
                    }
                });
            }));
            ti.set_on_escape(Box::new(move |cx| {
                let weak = weak_esc.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| c.menu_dismiss(cx));
                });
            }));
        });
        self.composer.menu_search = Some(input);
    }

    /// 搜索栏是否正持焦（焦点仲裁白名单判据）。
    pub(crate) fn menu_search_focused(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        self.active_menu(cx).is_some()
            && self
                .composer
                .menu_search
                .as_ref()
                .is_some_and(|i| i.read(cx).focus_handle_in(cx).is_focused(window))
    }

    /// 万能搜索视图里条目 ix 在滚动容器中的子元素位（组头占位）。
    fn menu_scroll_child_ix(items: &[MenuItem], ix: usize) -> usize {
        let mut headers = 0;
        for i in 0..=ix.min(items.len().saturating_sub(1)) {
            if i > 0 && items[i].kind != items[i - 1].kind {
                headers += 1;
            }
        }
        ix + headers
    }

    fn menu_scroll_to(&self, items: &[MenuItem], ix: usize) {
        self.composer
            .menu_scroll
            .scroll_to_item(Self::menu_scroll_child_ix(items, ix));
    }
}

/// 会话标题口径（与会话详情卡一致：会话名优先，空则预览/占位）。
pub(crate) fn session_title(s: &SessionInfo) -> &str {
    match s.name.as_deref() {
        Some(n) if !n.trim().is_empty() => n,
        _ => {
            if s.preview.is_empty() {
                "(empty)"
            } else {
                &s.preview
            }
        }
    }
}

/// 统一菜单弹层（挂胶囊正上方，同宽；见模块头注）。搜索栏 + tab 条 +
/// 滚动区；滚动区右侧留白比左侧多 4px 给滚动条让位，滚动条贴壳右缘。
/// 条目 = 滚动容器直接子元素，`menu_scroll.scroll_to_item` 按键跟随
/// （万能搜索视图的子元素位含组头，见 menu_scroll_child_ix）。
pub(crate) fn slash_menu_view(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
    cx: &gpui::App,
) -> gpui::AnyElement {
    let searching = chat.menu_searching();
    let trigger = chat.menu_trigger(cx);
    let active_kind = trigger.as_ref().map(|tr| tr.kind);
    let items = chat.menu_items(cx);
    let n = items.len();

    // ---- tab 条（topbar tab_body 复用：连体态，条上不画横线）----
    let tab = |kind: MenuKind| -> gpui::AnyElement {
        let active = !searching && active_kind == Some(kind);
        let enabled = trigger
            .as_ref()
            .is_some_and(|tr| chat.menu_tab_available(kind, tr));
        let badge = div()
            .font_family(crate::editor::markdown::MONO_FAMILY)
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(if active { t.accent } else { t.text_faint }))
            .child(SharedString::from(kind.prefix().to_string()))
            .into_any_element();
        let tab = crate::titlebar::tab_body(
            None,
            tr(kind.label_key()).into(),
            active,
            0.,
            t,
            None,
            Some(badge),
            false,
        )
        .h(px(STRIP_H - 4.))
        .when(!enabled, |d| {
            d.opacity(0.45).cursor(gpui::CursorStyle::Arrow)
        });
        let tab = if enabled {
            let weak = weak.clone();
            tab.id(SharedString::from(format!("menu-tab-{}", kind.prefix())))
                .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| c.menu_switch_tab(kind, cx));
                })
                .into_any_element()
        } else {
            tab.into_any_element()
        };
        tab
    };

    // ---- 行条目 ----
    let card = |ix: usize, child_ix: usize, c: &MenuItem| -> gpui::AnyElement {
        let active = ix == chat.composer.menu_ix;
        let weak = weak.clone();
        let kind = c.kind;
        // 文件条目：目录前缀暗色 + 文件名亮色，目录尾随 `/`（pi-web 渲染 parity）
        let (dir_el, name_text) = if c.kind == MenuKind::File {
            match c.label.rfind('/') {
                Some(slash) => (
                    Some(c.label[..=slash].to_string()),
                    c.label[slash + 1..].to_string(),
                ),
                None => (None, c.label.clone()),
            }
        } else {
            (None, c.label.clone())
        };
        let dir_el = dir_el.map(|dir| {
            div()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(13.))
                .text_color(rgb(t.text_dim))
                .whitespace_nowrap()
                .child(SharedString::from(dir))
                .into_any_element()
        });
        let weak_hover = weak.clone();
        let weak_click = weak.clone();
        let click_item = c.clone();
        let row = div()
            .id(SharedString::from(format!("menu-item-{ix}")))
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
            // 点击条目 = 确认补全（此前缺失过：点击无效果 → 用户点发送时
            // 菜单外的原始查询文本直发，@ 提及丢目录前缀）
            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                let _ = weak_click.update(cx, |chat, cx| {
                    chat.accept_menu(click_item.clone(), cx);
                });
            })
            .on_hover(move |h: &bool, _w, cx| {
                // 悬停跟随高亮（pi-web onMouseEnter parity）
                if *h {
                    let _ = weak_hover.update(cx, |chat, cx| {
                        if chat.composer.menu_ix != ix {
                            chat.composer.menu_ix = ix;
                            chat.composer.menu_scroll.scroll_to_item(child_ix);
                            cx.notify();
                        }
                    });
                }
            })
            .child(match kind {
                MenuKind::File => crate::ui::icon(
                    if c.is_dir { "folder" } else { "file" },
                    14.,
                    if active { t.accent } else { t.text_dim },
                ),
                MenuKind::Session => crate::ui::icon(
                    "message-square-text",
                    14.,
                    if active { t.accent } else { t.text_dim },
                ),
                _ => crate::ui::icon(
                    if kind == MenuKind::Skill {
                        "wand"
                    } else {
                        "icon-terminal-solid"
                    },
                    13.,
                    if active { t.accent } else { t.text_dim },
                ),
            })
            .children(dir_el)
            .child({
                let mut name = div()
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(13.))
                    .text_color(rgb(t.text))
                    .whitespace_nowrap()
                    .child(SharedString::from(name_text));
                // 会话标题固定 400px 宽、超长省略号截断（2026-11-11 用户定稿）
                if kind == MenuKind::Session {
                    name = name.w(px(400.)).min_w_0().overflow_hidden().text_ellipsis();
                }
                name
            })
            .when(kind == MenuKind::File && c.is_dir, |d| {
                d.child(
                    div()
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(13.))
                        .text_color(rgb(t.text_dim))
                        .child(tr("session.slash_prefix")),
                )
            })
            .when(!c.desc.is_empty(), |d| {
                d.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_dim))
                        .whitespace_nowrap()
                        .overflow_hidden()
                        .text_ellipsis()
                        .child(SharedString::from(c.desc.replace('\n', " "))),
                )
            });
        row.into_any_element()
    };

    // 滚动区子元素：万能搜索视图按类目分组（组头 + 条目），子元素位与
    // 键盘导航的 scroll_to_item 对齐（menu_scroll_child_ix）
    let mut children: Vec<gpui::AnyElement> = Vec::new();
    let mut last_kind: Option<MenuKind> = None;
    for (ix, c) in items.iter().enumerate() {
        if searching && last_kind != Some(c.kind) {
            let count = items.iter().filter(|i| i.kind == c.kind).count();
            children.push(
                div()
                    .text_size(crate::appearance::ui_size(11.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(format!(
                        "{} · {count}",
                        tr(c.kind.label_key())
                    )))
                    .into_any_element(),
            );
            last_kind = Some(c.kind);
        }
        let child_ix = children.len();
        children.push(card(ix, child_ix, c));
    }

    let empty_text: SharedString = if searching {
        tr("menu.no_search_result").into()
    } else {
        match active_kind {
            Some(MenuKind::File) => tr("composer.no_matching_files").into(),
            Some(MenuKind::Session) => tr("menu.no_session_found").into(),
            _ => tr("menu.no_command_found").into(),
        }
    };

    div()
        .occlude()
        // 点菜单外任意处收起（capture 阶段监听）；不 stop_propagation，
        // 这次点击继续落到下层界面（如直接点发送仍然发送）
        .on_mouse_down_out({
            let weak = weak.clone();
            move |_, _, cx| {
                let _ = weak.update(cx, |c, cx| c.menu_dismiss(cx));
            }
        })
        // 搜索栏持焦时 ↑/↓/Tab 走这里（绑定在 "Input" 上下文截获，自搜索
        // 输入框冒泡到弹层根；composer 焦点时由胶囊侧同名 handler 接管）
        .on_action({
            let weak = weak.clone();
            move |_: &crate::ComposerUp, _, cx| {
                let _ = weak.update(cx, |c, cx| {
                    c.menu_move(-1, cx);
                });
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &crate::ComposerDown, _, cx| {
                let _ = weak.update(cx, |c, cx| {
                    c.menu_move(1, cx);
                });
            }
        })
        .on_action({
            let weak = weak.clone();
            move |_: &crate::ComposerTab, _, cx| {
                let _ = weak.update(cx, |c, cx| {
                    c.menu_accept_current(cx);
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
        .max_h(px(520.))
        .overflow_hidden()
        // ---- 搜索栏（菜单底色同体；万能搜索入口）----
        .child(
            div()
                .flex_shrink_0()
                .h(px(36.))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(10.))
                .bg(rgb(t.bg))
                .border_b_1()
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66)))
                .child(crate::ui::icon("search", 14., t.text_faint))
                .child(
                    div().flex_1().min_w_0().children(
                        chat.composer
                            .menu_search
                            .as_ref()
                            .map(|i| i.clone().into_any_element()),
                    ),
                ),
        )
        // ---- tab 条 + 「打开文件」钮 + Tab插入 提示 ----
        .child(
            div()
                .flex_shrink_0()
                .h(px(STRIP_H))
                .flex()
                .items_end()
                .px(px(8.))
                .bg(rgb(t.chrome))
                // 条下 4px 菜单底色带（2026-11-11 用户定稿）：激活 tab 的
                // mb(-1) 连体缘落在这条带上，接缝不可见
                .border_b_4()
                .border_color(rgb(t.bg))
                .children(MenuKind::ALL.iter().map(|k| tab(*k)))
                .child(
                    // 浅图标钮（bar.rs eye_button 同款 24×24 r5）：系统文件
                    // 选择器，选中文件以 `@path ` 插入
                    div()
                        .id("menu-open-file")
                        .ml(px(4.))
                        .mb(px(3.))
                        .size(px(24.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(5.))
                        .text_color(rgb(t.text_muted))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .with_tip("menu.open_file")
                        .on_mouse_down(gpui::MouseButton::Left, {
                            let weak = weak.clone();
                            move |_, _, cx| {
                                let _ = weak.update(cx, |c, cx| c.menu_open_file(cx));
                            }
                        })
                        .child(crate::ui::icon("plus", 14., t.text_muted)),
                )
                .child(
                    div()
                        .ml_auto()
                        .h_full()
                        .flex()
                        .items_center()
                        .px(px(4.))
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(11.))
                        .text_color(rgb(t.text_dim))
                        .child(tr("menu.tab_insert")),
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
                        .id("menu-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(&chat.composer.menu_scroll)
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
                                .child(empty_text),
                        );
                    } else {
                        scroll = scroll.flex().flex_col().gap(px(8.)).children(children);
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
                                &chat.composer.menu_scroll,
                            )),
                    )
                }),
        )
        .into_any_element()
}
