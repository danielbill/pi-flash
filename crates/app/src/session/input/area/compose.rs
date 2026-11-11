//! composer 装配：附件条、键位接线、包裹容器与懒建。

use super::*;

/// 附加图片缩略图（v58 图片粘贴）：56×56 圆角方块，panel 左上角并列、
/// 间隔 5px；右上角悬浮 16px 圆形 X 点击删除（pi-web ImagePreview 布局
/// parity：X 偏移 -4,-4 半嵌在缩略图角上）。无附件返回 None。
pub(super) fn pending_images_strip(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> Option<gpui::AnyElement> {
    if chat.composer.pending_images.is_empty() {
        return None;
    }
    let thumbs: Vec<gpui::AnyElement> = chat
        .composer
        .pending_images
        .iter()
        .enumerate()
        .map(|(i, img)| {
            let weak_i = weak.clone();
            let weak_open = weak.clone();
            let thumb = img.thumb.clone();
            let thumb_for_open = thumb.clone();
            div()
                .id(SharedString::from(format!("img-thumb-{i}")))
                .relative()
                .size(px(56.))
                .flex_shrink_0()
                .child(
                    // 点击看大图（X 是 sibling 不在祖先链，两 handler 互不误触）
                    div()
                        .id(SharedString::from(format!("img-view-{i}")))
                        .size_full()
                        .rounded(px(6.))
                        .border_1()
                        .border_color(rgb(t.border))
                        .bg(rgb(t.bg_panel))
                        .overflow_hidden()
                        .cursor_pointer()
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let Some(image) = thumb_for_open.clone() else {
                                return;
                            };
                            let _ = weak_open.update(cx, |c, cx| {
                                c.dlg.dialog = Some(crate::Dialog::ImagePreview { image });
                                cx.notify();
                            });
                        })
                        .child(match thumb {
                            Some(image) => gpui::img(image)
                                .size_full()
                                .object_fit(gpui::ObjectFit::Cover)
                                .into_any_element(),
                            None => div().into_any_element(),
                        }),
                )
                .child(
                    div()
                        .id(SharedString::from(format!("img-del-{i}")))
                        .absolute()
                        .top(px(-4.))
                        .right(px(-4.))
                        .size(px(16.))
                        .rounded_full()
                        .bg(rgb(t.bg_panel))
                        .border_1()
                        .border_color(rgb(t.border))
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(|s| s.bg(rgb(t.bg_hover)))
                        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            // X 与图片盒的 hitbox 都含点击点（X 叠在其角
                            // 上），且二者非祖先——不拦截会继续派发到图
                            // 片盒的"开大图"handler（实测删除前弹预览）
                            cx.stop_propagation();
                            let _ = weak_i.update(cx, |c, cx| {
                                if i < c.composer.pending_images.len() {
                                    c.composer.pending_images.remove(i);
                                }
                                cx.notify();
                            });
                        })
                        .child(icon("x", 12., t.text_muted)),
                )
                .into_any_element()
        })
        .collect();
    Some(
        div()
            .w_full()
            .pt(px(4.))
            .px(px(12.))
            .flex()
            .flex_wrap()
            .gap(px(5.))
            .children(thumbs)
            .into_any_element(),
    )
}

