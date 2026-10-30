//! 090 排队卡条（ZCode 桌面端排队 UI parity）：capsule 内编辑区上方，每张
//! 卡 = 拖拽手柄 + 首行预览 + [↑ 立即] + 铅笔（撤回编辑）+ 垃圾桶。
//! 排队条目**不进气泡流**（卡片即唯一呈现，flush 后 pi 回显才是气泡），
//! 派发中（flush_in_flight）整条半透明禁点——ZCode dispatch promoting 态。
//!
//! 拖拽重排照 function_panel/psp_drag 三件套（on_drag 发起 / 行侧
//! on_drag_move 半卡判定插入位 / 容器 on_drop 提交 + 看门狗兜底出窗松手）；
//! 载荷 = [`QueueDrag`]，重排落 SessionRuntime::queue_move。

use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb};

use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::ui::WithTip;
use crate::ui::icon;
use crate::{Chat, QueueDragState};

/// 拖拽载荷（drop 监听按 TypeId 匹配，与 psp 的 ProjDragTab 互不串扰）。
#[derive(Debug, Clone)]
pub(crate) struct QueueDrag {
    pub(crate) ix: usize,
}

/// 看门狗判定「再无 drag_move 续期」的阈值（psp_drag 同款：gpui 对窗口外
/// 松手不派发任何事件，拖拽态只能靠 staleness 兜底）。
const DRAG_STALE: Duration = Duration::from_millis(700);
/// 看门狗轮询间隔。
const WATCHDOG_TICK: Duration = Duration::from_millis(400);

/// 拖拽进行中的状态同步漏斗（容器与行侧共用）：条区内续期/重建（重建起
/// 看门狗），拖出条区立即清态。
fn sync_drag(chat: &mut Chat, inside: bool, cx: &mut gpui::Context<Chat>) {
    if inside {
        if let Some(d) = &mut chat.composer.queue_drag {
            d.at = Instant::now();
        } else {
            chat.composer.queue_drag = Some(QueueDragState { at: Instant::now() });
            chat.composer.queue_drop_ix = None;
            spawn_watchdog(cx);
        }
    } else if chat.composer.queue_drag.is_some() || chat.composer.queue_drop_ix.is_some() {
        chat.composer.queue_drag = None;
        chat.composer.queue_drop_ix = None;
        cx.notify();
    }
}

/// 拖拽看门狗（psp_drag 同款，注释详见彼处）：staleness 兜底清态。
fn spawn_watchdog(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(WATCHDOG_TICK).await;
            let alive = this
                .update(cx, |chat, cx| match &chat.composer.queue_drag {
                    None => false,
                    Some(d) => {
                        if d.at.elapsed() > DRAG_STALE {
                            chat.composer.queue_drag = None;
                            chat.composer.queue_drop_ix = None;
                            cx.notify();
                            false
                        } else {
                            true
                        }
                    }
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
        }
    })
    .detach();
}

/// 拖拽幽灵（gpui 画在光标处）：与卡同款小卡。gpui 以 min-content 约束
/// 幽灵布局，必须显式定宽。
pub(crate) struct QueueDragGhost {
    text: String,
}

impl gpui::Render for QueueDragGhost {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let t = crate::appearance::theme::theme();
        div()
            .w(px(320.))
            .h(px(28.))
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.nav))
            .shadow_lg()
            .text_size(crate::appearance::ui_size(12.))
            .text_color(rgb(t.text_dim))
            .child(icon("menu", 12., t.text_faint))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(self.text.clone())),
            )
    }
}

