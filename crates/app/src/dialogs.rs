//! Modal dialogs: ModelSelect / GitDiff / ImagePreview
//! (pi-web parity surfaces layered over the app root). Free function over
//! Chat state; entity split lands in phase E (ARCHITECTURE.md §2).
use crate::*;

use std::path::PathBuf;

use gpui::{App, Div, MouseButton, SharedString, div, px, relative, rgb};

use crate::Chat;
use crate::Dialog;
use crate::TextInput;
use crate::appearance::theme;
use crate::i18n::{tf, tr};
use crate::session::model_picker::RegistryState;
use crate::ui::{icon, icon_hover};

/// 弹窗公共外壳 = `ui::overlay::layer`（遮挡/外点关闭/ESC 关闭三条全局规则
/// 的唯一实现）+ 居中排布 + 卡片停传播。参数 `chat` 只用来取浮层焦点。
fn dialog_shell(chat: &Chat, weak: &gpui::WeakEntity<Chat>, panel: Div) -> Div {
    let weak_bg = weak.clone();
    crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), move |_w, cx| {
        let _ = weak_bg.update(cx, |c, cx| {
            c.dlg.dialog = None;
            cx.notify();
        });
    })
    .flex()
    .items_center()
    .justify_center()
    .child(crate::ui::overlay::stop_click(panel))
}

pub(crate) fn render_dialogs(
    mut root: Div,
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &theme::Theme,
    cx: &App,
) -> Div {
    // dialogs mount as a CHILD of the chat root — on top of the content,
    // never replacing it (replacing the root blanks the whole UI behind
    // the dialog; settings parity = content stays visible beneath)
    if let Some(Dialog::ProviderPicker { input, scroll }) = chat.dlg.dialog.as_ref() {
        root = root.child(render_provider_picker(chat, weak, input, scroll, t, cx));
    }
    if let Some(Dialog::GitDiff { path, patch }) = chat.dlg.dialog.as_ref() {
        root = root.child(render_git_diff(chat, weak, path, patch, t, cx));
    }
    if let Some(Dialog::ProjectPicker {
        input,
        fresh,
        scroll,
    }) = chat.dlg.dialog.as_ref()
    {
        root = root.child(render_project_picker(
            chat, weak, input, *fresh, scroll, t, cx,
        ));
    }
    if let Some(Dialog::ImagePreview { image }) = chat.dlg.dialog.as_ref() {
        root = root.child(render_image_preview(chat, weak, image, t));
    }
    if let Some(Dialog::SessionInfo { kind }) = chat.dlg.dialog.as_ref() {
        root = root.child(crate::top_panels::session_info_dialog(
            *kind, chat, weak, t, cx,
        ));
    }
    if let Some(Dialog::FileDirty { paths }) = chat.dlg.dialog.as_ref() {
        root = root.child(render_file_dirty(chat, weak, paths, t));
    }
    if let Some(Dialog::WxQr) = chat.dlg.dialog.as_ref() {
        let panel = crate::remote_control::panel::wx_qr_panel(chat, weak, t);
        root = root.child(dialog_shell(chat, weak, panel));
    }
    root
}

