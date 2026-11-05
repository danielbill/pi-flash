//! psp 删除确认/错误浮层：文件树删除确认（zed build_removal_prompt 同款，
//! 未保存修改并入确认）+ 删除失败持久弹层（trash Err 必看）。
//! 自 psp_overlays.rs 切出（itlq.12 超规拆分，纯结构搬移）。

use gpui::{div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::Theme;

/// 删除失败持久弹层：trash Err 必看（状态栏 toast 一闪容易漏）。
pub(crate) fn tree_del_error_layer(
    chat: &mut Chat,
    t: &'static Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let msg = chat.dlg.tree_del_error.clone().expect("checked by caller");
    let weak_ok = weak.clone();
    let layer = crate::ui::menu::menu_layer(Some(&chat.dlg.dialog_focus), &weak, |c, _cx| {
        c.dlg.tree_del_error = None;
    });
    let card = crate::ui::overlay::stop_click(
        div()
            .min_w(px(320.))
            .p(px(14.))
            .bg(rgb(t.bg))
            .border_1()
            .border_color(gpui::rgba(crate::appearance::theme::danger_alpha(t, 0x73)))
            .rounded(px(10.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.5))
                    .text_color(rgb(t.text))
                    .child(crate::i18n::tf(
                        "extensions.remove_failed_msg",
                        &[("msg", msg)],
                    )),
            )
            .child(div().flex().justify_end().child(crate::ui::button(
                "del-err-ok",
                crate::i18n::tr("extensions.ok"),
                crate::ui::BtnSize::Md,
                crate::ui::BtnVariant::Secondary,
                false,
                move |_, _, cx| {
                    let _ = weak_ok.update(cx, |c, cx| {
                        c.dlg.tree_del_error = None;
                        cx.notify();
                    });
                },
            ))),
    );
    layer
        .flex()
        .items_center()
        .justify_center()
        .child(card)
        .into_any_element()
}

/// 删除确认：统一 overlay::confirm（自动居中 + 取消/确认两键）。
pub(crate) fn tree_delete_confirm(
    path: &std::path::PathBuf,
    dirty_count: usize,
    focus: &gpui::FocusHandle,
    cx: &mut gpui::Context<Chat>,
) -> gpui::AnyElement {
    let weak = cx.entity().downgrade();
    let weak_cancel = weak.clone();
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    // zed 同款：打开中的脏文件删除时明确「未保存更改将丢失」，一次确认
    // = 删文件 + 弃缓冲（discard_file_tab 强关，不再二次确认）；目录删除
    // 报其子树内全部受影响的脏 tab 数
    let message = if dirty_count > 0 {
        crate::i18n::tf(
            "tree.delete_confirm_dirty",
            &[("name", name), ("n", dirty_count.to_string())],
        )
    } else {
        crate::i18n::tf("tree.delete_confirm", &[("name", name)])
    };
    crate::ui::overlay::confirm(
        focus,
        message,
        move |_w, cx| {
            let _ = weak_cancel.update(cx, |c, cx| {
                c.dlg.tree_confirm_del = None;
                cx.notify();
            });
        },
        // 确认走统一方法（取槽→清槽→commit），自动化 op file.delete_confirm
        // 也调它——回调里不再自持路径副本
        move |_w, cx| {
            let _ = weak.update(cx, |c, cx| c.tree_delete_confirm_commit(cx));
        },
    )
}
