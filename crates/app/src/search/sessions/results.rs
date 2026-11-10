//! 会话查询**结果区**：会话块列表 + 片段行 + 分组（从 sessions.rs 外移）。
//!
//! 版式与项目搜索逐项一致：
//! - 组头复用 [`crate::search::results::card_header`]（块 p4、内条 rounded4 +
//!   border、28×28 收展钮、标题 MEDIUM + 副标题 muted）；
//! - 正文单色（`file_font()` 同源）、命中底色走
//!   [`crate::search::results::match_highlights`]；
//! - 结果区两侧留白（`pl 16 / pr 16`）、竖向滚动条常显。

use gpui::prelude::*;
use gpui::{AnyElement, SharedString, StyledText, WeakEntity, div, px, rgb};
use std::rc::Rc;

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::tr;

use crate::search::results::{HeaderCallbacks, card_header, match_highlights};

/// 片段行相对块左缘的缩进：对齐组头标题的文字起始线
/// （块内 `HEADER_PAD 4` + 条内 `pl 4` + 收展钮 28 + `gap 6` ≈ 24 偏低，实测
/// 标题首字落在块左缘 +24，这里取同值）
const SNIPPET_INDENT: f32 = 24.;

/// 一行片段的高度（与项目搜索行高同源）
fn line_h() -> f32 {
    (crate::appearance::file_font().size * 1.5).round()
}

/// 结果区：空态（标题 + 提示）或「会话块」列表。
///
/// 列表用 `gpui::list`（与项目搜索同款虚拟化）：自建 `overflow_y_scroll`
/// 容器会让长 URL 的**最小内容宽**把滚动容器顶宽，右边距被吃掉、版式变成
/// 「左贴边右空一截」（2026-10-21 用户实测）。
pub(super) fn results(chat: &mut Chat, weak: &WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    if chat.psp.search_hits.is_empty() {
        let (title, hint): (SharedString, SharedString) =
            if !chat.psp.search_needle.is_empty() && !chat.psp.search_running {
                (
                    tr("session_search.no_match").into(),
                    SharedString::from(tr("session_search.no_match_hint")),
                )
            } else if chat.psp.search_running {
                (tr("session_search.searching").into(), "".into())
            } else {
                (
                    tr("session_search.title").into(),
                    SharedString::from(tr("session_search.hint")),
                )
            };
        let mut col = div().flex().flex_col().gap(px(4.)).child(
            div()
                .text_size(px(16.))
                .text_color(rgb(t.text))
                .child(title),
        );
        if !hint.is_empty() {
            // Zed landing 的副标题档：14px muted，下留 8px
            col = col.child(
                div()
                    .text_size(px(14.))
                    .text_color(rgb(t.text_muted))
                    .mb(px(8.))
                    .child(hint),
            );
        }
        return div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .overflow_hidden()
            .bg(rgb(t.bg))
            .child(col)
            .into_any_element();
    }

    // 按会话分组（命中已按会话成段返回，相邻即同组）
    let groups: Vec<SessionGroup> = group_hits(&chat.psp.search_hits);
    let list = chat.psp.session_list.clone();
    let scrollbar = chat.psp.session_sb_state.clone();
    if list.item_count() != groups.len() {
        list.reset(groups.len());
    }
    let collapsed = chat.psp.session_collapsed.clone();
    let groups = Rc::new(groups);
    let weak_rows = weak.clone();
    results_shell(
        &list,
        &scrollbar,
        gpui::list(list.clone(), move |ix, _window, _cx| {
            let Some(group) = groups.get(ix) else {
                return div().into_any_element();
            };
            let is_collapsed = collapsed.contains(&group.key);
            let first_ts = group.hits.first().and_then(|h| h.ts);
            let group_subagent = group.hits.first().is_some_and(|h| h.subagent);
            let path_open = group.path.clone();
            let weak_open = weak_rows.clone();
            let open: Rc<dyn Fn(&mut gpui::App)> = Rc::new(move |cx| {
                let path = path_open.clone();
                let _ = weak_open.update(cx, |c, cx| {
                    c.jump_to_hit(path, first_ts, group_subagent, cx)
                });
            });
            let key = group.key.clone();
            let weak_toggle = weak_rows.clone();
            let toggle: Rc<dyn Fn(&mut gpui::App)> = Rc::new(move |cx| {
                let k = key.clone();
                let _ = weak_toggle.update(cx, |c, cx| c.session_search_toggle_block(&k, cx));
            });
            let mut item = div()
                .w_full()
                .flex()
                .flex_col()
                .min_w_0()
                .child(card_header(
                    format!("ss-{ix}"),
                    group.title.clone(),
                    group.sub.clone(),
                    None,
                    is_collapsed,
                    HeaderCallbacks { open, toggle },
                    t,
                ));
            if !is_collapsed {
                for hit in group.hits.iter() {
                    item = item.child(snippet_row(hit, &weak_rows, t));
                }
            }
            item.into_any_element()
        })
        .flex_1()
        .min_h_0()
        .min_w_0(),
    )
    .into_any_element()
}

/// 结果区外壳（与项目搜索 `results_shell` 同配方，抽出本地版是为了让会话块
/// 与文件块用**完全一致**的边距：两侧等距 16px，滚动条画在右边距内）。
fn results_shell(
    list_state: &gpui::ListState,
    scrollbar: &gpui_component::scroll::ScrollbarState,
    list: impl gpui::IntoElement,
) -> gpui::Stateful<gpui::Div> {
    let mut shell = div()
        .id("session-search-results")
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .relative()
        .pt(px(1.))
        .pl(px(16.))
        .pr(px(16.))
        .child(list);
    if list_state.max_offset_for_scrollbar().height > px(0.) {
        let handle = crate::ui::list_handle::ListStateHandle(list_state.clone());
        shell = shell.child(
            div()
                .absolute()
                .top(px(4.))
                .bottom(px(4.))
                .right(px(4.))
                .w(px(8.))
                .child(gpui_component::scroll::Scrollbar::vertical(
                    scrollbar, &handle,
                )),
        );
    }
    shell
}

