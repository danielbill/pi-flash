//! 004 ProjectPicker 打开项目弹窗：搜索框 + 【打开文件夹】 + 最近 30 天
//! 活动项目列表（键盘 ↑/↓/Enter 同款卡片拦截）。自顶层 dialogs.rs 拆出
//! （090 目录化）；列表聚合（后台扫描）与键盘导航同住一文件。

use crate::*;

use std::path::PathBuf;

use gpui::{App, Div, MouseButton, SharedString, div, px, rgb};

use super::dialog_shell;
use crate::TextInput;
use crate::appearance::theme;
use crate::i18n::tr;
use crate::ui::icon;

/// 004 projectManager 打开项目菜单：500×500 居中卡片——顶部搜索框，
/// 【打开文件夹】行（走 psp 目录选择器），下面是最近 30 天活动项目列表
/// （字母序、行高 40、吃满剩余高度超出滚动）。当前项目行尾打勾（004：从某项目
/// 新建会话打开时默认选中）。列表数据 `project_hits` 由打开器后台扫描
/// 回填，本函数只做搜索词过滤。键盘：↑/↓ 在【打开文件夹】+项目行上移动
/// （0 = 打开文件夹，默认选中当前项目行、无则回退打开文件夹行），Enter
/// 激活（模型菜单同款卡片拦截——单行输入的 Enter 会向上冒泡）。
pub(super) fn render_project_picker(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    input: &gpui::Entity<TextInput>,
    fresh: bool,
    scroll: &gpui::ScrollHandle,
    t: &theme::Theme,
    _cx: &App,
) -> Div {
    // 004 v3：列表吃满弹窗剩余高度（面板固定 500，容纳几条就显示几条），
    // 超出出滚动条——不再按固定行数限高
    const ROW_H: f32 = 40.;
    // 键盘选中行号：0 = 【打开文件夹】，1..=N = 过滤后的项目行。回填/过滤
    // 与按键存在竞态，渲染期钳制兜底
    let visible = project_visible(chat);
    let sel = chat.psp.project_sel.min(visible.len());
    // 行直接挂滚动容器（composer 菜单同款）：scroll_to_item 按**直接子行**
    // 定位，中间隔一层包装就够不着——不再包 list 外壳
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    if visible.is_empty() {
        let empty = if chat.psp.project_hits.is_empty() {
            tr("project.idle_30d")
        } else {
            tr("project.no_match")
        };
        rows.push(
            div()
                .py_4()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_dim))
                .child(empty)
                .into_any_element(),
        );
    }
    for (row_ix, hit_ix) in visible.iter().enumerate() {
        let p = &chat.psp.project_hits[*hit_ix];
        let path = p.path.clone();
        let weak_row = weak.clone();
        let name: SharedString = p.name.clone().into();
        let selected = chat.ws.cwd.clone().is_some_and(|cur| {
            crate::services::workspace::same_ws(&p.path.to_string_lossy(), &cur.to_string_lossy())
        });
        rows.push(
            div()
                .id(SharedString::from(format!("proj-{row_ix}")))
                .h(px(ROW_H))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(8.))
                .cursor_pointer()
                // 键盘选中高亮（bg_selected 递进于 hover，共存无歧义）
                .when(sel == row_ix + 1, |d| d.bg(rgb(t.bg_selected)))
                .hover(|s| s.bg(rgb(t.bg_hover)))
                // fresh（新会话页来源）= 落全新草稿，不恢复 last_open——004
                // 选项目是为了在这个项目里开新会话；psp 来源保持切项目
                // 恢复上次会话的既定行为
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_row.update(cx, |c, cx| {
                        // 菜单关闭是 UI 入口的职责（业务函数只管业务）
                        c.dlg.dialog = None;
                        if fresh {
                            c.new_session_in(path.clone(), cx);
                        } else {
                            c.switch_project(path.clone(), cx);
                        }
                        cx.notify();
                    });
                })
                .child(icon("folder", 16., t.text_muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(crate::appearance::ui_size(13.))
                        .text_color(rgb(t.text))
                        .child(name),
                )
                .when(selected, |d| d.child(icon("check", 14., t.accent)))
                .into_any_element(),
        );
    }
    // 打开文件夹 = 目录选择器（psp 同款；无 30 天活动项目时的主入口）；
    // 落点跟随 fresh：新会话页来源选完落新草稿，不恢复该目录的上次会话
    let weak_open = weak.clone();
    let open_folder = div()
        .id("proj-open-folder")
        .h(px(ROW_H))
        .px_2()
        .flex()
        .items_center()
        .gap_2()
        .rounded(px(8.))
        .cursor_pointer()
        .text_size(crate::appearance::ui_size(13.))
        .text_color(rgb(t.text))
        // 键盘行号 0：默认选中（无当前项目时）与 ↑ 回绕终点
        .when(sel == 0, |d| d.bg(rgb(t.bg_selected)))
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak_open.update(cx, |c, cx| {
                // 菜单关闭归 UI 入口（系统目录选择器接管之前的收尾）
                c.dlg.dialog = None;
                c.pick_project_folder(fresh, cx);
                cx.notify();
            });
        })
        .child(icon("folder-plus", 16., t.text_muted))
        .child(tr("action.open_folder"));
    let weak_enter = weak.clone();
    let weak_up = weak.clone();
    let weak_down = weak.clone();
    let panel = div()
        .w(px(500.))
        .h(px(500.))
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(10.))
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .shadow_lg()
        // Enter：与模型菜单同路——过滤输入单行模式的 Enter 会向上冒泡，
        // 在卡片上拦下；激活会丢弃派发中的弹窗/输入实体，defer 出派发周期
        .on_key_down(move |ev: &gpui::KeyDownEvent, _w, cx| {
            if ev.keystroke.key != "enter" {
                return;
            }
            cx.stop_propagation();
            let weak = weak_enter.clone();
            cx.defer(move |cx| {
                let _ = weak.update(cx, |c, cx| c.project_activate(fresh, cx));
            });
        })
        // ↑/↓：搜索框焦点下的组件词移动已被 Input 上下文绑定的
        // ComposerUp/Down 抢占（main.rs），沿 bubble 到卡片即菜单导航；
        // 走到根节点的兜底重派发在弹窗场景轮不到（这里更近）
        .on_action(move |_: &crate::ComposerUp, _w, cx| {
            let _ = weak_up.update(cx, |c, cx| c.project_move_sel(-1, cx));
        })
        .on_action(move |_: &crate::ComposerDown, _w, cx| {
            let _ = weak_down.update(cx, |c, cx| c.project_move_sel(1, cx));
        })
        .child(input.clone())
        .child(open_folder)
        // 打开文件夹与项目列表之间的分隔线（用户 2026-10-07 定稿）
        .child(
            div()
                .h(px(1.))
                .w_full()
                .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66))),
        )
        // 列表吃满剩余高度：flex_1 + min_h_0 才会真的收缩滚动（工具定义
        // 列表同款）；滚动条 = ZED Regular 移植（可滚动即常显），absolute
        // 盖在滚动容器右缘、不随内容滚
        .child(
            div()
                .relative()
                .mt_1()
                .flex_1()
                .min_h_0()
                .flex()
                .child(
                    div()
                        .id("project-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .flex()
                        .flex_col()
                        .pr(px(6.))
                        .children(rows),
                )
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .right_0()
                        .w(px(10.))
                        .child(crate::ui::psp_scrollbar::menu_scrollbar(scroll)),
                ),
        );
    dialog_shell(chat, weak, panel)
}


