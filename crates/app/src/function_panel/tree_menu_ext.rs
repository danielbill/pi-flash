//! 文件树右键菜单的【扩展菜单】项（beads 360 反馈④）：主行 hover 右侧
//! 浮现插件贡献的菜单项（contributes.file_menu）二级子卡。自 tree_menu.rs
//! 切出（itlq.12 超规拆分，纯结构搬移）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;

use super::psp_overlays::{menu_item, menu_item_asset};

/// 【扩展菜单】项（beads 360 反馈④）：hover 右侧浮现插件贡献的菜单项。
/// 开合纪律：主项 hover 离开**不关**（主项→子卡间隙防闪关），子卡 hover
/// 离开才关；点外收菜单时随 `tree_menu = None` 一并归 false。
pub(crate) fn ext_menu_entry(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    path: &std::path::PathBuf,
) -> gpui::Stateful<gpui::Div> {
    let t = T();
    let label = tr("file_tree.ext_menu").to_string();
    let open = chat.dlg.plugin_submenu;

    // 子卡：各插件贡献的 file_menu 项（menu_card 同形制，右浮于主行旁）
    // 高度上限 = 主菜单实测高度（measure_height 每帧写入），条目超出则内部滚动
    let cap = (chat.dlg.tree_menu_h.get() - 10.).max(120.);
    let mut sub = div()
        .id("ftm-plugin-ext-sub")
        .max_h(px(cap))
        .overflow_y_scroll()
        .track_scroll(&chat.dlg.tree_sub_scroll)
        .absolute()
        // 子卡锚定（§5.6：沿父卡右缘 +4）：left 100% 锚主行右缘，
        // ml 8 = 主卡右留白 4 + 规范偏移 4 → 间隙恒 4px，不随主卡实宽
        // 浮动（原 left(202) 相对行定位、主卡被长文案撑宽即穿帮）
        .left(gpui::relative(1.))
        .ml(px(8.))
        .top(px(-5.))
        .min_w(px(180.))
        .p(px(crate::ui::tokens::space::SP2))
        .bg(rgb(t.bg))
        .border_1()
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
        .rounded(px(crate::ui::tokens::radius::CARD))
        .shadow_lg()
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
    let weak_sub = weak.clone();
    sub = sub.on_hover(move |hovered, _w, cx| {
        let _ = weak_sub.update(cx, |c, cx| {
            c.dlg.plugin_submenu = *hovered;
            cx.notify();
        });
    });
    for (pix, entry) in chat.plugins.plugins.iter().enumerate() {
        if entry.disabled {
            continue;
        }
        for fm in &entry.manifest.file_menu {
            let item_label = crate::pf_plugin::pick_lang(&fm.title_zh, &fm.title_en);
            let item_id = SharedString::from(format!("ftm-plugin-{}-{}", entry.manifest.id, fm.id));
            // 图标可以是插件包内资产（`assets:bolt.svg` / `assets/x.svg`）——
            // 之前只认宿主内置名，插件想用自带图标就得往 PF 里塞 SVG
            let icon = crate::pf_plugin::plugin_icon_asset(&entry.manifest.id, &fm.icon);
            let weak_item = weak.clone();
            let item_path = path.clone();
            let menu_id = fm.id.clone();
            sub = sub.child(
                menu_item_asset(item_id, icon, 14., &item_label, false, false)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        cx.stop_propagation();
                        let _ = weak_item.update(cx, |c, cx| {
                            c.plugin_menu_fire(pix, &menu_id, &item_path, window, cx);
                            c.dlg.tree_menu = None;
                            c.dlg.plugin_submenu = false;
                            cx.notify();
                        });
                    }),
            );
        }
    }

    // 主行：复用 menu_item（与 tree_item 同款形制），hover 开子卡
    let weak_main = weak.clone();
    menu_item("ftm-plugin-ext", "puzzle", &label, false, false)
        .relative()
        .on_hover(move |hovered, _w, cx| {
            if !*hovered {
                return; // 主项离开不关：防主项→子卡间隙闪关
            }
            let _ = weak_main.update(cx, |c, cx| {
                c.dlg.plugin_submenu = true;
                cx.notify();
            });
        })
        .when(open, |d| d.child(sub))
}
