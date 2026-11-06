//! Models tab 左栏渲染：mc_models_view 骨架 + 单一 Provider/模型列表
//! （mc_models_sidebar）+ 添加入口。

use super::detail::mc_models_detail;
use super::*;

// ---------------------------------------------------------------------------
// view
// ---------------------------------------------------------------------------

/// The models tab: 左栏上下两等高列表 + 右栏详情（无 banner / footer）。
pub(crate) fn mc_models_view(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    d: &super::SettingsFormData,
) -> gpui::AnyElement {
    let t = T();
    let section = d.section.as_str();
    let error = &d.error;
    // models.json 不可读：错误条常挂详情区顶部（无页面级 footer 了）
    let mut detail_error = error.clone();
    if let Some(e) = &chat.cfg.mc_mj_error {
        detail_error = Some(crate::i18n::tf(
            "models.json_read_failed",
            &[("e", e.clone())],
        ));
    }

    // ---- split view -------------------------------------------------------
    let selected = if section.is_empty() {
        chat.mc_provider_ids().first().cloned().unwrap_or_default()
    } else {
        section.to_string()
    };
    // 本帧的启用集合（O(1) 行查询；一次构建，上下列表/详情共用）
    let enabled_set: std::collections::HashSet<String> =
        chat.cfg.mc_state.enabled.iter().cloned().collect();
    // 合并目录一帧只算一遍（2026-11-04 卡顿修复：此前 sidebar 每 Provider
    // 各自 catalog_with_ledger() → O(商×目录) 的读盘/克隆，每帧 11 次读盘起）
    let listing = {
        let __t = std::time::Instant::now();
        let l = chat.catalog_with_ledger();
        crate::perf_span("frame.models.listing", __t);
        l
    };
    let sb = {
        let __t = std::time::Instant::now();
        let el = mc_models_sidebar(
            chat,
            &weak.clone(),
            &listing,
            &enabled_set,
            &d.enabled_filter,
            &d.enabled_filter_value,
            t,
        );
        crate::perf_span("frame.models.sidebar", __t);
        el
    };
    let detail = {
        let __t = std::time::Instant::now();
        let el = mc_models_detail(
            chat,
            &weak.clone(),
            &selected,
            d,
            &detail_error,
            &enabled_set,
        );
        crate::perf_span("frame.models.detail", __t);
        el
    };
    div()
        .flex()
        .w_full()
        .h_full()
        .min_h_0()
        .flex_1()
        .min_w_0()
        .child(sb)
        .child(detail)
        .into_any_element()
}