/// 023 fileView：关闭带未保存修改的文件 tab 前确认（保存并关闭 / 不保存
/// 关闭 / 取消，对齐 Zed 的三选）。
fn render_file_dirty(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    paths: &[PathBuf],
    t: &theme::Theme,
) -> Div {
    let _ = chat;
    let weak_save = weak.clone();
    let weak_drop = weak.clone();
    let weak_cancel = weak.clone();
    let p_save: Vec<PathBuf> = paths.to_vec();
    let p_drop: Vec<PathBuf> = paths.to_vec();
    // 单文件照旧显示文件名；批量（标签右键 Close Others/All 收拢）显示
    // 数量 + 前 3 个名字（zed「N unsaved」同款语义，三键对整批生效）
    let name: SharedString = match paths {
        [p] => p
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
            .into(),
        many => {
            let shown: Vec<String> = many
                .iter()
                .take(3)
                .map(|p| {
                    p.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| p.display().to_string())
                })
                .collect();
            let more = if many.len() > 3 { " …" } else { "" };
            format!(
                "{} {}{}",
                tf("conflict.unsaved_n_files", &[("n", many.len().to_string())]),
                shown.join("、"),
                more
            )
            .into()
        }
    };
    let btn = |id: &'static str, label: &'static str, accent: bool| {
        div()
            .id(id)
            .px(px(12.))
            .py(px(5.))
            .rounded(px(7.))
            .text_size(crate::appearance::ui_size(12.))
            .cursor_pointer()
            .when(accent, |d| {
                d.bg(rgb(t.accent)).text_color(rgb(t.accent_contrast))
            })
            .when(!accent, |d| d.border_1().border_color(rgb(t.border)))
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .child(SharedString::from(label.to_string()))
    };
    dialog_shell(
        chat,
        weak,
        div()
            .w(px(380.))
            .p(px(18.))
            .bg(rgb(t.bg))
            .border_1()
            .border_color(gpui::rgba(theme::border_alpha(t, 0x8c)))
            .rounded(px(10.))
            .shadow_lg()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(13.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(tr("conflict.unsaved_count").to_string())),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(format!(
                        "{} {name}",
                        tr("conflict.unsaved_list")
                    ))),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        btn("fv-dirty-cancel", tr("action.cancel"), false).on_mouse_down(
                            gpui::MouseButton::Left,
                            move |_, _, cx| {
                                let _ = weak_cancel.update(cx, |c, cx| {
                                    c.dlg.dialog = None;
                                    cx.notify();
                                });
                            },
                        ),
                    )
                    .child(
                        btn("fv-dirty-drop", tr("conflict.close_without_saving"), false)
                            .on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
                                let _ = weak_drop.update(cx, |c, cx| {
                                    // 不保存关闭 = 整批丢弃（批量收拢时对全部
                                    // 脏文件生效；单关时集合只有一个）
                                    for p in &p_drop {
                                        c.discard_file_tab(p, cx);
                                    }
                                    c.dlg.dialog = None;
                                    cx.notify();
                                });
                            }),
                    )
                    .child(
                        btn("fv-dirty-save", tr("conflict.save_and_close"), true).on_mouse_down(
                            gpui::MouseButton::Left,
                            move |_, _, cx| {
                                let _ = weak_save.update(cx, |c, cx| {
                                    for p in &p_save {
                                        c.save_file(p, cx);
                                        c.discard_file_tab(p, cx);
                                    }
                                    c.dlg.dialog = None;
                                    cx.notify();
                                });
                            },
                        ),
                    ),
            ),
    )
}

