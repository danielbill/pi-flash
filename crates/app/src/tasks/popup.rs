//! 046 任务表单弹层（popup.rs）：从 form.rs 拆出（行数红线 §1）。
//! freq/interval/time 弹层菜单本体（menus_overlay 挂 overlay 层）+ 行/列表卡小件。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb, rgba};

use crate::Chat;
use crate::appearance::theme::{self, Theme, theme};
use crate::appearance::ui_size;
use crate::editor::markdown::MONO_FAMILY;
use crate::i18n::{tf, tr};
use crate::session::menu_rows::{pill_row, thinking_rows, tools_preset_rows};
use crate::session::model_picker::{ModelPickAction, model_menu_body};
use crate::tasks::INTERVALS;
use crate::tasks::form::{sel_btn, trg_wrap, weekday_key};
use crate::tasks::popup_slot;
use crate::ui::icon;
use crate::ui::tokens::{menu, radius, space};

pub(crate) fn time_pick(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    _t: &'static Theme,
) -> AnyElement {
    let page = &chat.tasks.page;
    let t = theme();
    trg_wrap(
        popup_slot::TIME,
        weak,
        chat,
        sel_btn(
            "task-sel-time",
            format!("{:02}:{:02}", page.hour, page.minute),
            None,
            page.popup == Some(popup_slot::TIME),
            &t,
        ),
    )
}

// ---------------------------------------------------------------------------
// 锚定菜单（menus_overlay 统一挂 overlay 栈；卡片内容复用 inputpanel 组件）
// ---------------------------------------------------------------------------

/// 菜单行（picker_card 内；label + ✓ 槽，pill_row 形制由各卡复用）。
fn menu_row(
    id: impl Into<SharedString>,
    label: String,
    selected: bool,
    on_pick: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
    weak: &gpui::WeakEntity<Chat>,
) -> AnyElement {
    let t = theme();
    let weak = weak.clone();
    div()
        .id(id.into())
        .mx(px(space::SP3))
        .px(px(menu::ROW_PX))
        .py(px(menu::ROW_PY))
        .rounded(px(radius::CARD))
        .flex()
        .items_center()
        .gap(px(space::SP4))
        .text_size(ui_size(12.5))
        .cursor_pointer()
        .text_color(rgb(if selected { t.text } else { t.text_muted }))
        .when(selected, |d| d.font_weight(gpui::FontWeight::SEMIBOLD))
        .hover(|h| h.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            let _ = weak.update(cx, |c, cx| {
                on_pick(c, cx);
                cx.notify();
            });
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(SharedString::from(label)),
        )
        .when(selected, |d| d.child(icon("check", 13., t.accent)))
        .into_any_element()
}

/// 数值/档位列表卡（限高滚动）。
fn list_card(rows: Vec<AnyElement>, max_h: f32) -> AnyElement {
    div()
        .id(SharedString::from(format!("task-menu-{}", rows.len())))
        .max_h(px(max_h))
        .overflow_y_scroll()
        .py(px(4.))
        .flex()
        .flex_col()
        .children(rows)
        .into_any_element()
}

/// 菜单分组分隔线（HTML pm-sep：1px bd-soft，左右缩进）。
fn menu_sep() -> AnyElement {
    let t = theme();
    div()
        .mx(px(6.))
        .my(px(4.))
        .h(px(1.))
        .flex_none()
        .bg(rgba(theme::border_alpha(t, 0x45)))
        .into_any_element()
}

/// 周期档菜单行（五档共用：freq 主菜单与 cmin 间隔菜单的「换档」分组）。
fn freq_menu_row(ix: usize, chat: &Chat, weak: &gpui::WeakEntity<Chat>) -> AnyElement {
    let page = &chat.tasks.page;
    let sel = page.unit_ix == ix;
    let label = if ix == 0 {
        // 每 N 分钟档：主文案带当前间隔（INTERVALS 档位值）
        tf(
            tr("task.sum_cmin"),
            &[("n", INTERVALS[page.interval_ix.min(9)].to_string())],
        )
    } else {
        tr(match ix {
            1 => "task.unit_hour",
            2 => "task.unit_day",
            3 => "task.unit_week",
            _ => "task.unit_month",
        })
        .to_string()
    };
    menu_row(
        SharedString::from(format!("task-mi-freq-{ix}")),
        label,
        sel,
        move |c, _cx| {
            c.tasks.page.unit_ix = ix;
            // HTML setFreq 同款：切档重置到合理默认（每小时锚第 0 分；
            // 其余日档锚 09:00）；cmin 保留现间隔
            if ix == 1 {
                c.tasks.page.minute = 0;
            } else if ix > 1 {
                c.tasks.page.hour = 9;
                c.tasks.page.minute = 0;
            }
            c.tasks.page.popup = None;
        },
        weak,
    )
}