/// 左栏（合并后的单一列表）：Provider 行下直接平铺该商已启用的模型行
/// （常开，无折叠态）。排序三档——默认模型所在 Provider → 有启用模型的
/// → 没模型的，档内按显示名；组内模型按显示名。模型行与 inputpanel 菜单
/// 共用组件（缩进槽 / 五角星）；行 switch 整商启停（决策 4）。过滤框沿用
/// MODEL_FILTER_MIN 规则（不足不渲染、文本不生效），只筛模型行，
/// Provider 行常驻。行不画选中底色（选中反馈在右栏详情）。
fn mc_models_sidebar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    listing: &[pi_link::protocol::ModelInfo],
    enabled_set: &std::collections::HashSet<String>,
    enabled_filter: &gpui::Entity<crate::TextInput>,
    enabled_filter_value: &str,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    // ---- Provider 全集 + 归属（042-1 UI：内置及插件在上、自定义在下）-------
    // 归属判据 = **在不在 pi 的 SDK 注册表**，不是「谁写的文件」：models.json
    // 里出现内置商条目（走代理只改 baseUrl / upsert 模型）是合法覆盖，不能
    // 因此判成自定义商。注册表未就绪时按旧判据过渡（models.json 有条目）。
    // 注册表就绪后仍需 models.json 条目佐证才算自定义——扩展注册的 provider
    //（pi-freeflow 等）同样不在注册表 dump 里，但 models.json 没它的条目：
    // 不进自定义组，归「内置及插件」组、无凭据也出列（用户要能看到插件带
    // 的模型，2026-11-01 反馈）。
    // 成员商清单 = 统一接口（Chat::sidebar_providers，picker 同源）；
    // 这里保留 is_custom 闭包只做「内置及插件 / 自定义」分组
    let provs: Vec<String> = chat.sidebar_providers();
    let models_doc =
        crate::services::mj_doc_cache::read().unwrap_or_else(|_| serde_json::json!({}));
    let custom_entries: Vec<(String, &serde_json::Value)> =
        pi_link::models_json::providers(&models_doc);
    let in_models_json = |p: &str| custom_entries.iter().any(|(n, _)| n == p);
    // models.json 里用户手写的 apiKey 也算「已配置」（规则 1：只读不改；
    // 启用列表展示用，不参与成员资格——成员资格在 sidebar_providers）
    let key_in_models_json = |p: &str| {
        custom_entries.iter().any(|(n, e)| {
            n == p
                && e.get("apiKey")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| !s.trim().is_empty())
        })
    };
    let is_custom = |p: &str| -> bool {
        let src = match chat.cfg.provider_registry.as_deref() {
            Some(dump) => dump.source(p, in_models_json(p)),
            None => pi_link::registry::source_without_registry(in_models_json(p)),
        };
        src == pi_link::registry::ProviderSource::Custom
    };
    // 每商 (enabled, total)：合并目录上**单趟**统计（2026-11-04 修复：此前
    // 每 Provider 各自 catalog_with_ledger() 重算整份清单，O(商×目录)）
    let mut counts_map: std::collections::HashMap<&str, (usize, usize)> =
        std::collections::HashMap::new();
    for m in listing {
        let e = counts_map.entry(m.provider.as_str()).or_insert((0, 0));
        e.1 += 1;
        if enabled_set.contains(&format!("{}/{}", m.provider, m.id)) {
            e.0 += 1;
        }
    }
    let counts: Vec<(usize, usize)> = provs
        .iter()
        .map(|p| counts_map.get(p.as_str()).copied().unwrap_or((0, 0)))
        .collect();

    // 三档：0 = 默认模型所在商，1 = 有启用模型，2 = 无；档内显示名字典序
    let default_prov = chat.cfg.mc_default_model.as_ref().map(|(p, _)| p.clone());
    let tier_name = |ix: usize| {
        let tier = match default_prov.as_deref() {
            Some(dp) if dp == provs[ix] => 0u8,
            _ if counts[ix].0 > 0 => 1,
            _ => 2,
        };
        (
            tier,
            crate::ui::provider_display_name(&provs[ix]).to_lowercase(),
        )
    };
    let mut builtin_order: Vec<usize> = (0..provs.len())
        .filter(|&ix| !is_custom(&provs[ix]))
        .collect();
    builtin_order.sort_by_key(|&ix| tier_name(ix));
    let mut custom_order: Vec<usize> = (0..provs.len())
        .filter(|&ix| is_custom(&provs[ix]))
        .collect();
    custom_order.sort_by_key(|&ix| tier_name(ix));

    // ---- 过滤（菜单同规则）：只筛模型行，Provider 行常驻 ------------------
    let total_enabled: usize = counts.iter().map(|(en, _)| *en).sum();
    let show_filter = total_enabled >= crate::session::model_picker::MODEL_FILTER_MIN;
    let q = if show_filter {
        enabled_filter_value.trim().to_lowercase()
    } else {
        String::new()
    };
    let label = |m: &pi_link::protocol::ModelInfo| {
        if m.name.is_empty() {
            m.id.clone()
        } else {
            m.name.clone()
        }
    };
    let hits = |m: &pi_link::protocol::ModelInfo, p: &str| {
        q.is_empty()
            || m.id.to_lowercase().contains(&q)
            || m.name.to_lowercase().contains(&q)
            || crate::ui::provider_display_name(p)
                .to_lowercase()
                .contains(&q)
    };

    // ---- 行序：Provider 行 + 其下启用模型行（组内显示名序）----------------
    enum SbRow {
        /// `prov_rows` 下标
        Prov(usize),
        /// `flat` 下标
        Model(usize),
        /// 内置组 / 自定义组之间的分隔线
        Divider,
    }
    struct ProvRow {
        id: String,
        is_custom: bool,
        enabled_n: usize,
        total: usize,
        on: bool,
        switchable: bool,
        show_count: bool,
    }
    let mut prov_rows: Vec<ProvRow> = Vec::new();
    let mut flat: Vec<pi_link::protocol::ModelInfo> = Vec::new();
    let mut rows: Vec<SbRow> = Vec::new();
    // 启用模型（mc_sorted_enabled 同语义）：同一份 listing 上一次过滤一趟
    //（2026-11-04 修复：此前这里又整体 catalog_with_ledger() 一遍）
    let all_enabled = chat.cfg.mc_state.all_enabled;
    let mut enabled_models: Vec<pi_link::protocol::ModelInfo> = listing
        .iter()
        .filter(|m| all_enabled || enabled_set.contains(&format!("{}/{}", m.provider, m.id)))
        .cloned()
        .collect();
    enabled_models.sort_by_key(|m| Chat::mc_sort_key(m));
    // 两组之间插一条分隔线（两组都非空才画）
    let show_divider = !builtin_order.is_empty() && !custom_order.is_empty();
    let ordered: Vec<usize> = builtin_order
        .iter()
        .copied()
        .chain(show_divider.then_some(usize::MAX))
        .chain(custom_order.iter().copied())
        .collect();
    for &ix in &ordered {
        if ix == usize::MAX {
            rows.push(SbRow::Divider);
            continue;
        }
        let p = &provs[ix];
        let (en, total) = counts[ix];
        let is_custom = is_custom(p);
        let configured = chat.mc_configured(p) || key_in_models_json(p);
        rows.push(SbRow::Prov(prov_rows.len()));
        prov_rows.push(ProvRow {
            id: p.clone(),
            is_custom,
            enabled_n: en,
            total,
            on: chat.cfg.mc_state.all_enabled || en > 0,
            switchable: !chat.cfg.mc_project_scope && total > 0 && configured,
            show_count: !chat.cfg.mc_state.all_enabled,
        });
        let mut ms: Vec<pi_link::protocol::ModelInfo> = enabled_models
            .iter()
            .filter(|m| &m.provider == p && hits(m, p))
            .cloned()
            .collect();
        ms.sort_by_key(|m| label(m).to_lowercase());
        for m in ms {
            flat.push(m);
            rows.push(SbRow::Model(flat.len() - 1));
        }
    }

    // 行距统一：全表（Provider 行 + 模型行）同一行高，严格等距
    let row_h = (f32::from(crate::appearance::ui_size(11.)) + 19.).max(30.);
    let default_ref = chat
        .cfg
        .mc_default_model
        .as_ref()
        .map(|(p, id)| format!("{p}/{id}"));
    let pins = chat.cfg.mc_state.pins.clone();
    let weak_rows = weak.clone();

    let list = vlist(
        "mc-merged-list",
        rows.len(),
        row_h,
        VListHeight::Fill,
        false,
        true,
        tr("model.no_provider_match"),
        None,
        move |ix, _window, _cx| match &rows[ix] {
            SbRow::Prov(pr) => {
                let d = &prov_rows[*pr];
                let pid = d.id.clone();
                let pid_sw = d.id.clone();
                let (is_custom, on) = (d.is_custom, d.on);
                let row_weak = weak_rows.clone();
                div()
                    .id(SharedString::from(format!("mc-side-{}", d.id)))
                    .h(px(row_h))
                    // vlist 行 fit-content：不写 w_full switch 贴着文字
                    .w_full()
                    .px(px(15.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgb(t.bg_hover)))
                    .text_size(crate::appearance::ui_size(12.))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = row_weak.update(cx, |c, cx| {
                            if is_custom {
                                c.mj_select(format!("mj:p:{pid}"), cx);
                            } else {
                                c.mc_select_provider(pid.clone(), cx);
                            }
                        });
                    })
                    .child(crate::ui::provider_icon(&d.id, 16., t.text_muted))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .text_ellipsis()
                            .child(SharedString::from(
                                crate::ui::provider_display_name(&d.id).to_string(),
                            )),
                    )
                    .children(d.show_count.then(|| {
                        div()
                            .font_family(crate::editor::markdown::MONO_FAMILY)
                            .text_size(crate::appearance::ui_size(10.))
                            .text_color(rgb(t.text_dim))
                            .child(SharedString::from(format!("{}/{}", d.enabled_n, d.total)))
                            .into_any_element()
                    }))
                    // 整商 switch（决策 4：enabledModels 整商开关，凭据不动）；
                    // 未配置/零模型的内置商恒关禁用，点行进详情配 key
                    .child(config_switch(
                        format!("mc-prov-sw-{}", d.id),
                        &weak_rows,
                        on,
                        !d.switchable,
                        move |c, cx| c.mc_toggle_provider(&pid_sw, !on, cx),
                    ))
                    .into_any_element()
            }
            SbRow::Divider => div()
                .h(px(row_h))
                .w_full()
                .px(px(15.))
                .flex()
                .items_center()
                .child(div().h(px(1.)).w_full().bg(rgb(t.border)))
                .into_any_element(),
            SbRow::Model(fx) => {
                let m = &flat[*fx];
                let r = format!("{}/{}", m.provider, m.id);
                let is_default = default_ref.as_deref() == Some(r.as_str());
                let pin = pins.iter().find(|(p, _)| p == &r).map(|(_, l)| l.clone());
                let row = crate::session::model_picker::group_model_row(
                    &weak_rows, m, is_default, false, false, false, row_h, 15., pin, None, t,
                );
                // 042-1：点模型行打开模型详情弹窗（内置模型走 modelOverrides 覆盖；
                // 行内开关/五角星自己 stop_propagation，不会误触）
                let open_weak = weak_rows.clone();
                let (op, oi) = (m.provider.clone(), m.id.clone());
                div()
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        let _ = open_weak
                            .update(cx, |c, cx| c.mj_open_model(op.clone(), oi.clone(), cx));
                    })
                    .child(row)
                    .into_any_element()
            }
        },
    );

    sidebar_shell("mc-sidebar")
        .w(px(LIST_W))
        .child(mc_add_action(weak))
        .when(show_filter, |d| {
            d.child(div().px(px(15.)).pb(px(4.)).child(enabled_filter.clone()))
        })
        .child(list)
        .into_any_element()
}

