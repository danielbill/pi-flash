//! TextView 包装（消息划选一期）：agent **定稿**文本块换用 gpui-component
//! TextView（鼠标划选 + 根级 Ctrl+C 复制，见 shell/render.rs 兜底）；流式
//! 块仍走自研渲染器（TextView 文本变更是 200ms 防抖后台重解析，会拖慢流式
//! 显示）。用户消息不动（气泡/技能折叠逻辑特殊）。
//!
//! 为什么是 RenderOnce 包装而不是参数下传：list() 条目闭包里 `rt_view`
//! 不可变借用锁死 cx（turn_msgs 持 &Msg），再把 `&mut App` 传进渲染链必然
//! E0502；RenderOnce::render 是元素树实现期回调，天然持有 window+cx，
//! TextView::markdown（use_keyed_state 需要 &mut Window）在此构造零改造。

use gpui::{
    App, ElementId, IntoElement, RenderOnce, SharedString, Styled, Window, div, prelude::*, px,
    relative, rgb,
};
use gpui_component::ActiveTheme;
use gpui_component::text::TextView;

use crate::appearance::theme::theme as T;
use crate::editor::markdown;

#[derive(IntoElement, Clone)]
pub(crate) struct AgentMarkdown {
    id: ElementId,
    text: SharedString,
}

impl AgentMarkdown {
    /// id 必须带文本哈希：use_keyed_state 按全局 id 存解析树，会话切换/
    /// 分支跳转后同位同 id 会先画 200ms 旧内容（异步重解析防抖期）。
    /// 还必须带主题名（issue#4）：gpui-component CodeBlock 在解析期把
    /// highlight_theme 烘进逐 span 样式，id 不含主题时文本不变就不重
    /// 解析——浅色主题下渲染过的消息切到暗主题后，代码块永远停在浅色
    /// 语法盘上（暗底配 #0433ff/#0000A2，即用户截图的难读色）。
    pub(crate) fn new(id: SharedString, text: impl Into<SharedString>) -> Self {
        Self {
            id: ElementId::from(SharedString::from(format!(
                "{}#{}",
                id,
                crate::appearance::theme::theme_name()
            ))),
            text: text.into(),
        }
    }
}

impl RenderOnce for AgentMarkdown {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = T();
        div()
            .w_full()
            // 正文样式对齐自研渲染器：会话字体/字号、1.5 行高（gpui 0.2.2
            // StyledText 排版取容器继承链，字号/族必须挂容器）
            .font_family(crate::appearance::session_font().family)
            .text_size(crate::appearance::sess_size(0.))
            .line_height(relative(1.5))
            .text_color(rgb(t.text))
            .child(
                TextView::markdown(self.id, self.text, window, cx)
                    .selectable()
                    .style(text_view_style(cx)),
            )
    }
}

/// PF 主题 → TextViewStyle：标题梯度抄 markdown::size_for_level（pi-web
/// em 档，相对 14px 基准的绝对差），代码块底色抄 render_code_block 的
/// mix(t.bg, t.bg_panel, 0.92)，高亮盘用已随 sync_gpui_tokens 映射的
/// gpui-component Theme（选区色/前景色组件内自取，无需在此传）。
fn text_view_style(cx: &App) -> gpui_component::text::TextViewStyle {
    let t = T();
    // Styled 方法是 move 链式（self -> Self），只能整体一条链不能逐语句调
    // 边框对齐 pi-web .markdown-code-block（1px var(--border) + 圆角）——
    // 语言栏/复制/行号是 vendor 级活（pi-flash-lamn 二期），此处先补框
    let code_block = gpui::StyleRefinement::default()
        .bg(gpui::rgb(crate::appearance::theme::mix_rgb(
            t.bg, t.bg_panel, 0.92,
        )))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(7.))
        .font_family(markdown::MONO_FAMILY)
        .text_size(crate::appearance::sess_size(-1.));
    gpui_component::text::TextViewStyle {
        // 块间距：自研渲染器正文块 CSS 规格 8px（root 16px 下 0.5rem）
        paragraph_gap: gpui::rems(0.5),
        heading_base_font_size: crate::appearance::sess_size(0.),
        heading_font_size: Some(std::sync::Arc::new(|level, base| {
            let delta = match level {
                1 => 14. * 0.16,
                2 => 14. * 0.08,
                3 => -14. * 0.02,
                _ => 0.,
            };
            px(f32::from(base) + delta)
        })),
        highlight_theme: cx.theme().highlight_theme.clone(),
        code_block,
        is_dark: t.dark,
    }
}