/// 042 ProviderPicker（对齐 pi-web AddProviderPicker 内容与版式）：近满窗
/// 大面板 + 顶部全宽搜索（分隔线压底）+ 滚动卡片网格。数据 = SDK 全量
/// 注册表 dump（registry.rs：打开弹窗时后台拉起，vendor VERSION 缓存）：
/// 「API KEY」= 声明 apiKey.login 且无 api_key 凭据，副标题 N models（全量
/// registry 计数）→ 点击进详情区 API KEY 表单；「订阅服务」= 声明 oauth
/// 且无 oauth 凭据，副标题 OAuth，点击提示未接入（OAuth 登录缓行，
/// 051/beads pi-flash-1cb）。组序 2026-10-10 用户定稿：API KEY → 订阅
/// 服务（自定义组已移除：PF 只提供内置 + 订阅，存量自定义商走左栏合并
/// 列表既有编辑器）；组内保持 registry 原始顺序不排序，已配置的 provider
/// 不列（pi-web !configured / !loggedIn 同款）。
fn render_provider_picker(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    input: &gpui::Entity<TextInput>,
    scroll: &gpui::ScrollHandle,
    t: &theme::Theme,
    cx: &App,
) -> Div {
    let q = input.read(cx).value().trim().to_lowercase();

    let dump = chat.cfg.provider_registry.clone();
    let reg_state = chat.cfg.registry_state.clone();

    // PF 自定义账本的 provider 不进 API KEY 网格（pi-web 排除 custom
    // sources 同款：自定义入口只有「自定义」卡）
    let custom_names: Vec<String> = pi_link::models_json::providers(&chat.cfg.mc_models_json)
        .into_iter()
        .map(|(n, _)| n)
        .collect();

    // API KEY 组：registry 原始顺序（不排序，对齐 pi-web）；匹配显示名或 id。
    // 已配置（auth.json 有 key）不列——**被【移除】的商除外**：移除删的是
    // PF 账本（pf-auth）的 key，pi 的 auth.json 里旧 key 仍在（PF 不写 pi
    // 文件），但用户必须能从弹窗重新配置 → 除名商无视 configured 过滤。
    let removed = crate::services::workspace::removed_providers();
    let api_cards: Vec<(String, String, usize)> = match &dump {
        Some(d) => d
            .providers
            .iter()
            .filter(|p| {
                p.has_api_key_login
                    && (d.credential(&p.id) != Some("api_key")
                        || removed.iter().any(|r| r == &p.id))
            })
            .filter(|p| !custom_names.iter().any(|n| n == &p.id))
            .filter(|p| {
                q.is_empty()
                    || p.name.to_lowercase().contains(&q)
                    || p.id.to_lowercase().contains(&q)
            })
            .map(|p| (p.id.clone(), p.name.clone(), d.model_count(&p.id)))
            .collect(),
        None => Vec::new(),
    };
    // 订阅服务组：只匹配 OAuth 显示名（pi-web 同款）
    let oauth_cards: Vec<(String, String)> = match &dump {
        Some(d) => d
            .oauth_providers()
            .iter()
            .filter(|p| {
                q.is_empty()
                    || pi_link::registry::RegistryDump::oauth_display_name(p)
                        .to_lowercase()
                        .contains(&q)
            })
            .map(|p| {
                (
                    p.id.clone(),
                    pi_link::registry::RegistryDump::oauth_display_name(p).to_string(),
                )
            })
            .collect(),
        None => Vec::new(),
    };

    let group_header = |label: &'static str, top_pad: f32| {
        div()
            .pt(px(top_pad))
            .pb(px(6.))
            .text_size(crate::appearance::ui_size(11.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(rgb(t.text_dim))
            .child(tr(label))
    };

    // 自定义组已移除（2026-10-10 用户定稿：PF 只提供内置 + 订阅；存量自定义
    // 商仍走左栏合并列表的既有编辑器）

    let status_row = |text: String| {
        div()
            .py(px(8.))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text_dim))
            .child(SharedString::from(text))
    };
    // 卡片公共 chrome（pi-web cardStyle）：w292 圆角10 边框，hover 描边强调
    let card = |id: String| {
        div()
            .id(SharedString::from(id))
            .w(px(292.))
            .p(px(14.))
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.bg))
            .cursor_pointer()
            .hover(|h| h.border_color(rgb(t.accent)).bg(rgb(t.bg_hover)))
    };
    // 卡片文本块：标题 + 副标题（左侧 flex_1）
    let card_text = |title: &str, sub: String| {
        div()
            .flex_1()
            .min_w_0()
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.5))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(title.to_string())),
            )
            .child(
                div()
                    .mt(px(2.))
                    .text_size(crate::appearance::ui_size(10.5))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(sub)),
            )
    };

    let mut body = div().flex().flex_col();
    // 注册表未就绪/失败：清单区给状态行
    match (&dump, &reg_state) {
        (None, state) if !matches!(state, RegistryState::Failed(_)) => {
            body = body.child(status_row(tr("model.loading_registry").to_string()));
        }
        (None, RegistryState::Failed(e)) => {
            body = body.child(status_row(crate::i18n::tf(
                "models.registry_read_failed",
                &[("e", e.clone())],
            )));
        }
        _ => {}
    }

    // 组序（2026-10-10 用户定稿）：API KEY → 订阅服务（自定义组已移除）
    if !api_cards.is_empty() {
        body = body.child(group_header("API KEY", 0.));
        let mut grid = div().flex().flex_wrap().justify_center().gap(px(10.));
        for (p, name, total) in &api_cards {
            let weak_card = weak.clone();
            let pid = p.clone();
            let el = card(format!("pp-{p}"))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let pid = pid.clone();
                    let _ = weak_card.update(cx, |c, cx| {
                        c.dlg.dialog = None;
                        c.mc_select_provider(pid, cx);
                        cx.notify();
                    });
                })
                .flex()
                .items_center()
                .gap(px(10.))
                .child(card_text(
                    name,
                    crate::i18n::tf("model.count", &[("n", total.to_string())]),
                ))
                .child(crate::ui::provider_icon(p, 24., t.text_muted));
            grid = grid.child(el);
        }
        body = body.child(grid);
    }
    if !oauth_cards.is_empty() {
        body = body.child(group_header(tr("dialogs.subscription_services"), 14.));
        let mut grid = div().flex().flex_wrap().justify_center().gap(px(10.));
        for (p, name) in &oauth_cards {
            let weak_card = weak.clone();
            let el = card(format!("pp-oauth-{p}"))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_card.update(cx, |c, cx| {
                        // OAuth 登录缓行（051/beads pi-flash-1cb）：提示不关弹窗
                        c.set_status(crate::i18n::tr("model.oauth_unavailable").to_string(), cx);
                    });
                })
                .flex()
                .items_center()
                .gap(px(10.))
                .child(card_text(name, "OAuth".to_string()))
                .child(crate::ui::provider_icon(p, 24., t.text_muted));
            grid = grid.child(el);
        }
        body = body.child(grid);
    }
    if dump.is_some() && api_cards.is_empty() && oauth_cards.is_empty() {
        body = body.child(status_row(tr("model.no_provider_match").to_string()));
    }

    // 近满窗大面板：搜索条压顶（分隔线），下方滚动区吃满剩余高度
    let panel = div()
        .w(px(980.))
        .h(relative(0.88))
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(12.))
        .overflow_hidden()
        .flex()
        .flex_col()
        .shadow_lg()
        .child(
            div()
                .px(px(16.))
                .py(px(10.))
                .border_b_1()
                .border_color(rgb(t.border))
                .child(input.clone()),
        )
        .child(
            // 滚动区 + 常显滚动条（vlist fill_wrapper 同款：relative 壳挂兄弟滚动条）
            div()
                .flex_1()
                .min_h_0()
                .relative()
                .child(
                    div()
                        .id("pp-body")
                        .flex_1()
                        .min_h_0()
                        .h_full()
                        .overflow_y_scroll()
                        .track_scroll(scroll)
                        .px(px(20.))
                        .pt(px(12.))
                        .pb(px(16.))
                        .child(body),
                )
                .child(crate::ui::psp_scrollbar::menu_scrollbar(scroll)),
        );
    dialog_shell(chat, weak, panel)
}