/// 上列表头行【+添加Provider】（原底部钮上移；开两组清单弹窗）。
fn mc_add_action(weak: &gpui::WeakEntity<Chat>) -> gpui::AnyElement {
    let t = T();
    let weak = weak.clone();
    div()
        .id("mc-add-provider")
        .flex_shrink_0()
        .px(px(6.))
        .pt(px(8.))
        .pb(px(6.))
        .child(
            div()
                .id("mc-add-provider-go")
                .h(px(30.))
                .px(px(8.))
                .rounded(px(5.))
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(crate::appearance::ui_size(12.))
                .cursor_pointer()
                .text_color(rgb(t.text_dim))
                .hover(|s| s.bg(rgb(t.bg_hover)))
                .child(crate::ui::icon_hover("plus", 13., t.text_dim))
                .child(tr("model.add_provider"))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.dlg.dialog = Some(c.provider_picker_dialog(cx));
                        cx.notify();
                    });
                }),
        )
        .into_any_element()
}

/// 详情页文字按钮：统一 [`ui::button`] 规范样式（BTN_LG 32 高，设置页表单
/// 主按钮档；Danger/Primary/Secondary 三变体），回调签名为 Chat 动作的糖。
#[allow(clippy::too_many_arguments)]
pub(crate) fn detail_button(
    id: &'static str,
    weak: &gpui::WeakEntity<Chat>,
    label: &str,
    variant: crate::ui::button::BtnVariant,
    disabled: bool,
    on_click: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
) -> gpui::AnyElement {
    let weak = weak.clone();
    // 宽度钉死 84：详情页三钮（移除/保存/获取）同宽右对齐（用户定稿）
    let el = crate::ui::button::button_base(id, crate::ui::button::BtnSize::Lg, variant)
        .w(px(84.))
        .child(SharedString::from(label.to_string()));
    if disabled {
        return el.opacity(0.5).into_any_element();
    }
    el.on_mouse_down(gpui::MouseButton::Left, move |_, _, cx| {
        let _ = weak.update(cx, |c, cx| on_click(c, cx));
    })
    .into_any_element()
}