// ---------------------------------------------------------------------------
// 键盘导航与打开器（impl Chat）
// ---------------------------------------------------------------------------

impl Chat {
    /// 004 projectManager 打开项目菜单（触发：psp 打开项目 icon / 012 新会话页
    /// 操作栏）：搜索框 + 【打开文件夹】 + 最近 30 天活动项目列表。列表后台
    /// 扫描回填（进程级持久 Scanner，索引命中不重读盘）；「从某项目新建会话
    /// 打开」的默认选中在渲染期对 same_ws 打勾，见 render_project_picker。
    /// `fresh`（新会话页来源）决定选项目后的落点：全新草稿 vs 恢复上次
    /// 会话（见 Dialog::ProjectPicker 注释）。键盘 ↑/↓/Enter 见
    /// project_move_sel / project_activate；打开/回填即落默认选中
    /// （当前项目行，不在列则【打开文件夹】行）。
    pub(crate) fn open_project_picker(&mut self, fresh: bool, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        let weak_esc = weak.clone();
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .placeholder(tr("project.search_placeholder"))
                .on_change(Box::new(move |q: &str, cx: &mut App| {
                    let _ = weak.update(cx, |c, cx| {
                        c.psp.project_filter = q.to_string();
                        // 过滤词变了行集就变：当前项目仍匹配则保持选中，
                        // 否则跳默认（首个项目行 / 打开文件夹）
                        c.psp.project_sel = c.project_sel_default();
                        cx.notify();
                    });
                }))
                .on_escape(Box::new(move |cx: &mut App| {
                    let _ = weak_esc.update(cx, |c, cx| {
                        c.dlg.dialog = None;
                        cx.notify();
                    });
                }))
        });
        self.psp.project_hits.clear();
        self.psp.project_filter.clear();
        // 回填前列表为空，先落【打开文件夹】行；扫描回来再跳当前项目
        self.psp.project_sel = 0;
        self.dlg.dialog = Some(Dialog::ProjectPicker {
            input,
            fresh,
            scroll: gpui::ScrollHandle::new(),
        });
        cx.notify();
        // 后台聚合：全量会话按 cwd 聚合 → 30 天窗口（004 v2）→ 字母序。
        // 候选上限给足余量（索引命中，无盘读）；超深处项目极少见。
        cx.spawn(async move |weak, cx| {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0);
            let cutoff = now_ms - 30 * 86_400_000;
            let mut entries = cx
                .background_spawn(async move { collect_recent_projects(cutoff) })
                .await;
            let _ = weak.update(cx, |c, cx| {
                // 当前项目兜底入选（无 30 天活动也显示，004：默认选中该项目）；
                // 无项目态没有「当前项目」，不兜底
                if let Some(cwd) = c.ws.cwd.clone() {
                    let cwd_key = cwd.to_string_lossy().to_string();
                    if !entries.iter().any(|p| {
                        crate::services::workspace::same_ws(&p.path.to_string_lossy(), &cwd_key)
                    }) {
                        let name = cwd
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| cwd_key.clone());
                        entries.push(crate::ProjectEntry { name, path: cwd });
                        entries.sort_by(|a, b| project_sort_key(a).cmp(&project_sort_key(b)));
                    }
                }
                c.psp.project_hits = entries;
                // 回填即落默认选中（当前项目行；被过滤词挡住则原默认）
                c.psp.project_sel = c.project_sel_default();
                cx.notify();
            });
        })
        .detach();
    }

    /// ↑/↓（open_project_picker 的卡片 on_action）：键盘选中行在【打开
    /// 文件夹】+ 过滤后项目行的连续行号上移动。0 = 打开文件夹行（常驻
    /// 可视区顶部，无需滚动）；1.. = 项目行，scroll_to_item 按滚动容器
    /// **直接子行**跟随（行挂法见 render_project_picker）。
    pub(crate) fn project_move_sel(&mut self, delta: i32, cx: &mut Context<Self>) {
        let max = project_visible(self).len();
        let next = ((self.psp.project_sel.min(max) as i32) + delta).clamp(0, max as i32) as usize;
        if next == self.psp.project_sel {
            return;
        }
        self.psp.project_sel = next;
        if next >= 1 {
            if let Some(Dialog::ProjectPicker { scroll, .. }) = self.dlg.dialog.as_ref() {
                scroll.scroll_to_item(next - 1);
            }
        }
        cx.notify();
    }

    /// 键盘激活（Enter）当前选中行：行号 0 = 打开目录选择器；项目行走
    /// 行点击同款落点（fresh = 新会话页来源落全新草稿，否则切项目恢复
    /// 上次会话）。
    pub(crate) fn project_activate(&mut self, fresh: bool, cx: &mut Context<Self>) {
        let visible = project_visible(self);
        let sel = self.psp.project_sel.min(visible.len());
        if sel == 0 {
            self.pick_project_folder(fresh, cx);
            return;
        }
        let path = self.psp.project_hits[visible[sel - 1]].path.clone();
        // 键盘 Enter 同一条 UI 收尾（菜单关闭不归业务函数管）
        self.dlg.dialog = None;
        if fresh {
            self.new_session_in(path, cx);
        } else {
            self.switch_project(path, cx);
        }
    }

    /// 打开项目菜单的默认选中：当前项目（= ws.cwd，见 `picker_current_cwd`）
    /// 在过滤结果里的行号 +1；
    /// 不在列（列表未回填 / 被搜索词滤掉）但有项目行时取首个项目行，连
    /// 项目行都没有才落【打开文件夹】。打开、列表回填、改过滤词三处共用。
    pub(crate) fn project_sel_default(&self) -> usize {
        let visible = project_visible(self);
        let cur = visible.iter().position(|&ix| {
            self.ws.cwd.clone().is_some_and(|cur| {
                crate::services::workspace::same_ws(
                    &self.psp.project_hits[ix].path.to_string_lossy(),
                    &cur.to_string_lossy(),
                )
            })
        });
        match cur {
            Some(row) => row + 1,
            None if !visible.is_empty() => 1,
            None => 0,
        }
    }

}

