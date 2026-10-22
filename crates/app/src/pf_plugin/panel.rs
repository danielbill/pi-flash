//! 插件面板渲染（beads 360 反馈②④）。**组件树先硬编码快捷面板这棵**
//! （quick.json 两分区结构，设计定案；通用解析器后续抽象）：
//! - 上下两分区：`--- 可执行程序 ---`（isExec=true）/ `--- 可编辑文档 ---`，
//!   宿主按行内 isExec 分组；
//! - 格子 = 图标(40×40 上) + 短名(下)，流式换行，ellipsis 兜底；tip 全路径；
//! - 点击 = 宿主按行内 isExec 分派 run / edit（`$auto` 语义，02 §5）；
//! - 溢出出滚动条（gpui-component Scrollbar，普通 ScrollHandle 直配）。
//! 数据 = 插件 render 缓存的 rows（帧路径不进 JS）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use pf_plugin_host::js::JsAction;

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::appearance::ui_size;
use crate::content::empty_hint;
use crate::ui::tooltip::WithTip;

/// 格子尺寸（40 图标 + 短名一行的最小整卡；bead 360 ④ 定值 40×40 指图标）。
const CELL: f32 = 96.;
const CELL_ICON: f32 = 40.;

/// 插件面板主体（dock 的 `DockPanel::Plugin` 分支）。
pub(crate) fn plugin_panel_view(
    chat: &Chat,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let Some(entry) = chat.plugins.plugins.get(ix) else {
        return empty_hint("plugin", t);
    };

    // 全空 → 居中引导（文案 = 插件数据域，随语言取 zh/en）
    if !entry.rows_loaded {
        return div()
            .id("pf-panel")
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .child(empty_hint("…", t))
            .into_any_element();
    }
    if entry.rows.is_empty() {
        let tip = crate::i18n::tr("plugin.empty_hint");
        return div()
            .id("pf-panel")
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .px(px(12.))
            .child(
                div()
                    .text_size(ui_size(11.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(tip.to_string())),
            )
            .into_any_element();
    }

    // 按 isExec 分上下两栏（01 规范 §10 quick.json 两分区设计）。
    // 要求⑤：各栏独立滚动 + 独立滚动条（非空栏 flex_1 平分剩余高度，
    // 空栏收成组头一行不占空间）。
    let (exec, docs): (
        Vec<&crate::pf_plugin::PanelRow>,
        Vec<&crate::pf_plugin::PanelRow>,
    ) = entry.rows.iter().partition(|r| r.is_exec);

    let mut wrap = div()
        .id("pf-panel")
        .flex_1()
        .min_h_0()
        .relative()
        .flex()
        .flex_col();
    if !exec.is_empty() || !docs.is_empty() {
        if !exec.is_empty() {
            wrap = wrap.child(pane(
                "pf-pane-exec",
                crate::i18n::tr("plugin.exec_group"),
                grid(exec, ix, weak),
                &entry.scroll_exec,
            ));
        }
        if !docs.is_empty() {
            wrap = wrap.child(pane(
                "pf-pane-docs",
                crate::i18n::tr("plugin.doc_group"),
                grid(docs, ix, weak),
                &entry.scroll_docs,
            ));
        }
    } else {
        // 全空（理论上 rows 非空才到这，防御）→ 引导文案
        let tip = crate::i18n::tr("plugin.empty_hint");
        wrap = wrap.child(
            div()
                .flex_1()
                .items_center()
                .justify_center()
                .child(empty_hint(tip, t)),
        );
    }
    wrap.into_any_element()
}

/// 一个分栏：组头 + 独立滚动格子区 + 溢出滚动条（各自管理，要求⑤）。
fn pane(
    id: &'static str,
    label: &str,
    grid: gpui::AnyElement,
    scroll: &gpui::ScrollHandle,
) -> gpui::AnyElement {
    let t = T();
    let mut pane = div()
        .id(SharedString::from(id))
        .flex_1()
        .min_h_0()
        .relative()
        .flex()
        .flex_col()
        .child(group_header(label, t))
        .child(
            div()
                .id(SharedString::from(format!("{id}-scroll")))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .track_scroll(scroll)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .px(px(10.))
                        .pt(px(4.))
                        .pb(px(6.))
                        .child(grid),
                ),
        );
    if scroll.max_offset().height > px(0.) {
        let sb_state = gpui_component::scroll::ScrollbarState::default();
        pane = pane.child(
            div()
                .absolute()
                .top(px(20.)) // 组头之下
                .bottom(px(0.))
                .right(px(3.))
                .w(px(8.))
                .child(gpui_component::scroll::Scrollbar::vertical(
                    &sb_state, scroll,
                )),
        );
    }
    pane.into_any_element()
}

