//! 设置「快捷键」页（082）：居中索引版式——无页标题（弹窗头已有页名），
//! 组标题带条数（细线延伸），行内纯文字 + 点线引导，键帽为平面样式
//! （细边框 + 底部厚边，修饰键浅一档灰）。组标题与行文案的左缘都落在
//! 原数字列起点（行内 px(2) 内边距处），去掉编号后不额外缩进。键帽来源
//! 分两类：
//! - 有 action 的六项：keymap 反查（`highest_precedence_binding_for_action`
//!   首键，单一事实源，与按钮 tooltip 键位 chip 同源，改绑定自动跟随）；
//! - Ctrl+←/→ 与 F2：走渲染根按键兜底、**故意没有 action**（None-context
//!   绑定会把输入框词移动抢掉，见 shell/render.rs），键串静态解析。

use super::*;
use gpui::{Action, AsKeystroke, FontWeight, Keystroke, Window};

/// 内容列宽（弹窗内容区约 1100，600 居中 = 设计稿比例）。
const COL_W: f32 = 600.;

pub(crate) fn mc_shortcuts_view(window: &Window) -> gpui::AnyElement {
    // 全局：前六项 keymap 反查，后两键走渲染根兜底（无 action，见模块注释）；
    // 说明行只有文件树的「重命名」需要（其余行不写注释说明）
    let global: Vec<(&str, &str, Option<Keystroke>)> = vec![
        (
            tr("psp.open_project"),
            "",
            action_key(&crate::OpenProjectPicker, window),
        ),
        (
            tr("psp.new_session"),
            "",
            action_key(&crate::NewSession, window),
        ),
        (
            tr("psp.search_sessions"),
            "",
            action_key(&crate::OpenSessionSearch, window),
        ),
        (
            tr("shortcuts.search_files"),
            "",
            action_key(&crate::OpenFileSearch, window),
        ),
        (
            tr("psp.maximize_session_list"),
            "",
            action_key(&crate::MaximizeSessionList, window),
        ),
        (
            tr("file_tree.maximize"),
            "",
            action_key(&crate::MaximizeFileTree, window),
        ),
        (tr("shortcuts.view_left"), "", Some(bare_key("ctrl-left"))),
        (tr("shortcuts.view_right"), "", Some(bare_key("ctrl-right"))),
    ];
    let file_tree: Vec<(&str, &str, Option<Keystroke>)> = vec![(
        tr("file_tree.rename"),
        tr("shortcuts.rename_hint"),
        Some(bare_key("f2")),
    )];

    // 弹窗 body 是 block 容器（taffy block 不解析流内auto margin），居中
    // 走 items_center + max_w
    let mut root = div().w_full().flex().flex_col().items_center();
    let mut col = div().w_full().max_w(px(COL_W)).flex().flex_col();

    // 组：组名 + 细线延伸 + 条数；行内标题 + 点线引导，说明行按行可选
    let groups = vec![
        (tr("shortcuts.group_global"), global),
        (tr("shortcuts.group_file_tree"), file_tree),
    ];
    let mut row_ix = 0usize;
    for (gi, (name, entries)) in groups.into_iter().enumerate() {
        col = col.child(group_title(name, entries.len(), gi == 0));
        let last = entries.len().saturating_sub(1);
        for (ri, (title, desc, key)) in entries.into_iter().enumerate() {
            row_ix += 1;
            col = col.child(index_row(row_ix, title, desc, key.as_ref(), ri != last));
        }
    }

    root = root.child(col);
    root.into_any_element()
}

/// 有 action 的键：keymap 反查首个键串（与 Kbd::binding_for_action 同口径）。
fn action_key(action: &dyn Action, window: &Window) -> Option<Keystroke> {
    window
        .highest_precedence_binding_for_action(action)
        .and_then(|b| b.keystrokes().first().map(|k| k.as_keystroke().clone()))
}

/// 无 action 的键（渲染根按键兜底三键）：静态解析键串。
fn bare_key(spec: &str) -> Keystroke {
    Keystroke::parse(spec).expect("static key spec parses")
}

/// 组标题行：`全局 ————— 7 条`（无编号，组名即原数字列起点）。first 控制
/// 与上一组的间距。
fn group_title(name: &str, count: usize, first: bool) -> gpui::AnyElement {
    let t = T();
    div()
        .flex()
        .items_center()
        .gap(px(9.))
        .mt(px(if first { 2. } else { 22. }))
        .mb(px(4.))
        .child(
            div()
                .text_size(crate::appearance::ui_size(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(SharedString::from(name.to_string())),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .h(px(1.))
                .bg(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x66))),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(crate::i18n::tf(
                    "shortcuts.count",
                    &[("n", count.to_string())],
                ))),
        )
        .into_any_element()
}