/// 按键路由：组件处理的键（Enter/方向键/粘贴/undo…）到不了这里；这里只
/// 接组件无绑定的 Shift+Enter、已传播的 Escape，以及覆盖绑定截获的
/// ↑/↓/Tab 动作。（自 input_area 拆出，函数行数守卫）
pub(super) fn wire_composer_keys(
    capsule: gpui::Stateful<gpui::Div>,
    cx: &mut Context<Chat>,
) -> gpui::Stateful<gpui::Div> {
    capsule
        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
            let key = ev.keystroke.key.as_str();
            if key == "enter" && ev.keystroke.modifiers.shift {
                // Shift+Enter 永远是换行，无论 agent 在不在跑（2026-10-16
                // 用户定批：composer 只有 Enter 一个发送键，排队语义全
                // 并到 Enter 上）。历史上这里曾抢走做“排队追问”
                // ——用户一输字就被发走、输入框抹掉。
                // IME 组合期平台不派发按键（gpui windows events.rs），
                // 无需组合守卫
                if let Some(c) = &this.composer.composer {
                    c.update(cx, |f, fcx| f.insert_newline(window, fcx));
                }
                cx.stop_propagation();
                return;
            }
            if key == "escape" {
                // 走到这说明组件的 escape 已处理（解除 IME 标记且未清
                // 草稿）并传播；这里做 app 层语义：菜单取消 / 中止
                let menu = this.active_menu(cx);
                if let Some(kind) = menu {
                    if kind == MenuKind::Slash && !this.composer.input.is_empty() {
                        let v = format!("{} ", this.composer.input);
                        this.set_input(v, cx);
                    }
                    // @ 菜单 Esc = 只收起（pi-web parity：token 原样保留，
                    // 下一次输入变化重新打开）
                    this.composer.menu_dismissed = true;
                    this.composer.menu_ix = 0;
                    cx.notify();
                } else {
                    // `!` 执行中优先中止 bash（rpc abort_bash），其次模型流
                    let bash_running = this.rt().read(cx).bash_running;
                    let streaming = this.rt().read(cx).agent_running;
                    if bash_running {
                        this.abort_bash(cx);
                    } else if streaming {
                        this.abort_stream(cx);
                    }
                }
                cx.stop_propagation();
            }
        }))
        .on_action(cx.listener(|this, _: &ComposerUp, window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu(cx).is_some() && !items.is_empty() {
                this.composer.menu_ix = this.composer.menu_ix.saturating_sub(1);
                this.composer
                    .menu_scroll
                    .scroll_to_item(this.composer.menu_ix);
                cx.notify();
            } else if this.composer.input.is_empty() && !this.composer.history.is_empty() {
                let ix = match this.composer.history_ix {
                    None => this.composer.history.len() - 1,
                    Some(i) => i.saturating_sub(1),
                };
                this.composer.history_ix = Some(ix);
                let v = this.composer.history[ix].clone();
                this.set_input(v, cx);
            } else {
                // 非空多行：上移一行交还组件
                window.dispatch_action(Box::new(gpui_component::input::MoveUp), cx);
            }
        }))
        .on_action(cx.listener(|this, _: &ComposerDown, window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu(cx).is_some() && !items.is_empty() {
                this.composer.menu_ix = (this.composer.menu_ix + 1).min(items.len() - 1);
                this.composer
                    .menu_scroll
                    .scroll_to_item(this.composer.menu_ix);
                cx.notify();
            } else if this.composer.input.is_empty() {
                if let Some(i) = this.composer.history_ix {
                    if i + 1 < this.composer.history.len() {
                        this.composer.history_ix = Some(i + 1);
                        let v = this.composer.history[i + 1].clone();
                        this.set_input(v, cx);
                    } else {
                        this.composer.history_ix = None;
                        this.set_input(String::new(), cx);
                    }
                }
            } else {
                window.dispatch_action(Box::new(gpui_component::input::MoveDown), cx);
            }
        }))
        .on_action(cx.listener(|this, _: &ComposerTab, _window, cx| {
            let items = this.menu_items(cx);
            if this.active_menu(cx).is_some() && !items.is_empty() {
                let ix = this.composer.menu_ix.min(items.len() - 1);
                let insert = items[ix].insert.clone();
                this.accept_menu(insert, cx);
            }
        }))
        // Ctrl+V 截获（bind_keys 覆盖组件粘贴绑定）：剪贴板含图 → 附件化；
        // 否则重派发组件 Paste 走原文本粘贴
        .on_action(cx.listener(|this, _: &ComposerPaste, window, cx| {
            if !this.attach_clipboard_image(cx) {
                window.dispatch_action(Box::new(gpui_component::input::Paste), cx);
            }
        }))
}

