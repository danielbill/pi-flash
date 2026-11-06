//! 会话查询面板（013 的弹窗版 → 083 同版式面板，2026-10-21 用户定案）。
//!
//! **只查询**：没有替换 / 包含排除过滤 / 正则 / 大小写 / 整词——一个查询框
//! 加结果列表，其余版式与项目搜索（[`super::results`]）逐项一致：
//! - 组头复用 [`super::results::card_header`]（块 p4、内条 rounded4 + border、
//!   28×28 收展钮、标题 MEDIUM + 副标题 muted）；
//! - 正文单色（`file_font()` 同源）、命中底色走 [`super::results::match_highlights`]；
//! - 结果区两侧留白（`pl 8 / pr 18`）、竖向滚动条常显。
//!
//! 分组：一次命中 = 一条会话消息的片段（before/match/after）；按会话文件分组，
//! 组头 = 会话名（无名字用预览）+ 相对时间，点击组头跳到该会话首条命中。

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, WeakEntity, div, px, rgb};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::kit::icon_toggle;
use crate::ui::tokens::space;
use crate::{ContentView, PanelTab};

mod results;

use self::results::results;

/// 会话查询的防抖（沿用 013 弹窗版的 300ms）
const DEBOUNCE_MS: u64 = 300;

impl Chat {
    /// Ctrl+K / psp「搜索会话」钮：打开会话查询面板（单例 tab），已开则
    /// 聚焦查询框。**替换掉原来的 `Dialog::SessionSearch` 弹窗**
    /// （2026-10-21 用户定案：照文件查询的界面改造成面板，只查询）。
    pub(crate) fn open_session_search_panel(&mut self, cx: &mut Context<Self>) {
        self.ensure_session_input(cx);
        // 焦点当帧兑现（同项目搜索：`focus_soon` 在输入框自己那帧 focus）
        if let Some(q) = self.psp.session_query.clone() {
            q.update(cx, |input, cx| input.focus_soon(cx));
        }
        let existing = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::SessionSearch));
        let ix = match existing {
            Some(ix) => ix,
            None => {
                self.ws.panel_tabs.push(PanelTab::SessionSearch);
                self.ws.panel_tabs.len() - 1
            }
        };
        self.activate_panel_tab(ix, cx);
        self.set_content_view(ContentView::SessionSearch);
        cx.notify();
    }

    /// 关会话查询 tab（同项目搜索：状态保留，再开还是上次的查询）。
    pub(crate) fn close_session_search_tab(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::SessionSearch))
        else {
            return;
        };
        self.psp.search_gen += 1; // 作废在跑的批次
        self.close_panel_tab(ix, cx);
        if self.layout.content_view == ContentView::SessionSearch {
            self.fallback_after_closes(cx);
        }
        cx.notify();
    }

    /// 懒建查询框（`TextInput::new` 不需要 Window，可在动作里建）。
    pub(crate) fn ensure_session_input(&mut self, cx: &mut Context<Self>) {
        if self.psp.session_query.is_some() {
            return;
        }
        let weak = cx.weak_entity();
        let weak_esc = cx.weak_entity();
        let input = cx.new(|cx| {
            crate::ui::TextInput::new(cx)
                // 与项目搜索同一条定案：搜索框**不加 border**（flat 档）
                .flat()
                .placeholder(tr("session_search.placeholder"))
                .on_change(Box::new(move |q: &str, cx: &mut gpui::App| {
                    let _ = weak.update(cx, |c, cx| c.session_search_changed(q, cx));
                }))
                // Esc = 收起会话查询面板（defer 出帧再关：关 tab 会 drop 正在
                // 派发事件的输入框实体——与项目搜索同一条）
                .on_escape(Box::new(move |cx: &mut gpui::App| {
                    let weak = weak_esc.clone();
                    cx.defer(move |cx| {
                        let _ = weak.update(cx, |c, cx| c.close_session_search_tab(cx));
                    });
                }))
        });
        self.psp.session_query = Some(input);
    }

    /// 查询词变化：代际 +1、防抖 300ms 后后台扫本项目会话（013 原逻辑，
    /// 陈旧响应按代际丢弃）。
    pub(crate) fn session_search_changed(&mut self, q: &str, cx: &mut Context<Self>) {
        self.psp.search_gen += 1;
        let epoch = self.psp.search_gen;
        let needle = q.trim().to_string();
        self.psp.search_needle = needle.clone();
        self.psp.search_hits.clear();
        self.psp.search_truncated = false;
        // 结果集换了 → 收起状态作废（旧块对不上新结果），列表回顶
        self.psp.session_collapsed.clear();
        self.psp.session_list.reset(0);
        if needle.is_empty() {
            self.psp.search_running = false;
            cx.notify();
            return;
        }
        self.psp.search_running = true;
        cx.spawn(async move |weak, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(DEBOUNCE_MS))
                .await;
            let cancelled = weak
                .update(cx, |c, _| c.psp.search_gen != epoch)
                .unwrap_or(true);
            if cancelled {
                return;
            }
            let needle2 = needle.clone();
            // **全部会话**（跨项目）：按会话文件 mtime 倒序，最近的会话先出
            let resp = cx
                .background_spawn(async move { pi_link::sessions::search_sessions_all(&needle2) })
                .await;
            let _ = weak.update(cx, |c, cx| {
                if c.psp.search_gen != epoch {
                    return;
                }
                c.psp.search_running = false;
                c.psp.search_truncated = resp.truncated;
                c.psp.search_hits = resp.hits;
                cx.notify();
            });
        })
        .detach();
    }

    /// 收展一个会话块（键 = 会话文件路径）。
    pub(crate) fn session_search_toggle_block(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.psp.session_collapsed.remove(key) {
            self.psp.session_collapsed.insert(key.to_string());
        }
        // 块高变了：逼 `ListState` 重测这一项（否则收起后仍按旧高度留白）。
        // 块下标 = 该会话在分组后的位置（按首现顺序数不同会话）
        let mut ix = None;
        let mut seen: Option<String> = None;
        let mut n = 0usize;
        for h in &self.psp.search_hits {
            let k = h.session_path.to_string_lossy().to_string();
            if seen.as_deref() != Some(k.as_str()) {
                if k == key {
                    ix = Some(n);
                    break;
                }
                seen = Some(k);
                n += 1;
            }
        }
        if let Some(ix) = ix {
            self.psp.session_list.splice(ix..ix + 1, 1);
        }
        cx.notify();
    }
}

