//! Pi 扩展市场——单张扩展卡及其全部子件（040-1）。
//!
//! 卡片两段式：上段 = 缩略槽 + 名称/描述/元信息；下段 = 链接 …… 命令胶囊 + 动作。
//! 卡片高度量（`card_metrics`）归父模块：page 骨架（行高/骨架占位）也用同一套量。

use gpui::prelude::*;

use gpui::{AnyElement, IntoElement, MouseButton, SharedString, Styled};

use super::Chat;
use super::render::{
    DESC_LINE_H, DESC_LINES, ROW_PAD_R, RowData, RowStatus, THUMB_H, THUMB_W, card_metrics,
};
use crate::appearance::theme::{Theme, theme as T};
use crate::i18n::tr;
use crate::ui::tokens::{radius, space};
use crate::ui::{BtnSize, BtnVariant};

/// 单张包卡（v15）：**两段式**
/// 上段 = 缩略槽 + 名称/描述/元信息；下段 = 链接 …… 命令胶囊 + 动作。
/// 命令胶囊与动作按钮在下段**同一行、只隔 SP4(8)**（原先隔着一整个动作列，
/// 视觉上空一大块）。动作右缘恒贴卡片内容右缘 → 跨行仍然对齐。
pub(super) fn pkg_card(
    row: &RowData,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    busy_any: bool,
) -> AnyElement {
    let has_meta = !row.author.is_empty() || !row.downloads.is_empty() || !row.ago.is_empty();
    let card = gpui::div()
        .id(SharedString::from(format!("pi-ext-card-{}", row.name)))
        .h(gpui::px(card_metrics::card_h()))
        .w_full()
        .mt(gpui::px(space::SP4))
        .flex()
        .items_start()
        .gap(gpui::px(space::SP7))
        .px(gpui::px(space::SP7))
        .py(gpui::px(space::SP5))
        .rounded(gpui::px(radius::MODAL))
        .border_1()
        .border_color(gpui::rgb(t.border))
        .bg(gpui::rgb(t.bg))
        .hover(|s| s.border_color(gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.42))))
        .child(thumb_grid(t))
        .child(
            gpui::div()
                .flex_1()
                .min_w_0()
                .h_full()
                .flex()
                .flex_col()
                .gap(gpui::px(space::SP2))
                // ① 名称 + 版本 + 类型（右端：已装启停开关）
                .child(
                    gpui::div()
                        .flex()
                        .flex_shrink_0()
                        // 顶部对齐：开关/版本徽标贴行顶，使「上边距 = 下边距」
                        // （底行按钮贴卡片底内边距，两者都等于卡片 py）。
                        .items_start()
                        .gap(gpui::px(space::SP4))
                        .min_w_0()
                        .child(name_link(row, t))
                        .children((!row.version.is_empty()).then(|| badge(&row.version, t)))
                        .child(gpui::div().flex_1())
                        .children(switch(row, weak)),
                )
                // ② 描述：**恰好两行**的盒（行高×2，随面板字号缩放）；
                // 右内距 = 动作簇宽 + 8 —— 说明右缘与命令胶囊右缘同线，
                // **绝不伸进按钮空间**（用户定稿）
                .children((!row.desc.is_empty()).then(|| {
                    // 右内距 = 动作簇宽 + 8（与设置-扩展页同口径）：
                    // 描述右缘 ⊥ **命令胶囊右缘**，绝不伸进按钮空间。
                    // 宽度不再限 640：描述应跟到卡片内容右缘为止（按钮列在右）。
                    let acts_w = match row.status {
                        RowStatus::NotInstalled => 40.,    // [安装]
                        RowStatus::UpdateAvailable => 72., // [垃圾][更新]
                        RowStatus::Installed => 24.,       // [垃圾]
                    };
                    gpui::div()
                        .w_full()
                        .pr(gpui::px(acts_w + 8.))
                        .text_size(crate::appearance::ui_size(12.))
                        .line_height(crate::appearance::ui_size(DESC_LINE_H))
                        .text_color(gpui::rgb(t.text_muted))
                        // 硬截行而不是固高盖裁：line_clamp 按行数 shape，
                        // 行高恰好整数行 → 不会出现半行。
                        .line_clamp(DESC_LINES)
                        // 不参与 flex 收缩：卡片高度由内容算（card_metrics），
                        // 若这个盒允许被挤，第三行会被压成半行。
                        .flex_shrink_0()
                        .h(gpui::px(card_metrics::desc_h()))
                        .child(SharedString::from(row.desc.clone()))
                        .into_any_element()
                }))
                // 弹性占位：把底行顶到卡片底内边距上，
                // 保证「上边距 = 下边距 = 卡片 py」恒成立
                .child(gpui::div().flex_1())
                // ③ 底行（v17：在「倒数第二行」的位置，即与标题/描述同列缩进）
                .child(
                    gpui::div()
                        .w_full()
                        .flex()
                        .flex_shrink_0()
                        .items_center()
                        .gap(gpui::px(space::SP4))
                        .children(meta_line(row, t))
                        .when(has_meta, |d| d.child(dot(t)))
                        .child(links(row, t))
                        .child(gpui::div().flex_1())
                        .children(cmd_pill(row, t))
                        .child(actions(row, weak, t, busy_any)),
                ),
        );
    // 行右边距：给 `vlist` 常显滚动条让位（滚动条压在卡片上 = 设计稿禁止）
    gpui::div()
        .w_full()
        .pr(gpui::px(ROW_PAD_R))
        .child(card)
        .into_any_element()
}