/// 004 打开项目菜单的可见项目行（`project_hits` 下标；搜索词过滤）。渲染
/// 与键盘导航（move/activate/默认选中）共用这一口径，防两处过滤漂移。
fn project_visible(chat: &Chat) -> Vec<usize> {
    let needle = chat.psp.project_filter.trim().to_lowercase();
    chat.psp
        .project_hits
        .iter()
        .enumerate()
        .filter(|(_, p)| needle.is_empty() || p.name.to_lowercase().contains(&needle))
        .map(|(ix, _)| ix)
        .collect()
}

/// 004 打开项目菜单的列表聚合：30 天窗口内有会话活动的项目（一个 cwd 一行，
/// 活动时刻不再保留），按项目名字母序输出（后台线程执行）。聚合键走
/// ws 归一化（Windows 盘符/大小写、正反斜杠差异同项目合并，首见路径上屏）。
fn collect_recent_projects(cutoff_ms: i64) -> Vec<crate::ProjectEntry> {
    let mut by_key: std::collections::HashMap<String, crate::ProjectEntry> =
        std::collections::HashMap::new();
    // PF 项目注册表先行：打开项目即注册，零会话也入选（同 30 天窗口；
    // 目录已删的注册项不再出现）。注册表拼写占坑，下方会话扫描不覆盖
    for r in pi_link::projects::project_records() {
        if r.last_opened_ms < cutoff_ms || !r.path.is_dir() {
            continue;
        }
        let key = crate::services::workspace::same_ws_key(&r.path.to_string_lossy());
        by_key.entry(key).or_insert(crate::ProjectEntry {
            name: crate::services::workspace::project_name(&r.path),
            path: r.path.clone(),
        });
    }
    for s in pi_link::sessions::list_sessions(2000) {
        let ms = s
            .modified
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        if ms < cutoff_ms {
            continue;
        }
        let key = crate::services::workspace::same_ws_key(&s.cwd);
        if by_key.contains_key(&key) {
            continue;
        }
        let path = PathBuf::from(&s.cwd);
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| s.cwd.clone());
        by_key.insert(key, crate::ProjectEntry { name, path });
    }
    let mut rows: Vec<crate::ProjectEntry> = by_key.into_values().collect();
    rows.sort_by(|a, b| project_sort_key(a).cmp(&project_sort_key(b)));
    rows
}

/// 字母序排序键：名字不分大小写，路径做稳定次序。
fn project_sort_key(p: &crate::ProjectEntry) -> (String, String) {
    (
        p.name.to_lowercase(),
        p.path.to_string_lossy().to_lowercase(),
    )
}