/// composer 定位包装：会话界面（`hero=false`）= 0 高 wrapper + 胶囊绝对定位
/// 悬浮在聊天区底部之上（聊天消息从胶囊后滚过）；012 新会话页（`hero=true`）
/// = 正常流，由新会话页的内容簇摆位（012 的「下移 10%」）。
pub(super) fn composer_wrap(
    inner: gpui::Div,
    hero: bool,
    show_scroll_btn: bool,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    if hero {
        return div()
            .id("composer-wrap")
            .relative()
            .w_full()
            .flex_shrink_0()
            .child(inner)
            .into_any_element();
    }
    div()
        .id("composer-wrap")
        .relative()
        .h(px(0.))
        .flex_shrink_0()
        .child(
            div()
                .absolute()
                // 034：永久上移 8（原 20 → 28）。状态带是 absolute 覆盖层，
                // 开闭不改变布局，所以这里不随开关变——胶囊永不动。
                .bottom(px(crate::session::footer_bar::BAR_H))
                .left_0()
                .right_0()
                // 与消息列表同一水平内边距（session_list 的 px(15)）→ 胶囊列宽
                // 恒等于消息列宽，窗口缩放时同步收缩，不再靠百分比猜
                .px(px(15.))
                .flex()
                .flex_col()
                .items_center()
                // 按钮与胶囊间距 20px：随输入框增高自动上移，永不叠进面板
                .gap(px(20.))
                .when(show_scroll_btn, |d| {
                    d.child(crate::session::scroll_to_bottom_button(weak, t))
                })
                .child(inner),
        )
        .into_any_element()
}