/// 缩略槽（设计稿 v28）：88×72 格子纸底纹 + 距底 26% 的 accent 细线。
///
/// 为什么不是图片：npm 搜索不回图；v1 是版式占位，
/// v2 换 `pi.image` 时只需替换本函数内容（尺寸/圆角/裁切不变）。
fn thumb_grid(t: &'static Theme) -> AnyElement {
    const STEP: f32 = 12.;
    let line = || {
        gpui::div()
            .absolute()
            .bg(gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.16)))
    };
    let mut b = gpui::div()
        .relative()
        .w(gpui::px(THUMB_W))
        .h(gpui::px(THUMB_H))
        .flex_shrink_0()
        .rounded(gpui::px(radius::CTRL))
        .border_1()
        .border_color(gpui::rgb(t.border))
        .bg(gpui::rgb(t.bg_panel))
        .overflow_hidden();
    let mut x = STEP;
    while x < THUMB_W {
        b = b.child(line().left(gpui::px(x)).top_0().bottom_0().w(gpui::px(1.)));
        x += STEP;
    }
    let mut y = STEP;
    while y < THUMB_H {
        b = b.child(line().top(gpui::px(y)).left_0().right_0().h(gpui::px(1.)));
        y += STEP;
    }
    let accent_y = (THUMB_H * 0.74).round();
    b.child(
        gpui::div()
            .absolute()
            .left_0()
            .right_0()
            .top(gpui::px(accent_y))
            .h(gpui::px(1.))
            .bg(gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.5))),
    )
    .into_any_element()
}

/// 扩展名称：可点，点开 **pi.dev 详情页**（`pi.dev/packages/<name>`）。
///
/// 为什么是 pi.dev 而不是 npm：市场页的说明页在 pi.dev（模型适用的扩展中心，
/// 有安装指引与适配说明）；npm 页面只有 README。底行的「npm」链接仍指 npm（名字清晰不混）。
/// hover 转 accent，与卡片里其他链接同一反馈。
fn name_link(row: &RowData, t: &'static Theme) -> AnyElement {
    let url = format!("https://pi.dev/packages/{}", row.name);
    gpui::div()
        .id(SharedString::from(format!("pi-ext-name-{}", row.name)))
        .min_w_0()
        .text_size(crate::appearance::ui_size(15.))
        .line_height(crate::appearance::ui_size(20.))
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(gpui::rgb(if row.enabled { t.text } else { t.text_muted }))
        .cursor_pointer()
        .hover(|s| s.text_color(gpui::rgb(t.accent)))
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis()
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            cx.open_url(&url);
        })
        .child(SharedString::from(row.name.clone()))
        .into_any_element()
}

fn badge(text: &str, t: &'static Theme) -> AnyElement {
    gpui::div()
        .flex_shrink_0()
        .h(gpui::px(18.))
        .px(gpui::px(space::SP2))
        .flex()
        .items_center()
        .rounded(gpui::px(radius::XS))
        .border_1()
        .border_color(gpui::rgb(t.border))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(10.))
        .text_color(gpui::rgb(t.text_faint))
        .child(SharedString::from(format!("v{text}")))
        .into_any_element()
}

