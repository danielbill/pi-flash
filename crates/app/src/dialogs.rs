//! Modal dialogs: ModelSelect / GitDiff / SessionSearch / ImagePreview
//! (pi-web parity surfaces layered over the app root). Free function over
//! Chat state; entity split lands in phase E (ARCHITECTURE.md §2).

use std::path::PathBuf;

use gpui::{App, Div, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Dialog;
use crate::Chat;
use crate::TextInput;
use crate::i18n::tr;
use crate::session::model_picker::RegistryState;
use crate::services::format::time_ago;
use crate::theme;
use crate::ui::{icon, icon_hover};

/// 弹窗公共外壳 = `ui::overlay::layer`（遮挡/外点关闭/ESC 关闭三条全局规则
/// 的唯一实现）+ 居中排布 + 卡片停传播。参数 `chat` 只用来取浮层焦点。
fn dialog_shell(chat: &Chat, weak: &gpui::WeakEntity<Chat>, panel: Div) -> Div {
    let weak_bg = weak.clone();
    crate::ui::overlay::layer(
        true,
        Some(&chat.dialog_focus),
        move |_w, cx| {
            let _ = weak_bg.update(cx, |c, cx| {
                c.dialog = None;
                cx.notify();
            });
        },
    )
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
            if let Some(Dialog::ProviderPicker { input }) = chat.dialog.as_ref() {
                root = root.child(render_provider_picker(chat, weak, input, t, cx));
            }
            if let Some(Dialog::GitDiff { path, patch }) = chat.dialog.as_ref() {
                root = root.child(render_git_diff(chat, weak, path, patch, t, cx));
            }
            if let Some(Dialog::SessionSearch { input }) = chat.dialog.as_ref() {
                root = root.child(render_session_search(chat, weak, input, t, cx));
            }
            if let Some(Dialog::ProjectPicker { input, fresh, scroll }) = chat.dialog.as_ref() {
                root = root.child(render_project_picker(chat, weak, input, *fresh, scroll, t, cx));
            }
            if let Some(Dialog::ImagePreview { image }) = chat.dialog.as_ref() {
                root = root.child(render_image_preview(chat, weak, image, t));
            }
            if let Some(Dialog::SessionInfo { kind }) = chat.dialog.as_ref() {
                root = root.child(crate::top_panels::session_info_dialog(
                    *kind,
                    chat,
                    weak,
                    t,
                    cx,
                ));
            }
            if let Some(Dialog::FileDirty { path }) = chat.dialog.as_ref() {
                root = root.child(render_file_dirty(chat, weak, path, t));
            }
            if let Some(Dialog::WxQr) = chat.dialog.as_ref() {
                root = root.child(render_wx_qr(chat, weak, t));
            }
    root
}

/// 060 远程控制：扫码弹窗（原手机图标入口已隐藏 2026-10-10，自动化
/// WX_QR_OPEN 仍可打开）。
///
/// **窗体复用设置弹窗那套大卡片**（`ui::overlay::big_card`）—— top_panels 的
/// 头注写明「不要再自造窗体」，这里照做：`layer` 负责遮挡/外点/ESC，
/// `big_card` 负责 chrome 顶条 + × + 0.7×0.98 尺寸。
///
/// **不持数据** —— 内容全从 `chat.remote.qr` 现读，扫码 worker 的事件由 200ms
/// 泵 drain 后 `cx.notify()`，弹窗随之重绘。
fn render_wx_qr(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &theme::Theme,
) -> gpui::AnyElement {
    use crate::remote_control::QrState;
    use crate::settings::remote::mono_lines;

    let body_text = |text: String, dim: bool| -> gpui::AnyElement {
        div()
            .text_size(crate::appearance::ui_size(12.))
            .text_color(rgb(if dim { t.text_dim } else { t.text }))
            .child(SharedString::from(text))
            .into_any_element()
    };
    let action_btn = |id: &'static str, label: &'static str, accent: bool, go: bool| {
        let weak_b = weak.clone();
        div()
            .id(id)
            .px(px(12.))
            .py(px(6.))
            .rounded(px(7.))
            .text_size(crate::appearance::ui_size(12.))
            .cursor_pointer()
            .when(accent, |d| {
                d.bg(rgb(t.accent)).text_color(rgb(t.accent_contrast))
            })
            .when(!accent, |d| {
                d.border_1().border_color(rgb(t.border))
            })
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .child(SharedString::from(label.to_string()))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_b.update(cx, |c, cx| {
                    if go {
                        c.remote.begin_qr(); // 幂等：已在扫码/已出码时不重发
                    } else {
                        c.dialog = None;
                    }
                    cx.notify();
                });
            })
            .into_any_element()
    };

    // QR 用**实心方块**画，不用字符画：字形抗锯齿会在模块之间留细缝，
    // 手机容易解不出来（用户实测扫不上）。按行把连续深色段合并成一个方块，
    // 49x49 的网格约 600 个元素，不是 2401 个。
    let qr_grid = |url: &str| -> gpui::AnyElement {
        const MOD: f32 = 8.; // 8px/模块；49 模块 = 392px，正好塞进 0.7 宽大卡片
        const DARK: u32 = 0x111827;
        let grid = match wxprobe::qr::qr_grid(url, 4) {
            Ok(g) => g,
            Err(e) => return body_text(format!("二维码生成失败：{e}"), false),
        };
        let side = grid.size as f32 * MOD;
        let mut canvas = div()
            .w(px(side))
            .h(px(side))
            .flex_shrink_0()
            .bg(rgb(0xff_ffff))
            .flex()
            .flex_col();
        for y in 0..grid.size {
            let mut row = div().h(px(MOD)).flex().flex_row().flex_shrink_0();
            let mut cursor = 0usize;
            for (x, len) in grid.runs(y) {
                if x > cursor {
                    row = row.child(div().w(px((x - cursor) as f32 * MOD)).h(px(MOD)));
                }
                row = row.child(
                    div()
                        .w(px(len as f32 * MOD))
                        .h(px(MOD))
                        .bg(rgb(DARK)),
                );
                cursor = x + len;
            }
            if cursor < grid.size {
                row = row.child(div().w(px((grid.size - cursor) as f32 * MOD)).h(px(MOD)));
            }
            canvas = canvas.child(row);
        }
        div().flex().justify_center().child(canvas).into_any_element()
    };

    let mut body = div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(12.))
        .py(px(8.))
        .child(body_text("远程控制 · 微信".into(), true));

    match &chat.remote.qr {
        QrState::Idle => {
            body = body.child(body_text(
                "点下面的按钮取二维码，用手机微信扫一扫。".into(),
                true,
            ));
            body = body.child(action_btn("wx-qr-begin", "获取二维码", true, true));
        }
        QrState::Loading => body = body.child(body_text("正在获取二维码…".into(), true)),
        QrState::Ready { url } => {
            body = body.child(qr_grid(url));
            body = body.child(body_text(
                "用手机微信「扫一扫」对屏扫码；二维码约 2 分钟后过期。".into(),
                true,
            ));
            body = body.child(action_btn("wx-qr-refresh", "重新获取", false, true));
        }
        QrState::Scanned => body = body.child(body_text("已扫码，请在手机上确认…".into(), true)),
        QrState::Done { bot_id } => {
            let mut lines = vec!["✅ 绑定成功".to_string()];
            if let Some(b) = bot_id {
                lines.push(format!("bot_id = {b}"));
            }
            body = body.child(
                div().flex().justify_center().child(mono_lines(lines, 12., false)).into_any_element(),
            );
            body = body.child(body_text(
                "接下来在微信里发 /task 选一个会话，即可开始聊天。".into(),
                true,
            ));
        }
        QrState::Expired => {
            body = body.child(body_text("二维码已过期，请重新获取。".into(), false));
            body = body.child(action_btn("wx-qr-again", "获取二维码", true, true));
        }
        QrState::Error(m) => {
            body = body.child(body_text(format!("状态接口：{m}"), false));
            body = body.child(action_btn("wx-qr-retry", "重试", false, true));
        }
    }

    // 绑定码：微信里 `/bind <code>` 用
    let code = chat.remote.bind_code(&chat.active_key);
    // 上面的「绑定成功」说的是**渠道**（扫码授权）；这里说的是 `/bind`，
    // 两者别混 —— 用户被「已绑定/未绑定」并排显示搞糊涂过
    let bind_line = match chat.remote.bound.as_deref() {
        Some(k) => format!("/bind 已确认 · 当前会话 {k}"),
        None => format!("可选：微信里发 /bind {code} 确认授权（不影响选会话）"),
    };
    body = body.child(body_text(bind_line, true));
    body = body.child(action_btn("wx-qr-close", "关闭", false, false));

    let weak_dismiss = weak.clone();
    let dismiss = move |_w: &mut gpui::Window, cx: &mut gpui::App| {
        let _ = weak_dismiss.update(cx, |c, cx| {
            c.dialog = None;
            cx.notify();
        });
    };
    crate::ui::overlay::layer(true, Some(&chat.dialog_focus), dismiss.clone())
        .flex()
        .items_center()
        .justify_center()
        .child(crate::ui::overlay::big_card(
            "远程控制 · 微信",
            None,
            body.into_any_element(),
            t,
            dismiss,
        ))
        .into_any_element()
}