/// 分组标题（model_picker::group_header_row 同形制：小字 SEMIBOLD dim）。
fn group_header(label: &str, t: &'static crate::appearance::theme::Theme) -> gpui::AnyElement {
    div()
        .h(px(20.))
        .px(px(2.))
        .flex()
        .items_center()
        .child(
            div()
                .text_size(ui_size(9.5))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(label.to_string())),
        )
        .into_any_element()
}

/// 格子流式网格（一行放不下自动换行；空组不进这里——组头同隐）。
fn grid(
    rows: Vec<&crate::pf_plugin::PanelRow>,
    ix: usize,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::AnyElement {
    let mut grid = div()
        .flex()
        .flex_wrap()
        .gap(px(crate::ui::tokens::space::SP2));
    for (i, row) in rows.iter().enumerate() {
        grid = grid.child(cell(ix, i, row, weak));
    }
    grid.into_any_element()
}

/// 单个格子：图标(上) + 短名(下)；tip 全路径；点击 $auto 分派。
fn cell(
    ix: usize,
    i: usize,
    row: &crate::pf_plugin::PanelRow,
    weak: &gpui::WeakEntity<Chat>,
) -> gpui::Stateful<gpui::Div> {
    let t = T();
    let name = if row.name.is_empty() {
        row.path
            .rsplit(['\\', '/'])
            .next()
            .unwrap_or(&row.path)
            .to_string()
    } else {
        row.name.clone()
    };
    // fileIcon = 宿主预解析的**完整资产路径**（file_icons::get_icon 返回
    // "icons/file_icons/xxx.svg" 形态）——曾再拼一层 icons/ 前缀致图标空白
    let icon = if row.file_icon.is_empty() {
        "icons/file.svg".to_string()
    } else if row.file_icon.contains('/') {
        row.file_icon.clone()
    } else {
        format!("icons/{}.svg", row.file_icon)
    };
    let path = row.path.clone();
    let is_exec = row.is_exec;
    div()
        .id(SharedString::from(format!("pf-cell-{i}")))
        .w(px(CELL))
        .h(px(CELL))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(crate::ui::tokens::space::SP2))
        .rounded(px(crate::ui::tokens::radius::CTRL))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .with_tip_text(row.path.clone())
        .on_mouse_down(MouseButton::Left, {
            let weak = weak.clone();
            let path = path.clone();
            move |_, window, cx| {
                let _ = weak.update(cx, |c, cx| {
                    // $auto：宿主按 isExec 分派 run / edit，插件不写 if-else
                    let action = if is_exec {
                        JsAction::Run(path.clone())
                    } else {
                        JsAction::Edit(path.clone())
                    };
                    c.plugin_dispatch(ix, vec![action], window, cx);
                });
            }
        })
        // 图标：宿主按 fileIcon 引用渲染（不透明引用，宿主词汇表）
        .child(
            gpui::svg()
                .path(SharedString::from(icon))
                .text_color(rgb(t.text))
                .size(px(CELL_ICON)),
        )
        // 短名（随界面字号缩放 + 超宽省略）
        .child(
            div()
                .max_w(px(CELL - 8.))
                .text_size(ui_size(11.))
                .text_color(rgb(t.text_muted))
                .text_ellipsis()
                .child(SharedString::from(name)),
        )
}