/// 已装启停开关（卡片行内档 = `switch::SM`：**24×14 / knob 8**）。
///
/// 视觉配方与 `ui::kit::switch_base`（MD 28×16）逐条对齐——1px 轨道描边、
/// 开态 accent、关态 `bg_selected`、关态 knob `text_muted`、开态 knob `bg`；
/// 唯一差别是尺寸走 SM 档（用户定稿：卡片里的开关要小，别用 28×16）。
///
/// 为什么不直接调 `switch_base`：它把 MD 尺寸写死，而 Settings 模块受保护
/// （未经授权不改）。等授权后给 `switch_base` 加 `size: SwitchSize` 参数，
/// 这里改成传入 SM 即可删掉本地绘制。
fn switch(row: &RowData, weak: &gpui::WeakEntity<Chat>) -> Option<AnyElement> {
    use crate::ui::tokens::switch as sw;
    let ix = row.installed_ix?;
    let on = row.enabled;
    let t = T();
    let weak_sw = weak.clone();
    let track: gpui::Hsla = if on {
        gpui::rgba(crate::appearance::theme::rgba_a(t.accent, 0.10)).into()
    } else {
        gpui::rgb(t.bg_panel).into()
    };
    let border: gpui::Hsla = if on {
        gpui::rgb(t.accent).into()
    } else {
        gpui::rgb(t.border).into()
    };
    Some(
        gpui::div()
            // 第一行右端（flex 撑开后由弹黄盒推到最右，右缘 = 内容右缘）
            .child(
                gpui::div()
                    .id(SharedString::from(format!("pi-ext-sw-{}", row.name)))
                    .w(gpui::px(sw::SM_W))
                    .h(gpui::px(sw::SM_H))
                    .flex_shrink_0()
                    .rounded(gpui::px(sw::SM_H / 2.))
                    .border_1()
                    .border_color(border)
                    .bg(track)
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = weak_sw.update(cx, |c, cx| c.mc_toggle_package(ix, cx));
                    })
                    .child(
                        gpui::div()
                            .ml(gpui::px(if on { sw::SM_ON_ML } else { sw::SM_OFF_ML }))
                            .size(gpui::px(sw::SM_KNOB))
                            .rounded_full()
                            .bg(if on {
                                gpui::rgb(t.accent)
                            } else {
                                gpui::rgb(t.text_soft)
                            }),
                    ),
            )
            .into_any_element(),
    )
}

/// 作者 · 下载量/月 · 发布距今天数。本地已装包可能三项全缺 → None（不占位）。
fn meta_line(row: &RowData, t: &'static Theme) -> Option<AnyElement> {
    let mut meta = gpui::div()
        .flex()
        .items_center()
        .gap(gpui::px(space::SP3))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(gpui::rgb(t.text_soft));
    if !row.author.is_empty() {
        meta = meta
            .child(
                gpui::div()
                    .text_color(gpui::rgb(t.text_dim))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(SharedString::from(row.author.clone())),
            )
            .child(dot(t));
    }
    if !row.downloads.is_empty() {
        meta = meta
            .child(
                gpui::div()
                    .flex()
                    .items_center()
                    .gap(gpui::px(space::SP1))
                    .child(crate::ui::icon("download", 11., t.text_soft))
                    .child(SharedString::from(format!("{}/mo", row.downloads))),
            )
            .child(dot(t));
    }
    if !row.ago.is_empty() {
        meta = meta.child(SharedString::from(row.ago.clone()));
    }
    let empty = row.author.is_empty() && row.downloads.is_empty() && row.ago.is_empty();
    if empty {
        None
    } else {
        Some(meta.into_any_element())
    }
}

pub(super) fn dot(t: &'static Theme) -> AnyElement {
    gpui::div()
        .w(gpui::px(2.))
        .h(gpui::px(2.))
        .rounded_full()
        .bg(gpui::rgb(t.text_faint))
        .into_any_element()
}

/// 链接（npm / repo）：下段最左，点击开系统浏览器。
fn links(row: &RowData, t: &'static Theme) -> AnyElement {
    let mut links = gpui::div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(gpui::px(space::SP6))
        .text_size(crate::appearance::ui_size(11.))
        .text_color(gpui::rgb(t.text_soft));

    let npm_url = format!("https://www.npmjs.com/package/{}", row.name);
    links = links.child(
        gpui::div()
            .id(SharedString::from(format!("pi-ext-npm-{}", row.name)))
            .flex()
            .items_center()
            .gap(gpui::px(space::SP1))
            .cursor_pointer()
            .hover(|s| s.text_color(gpui::rgb(t.accent)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                cx.open_url(&npm_url);
            })
            .child(crate::ui::icon("plug", 11., t.text_soft))
            .child("npm"),
    );

    if let Some(repo) = &row.repo {
        let url = repo.clone();
        links = links.child(
            gpui::div()
                .id(SharedString::from(format!("pi-ext-repo-{}", row.name)))
                .flex()
                .items_center()
                .gap(gpui::px(space::SP1))
                .cursor_pointer()
                .hover(|s| s.text_color(gpui::rgb(t.accent)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    cx.stop_propagation();
                    cx.open_url(&url);
                })
                .child(crate::ui::icon("git-branch", 11., t.text_soft))
                .child("repo"),
        );
    }
    links.into_any_element()
}

