//! composer 主输入区：input_area 键路由与装配、外层 wrap、惰性建
//! ComposerInput 实体（ensure_composer）。

use std::rc::Rc;

use gpui::{
    Context, KeyDownEvent, MouseButton, MouseDownEvent, SharedString, div, prelude::*, px, rgb,
};

use super::*;
use crate::Chat;
use crate::ComposerInput;
use crate::MenuKind;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;
use crate::ui::icon;
use crate::{ComposerDown, ComposerPaste, ComposerTab, ComposerUp};

/// `/` `@` 是否出现在输入中（菜单/过滤相关的廉价判定）。
/// 单次字节扫描（memchr 级），不跑两遍 `contains`。
fn has_menu_trigger(s: &str) -> bool {
    s.as_bytes().iter().any(|&b| b == b'/' || b == b'@')
}

/// `hero` = 012 新会话页模式：胶囊走正常流（由新会话页内容簇摆位），
/// 不再 0 高 + 绝对定位贴聊天区底。
pub(crate) fn input_area(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    streaming: bool,
    input_focused: bool,
    hero: bool,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let t = T();
    let can_queue = !chat.composer.input.is_empty() || !chat.composer.pending_images.is_empty();
    // 新会话默认值（pi selectInitialModelScope parity：settings 默认模型
    // 在 scope 内 → 用它，否则 scope 首个；思考档 pin > per-model > 全局）
    // ——草稿态（无 state）的 pill 显示就落在它身上，进程起来后由 pi 的
    // get_state 接管。只在 fallback 真会被消费时才算：它每帧全量扫模型
    // 目录（逐模型 format! 拼键 + 排序键），挂会话后模型/思考都有 state
    // 答案，扫描是纯浪费（打字每键一帧，最忌每帧 O(目录)）。
    let (model_label, thinking_label, tools_label, ctx_pct, compacting) = {
        let r = chat.rt().read(cx);
        let state_model = r.state.as_ref().and_then(|s| s.model_label());
        let state_thinking = r.state.as_ref().and_then(|s| s.thinking_level.clone());
        let need_draft = (state_model.is_none() && r.pending_model.is_none())
            || (state_thinking.is_none() && r.thinking_override.is_none());
        let (draft_model, draft_thinking) = if need_draft {
            chat.new_session_default()
        } else {
            (None, None)
        };
        // 手选（pending_model）优先于 state：它只在无活进程时写入（草稿 /
        // 进程已死），是用户最新意图，spawn 时才变成 --model；活进程下恒空
        let model_label = r
            .pending_model
            .as_ref()
            .map(|(p, id)| chat.model_display_name(p, id))
            .or(state_model)
            .or_else(|| {
                draft_model
                    .as_ref()
                    .map(|(p, id)| chat.model_display_name(p, id))
            })
            .unwrap_or_else(|| tr("model.select_title").to_string());
        let thinking_label = state_thinking
            .or_else(|| r.thinking_override.clone())
            .or_else(|| draft_thinking.clone())
            .unwrap_or_else(|| "auto".to_string());
        (
            model_label,
            thinking_label,
            r.tool_preset_label(),
            r.stats.as_ref().and_then(|s| s.context_percent),
            r.compacting,
        )
    };

    // 输入组件：惰性创建 + 每帧同步占位/值（set_value 同值跳过）
    let composer = ensure_composer(chat, weak, cx);
    // `!` shell 模式（031 pi-web bashMode parity）：去前导空白后以 ! 开头且
    // 无图片附着——边框着色 + 提示行（!! = 输出仅本地）
    let bash_mode = chat.composer.pending_images.is_empty()
        && chat.composer.input.trim_start().starts_with('!');
    // @ 菜单可能打开：预热/续期文件索引（TTL 内幂等，后台构建）
    if chat.active_menu(cx) == Some(MenuKind::At) {
        chat.ensure_at_index(cx);
    }
    let ph: SharedString = if compacting {
        // 压缩期间输入锁死（下方 on_change/on_submit 丢弃变更），用
        // placeholder 文案告知用户系统在做什么
        tr("composer.compacting_wait2").into()
    } else if streaming {
        // 090：按发送方式分档（排队=继续输入以排队 / 插话=Enter 即插话）
        if chat.rt().read(cx).queue_mode() {
            tr("composer.hint_queue").into()
        } else {
            tr("composer.hint_steer").into()
        }
    } else {
        tr("composer.hint_shortcuts").into()
    };
    // 一次实体 update 搞定全部镜像（P4）：四次 update = 四次租借 +
    // 四次可能的重绘标记，而它们本属于同一帧的同一件事。
    // set_value 只传借用切片（长草稿不全文拷贝，同值短路零分配）。
    //
    // 命令名同步只在 '/' 前缀时算（chip 派生只消费 '/' 前缀），普通打字
    // 每键不碰命令表；会话进程答案优先，否则启动装载的（项目 skill + 扩展
    // 缓存）。names 在 update 外先算好，避与 chat 的借用嵌套。
    let names: Option<Vec<String>> = if chat.composer.input.starts_with('/') {
        Some(
            chat.slash_commands(cx)
                .iter()
                .map(|c| c.name.clone())
                .collect(),
        )
    } else {
        None
    };
    {
        let input = &chat.composer.input;
        composer.update(cx, |f, fcx| {
            f.set_placeholder(Some(ph));
            // 压缩锁：编辑器组件整块不挂载（无焦点/IME/粘贴），显示
            // placeholder 样式的压缩提示；草稿保留，解锁即恢复
            f.set_read_only(compacting, fcx);
            f.set_value(input, fcx);
            if let Some(names) = names {
                f.set_command_names(names, fcx);
            }
        });
    }

    // ---- 胶囊 ----
    let mut capsule = div()
        .id("composer")
        .w_full() // 尺寸由外层锚点统一定（消息列宽 / 内容列宽封顶），/ 菜单与胶囊同宽
        .rounded(px(16.))
        .border_1()
        // 边框不随 agent 运行变色（pi-web 的 streaming 琥珀色已去掉）：
        // 焦点态用 accent；`!` shell 模式用工具绿（pi-web tool-bg parity）
        .border_color(if bash_mode {
            gpui::rgba(crate::appearance::theme::rgba_a(0x22c55e, 0.4))
        } else if input_focused {
            rgb(t.accent)
        } else {
            rgb(t.border)
        })
        .bg(rgb(t.bg))
        .shadow_lg()
        // 鼠标透传修复（psp_overlays 同款方案）：胶囊浮在聊天区上，指针处
        // 双层 hitbox 都命中，事件漏进底下的消息列表（悬停/滚轮/点击穿
        // 透）。occlude 让胶囊自身 bounds 遮挡先绘制的 hitbox；控件行子
        // 元素后绘制不受影响（occlusion 只作用于更早的 hitbox）。
        .occlude()
        .flex()
        .flex_col()
        .pt(px(10.))
        .pb(px(12.)); // 操作栏到下边框的距离
    // 附加图片缩略图条（v58 图片粘贴；布局与删除/预览交互见 pending_images_strip）
    if let Some(strip) = pending_images_strip(chat, weak, t) {
        capsule = capsule.child(strip);
    }
    // 090 排队卡条（queue_cards.rs）：queue 模式运行中的发送以卡片呈现，
    // 不进气泡流；立即/撤回编辑/删除/拖拽重排
    if let Some(strip) = queue_cards_strip(chat, weak, t, cx) {
        capsule = capsule.child(strip);
    }
    // 编辑区：真输入组件（多行 AutoGrow，超出 max 出纵向滚动条）；右键
    // 弹输入菜单（031，事件从组件冒泡上来——vendored 组件右键只做
    // 「无选区时移动光标」，不吞事件）。压缩锁期间整块是提示占位、无
    // 可编辑对象，不弹
    let weak_menu = weak.clone();
    capsule = capsule.child(
        div()
            .id("composer-editor")
            .w_full()
            .min_h(px(60.))
            .pt(px(2.))
            .pb(px(2.))
            .px(px(6.))
            .on_mouse_down(MouseButton::Right, move |ev: &MouseDownEvent, _, cx| {
                if compacting {
                    return;
                }
                cx.stop_propagation();
                let _ = weak_menu.update(cx, |c, cx| {
                    c.dlg.input_menu = Some((f32::from(ev.position.x), f32::from(ev.position.y)));
                    cx.notify();
                });
            })
            .child(composer),
    );
    capsule = wire_composer_keys(capsule, cx);
    // 控件行
    // 锁定期间强制收起胶囊菜单（工具预设/思考），否则菜单会浮在
    // 锁定的控件上还能点
    if compacting && chat.dlg.pill_menu.is_some() {
        chat.dlg.pill_menu = None;
    }
    capsule = capsule.child(composer_bar(
        chat,
        streaming,
        can_queue,
        compacting,
        &model_label,
        &thinking_label,
        &tools_label,
        ctx_pct,
        t,
        cx,
    ));

    // 012 新会话页（hero）胶囊走正常流，由新会话页的内容簇摆位；会话界面
    // 则 0 高 wrapper + 胶囊绝对定位悬浮（聊天消息从胶囊后滚过）。/ 菜单两种
    // 模式同构：挂在测量元素正上方（pi-web：bottom 100% + 8px 间隙）
    // 斜杠与 @ 菜单共用 slash_menu_view（视图内部按 kind 分支渲染）
    let slash_open = chat.active_menu(cx).is_some();
    // 「回到最新」按钮：不贴底且有消息时，悬浮在输入面板顶部上方 20px
    let show_scroll_btn =
        !chat.rt().read(cx).pager.is_at_bottom() && !chat.rt().read(cx).messages.is_empty();
    // 内容列宽（设置-界面可调；与消息列同源）
    let content_w = crate::services::workspace::chat_content_width();
    // 胶囊高度测量槽（导航刻度条 033 居中用）；先克隆引用再拼元素，
    // 避免 &chat.composer.composer_h 与下方 slash_menu_view(&mut chat) 借用冲突
    let composer_h_slot = chat.composer.composer_h.clone();
    let inner = div()
        .relative()
        // 必须撑满父宽：为 `w_full`+`max_w(内容列宽)` 的测量元素提供宽度基准，
        // 否则 shrink-to-fit 会把胶囊挤成窄条（会话界面实测踩坑）
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .when(slash_open, |d| {
            d.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom(gpui::relative(1.))
                    .pb(px(8.))
                    // 与胶囊同宽同位：inner 会话界面下是全宽（仅 px(15) 内
                    // 边距），直接铺满会比内容列宽封顶的胶囊宽一截；居中约束
                    // 与测量元素（同款 max_w）对齐后左右边缘贴合胶囊
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(content_w))
                            .child(crate::slash_menu_view(chat, weak, t, cx)),
                    ),
            )
        })
        .child(
            // 测量元素只包胶囊本体（不含回底按钮，按钮显隐会
            // 引入 ±52px 抖动）；/ 菜单是 absolute 不占布局
            crate::ui::measure_height(
                "composer-h",
                &composer_h_slot,
                div()
                    .relative()
                    // 外层（会话界面 px(15) / 新会话页内容簇）已扣除内边距，
                    // 此处宽度 == 消息列宽，再按内容列宽封顶（pi-web 单一内容列宽）
                    .w_full()
                    .max_w(px(content_w))
                    .child(capsule),
            ),
        );
    composer_wrap(inner, hero, show_scroll_btn, weak, t)
}

/// 附加图片缩略图（v58 图片粘贴）：56×56 圆角方块，panel 左上角并列、
/// 间隔 5px；右上角悬浮 16px 圆形 X 点击删除（pi-web ImagePreview 布局
/// parity：X 偏移 -4,-4 半嵌在缩略图角上）。无附件返回 None。
fn pending_images_strip(
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
fn wire_composer_keys(
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
fn composer_wrap(
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
                .bottom(px(20.))
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
fn ensure_composer(
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
                // chip 激活（命令已确认成胶囊）时 Enter=发送——裸 "/命令"
                // 的输入形态与菜单过滤态相同，必须以 chip 态区分，否则
                // Enter 会走"接受补全"永远发不出去
                let chip = chat
                    .composer
                    .composer
                    .as_ref()
                    .map(|c| c.read(cx).chip_active())
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