/// 页面锚定菜单总入口（overlay_stack 挂载；settings ag_menus_overlay 同款）。
pub(crate) fn menus_overlay(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &gpui::Window,
    cx: &gpui::Context<Chat>,
) -> Option<AnyElement> {
    let slot = chat.tasks.page.popup?;
    let t = theme();
    let page = &chat.tasks.page;
    let vp = window.viewport_size();

    // 定位：PLUS 走专用算法——贴指令胶囊正上方、与胶囊同宽（缝 8；
    // inputpanel slash_menu 同位，2026-11-17 拍板「弹出样式和 InputPanel
    // 对齐」），锚定端 = 胶囊 bounds tracker（form.rs capsule 挂）；其余
    // 槽位走统一定位（按钮居中 + 半屏定向）。
    let (menu_w, pos) = match slot {
        popup_slot::PLUS => {
            let capsule = page.anchors.composer.borrow();
            match *capsule {
                Some(b) => (
                    b.size.width,
                    crate::session::picker_shell::PillCardPos {
                        left: b.origin.x,
                        top: None,
                        bottom: Some(vp.height - b.origin.y + px(8.)),
                    },
                ),
                None => (
                    px(380.),
                    crate::session::picker_shell::pill_anchor_edges(page.anchor, vp, px(380.)),
                ),
            }
        }
        _ => {
            let w = match slot {
                popup_slot::FREQ => 160.,
                popup_slot::INTERVAL => 160.,
                popup_slot::MINUTE => 96.,
                popup_slot::WD => 140.,
                popup_slot::MDAY => 140.,
                popup_slot::TIME => 132.,
                popup_slot::MODEL => 380.,
                popup_slot::TIER => 320.,
                popup_slot::THINKING => 320.,
                _ => return None,
            };
            (
                px(w),
                crate::session::picker_shell::pill_anchor_edges(page.anchor, vp, px(w)),
            )
        }
    };
    let weak_c = weak.clone();

    // ---- 各槽位卡片内容
    let content: AnyElement = match slot {
        popup_slot::MODEL => {
            let (models, _total) = chat.picker_models_from(None, None);
            let mut items: Vec<AnyElement> = vec![menu_row(
                "task-mi-inherit",
                tr("task.model_inherit").to_string(),
                page.model.is_none(),
                |c, _cx| c.tasks.page.model = None,
                weak,
            )];
            let on_pick: ModelPickAction = std::rc::Rc::new(|c: &mut Chat, _cx, p, id| {
                c.tasks.page.model = Some((p.clone(), id.clone()));
            });
            items.push(
                div()
                    .id("task-model-list")
                    .max_h(px(300.))
                    .overflow_y_scroll()
                    .child(model_menu_body(
                        weak,
                        &models,
                        page.model
                            .as_ref()
                            .map(|(p, m)| format!("{p}/{m}"))
                            .as_deref(),
                        None,
                        |_| false,
                        Some(on_pick),
                        t,
                    ))
                    .into_any_element(),
            );
            div()
                .flex()
                .flex_col()
                .py(px(4.))
                .children(items)
                .into_any_element()
        }
        popup_slot::TIER => {
            let rows: Vec<AnyElement> = tools_preset_rows(page.tools)
                .into_iter()
                .map(|r| {
                    let weak = weak.clone();
                    let key = r.key;
                    pill_row(&weak, move |c, _cx| c.tasks.page.tools = key, r)
                })
                .collect();
            list_card(rows, 300.)
        }
        popup_slot::THINKING => {
            let rows: Vec<AnyElement> = thinking_rows(page.thinking)
                .into_iter()
                .map(|r| {
                    let weak = weak.clone();
                    let key = r.key;
                    pill_row(&weak, move |c, _cx| c.tasks.page.thinking = key, r)
                })
                .collect();
            list_card(rows, 300.)
        }
        popup_slot::FREQ => {
            // 五档全列（HTML freqMenu 同源；2026-11-15 补「每 N 分钟」——
            // 原四档清单让 cmin 档不可达）
            let rows: Vec<AnyElement> = (0..5).map(|ix| freq_menu_row(ix, chat, &weak_c)).collect();
            list_card(rows, 240.)
        }
        popup_slot::INTERVAL => {
            // cmin 单钮菜单：间隔档清单 + 分组线 + 其余四档（逃出 cmin——
            // 单钮合一后这里是唯一换档口）
            let mut rows: Vec<AnyElement> = INTERVALS
                .iter()
                .enumerate()
                .map(|(ix, n)| {
                    let sel = page.interval_ix == ix;
                    menu_row(
                        SharedString::from(format!("task-mi-interval-{ix}")),
                        format!("{} {}", n, tr("task.minute_suffix")),
                        sel,
                        move |c, _cx| {
                            c.tasks.page.interval_ix = ix;
                            c.tasks.page.popup = None;
                        },
                        &weak_c,
                    )
                })
                .collect();
            rows.push(menu_sep());
            rows.extend((1..5).map(|ix| freq_menu_row(ix, chat, &weak_c)));
            list_card(rows, 360.)
        }
        popup_slot::MINUTE => {
            let rows: Vec<AnyElement> = (0..60)
                .map(|n| {
                    let sel = page.minute == n;
                    menu_row(
                        SharedString::from(format!("task-mi-minute-{n}")),
                        format!("{n:02}"),
                        sel,
                        move |c, _cx| {
                            c.tasks.page.minute = n;
                            c.tasks.page.popup = None;
                        },
                        &weak_c,
                    )
                })
                .collect();
            list_card(rows, 300.)
        }
        popup_slot::WD => {
            let rows: Vec<AnyElement> = (0..7)
                .map(|i| {
                    let sel = page.weekdays[i];
                    let key = weekday_key(i);
                    menu_row(
                        SharedString::from(format!("task-mi-wd-{i}")),
                        format!("{}{}", tr("task.weekday_prefix"), tr(key)),
                        sel,
                        move |c, _cx| {
                            let mut w = [false; 7];
                            w[i] = true;
                            c.tasks.page.weekdays = w;
                            c.tasks.page.popup = None;
                        },
                        &weak_c,
                    )
                })
                .collect();
            list_card(rows, 280.)
        }
        popup_slot::MDAY => {
            let rows: Vec<AnyElement> = (1u32..=31)
                .map(|n| {
                    let sel = page.month_day == n;
                    menu_row(
                        SharedString::from(format!("task-mi-md-{n}")),
                        format!("{} {}", n, tr("task.day_num")),
                        sel,
                        move |c, _cx| {
                            c.tasks.page.month_day = n;
                            c.tasks.page.popup = None;
                        },
                        &weak_c,
                    )
                })
                .collect();
            list_card(rows, 300.)
        }
        // 时间拾取（HTML tpick：双列 64px；点选不关卡，连续调时刻）
        popup_slot::TIME => {
            let hour_items: Vec<AnyElement> = (0..24)
                .map(|n| {
                    let weak = weak.clone();
                    let sel = page.hour == n;
                    div()
                        // ui-raw: ok HTML tpick 双列拾取行 h36 逐值复刻（§10.8 登记）
                        .id(SharedString::from(format!("task-tp-h-{n}")))
                        .h(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .font_family(MONO_FAMILY)
                        .text_size(ui_size(13.))
                        .cursor_pointer()
                        .when(sel, |d| {
                            d.bg(rgb(t.bg_selected))
                                .text_color(rgb(t.text))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                        })
                        .when(!sel, |d| {
                            d.text_color(rgb(t.text_dim))
                                .hover(|h| h.bg(rgb(t.bg_hover)))
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak.update(cx, |c, cx| {
                                c.tasks.page.hour = n;
                                cx.notify();
                            });
                        })
                        .child(SharedString::from(format!("{n:02}")))
                        .into_any_element()
                })
                .collect();
            let minute_items: Vec<AnyElement> = (0..60)
                .map(|n| {
                    let weak = weak.clone();
                    let sel = page.minute == n;
                    div()
                        // ui-raw: ok HTML tpick 双列拾取行 h36 逐值复刻（§10.8 登记）
                        .id(SharedString::from(format!("task-tp-m-{n}")))
                        .h(px(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .font_family(MONO_FAMILY)
                        .text_size(ui_size(13.))
                        .cursor_pointer()
                        .when(sel, |d| {
                            d.bg(rgb(t.bg_selected))
                                .text_color(rgb(t.text))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                        })
                        .when(!sel, |d| {
                            d.text_color(rgb(t.text_dim))
                                .hover(|h| h.bg(rgb(t.bg_hover)))
                        })
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak.update(cx, |c, cx| {
                                c.tasks.page.minute = n;
                                cx.notify();
                            });
                        })
                        .child(SharedString::from(format!("{n:02}")))
                        .into_any_element()
                })
                .collect();
            div()
                .flex()
                .child(
                    div()
                        .id("task-tp-h")
                        .w(px(64.))
                        .max_h(px(252.))
                        .overflow_y_scroll()
                        .border_r_1()
                        .border_color(rgba(theme::border_alpha(t, 0x45)))
                        .children(hour_items),
                )
                .child(
                    div()
                        .id("task-tp-m")
                        .w(px(64.))
                        .max_h(px(252.))
                        .overflow_y_scroll()
                        .children(minute_items),
                )
                .into_any_element()
        }
        popup_slot::PLUS => {
            // 「+」统一菜单技能页（InputPanel + 同语义）：技能清单，
            // 点行把 `/skill:name ` 追加进指令输入
            let skills: Vec<_> = chat
                .slash_commands(cx)
                .iter()
                .filter(|c| c.name.starts_with("skill:"))
                .cloned()
                .collect();
            plus_menu_body(chat, &skills, weak, t)
        }
        _ => return None,
    };

    let layer = crate::ui::menu::menu_layer(None, weak, |c, cx| {
        if c.tasks.page.popup.take().is_some() {
            cx.notify();
        }
    });
    // PLUS：统一菜单壳自带 chrome（slash_menu_view 同构 rounded8 + border +
    // shadow，2026-11-17 拍板），只包一层裸定位座；其余槽位走 picker_card 卡座
    if slot == popup_slot::PLUS {
        let base = div()
            .absolute()
            .left(pos.left)
            .w(menu_w)
            .max_h(px(520.))
            .flex();
        let base = match pos.top {
            Some(top) => base.top(top),
            None => base.bottom(pos.bottom.unwrap_or(px(64.))),
        };
        return Some(
            layer
                .child(crate::ui::overlay::stop_click(base.child(content)))
                .into_any_element(),
        );
    }
    Some(
        layer
            .child(crate::ui::overlay::stop_click(
                crate::session::picker_shell::picker_card(pos, menu_w, t).child(content),
            ))
            .into_any_element(),
    )
}

// ---------------------------------------------------------------------------
// 「+」统一菜单（InputPanel slash_menu_view 同构复刻，2026-11-17 拍板
// 「弹出样式和 InputPanel 对齐」）
// ---------------------------------------------------------------------------

/// 「+」统一菜单本体：搜索栏 + tab 条 + 技能卡滚动列表，壳自带 chrome
/// （rounded8 + border + shadow，max_h 520 = slash_menu_view 同值）。
/// 搜索按剥前缀名子串过滤（万能搜索同口径）；空态两档（无技能 / 无命中）。
fn plus_menu_body(
    chat: &Chat,
    skills: &[pi_link::protocol::SlashCommand],
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> AnyElement {
    let q = chat.tasks.page.plus_query.trim().to_lowercase();
    let hits: Vec<&pi_link::protocol::SlashCommand> = skills
        .iter()
        .filter(|c| {
            if q.is_empty() {
                return true;
            }
            c.name
                .strip_prefix("skill:")
                .unwrap_or(&c.name)
                .to_lowercase()
                .contains(&q)
        })
        .collect();
    let rows: Vec<AnyElement> = hits.iter().map(|c| plus_row(c, weak, t)).collect();
    let empty: SharedString = if skills.is_empty() {
        tr("agents.pick_empty").into()
    } else {
        tr("menu.no_search_result").into()
    };
    div()
        .w_full()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(8.))
        .shadow_lg()
        .max_h(px(520.))
        .overflow_hidden()
        .child(plus_search_bar(chat, t))
        .child(plus_tab_strip(t))
        .child(
            div()
                .relative()
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(if rows.is_empty() {
                    div()
                        .pt(px(10.))
                        .pb(px(10.))
                        .pl(px(10.))
                        .pr(px(14.))
                        .text_size(ui_size(11.))
                        .text_color(rgb(t.text_dim))
                        .child(empty)
                        .into_any_element()
                } else {
                    div()
                        .id("task-plus-list")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        // 右侧比左侧多 4px 给滚动条让位（menu-body 同款）
                        .pt(px(10.))
                        .pb(px(10.))
                        .pl(px(10.))
                        .pr(px(14.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .children(rows)
                        .into_any_element()
                }),
        )
        .into_any_element()
}

/// 搜索栏（menu_search_bar 同构：h36 + search icon + flat 输入，菜单底色同体）。
fn plus_search_bar(chat: &Chat, t: &'static Theme) -> AnyElement {
    div()
        .flex_shrink_0()
        .h(px(36.))
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .bg(rgb(t.bg))
        .child(icon("search", crate::ui::tokens::icon::SM, t.text_faint))
        .child(
            div().flex_1().min_w_0().children(
                chat.tasks
                    .page
                    .plus_search
                    .as_ref()
                    .map(|i| i.clone().into_any_element()),
            ),
        )
        .into_any_element()
}

/// tab 条静态复刻（menu_views::menu_tab_strip 同构）：技能页钉活（任务表单
/// 语境只有技能可插），其余三类 dim 0.45 = `menu_tab_available` false 同款；
/// 条下 5px 同色带承激活 tab 的连体缘。
fn plus_tab_strip(t: &'static Theme) -> AnyElement {
    div()
        .flex_shrink_0()
        .h(px(39.))
        .flex()
        .items_end()
        .px(px(space::SP4))
        // tab 栏钉面板字体槽（menu_tab_strip 同源，与搜索框一致）
        .font_family(SharedString::from(
            crate::appearance::panel_font().family.clone(),
        ))
        .bg(rgb(t.chrome))
        .border_b_5()
        .border_color(rgb(t.bg))
        .children(crate::MenuKind::ALL.iter().map(|k| {
            let active = *k == crate::MenuKind::Skill;
            let badge = div()
                .font_family(MONO_FAMILY)
                .text_size(ui_size(11.))
                .text_color(rgb(if active { t.accent } else { t.text_faint }))
                .child(SharedString::from(k.prefix().to_string()))
                .into_any_element();
            crate::titlebar::tab_body(
                None,
                tr(k.label_key()).into(),
                active,
                0.,
                t,
                None,
                Some(badge),
                false,
            )
            .h(px(34.))
            .rounded(px(0.))
            .mb(px(0.))
            .text_size(ui_size(11.))
            .border_l_0()
            .border_r_0()
            .when(!active, |d| {
                d.opacity(0.45).cursor(gpui::CursorStyle::Arrow)
            })
        }))
        .into_any_element()
}

/// 技能条目卡（menu.rs slash_menu_view card 同构逐值复刻）：32px 卡 +
/// wand icon + mono 剥前缀名 + 右侧说明；点击把 `/skill:名 ` 追加进指令
/// （点行即确认，无键盘导航——任务菜单不挂 composer 键路由）。
fn plus_row(
    c: &pi_link::protocol::SlashCommand,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> AnyElement {
    let weak = weak.clone();
    let name = c.name.clone();
    let label = name.strip_prefix("skill:").unwrap_or(&name).to_string();
    let desc = c.description.replace('\n', " ");
    let id = SharedString::from(format!("task-plus-{label}"));
    div()
        .id(id)
        .w_full()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        // ui-raw: ok slash_menu_view 条目卡逐值复刻（h32/gap8/px10/r5），§10.8 登记
        .h(px(32.))
        .flex_shrink_0()
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg_panel))
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let weak = weak.clone();
            let name = name.clone();
            let _ = weak.update(cx, |c, cx| {
                c.tasks.page.popup = None;
                if let Some(i) = c.tasks.page.desc.clone() {
                    i.update(cx, |inp, icx| {
                        let mut v = inp.value().to_string();
                        if !v.is_empty() && !v.ends_with(' ') && !v.ends_with('\n') {
                            v.push(' ');
                        }
                        v.push_str(&format!("/{name} "));
                        inp.set_value(v, icx);
                    });
                }
                cx.notify();
            });
        })
        .child(icon("wand", 13., t.text_dim))
        .child(
            div()
                .font_family(MONO_FAMILY)
                .text_size(ui_size(11.))
                .text_color(rgb(t.text))
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .child(SharedString::from(label)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_size(ui_size(10.))
                .text_color(rgb(t.text_dim))
                .whitespace_nowrap()
                .overflow_hidden()
                .text_ellipsis()
                .child(SharedString::from(desc)),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 表单 kind 区块（agent composer bar / script 文件）
// ---------------------------------------------------------------------------
