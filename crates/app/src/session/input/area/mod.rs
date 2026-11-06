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
pub(crate) mod compose;
use compose::{composer_wrap, ensure_composer, pending_images_strip, wire_composer_keys};

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
        // 034：为底部状态带收紧胶囊内部留白（原 12，−5 定案见 034 §3.1）
        .pb(px(7.)); // 操作栏到下边框的距离
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