/// 索引行：`打开项目 ················ [Ctrl][+][P]`（无编号，文案左缘=
/// 原数字列起点，即 px(2) 内边距处）。desc 为空则不画说明行（当前只有
/// 文件树「重命名」带说明）。sep_below = 组内行间细分隔线（组尾行
/// 与下一组标题之间不画）。悬停整行淡 accent 底（group/group_hover，见
/// docs/agents/gpui.md）。
fn index_row(
    ix: usize,
    title: &str,
    desc: &str,
    key: Option<&Keystroke>,
    sep_below: bool,
) -> gpui::AnyElement {
    let t = T();
    let mut name_col = div().flex().flex_col().flex_shrink_0().child(
        div()
            .text_size(crate::appearance::ui_size(13.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(rgb(t.text))
            .child(SharedString::from(title.to_string())),
    );
    if !desc.is_empty() {
        name_col = name_col.child(
            div()
                .mt(px(1.5))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(desc.to_string())),
        );
    }
    div()
        .id(SharedString::from(format!("sc-row-{ix}")))
        .group("sc-row")
        .flex()
        .items_center()
        .gap(px(12.))
        .px(px(2.))
        .py(px(9.))
        .when(sep_below, |d| {
            d.border_b_1()
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x40)))
        })
        .hover(|s| s.bg(gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.08))))
        .child(name_col)
        .child(
            // 点线引导符：1px 盒内画 dashed 底边
            div()
                .flex_1()
                .min_w_0()
                .mx(px(6.))
                .h(px(1.))
                .border_b_1()
                .border_dashed()
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x99))),
        )
        .children(key.map(keycaps))
        .into_any_element()
}

/// 键帽行：修饰键在前（Ctrl/Alt/Shift/Win），主键收尾，键间 `+`。
fn keycaps(stroke: &Keystroke) -> gpui::AnyElement {
    let m = stroke.modifiers;
    let mut labels: Vec<(String, bool)> = Vec::new();
    if m.control {
        labels.push(("Ctrl".into(), true));
    }
    if m.alt {
        labels.push(("Alt".into(), true));
    }
    if m.shift {
        labels.push(("Shift".into(), true));
    }
    if m.platform {
        labels.push(("Win".into(), true));
    }
    labels.push((key_label(&stroke.key), false));

    let mut row = div().flex().items_center().flex_shrink_0();
    for (i, (label, modifier)) in labels.into_iter().enumerate() {
        if i > 0 {
            row = row.child(plus_sep());
        }
        row = row.child(keycap(label, modifier));
    }
    row.into_any_element()
}

/// 平面键帽：外层 1px 深底衬（pb 露出 = 键帽底部厚边，gpui 边框单色做不
/// 出深浅两侧），内层细边框 + 面板底（修饰键灰一档、字降一级）。
fn keycap(label: String, modifier: bool) -> gpui::AnyElement {
    let t = T();
    let cap_bg = if t.dark { t.tool_bg } else { 0xffffff };
    // mix_rgb 权重 = 第一参数占比：往底色里掺 5% 文字色、往边框色里掺黑
    let mod_bg = crate::appearance::theme::mix_rgb(t.text, t.bg, if t.dark { 0.06 } else { 0.05 });
    let edge =
        crate::appearance::theme::mix_rgb(t.border, 0x000000, if t.dark { 0.45 } else { 0.86 });
    div()
        .flex_shrink_0()
        .rounded(px(5.))
        .pb(px(1.))
        .bg(rgb(edge))
        .child(
            div()
                .rounded(px(5.))
                .border_1()
                .border_color(rgb(t.border))
                .bg(rgb(if modifier { mod_bg } else { cap_bg }))
                .min_w(px(22.))
                .h(px(22.))
                .px(px(7.))
                .flex()
                .items_center()
                .justify_center()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(11.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(if modifier { t.text_muted } else { t.text }))
                .child(SharedString::from(label)),
        )
        .into_any_element()
}

/// 键帽间 `+`。
fn plus_sep() -> gpui::AnyElement {
    div()
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(T().text_faint))
        .child("+")
        .into_any_element()
}

/// 键名显示：方向键用符号（呼应设计稿），其余首字母大写（p → P、f2 → F2）。
fn key_label(key: &str) -> String {
    match key {
        "left" => "←".into(),
        "right" => "→".into(),
        "up" => "↑".into(),
        "down" => "↓".into(),
        other => {
            let mut cs = other.chars();
            match cs.next() {
                Some(c) => c.to_uppercase().collect::<String>() + cs.as_str(),
                None => String::new(),
            }
        }
    }
}