/// 项目 cwd → 项目名（末段；Windows 反斜杠 / POSIX 斜杠都认）。
/// 末段是 UUID（某些宿主应用按 id 建项目目录）时只留前 8 位——整串 UUID
/// 上屏比时间还长，读不出任何信息。
pub(super) fn project_name(cwd: &str) -> Option<String> {
    let trimmed = cwd.trim_end_matches(['\\', '/']);
    let name = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed);
    if name.is_empty() {
        return None;
    }
    let uuid_like = name.len() >= 32
        && name.chars().filter(|c| *c == '-').count() == 4
        && name.chars().all(|c| c == '-' || c.is_ascii_hexdigit());
    Some(if uuid_like {
        name.chars().take(8).collect()
    } else {
        name.to_string()
    })
}

/// 单行截断（超出加省略号）
fn cap_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// 一条命中行：单色正文 + 命中底色（无行号列——会话片段不是文件行）。
fn snippet_row(
    hit: &pi_link::sessions::SearchHit,
    weak: &WeakEntity<Chat>,
    t: &'static Theme,
) -> AnyElement {
    let font = crate::appearance::file_font();
    let text = format!("{}{}{}", hit.before, hit.match_text, hit.after);
    let start = hit.before.len();
    let span = start..(start + hit.match_text.len()).min(text.len());
    let highlights = match_highlights(&text, &[span], t);
    let path = hit.session_path.clone();
    let ts = hit.ts;
    let subagent = hit.subagent;
    let weak = weak.clone();
    div()
        .id(SharedString::from(format!(
            "ssh-{}-{}",
            path.to_string_lossy(),
            hit.ts.unwrap_or_default()
        )))
        .min_h(px(line_h()))
        .w_full()
        .flex()
        .items_start()
        // 左缩到**与组头标题文字同一条竖线**（块内 4 + 条内 4 + 收展钮 28 +
        // gap 6 ≈ 24 从块左缘起算），右侧同样收 24：片段一律收在组头之内、
        // 呈「嵌套在会话标题下」的层级——2026-10-21 用户「内容缩进不够，
        // 不能超出文件头，否则很难看」
        .pl(px(SNIPPET_INDENT))
        .pr(px(SNIPPET_INDENT))
        .cursor_pointer()
        .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
            let path = path.clone();
            let _ = weak.update(cx, |c, cx| c.jump_to_hit(path, ts, subagent, cx));
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .font_family(font.family.clone())
                .text_size(px(font.size))
                .line_height(gpui::relative(1.5))
                .text_color(rgb(t.text_muted))
                .child(StyledText::new(SharedString::from(text)).with_highlights(highlights)),
        )
        .into_any_element()
}

/// 一个会话块：组头 + 该会话的命中行（拥有所有权——`gpui::list` 的构建闭包
/// 需要 'static；命中总数上限 30，克隆代价可忽略）。
pub(super) struct SessionGroup {
    pub(super) key: String,
    path: std::path::PathBuf,
    pub(super) title: SharedString,
    pub(super) sub: SharedString,
    pub(super) hits: Vec<pi_link::sessions::SearchHit>,
}

pub(super) fn group_hits(hits: &[pi_link::sessions::SearchHit]) -> Vec<SessionGroup> {
    let mut out: Vec<SessionGroup> = Vec::new();
    // 按 key 建索引而不是只看「相邻」：引擎按文件成段返回时相邻判断就够，
    // 但一旦命中序列交错（未来加并发/分段扫描），同一个会话就会冒出多个
    // 组头——用户看到的是「一个会话头出现 N 次」（2026-10-21 实测）。按 key
    // 归并后**一个会话永远只有一个组头**，组序 = 首现序。
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for hit in hits {
        let key = hit.session_path.to_string_lossy().to_string();
        if let Some(&ix) = index.get(&key) {
            out[ix].hits.push(hit.clone());
            continue;
        }
        index.insert(key.clone(), out.len());
        {
            let raw_title: String = hit
                .session_name
                .clone()
                .or_else(|| (!hit.preview.is_empty()).then(|| hit.preview.clone()))
                .unwrap_or_else(|| {
                    hit.session_path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| key.clone())
                });
            // 组头标题单行限长：会话名常常就是首条用户消息（很长），不限长会
            // 把副标题（项目·时间）整块挤没
            let title: SharedString = cap_chars(&raw_title, 64).into();
            // 副标题 = 所属项目 · **绝对时间**（`8月21日 09:21`）。
            // 全量搜索后必须让用户看清是哪个项目的会话；时间用绝对时刻而不是
            // 「2小时前」——同名会话（某些宿主应用每个任务都发同一段指令正文，
            // 标题天然一样）只有精确到分钟才分得清，否则看起来像「同一个会话头
            // 出现 N 次」（2026-10-21 用户实测）。
            let stamp = {
                let ms = hit
                    .modified
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                crate::services::format::fmt_msg_time(ms)
            };
            let project = hit.project.as_deref().and_then(project_name);
            let sub: SharedString = match project {
                Some(name) => format!("{name} · {stamp}").into(),
                None => stamp.into(),
            };
            out.push(SessionGroup {
                key,
                path: hit.session_path.clone(),
                title,
                sub,
                hits: vec![hit.clone()],
            });
        }
    }
    out
}