/// 013 sessionSearchDialog + sessionSearchResultView: query on top, results
/// grouped by session below; a row click switches sessions and reveals the
/// GitDiff dialog surface (extracted from render_dialogs).
fn render_git_diff(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    path: &PathBuf,
    patch: &String,
    t: &theme::Theme,
    _cx: &App,
) -> Div {
    let path_text: SharedString = path.to_string_lossy().to_string().into();
    let mut body = patch.clone();
    if body.chars().count() > 60000 {
        body = body.chars().take(60000).collect();
        body.push_str("\n\n\u{2026} (truncated)");
    }
    let panel = div()
        .w(px(760.))
        .max_h(px(640.))
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(10.))
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .shadow_lg()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .child(icon_hover("git-branch", 12., t.accent))
                        .child(
                            div()
                                // §2 mono 辅助标签档（原 text_xs 裸字号）
                                .text_size(crate::appearance::ui_size(11.))
                                .font_family(crate::editor::markdown::MONO_FAMILY)
                                .text_color(rgb(t.text_muted))
                                .child(path_text),
                        ),
                )
                .child(
                    div()
                        .id("diff-close")
                        .px_2()
                        .cursor_pointer()
                        .text_color(rgb(t.text_muted))
                        .hover(|s| s.text_color(rgb(t.text)))
                        .on_mouse_down(MouseButton::Left, {
                            let weak = weak.clone();
                            move |_, _, cx| {
                                let _ = weak.update(cx, |c, cx| {
                                    c.dlg.dialog = None;
                                    cx.notify();
                                });
                            }
                        })
                        .child(icon_hover("x", 13., t.text_muted)),
                ),
        )
        .child(
            div()
                .flex_1()
                .max_h(px(520.))
                .overflow_hidden()
                .p_2()
                .rounded(px(6.))
                .bg(rgb(t.bg))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text))
                .child(SharedString::from(body)),
        );
    dialog_shell(chat, weak, panel)
}

/// composer 缩略图点击大图预览（v58）：dialog_shell 金标准外壳（点外关闭
/// /ESC/遮挡），图片居中按 max 限宽高等比缩放（messages.rs 结果图同款，
/// img 尊重 max 约束）；无头部控件——ESC 或点击弹窗外任意处关闭。
fn render_image_preview(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    image: &std::sync::Arc<gpui::Image>,
    t: &theme::Theme,
) -> Div {
    let weak_close = weak.clone();
    // 图片预览也要有看得见的关闭按钮（浮层规则 5）：× 绝对定位在卡片右上角
    let close = crate::ui::overlay::close_btn("image-preview-close", t, move |_w, cx| {
        let _ = weak_close.update(cx, |c, cx| {
            c.dlg.dialog = None;
            cx.notify();
        });
    });
    let panel = div()
        .relative()
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(10.))
        .p_2()
        .shadow_lg()
        .child(crate::session::messages::fitted_image(image.clone(), 1040., 680.).rounded(px(6.)))
        .child(div().absolute().top(px(6.)).right(px(6.)).child(close));
    dialog_shell(chat, weak, panel)
}