/// 023 fileView：关闭带未保存修改的文件 tab 前确认（保存并关闭 / 不保存
/// 关闭 / 取消，对齐 Zed 的三选）。
fn render_file_dirty(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    path: &PathBuf,
    t: &theme::Theme,
) -> Div {
    let _ = chat;
    let weak_save = weak.clone();
    let weak_drop = weak.clone();
    let weak_cancel = weak.clone();
    let p_save = path.clone();
    let p_drop = path.clone();
    let name: SharedString = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
        .into();
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
            .when(!accent, |d| {
                d.border_1().border_color(rgb(t.border))
            })
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
                    .child(SharedString::from(tr("未保存的修改").to_string())),
            )
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(
                        format!("{} {name}", tr("有未保存的修改：")),
                    )),
            )
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap(px(8.))
                    .child(
                        btn("fv-dirty-cancel", tr("取消"), false).on_mouse_down(
                            gpui::MouseButton::Left,
                            move |_, _, cx| {
                                let _ = weak_cancel.update(cx, |c, cx| {
                                    c.dialog = None;
                                    cx.notify();
                                });
                            },
                        ),
                    )
                    .child(
                        btn("fv-dirty-drop", tr("不保存关闭"), false).on_mouse_down(
                            gpui::MouseButton::Left,
                            move |_, _, cx| {
                                let _ = weak_drop.update(cx, |c, cx| {
                                    c.discard_file_tab(&p_drop, cx);
                                    c.dialog = None;
                                    cx.notify();
                                });
                            },
                        ),
                    )
                    .child(
                        btn("fv-dirty-save", tr("保存并关闭"), true).on_mouse_down(
                            gpui::MouseButton::Left,
                            move |_, _, cx| {
                                let _ = weak_save.update(cx, |c, cx| {
                                    c.save_file(&p_save, cx);
                                    c.discard_file_tab(&p_save, cx);
                                    c.dialog = None;
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
/// registry 计数）；「订阅服务」= 声明 oauth 且无 oauth 凭据，副标题
/// OAuth，点击提示未接入（OAuth 登录缓行，051/beads pi-flash-1cb）；
/// 「自定义」固定一张卡（虚线加号框 → 详情区空白表单）。组序 2026-10-10
/// 用户定稿：API KEY → 订阅服务 → 自定义；组内保持 registry 原始顺序不
/// 排序，已配置的 provider 不列（pi-web !configured / !loggedIn 同款）。
fn render_provider_picker(chat: &Chat, weak: &gpui::WeakEntity<Chat>, input: &gpui::Entity<TextInput>, t: &theme::Theme, cx: &App) -> Div {
    let q = input.read(cx).value().trim().to_lowercase();

    let dump = chat.provider_registry.clone();
    let reg_state = chat.registry_state.clone();

    // PF 自定义账本的 provider 不进 API KEY 网格（pi-web 排除 custom
    // sources 同款：自定义入口只有「自定义」卡）
    let custom_names: Vec<String> = pi_link::models_json::providers(&chat.mc_models_json)
        .into_iter()
        .map(|(n, _)| n)
        .collect();

    // API KEY 组：registry 原始顺序（不排序，对齐 pi-web）；匹配显示名或 id
    let api_cards: Vec<(String, String, usize)> = match &dump {
        Some(d) => d
            .api_key_providers()
            .iter()
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

    let custom_hit = q.is_empty()
        || "openai anthropic compatible".contains(&q)
        || tr("自定义端点格式").to_lowercase().contains(&q)
        || tr("自定义").to_lowercase().contains(&q);
    let custom_card = {
        let weak_card = weak.clone();
        div()
            .id("pp-custom")
            .w(px(292.))
            .p(px(14.))
            .rounded(px(10.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.bg))
            .cursor_pointer()
            .hover(|h| h.border_color(rgb(t.accent)).bg(rgb(t.bg_hover)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_card.update(cx, |c, cx| {
                    c.dialog = None;
                    if let Some(st) = c.settings.clone() {
                        st.update(cx, |s, cx| {
                            s.section = "__add_provider__".into();
                            s.error = None;
                            s.mj_name.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                            s.mj_base.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                            s.mj_key.update(cx, |ti, cx| ti.set_value(String::new(), cx));
                            s.mj_api = 0;
                            cx.notify();
                        });
                    }
                    cx.notify();
                });
            })
            .flex()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(crate::appearance::ui_size(12.5))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(rgb(t.text))
                            .child(tr("OpenAI / Anthropic compatible")),
                    )
                    .child(
                        div()
                            .mt(px(2.))
                            .text_size(crate::appearance::ui_size(10.5))
                            .text_color(rgb(t.text_dim))
                            .child(tr("自定义端点格式")),
                    ),
            )
            // pi-web 同款：右侧虚线圆角方块 + 加号
            .child(
                div()
                    .size(px(28.))
                    .flex_shrink_0()
                    .rounded(px(6.))
                    .border_1()
                    .border_dashed()
                    .border_color(rgb(t.border))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(icon("plus", 13., t.text_dim)),
            )
    };

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
    // 注册表未就绪/失败：自定义卡照常可用，清单区给状态行
    match (&dump, &reg_state) {
        (None, state) if !matches!(state, RegistryState::Failed(_)) => {
            body = body.child(status_row(tr("正在读取 Provider 注册表…").to_string()));
        }
        (None, RegistryState::Failed(e)) => {
            body = body.child(status_row(crate::i18n::tf(
                "读取 Provider 注册表失败：{e}",
                &[("e", e.clone())],
            )));
        }
        _ => {}
    }

    // 组序（2026-10-10 用户定稿）：API KEY → 订阅服务 → 自定义
    if !api_cards.is_empty() {
        body = body.child(group_header("API KEY", 0.));
        let mut grid = div().flex().flex_wrap().gap(px(10.));
        for (p, name, total) in &api_cards {
            let weak_card = weak.clone();
            let pid = p.clone();
            let el = card(format!("pp-{p}"))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let pid = pid.clone();
                    let _ = weak_card.update(cx, |c, cx| {
                        c.dialog = None;
                        c.mc_select_provider(pid, cx);
                        cx.notify();
                    });
                })
                .flex()
                .items_center()
                .gap(px(10.))
                .child(card_text(
                    name,
                    crate::i18n::tf("{n} models", &[("n", total.to_string())]),
                ))
                .child(crate::ui::provider_icon(p, 24., t.text_muted));
            grid = grid.child(el);
        }
        body = body.child(grid);
    }
    if !oauth_cards.is_empty() {
        body = body.child(group_header("订阅服务", 14.));
        let mut grid = div().flex().flex_wrap().gap(px(10.));
        for (p, name) in &oauth_cards {
            let weak_card = weak.clone();
            let el = card(format!("pp-oauth-{p}"))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_card.update(cx, |c, cx| {
                        // OAuth 登录缓行（051/beads pi-flash-1cb）：提示不关弹窗
                        c.set_status(
                            crate::i18n::tr("OAuth 登录暂未接入，订阅服务商暂时无法在此配置")
                                .to_string(),
                            cx,
                        );
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
    // 自定义组（最后；搜索不中整卡隐藏）
    body = body.child(group_header("自定义", 14.));
    if custom_hit {
        body = body.child(custom_card);
    }
    if dump.is_some() && api_cards.is_empty() && oauth_cards.is_empty() && !custom_hit {
        body = body.child(status_row(tr("没有匹配的 Provider").to_string()));
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
            div()
                .id("pp-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .px(px(20.))
                .pt(px(12.))
                .pb(px(16.))
                .child(body),
        );
    dialog_shell(chat, weak, panel)
}

/// 013 sessionSearchDialog + sessionSearchResultView: query on top, results
/// grouped by session below; a row click switches sessions and reveals the
/// GitDiff dialog surface (extracted from render_dialogs).
fn render_git_diff(chat: &Chat, weak: &gpui::WeakEntity<Chat>, path: &PathBuf, patch: &String, t: &theme::Theme, _cx: &App) -> Div {
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
                                                            c.dialog = None;
                                                            cx.notify();
                                                        });
                                                    }
                                                })
                                                .child(icon_hover("x", 12., t.text_muted)),
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
            c.dialog = None;
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
        .child(
            gpui::img(image.clone())
                .max_w(px(1040.))
                .max_h(px(680.))
                .rounded(px(6.)),
        )
        .child(div().absolute().top(px(6.)).right(px(6.)).child(close));
    dialog_shell(chat, weak, panel)
}


/// 004 projectManager 打开项目菜单：500×500 居中卡片——顶部搜索框，
/// 【打开文件夹】行（走 psp 目录选择器），下面是最近 30 天活动项目列表
/// （字母序、行高 40、吃满剩余高度超出滚动）。当前项目行尾打勾（004：从某项目
/// 新建会话打开时默认选中）。列表数据 `project_hits` 由打开器后台扫描
/// 回填，本函数只做搜索词过滤。
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
    let needle = chat.project_filter.trim().to_lowercase();
    let rows: Vec<&crate::ProjectEntry> = chat
        .project_hits
        .iter()
        .filter(|p| needle.is_empty() || p.name.to_lowercase().contains(&needle))
        .collect();
    let mut list = div().flex().flex_col();
    if rows.is_empty() {
        let empty = if chat.project_hits.is_empty() {
            tr("最近 30 天没有打开过的项目")
        } else {
            tr("没有匹配的项目")
        };
        list = list.child(
            div()
                .py_4()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_dim))
                .child(empty),
        );
    }
    for (ix, p) in rows.iter().enumerate() {
        let path = p.path.clone();
        let weak_row = weak.clone();
        let name: SharedString = p.name.clone().into();
        let selected = crate::services::workspace::same_ws(
            &p.path.to_string_lossy(),
            &chat.cwd.to_string_lossy(),
        );
        list = list.child(
            div()
                .id(SharedString::from(format!("proj-{ix}")))
                .h(px(ROW_H))
                .px_2()
                .flex()
                .items_center()
                .gap_2()
                .rounded(px(8.))
                .cursor_pointer()
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
                .when(selected, |d| d.child(icon("check", 14., t.accent))),
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
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak_open.update(cx, |c, cx| c.pick_project_folder(fresh, cx));
        })
        .child(icon("folder-plus", 16., t.text_muted))
        .child(tr("打开文件夹"));
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
        .child(input.clone())
        .child(open_folder)
        // 打开文件夹与项目列表之间的分隔线（用户 2026-10-07 定稿）
        .child(div().h(px(1.)).w_full().bg(gpui::rgba(crate::theme::border_alpha(t, 0x66))))
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
                        .child(list),
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