/// 22px 小图标动作钮（铅笔/垃圾桶；与 pending_images 删除钮同款 hover 态）。
fn icon_btn(
    id: String,
    name: &'static str,
    tip_key: &'static str,
    locked: bool,
    on_click: impl Fn(&mut gpui::App) + 'static,
    t: &'static Theme,
) -> gpui::Stateful<gpui::Div> {
    let cb = Rc::new(on_click);
    div()
        .id(SharedString::from(id))
        .size(px(22.))
        .rounded(px(5.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when(locked, |d| d.opacity(0.4))
        .when(!locked, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
        .with_tip(tip_key)
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            // 与兄弟节点/祖先（卡片 on_drag 等）的 hitbox 叠压，先拦
            cx.stop_propagation();
            if locked {
                return;
            }
            cb.clone()(cx);
        })
        .child(icon(name, 12., t.text_muted))
}

/// 排队卡条：无队列返回 None（不占布局；measure_height 自动跟随）。
pub(crate) fn queue_cards_strip(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    cx: &gpui::App,
) -> Option<AnyElement> {
    let rt = chat.rt().read(cx);
    if rt.queued_inputs.is_empty() {
        return None;
    }
    let locked = rt.flush_in_flight.is_some();
    let drop_ix = chat.composer.queue_drop_ix;

    let cards: Vec<AnyElement> = rt
        .queued_inputs
        .iter()
        .enumerate()
        .map(|(i, entry)| {
            let preview = entry
                .text
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .unwrap_or_default()
                .to_string();
            let ghost_text = preview.clone();
            let has_images = !entry.images.is_empty();

            let weak_now = weak.clone();
            let weak_edit = weak.clone();
            let weak_del = weak.clone();
            let weak_dragmove = weak.clone();

            div()
                .id(SharedString::from(format!("queue-card-{i}")))
                .w_full()
                .h(px(28.))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(if drop_ix == Some(i) {
                    rgb(t.accent)
                } else {
                    rgb(t.border)
                })
                .bg(rgb(t.bg_panel))
                .when(locked, |d| d.opacity(0.6))
                // 整卡可拖（手柄只是视觉锚；动作钮 stop_propagation 不受扰）
                .on_drag(QueueDrag { ix: i }, move |_, _, _window, cx| {
                    cx.new(|_| QueueDragGhost {
                        text: ghost_text.clone(),
                    })
                })
                .on_drag_move::<QueueDrag>(move |ev, _, cx| {
                    // 半卡判定：上/下半 = 插到本卡前/后（psp 行侧同款）
                    if !ev.bounds.contains(&ev.event.position) {
                        return;
                    }
                    let drag_ix = ev.drag(cx).ix;
                    let target = if ev.event.position.y < ev.bounds.center().y {
                        i
                    } else {
                        i + 1
                    };
                    let _ = weak_dragmove.update(cx, |c, cx| {
                        sync_drag(c, true, cx);
                        let no_op = target == drag_ix || target == drag_ix + 1;
                        let next = (!no_op).then_some(target);
                        if c.composer.queue_drop_ix != next {
                            c.composer.queue_drop_ix = next;
                            cx.notify();
                        }
                    });
                })
                // 拖拽手柄（视觉锚 + 抓手光标）
                .child(
                    div()
                        .id(SharedString::from(format!("queue-grip-{i}")))
                        .cursor(gpui::CursorStyle::OpenHand)
                        .child(icon("menu", 12., t.text_faint)),
                )
                // 远程插件来的条目：来源标识（与 composer 条目区分）
                .when(entry.remote, |d| {
                    d.child(icon("smartphone", 12., t.text_muted))
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap(px(4.))
                        .overflow_hidden()
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .text_ellipsis()
                                .text_size(crate::appearance::ui_size(12.))
                                .text_color(rgb(t.text))
                                .child(SharedString::from(preview)),
                        )
                        .when(has_images, |d| d.child(icon("image", 12., t.text_muted))),
                )
                // 「立即」pill（ZCode sendNow：该条以 steer 插进当前轮）
                .child(
                    div()
                        .id(SharedString::from(format!("queue-now-{i}")))
                        .h(px(22.))
                        .px(px(8.))
                        .rounded(px(5.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .bg(rgb(t.bg))
                        .flex()
                        .items_center()
                        .gap(px(3.))
                        .cursor_pointer()
                        .when(locked, |d| d.opacity(0.4))
                        .when(!locked, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
                        .with_tip("queue.send_now_tip")
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            cx.stop_propagation();
                            if locked {
                                return;
                            }
                            let _ = weak_now.update(cx, |c, cx| {
                                c.rt().update(cx, |r, cx| r.queue_send_now(i, cx));
                            });
                        })
                        .child(icon("arrow-up", 11., t.accent))
                        .child(
                            div()
                                .text_size(crate::appearance::ui_size(11.5))
                                .text_color(rgb(t.text_muted))
                                .child(tr("queue.send_now").to_string()),
                        ),
                )
                .child(icon_btn(
                    format!("queue-edit-{i}"),
                    "pencil",
                    "queue.edit",
                    locked,
                    move |cx| {
                        let _ = weak_edit.update(cx, |c, cx| {
                            c.with_active_editor(cx, |r, cx| r.queue_withdraw(i, cx));
                        });
                    },
                    t,
                ))
                .child(icon_btn(
                    format!("queue-del-{i}"),
                    "trash",
                    "queue.remove",
                    locked,
                    move |cx| {
                        let _ = weak_del.update(cx, |c, cx| {
                            c.rt().update(cx, |r, cx| r.queue_remove(i, cx));
                        });
                    },
                    t,
                ))
                .into_any_element()
        })
        .collect();

    Some(
        div()
            .id("queue-cards")
            .w_full()
            .pt(px(2.))
            .px(px(12.))
            .flex()
            .flex_col()
            .gap(px(4.))
            // 容器级 drag_move：条区内续期、拖出立即清态（psp 同款漏斗）
            .on_drag_move::<QueueDrag>({
                let w_sync = weak.clone();
                move |ev, _, cx| {
                    let inside = ev.bounds.contains(&ev.event.position);
                    let _ = w_sync.update(cx, |c, cx| sync_drag(c, inside, cx));
                }
            })
            .on_drop::<QueueDrag>({
                let w_drop = weak.clone();
                move |drag, _, cx| {
                    let from = drag.ix;
                    let _ = w_drop.update(cx, |c, cx| {
                        let target = c.composer.queue_drop_ix.take();
                        c.composer.queue_drag = None;
                        if let Some(target) = target {
                            c.rt().update(cx, |r, cx| r.queue_move(from, target, cx));
                        } else {
                            cx.notify();
                        }
                    });
                }
            })
            .children(cards)
            .into_any_element(),
    )
}