/// 惰性创建 composer 输入组件（InputState 构造需要 &mut Window，挂在渲染
/// 期；回调经 weak 回写 Chat，不改持有结构）。
pub(super) fn ensure_composer(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    cx: &mut Context<Chat>,
) -> gpui::Entity<ComposerInput> {
    if let Some(c) = &chat.composer.composer {
        return c.clone();
    }
    let weak_change = weak.clone();
    let weak_submit = weak.clone();
    let weak_submit2 = weak.clone();
    let weak_chip = weak.clone();
    let c = cx.new(|icx| {
        let mut f = ComposerInput::new(icx);
        f.set_on_change(Rc::new(move |v, cx| {
            let _ = weak_change.update(cx, |chat, cx| {
                if chat.composer.input != v {
                    // 赋值前快照旧值的派生界面态（**纯字符串**，不碰任何实体）
                    let had_text = !chat.composer.input.is_empty();
                    let was_bang = chat.composer.input.trim_start().starts_with('!');
                    let had_menu_chars = has_menu_trigger(&chat.composer.input);
                    // enter() 污染签名（原文尾追 \n）：组件 Enter 先插换行
                    // 触发本次 Change，随即 PressEnter → on_submit 用剥 \n 的
                    // 值覆盖回来。此间过滤集未变，不能复位 menu_ix——否则
                    // ↑/↓/悬停选中的条目在 on_submit 读到的恒是 0，Enter 永远
                    // 接受第 0 项（@ 提及表现为「回车插的还是裸查询文本」）
                    //
                    // 先比长度再全串比：只有「旧值 + 1 字节 \n」才可能命中，
                    // 否则立即短路（每键一次全串比较对长草稿是白付的 O(n)）
                    let enter_pollution = v.len() == chat.composer.input.len() + 1
                        && v.ends_with('\n')
                        && &v[..v.len() - 1] == chat.composer.input;
                    // 写入镜像：**复用已有容量**，不每键分配一个新 String
                    // （长草稿每键一次全量分配 + 释放，是打字延迟的来源之一）
                    chat.composer.input.clear();
                    chat.composer.input.push_str(v);
                    // 草稿页跨重启持久化（2026-11-07 用户令）：活跃 runtime
                    // 是未晋级草稿时节流落盘（≤3s 一次）；FileBound 会话
                    // 不落（输入属会话本体）
                    let draft_cwd = chat
                        .pool
                        .runtimes
                        .get(&chat.pool.active_key)
                        .map(|rt| {
                            let r = rt.read(cx);
                            (r.file.is_none() && !r.observer)
                                .then(|| r.cwd.to_string_lossy().to_string())
                        })
                        .flatten();
                    if let Some(cwd) = draft_cwd {
                        pi_link::drafts::save_throttled(&cwd, v);
                    }
                    if !enter_pollution {
                        // 输入再次变化 = 解除点外/Esc 收起（active_menu 门控的
                        // 声明语义；打字不走 set_input，必须在此复位）
                        chat.composer.menu_dismissed = false;
                        chat.composer.menu_ix = 0;
                        // 过滤集变了回顶部，避免新列表停留在旧滚动深处
                        chat.composer
                            .menu_scroll
                            .set_offset(gpui::point(px(0.), px(0.)));
                    }
                    // **派生界面态不变就不 notify**：正文中间打字/删字（绝大
                    // 多数键）只改组件自己的内容，它自绘自己；此前每键一次
                    // Chat 整窗重绘，长会话里就是「input panel 输入迟缓」。
                    //
                    // 判定只许用**纯字符串比较**：本回调由 ComposerInput 自身
                    // 的 update 触发，此刻该实体正被可变借用——任何
                    // `composer.composer.read(cx)`（如 active_menu → at_token）
                    // 都是重入借用，gpui 直接 panic（2026-10-18 实际踩过：
                    // 每敲一个键就崩）。
                    //
                    // 整窗重绘的消费者只有三样：发送钮可用态（空↔非空）、
                    // `!` shell 边框（前导 ! 态）、slash/@ 菜单及其过滤（菜单
                    // 列表由 Chat 渲）。宁可多 notify，不可漏。
                    let has_text = !chat.composer.input.is_empty();
                    let is_bang = chat.composer.input.trim_start().starts_with('!');
                    let has_menu_chars = has_menu_trigger(&chat.composer.input);
                    if had_text != has_text
                        || was_bang != is_bang
                        || had_menu_chars
                        || has_menu_chars
                    {
                        cx.notify();
                    }
                }
            });
        }));
        f.set_on_submit(Rc::new(move |v, cx| {
            let _ = weak_submit.update(cx, |chat, cx| {
                if chat.rt().read(cx).compacting {
                    return;
                }
                // 先落到剥离换行后的 v 再判菜单：组件 enter() 会先插一个
                // 换行并经 Change 污染 chat.composer.input（含空白使菜单判定失败，
                // 回车被误当发送）
                chat.composer.input = v.to_string();
                // 命令 chip 激活时 Enter=发送——裸 "/命令" 的输入形态与菜单
                // 过滤态相同，必须以 chip 态区分，否则 Enter 会走"接受补全"
                // 永远发不出去。会话 chip 不在此列：它与菜单态无歧义，正文
                // 里 @ 文件时 Enter 仍应先接受补全（chip_sends_on_enter）
                let chip = chat
                    .composer
                    .composer
                    .as_ref()
                    .map(|c| c.read(cx).chip_sends_on_enter())
                    .unwrap_or(false);
                let items = chat.menu_items(cx);
                if !chip && chat.active_menu(cx).is_some() && !items.is_empty() {
                    let ix = chat.composer.menu_ix.min(items.len() - 1);
                    let insert = items[ix].insert.clone();
                    chat.accept_menu(insert, cx);
                    return;
                }
                let can_queue =
                    !chat.composer.input.is_empty() || !chat.composer.pending_images.is_empty();
                // 090：发送方式收在 send_input 内部分流（queue 模式=本地队列，
                // steer 模式=插话），两处同口径，一处分叉不可
                if can_queue {
                    chat.send_input(cx);
                } else {
                    // 空输入回车：不发送，顺手清掉组件自插的换行
                    chat.set_input(String::new(), cx);
                }
            });
        }));
        // Ctrl+Enter（090 反向快捷键）：queue 模式 = 本条立即 steer 插话；
        // steer 模式 = 显式入队（排队护栏：入本地队列，不直接发出去）
        f.set_on_submit_secondary(Rc::new(move |v, cx| {
            let _ = weak_submit2.update(cx, |chat, cx| {
                if chat.rt().read(cx).compacting {
                    return;
                }
                chat.composer.input = v.to_string();
                let can_queue =
                    !chat.composer.input.is_empty() || !chat.composer.pending_images.is_empty();
                if !can_queue {
                    return;
                }
                if chat.rt().read(cx).queue_mode() {
                    chat.steer_input(cx);
                } else {
                    chat.queue_enqueue_direct(cx);
                }
            });
        }));
        f.set_on_chip_backspace(Rc::new(move |cx| {
            let _ = weak_chip.update(cx, |chat, cx| {
                chat.set_input(String::new(), cx);
            });
        }));
        f
    });
    chat.composer.composer = Some(c.clone());
    c
}