/// matched message (pi-web SessionSearch, dialog-mounted per 013).
fn render_session_search(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    input: &gpui::Entity<TextInput>,
    t: &theme::Theme,
    _cx: &App,
) -> Div {
    let status: SharedString = if chat.search_running {
        tr("搜索中…").into()
    } else if chat.search_needle.is_empty() {
        tr("输入关键词，搜索当前项目的会话内容").into()
    } else if chat.search_hits.is_empty() {
        tr("没有匹配结果").into()
    } else {
        format!("{} 条结果", chat.search_hits.len()).into()
    };
    let mut results = div().flex().flex_col();
    let mut last_session: Option<PathBuf> = None;
    for (hit_ix, hit) in chat.search_hits.iter().enumerate() {
        if last_session.as_ref() != Some(&hit.session_path) {
            last_session = Some(hit.session_path.clone());
            let age = time_ago(hit.modified);
            let label: SharedString = hit
                .session_name
                .clone()
                .unwrap_or_else(|| {
                    if hit.preview.is_empty() {
                        hit.session_path
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default()
                    } else {
                        hit.preview.clone()
                    }
                })
                .into();
            results = results.child(
                div()
                    .px_3()
                    .pt_2()
                    .pb_1()
                    // §2 组头辅助档（原 text_xs 裸字号）
                    .text_size(crate::appearance::ui_size(11.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text))
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .child(SharedString::from(label))
                    .child(
                        div()
                            .text_size(crate::appearance::ui_size(10.))
                            .font_weight(gpui::FontWeight::NORMAL)
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(age)),
                    ),
            );
        }
        let path = hit.session_path.clone();
        let ts = hit.ts;
        let weak_row = weak.clone();
        results = results.child(
            div()
                .id(SharedString::from(format!("hit-{hit_ix}")))
                .px_3()
                .py_1p5()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let path = path.clone();
                    let _ = weak_row.update(cx, |c, cx| c.jump_to_hit(path, ts, cx));
                })
                .child(
                    div()
                        // §2 结果正文档（原 text_xs 裸字号）
                        .text_size(crate::appearance::ui_size(12.))
                        .line_height(relative(1.5))
                        .text_color(rgb(t.text_muted))
                        .flex()
                        .flex_wrap()
                        .items_baseline()
                        .gap_1()
                        .child(SharedString::from(hit.before.clone()))
                        .child(
                            div()
                                .px_0p5()
                                .rounded(px(3.))
                                .bg(gpui::hsla(
                                    0., 0., 0.5, 0.15,
                                ))
                                .text_color(rgb(t.accent))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(SharedString::from(hit.match_text.clone())),
                        )
                        .child(SharedString::from(hit.after.clone())),
                ),
        );
    }
    let panel = div()
                    .w(px(620.))
                    .max_h(px(640.))
                    .bg(rgb(t.bg_panel))
                    .border_1()
                    .border_color(rgb(t.border))
                    .rounded(px(10.))
                    .p_3()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .shadow_lg()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                        cx.stop_propagation();
                    })
                    .child(input.clone())
                    .child({
                        let label: SharedString = if chat.search_truncated {
                            format!("{status} · {partial}", partial = tr("部分结果")).into()
                        } else {
                            status
                        };
                        div()
                            .px_1()
                            // §2 状态辅助档（原 text_xs 裸字号）
                            .text_size(crate::appearance::ui_size(11.))
                            .text_color(rgb(t.text_dim))
                            .child(label)
                    })
        .child(
            div()
                .id("search-results")
                .max_h(px(520.))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .child(results),
        );
    dialog_shell(chat, weak, panel)
}