/// 004 projectManager 打开项目菜单：500×500 居中卡片——顶部搜索框，
/// 【打开文件夹】行（走 psp 目录选择器），下面是最近 30 天活动项目列表
/// （字母序、行高 40、吃满剩余高度超出滚动）。当前项目行尾打勾（004：从某项目
/// 新建会话打开时默认选中）。列表数据 `project_hits` 由打开器后台扫描
/// 回填，本函数只做搜索词过滤。键盘：↑/↓ 在【打开文件夹】+项目行上移动
/// （0 = 打开文件夹，默认选中当前项目行、无则回退打开文件夹行），Enter
/// 激活（模型菜单同款卡片拦截——单行输入的 Enter 会向上冒泡）。
fn render_project_picker(
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
        let selected = chat.ws.cwd.as_deref().is_some_and(|cur| {
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
                        if fresh {
                            c.new_session_in(path.clone(), cx);
                        } else {
                            c.switch_project(path.clone(), cx);
                        }
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
            let _ = weak_open.update(cx, |c, cx| c.pick_project_folder(fresh, cx));
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
// 弹窗打开器 + 会话搜索（P2 归并自 actions_dialogs.rs）
// ---------------------------------------------------------------------------

impl Chat {
    pub(crate) fn open_git_diff(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // 无项目态：没有 git 仓库可 diff（git 面本为空，防御）
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let untracked = self
            .ws
            .git_files
            .iter()
            .any(|f| f.path == path && f.status == GitStatus::Untracked);
        let patch = git_file_diff(&cwd, &path, untracked);
        self.dlg.dialog = Some(Dialog::GitDiff { path, patch });
        cx.notify();
    }

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
        if fresh {
            self.new_session_in(path, cx);
        } else {
            self.switch_project(path, cx);
        }
    }

    /// 打开项目菜单的默认选中：当前项目（同 ws）在过滤结果里的行号 +1；
    /// 不在列（列表未回填 / 被搜索词滤掉）但有项目行时取首个项目行，连
    /// 项目行都没有才落【打开文件夹】。打开、列表回填、改过滤词三处共用。
    pub(crate) fn project_sel_default(&self) -> usize {
        let visible = project_visible(self);
        let cur = visible.iter().position(|&ix| {
            self.ws.cwd.as_deref().is_some_and(|cur| {
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

    /// Jump to a search hit: open (or switch to) the session, then reveal the
    /// matched row — located by payload timestamp, falling back to the first
    /// message containing the needle.
    pub(crate) fn jump_to_hit(&mut self, path: PathBuf, ts: Option<i64>, cx: &mut Context<Self>) {
        let needle = self.psp.search_needle.clone();
        let key = path.to_string_lossy().to_string();
        let already_open = self.pool.active_key == key || self.pool.runtimes.contains_key(&key);
        if self.pool.active_key != key {
            self.open_session(path.clone(), false, cx);
        }
        let rt = self.rt();
        let located = rt.update(cx, |r, cx| r.locate_message(ts, &needle, cx));
        if !located && !already_open {
            // messages still loading (pool miss) — apply after the reconcile
            self.psp.pending_locate = Some((path, ts, needle));
        }
        cx.notify();
    }

    /// 会话信息弹窗（系统提示词 / 工具定义，窗体 = 设置弹窗那套大卡片）
    /// 开关。入口两处（2026-10-16）：会话功能栏保留项之外，主入口迁到
    /// 上下文用量面板的入口行（session/input/ctx_tip.rs `ctx_actions_row`）。
    /// 同一面再点一次收起，换面直接切。
    ///
    /// 数据不在这里拉：pi 0.86+ 把系统提示词与工具声明写进 transcript 的
    /// system 消息，runtime 每次 get_messages 都 replay 一份（见
    /// `pi_link::transcript::transcript_system`），所以打开即是最新。
    pub(crate) fn open_session_info(&mut self, kind: crate::TopPanel, cx: &mut Context<Self>) {
        self.dlg.dialog = if self.session_info_open(kind) {
            None
        } else {
            Some(Dialog::SessionInfo { kind })
        };
        cx.notify();
    }

    /// 该面的弹窗是否正开着（会话功能栏点亮用）。
    pub(crate) fn session_info_open(&self, kind: crate::TopPanel) -> bool {
        matches!(self.dlg.dialog, Some(Dialog::SessionInfo { kind: k }) if k == kind)
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