/// 视图根（`ContentView::SessionSearch` 的落点）：查询条 + 结果区。
pub(crate) fn view(chat: &mut Chat, weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    div()
        .id("session-search")
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .text_color(rgb(t.text))
        .child(bar(chat, weak, t))
        .child(results(chat, weak, t))
        .into_any_element()
}

/// 查询条：✕ + 无边框查询框 + 命中计数（Zed 同款，无替换/过滤/开关列）。
fn bar(chat: &mut Chat, weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let hits = chat.psp.search_hits.len();
    let running = chat.psp.search_running;
    let truncated = chat.psp.search_truncated;
    let query = chat.psp.session_query.clone();
    let counter: SharedString = if truncated {
        format!("{hits}+").into()
    } else {
        format!("{hits}").into()
    };
    let mut row = div()
        .flex()
        .items_center()
        .gap(px(space::SP2))
        .w_full()
        .child(icon_toggle(
            "ssq-close",
            "close",
            false,
            weak,
            "search.close",
            false,
            |c, cx| c.close_session_search_tab(cx),
        ));
    if let Some(q) = query {
        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                // 搜索框不加 border（与项目搜索同一条定案）
                .pl(px(space::SP4))
                .pr(px(space::SP1))
                .child(div().flex_1().min_w_0().py(px(4.)).child(q)),
        );
    }
    row = row.child(
        div()
            .flex()
            .items_center()
            .gap(px(space::SP2))
            .pl(px(space::SP2))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(if hits > 0 { t.text_dim } else { t.text_faint }))
            .child(SharedString::from(counter))
            .when(running, |d| d.child(crate::ui::spinner(12., t.accent))),
    );
    div()
        .flex_shrink_0()
        .w_full()
        .px(px(space::SP6))
        .pt(px(space::SP4))
        .pb(px(space::SP3))
        .flex()
        .flex_col()
        .gap(px(space::SP2))
        .border_b_1()
        .border_color(rgb(t.border))
        .child(row)
        .into_any_element()
}

#[cfg(test)]
mod tests;