/// 安装命令胶囊：下段右端、**紧贴动作按钮（间隔 SP4=8）**，仅未安装时显示。
fn cmd_pill(row: &RowData, t: &'static Theme) -> Option<AnyElement> {
    if row.status != RowStatus::NotInstalled {
        return None;
    }
    Some(
        gpui::div()
            .flex_shrink_0()
            .h(gpui::px(22.))
            .max_w(gpui::px(360.))
            .flex()
            .items_center()
            .gap(gpui::px(space::SP2))
            .px(gpui::px(space::SP3))
            .rounded(gpui::px(radius::CTRL))
            .border_1()
            .border_color(gpui::rgb(t.border))
            .bg(gpui::rgb(t.bg_panel))
            .child(
                gpui::div()
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(10.))
                    .text_color(gpui::rgb(t.text_faint))
                    .child("$"),
            )
            .child(
                gpui::div()
                    .min_w_0()
                    .font_family(crate::editor::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(10.))
                    .text_color(gpui::rgb(t.text_dim))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(row.cmd.clone())),
            )
            .into_any_element(),
    )
}

/// 动作列（固定宽 104，永远右对齐）：卸载在最左，其次 更新 / 安装。
fn actions(
    row: &RowData,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    busy_any: bool,
) -> AnyElement {
    let name = row.name.clone();
    let mut group = gpui::div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_end()
        .gap(gpui::px(space::SP4));

    if row.busy {
        group = group.child(crate::ui::spinner(14., t.accent));
        return group.into_any_element();
    }

    // 未安装：只给【安装】
    if row.status == RowStatus::NotInstalled {
        let weak_i = weak.clone();
        let n = name.clone();
        group = group.child(crate::ui::button(
            format!("pi-ext-install-{name}"),
            tr("market.btn.install"),
            BtnSize::Md,
            BtnVariant::Secondary,
            busy_any,
            move |_, _, cx| {
                let n = n.clone();
                let _ = weak_i.update(cx, |c, cx| c.pi_ext_install(n, cx));
            },
        ));
        return group.into_any_element();
    }

    // 已安装：卸载 icon 恒在**最左**（不再有「已安装」文字键）。
    // 垃圾桶：28×28 / 普通描边（v20 用户定稿：市场页不要红色按钮），
    // hover 淡底。确认浮层（mc_ask_remove_package）仍走高危确认。
    let weak_r = weak.clone();
    let n = name.clone();
    group = group.child(
        gpui::div()
            .id(SharedString::from(format!("pi-ext-remove-{name}")))
            .size(gpui::px(crate::ui::tokens::btn::MD_H))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .rounded(gpui::px(radius::CTRL))
            .border_1()
            .border_color(gpui::rgba(crate::appearance::theme::danger_alpha(t, 0x59)))
            .bg(gpui::rgba(crate::appearance::theme::danger_alpha(t, 0x0f)))
            .cursor_pointer()
            .hover(|s| s.bg(gpui::rgba(crate::appearance::theme::danger_wash(t))))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let n = n.clone();
                let _ = weak_r.update(cx, |c, cx| c.pi_ext_remove(n, cx));
            })
            .child(crate::ui::icon("icon-trash-solid", 12., t.danger)),
    );

    // 有新版：更新键；已最新：什么都不加（卸载 icon 已是唯一动作）
    if row.status == RowStatus::UpdateAvailable {
        let weak_u = weak.clone();
        let n = name.clone();
        group = group.child(crate::ui::button(
            format!("pi-ext-update-{name}"),
            tr("market.btn.update"),
            BtnSize::Md,
            BtnVariant::Secondary,
            busy_any,
            move |_, _, cx| {
                let n = n.clone();
                let _ = weak_u.update(cx, |c, cx| c.pi_ext_update(n, cx));
            },
        ));
    }
    group.into_any_element()
}
