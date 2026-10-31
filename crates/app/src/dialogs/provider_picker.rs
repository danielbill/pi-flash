//! 042 ProviderPicker 大弹窗（对齐 pi-web AddProviderPicker 内容与版式）。
//! 自顶层 dialogs.rs 拆出（090 目录化）：数据 = SDK 全量注册表 dump
//! （registry.rs），三组卡片网格 + 顶部全宽搜索。

use crate::*;

use gpui::{App, Div, MouseButton, SharedString, div, px, relative, rgb};

use super::dialog_shell;
use crate::TextInput;
use crate::appearance::theme;
use crate::i18n::tr;
use crate::session::model_picker::RegistryState;

/// 042 ProviderPicker（对齐 pi-web AddProviderPicker 内容与版式）：近满窗
/// 大面板 + 顶部全宽搜索（分隔线压底）+ 滚动卡片网格。数据 = SDK 全量
/// 注册表 dump（registry.rs：打开弹窗时后台拉起，vendor VERSION 缓存）：
/// 「自定义」= OpenAI/Anthropic 兼容端点卡（042 M2 恢复，pi-web 同款）→
/// 空白新建表单；「API KEY」= 声明 apiKey.login 且无 api_key 凭据，副标题
/// N models（全量 registry 计数）→ 点击进详情区 API KEY 表单；「订阅服务」
/// = 声明 oauth 且无 oauth 凭据，副标题 OAuth，点击提示未接入（OAuth 登录
/// 缓行，051/beads pi-flash-1cb）。组序 2026-10-10 用户定稿 + 042 M2：
/// 自定义 → API KEY → 订阅服务；组内保持 registry 原始顺序不排序，已配置
/// 的 provider 不列（pi-web !configured / !loggedIn 同款）。自定义卡点击 =
/// 直接创建 new-provider（重名 -N）进详情表单（042-1，pi-web
/// addCustomProvider parity）。
pub(super) fn render_provider_picker(
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
    let models_doc = pi_link::models_json::read().unwrap_or_else(|_| serde_json::json!({}));
    let custom_names: Vec<String> = pi_link::models_json::providers(&models_doc)
        .into_iter()
        .map(|(n, _)| n)
        .collect();

    // API KEY 组：registry 原始顺序（不排序，对齐 pi-web）；匹配显示名或 id。
    // 已配置（auth.json 有 api_key 凭据）不列。
    let api_cards: Vec<(String, String, usize)> = match &dump {
        Some(d) => d
            .providers
            .iter()
            .filter(|p| p.has_api_key_login && d.credential(&p.id) != Some("api_key"))
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

    // 自定义组（pi-web AddProviderPicker「自定义」卡恢复，042 M2）：OpenAI /
    // Anthropic 兼容端点 → 空白新建表单。搜索词命中 "custom"/端点字样才显示
    let show_custom = q.is_empty()
        || "custom".contains(&q)
        || "openai-compatible".contains(&q)
        || "anthropic-compatible".contains(&q);

    // 组序：自定义 → API KEY → 订阅服务
    if show_custom {
        let weak_card = weak.clone();
        body = body
            .child(group_header(tr("model.picker_custom"), 0.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .justify_center()
                    .gap(px(10.))
                    .child(
                        card("pp-custom".into())
                            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                                let _ = weak_card.update(cx, |c, cx| {
                                    c.dlg.dialog = None;
                                    c.mj_open_add(cx);
                                    cx.notify();
                                });
                            })
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .child(card_text(
                                &tr("model.picker_custom_card"),
                                tr("model.picker_custom_sub").to_string(),
                            ))
                            .child(crate::ui::icon("plus", 18., t.text_muted)),
                    ),
            );
    }
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
    if dump.is_some() && !show_custom && api_cards.is_empty() && oauth_cards.is_empty() {
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

