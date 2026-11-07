//! `impl Render` —— 脏缓冲兑现与内件装配（视觉档位 compact/flat/form）。
//! 拆出自 text_input.rs，逻辑逐字未动。

use gpui::{
    Focusable as _, KeyDownEvent, ParentElement, Render, SharedString, Styled, Window, div,
    prelude::*,
};

use crate::appearance::theme::theme as T;
use gpui_component::input::TextInput as GpInput;

use super::TextInput;

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let state = self.ensure_state(window, cx).expect("input state");
        // apply dirty buffers (inner setters need the window)
        if let Some(v) = self.pending_value.take() {
            self.value = v.clone();
            let select_all = self.select_all_on_focus;
            state.update(cx, |st, scx| {
                st.set_value(v.clone(), window, scx);
                // select_all_on_focus 的**建态即选**路径：抢在首个 prepaint
                // 之前把选区落成全选——vendor prepaint 的光标跟随滚动块被
                // is_selected_all 门闸整个跳过。否则 set_value 把光标甩到
                // 文本尾，首帧布局按尾光标滚，CJK 长行 position_for_index
                // 越界外推会把横向滚动推出屏外（改名框一打开空白的根因，
                // 2026-10-13 pif-ui 实测复现）。focus 晚于首帧的场景（弹层
                // 筛选框先建后聚焦）仍走 Focus 事件路径。
                if select_all && !v.is_empty() {
                    let len = st.value().len();
                    st.set_selected_range_reversed(0..len, scx);
                }
            });
            if select_all {
                self.select_all_done = true; // 免 Focus 事件路径重复全选
            }
        }
        if let Some(r) = self.pending_select.take() {
            state.update(cx, |st, scx| st.set_selected_range(r, scx));
        }
        if let Some(ph) = self.pending_placeholder.take() {
            state.update(cx, |st, scx| st.set_placeholder(ph, window, scx));
        }
        // 颜色覆盖（幂等重设：vendor 侧值不变不 notify）
        let caret = self.caret_color.map(|c| gpui::rgb(c).into());
        let ph_color = self.placeholder_color.map(|c| gpui::rgb(c).into());
        let tx_color = self.text_color.map(|c| gpui::rgb(c).into());
        if caret.is_some() || ph_color.is_some() || tx_color.is_some() {
            state.update(cx, |st, scx| {
                st.set_caret_color(caret, scx);
                st.set_placeholder_color(ph_color, scx);
                st.set_text_color(tx_color, scx);
            });
        }
        if let Some(m) = self.pending_masked.take() {
            state.update(cx, |st, scx| st.set_masked(m, window, scx));
        }
        if self.want_select_all {
            self.want_select_all = false;
            // 全选 + 光标锚起点（vendor selection_reversed）：头部可见，
            // 键入仍整段替换
            let len = state.read(cx).value().len();
            state.update(cx, |st, scx| st.set_selected_range_reversed(0..len, scx));
        }
        if self.want_focus {
            self.want_focus = false;
            // vendor InputState::focus：句柄聚焦 + 光标 blink 同帧启动。
            // 裸 window.focus 会踩 gpui 订阅激活时序：on_focus 监听在
            // SubscriberSet 里以 active=false 插入、cx.defer 才激活，
            // retain 派发跳过未激活者——「建内件当帧 focus」的首发路径
            // 必错过同帧 Focus 相位的 focus-in，blink.visible 恒 false，
            // 新建/改名行光标不闪、直到首个键入 pause() 强制可见
            //（pif-ui 三连 shot 字节不变复现，键入后即刻恢复）
            state.update(cx, |st, scx| st.focus(window, scx));
        }

        let t = T();
        let focused = state.read(cx).focus_handle(cx).is_focused(window);
        // 多行：容器给足高度，内件 h_full 撑满（单行保持固定 30px；紧凑
        // 档 20px = vendor XSmall 内件同高，**零富余**——任何一侧都不留
        // 白隙，行内垂直居中交给宿主行 items_center 链）；lines(n) =
        // auto_grow，内件自算高（min = n × 行高），容器不设高、内件不撑满
        let auto_h = self.multiline && self.lines.is_some();
        let (h, fill) = if auto_h {
            (gpui::px(0.), false)
        } else if self.multiline {
            (gpui::px(170.), true)
        } else if self.form {
            (gpui::px(44.), false)
        } else if self.compact {
            (gpui::px(20.), false)
        } else {
            (gpui::px(30.), false)
        };
        let mut input = GpInput::new(&state)
            .appearance(!self.flat)
            .bordered(!self.flat)
            .disabled(self.disabled)
            .map(|input| {
                if self.compact {
                    use gpui_component::Sizable as _;
                    input.with_size(gpui_component::Size::XSmall)
                } else {
                    input
                }
            });
        if self.flat {
            // Zed 行内编辑零内距（project_panel 把 Editor 原样塞进 Label 位，
            // 文字起点由宿主行给）：flat 档宿主行已对好起点，内件 XSmall 的
            // input_px(4) 会让改名文字比行标签多缩进 4px（refine_style 在
            // input_px 之后生效，可覆盖）
            input = input.pl_0();
        }
        if !self.flat {
            input = input
                .bg(gpui::rgb(t.bg_panel))
                .border_color(gpui::rgb(if focused { t.accent } else { t.border }))
                .rounded(gpui::px(5.));
        }
        if fill {
            input = input.h_full();
        }
        div()
            .w_full()
            .when(self.disabled, |d| d.opacity(0.7))
            .when(!auto_h, |d| d.h(h))
            // compact：槽高与 XSmall 内件（vendor input_h 20px）同高、零富余
            //（曾 22px，stretch 对定高子件退化为 top-align，2px 全落底部把
            // 输入基线顶高 1px——改名行文字上跳根因）；items_center 兜底。
            // form（040-2）：44px 槽 + 30px 内件，同样必须居中，否则 placeholder
            // 贴着上边（用户实测「文字没垂直居中」）。
            .when(self.compact, |d| d.items_center())
            // form：items_center 对 vendor 内件不生效（内件 layout 高被
            // stretch 成槽高，文字仍按行盒顶对齐），直接用 padding 把 30px
            // 内件压到 44px 槽的垂直正中（(44-30)/2 = 7）
            .when(self.form, |d| d.pt(gpui::px(5.)).pb(gpui::px(5.)))
            // 文字轨 = Zed create_style 同款（editor.rs：SingleLine/AutoHeight →
            // ui_font + UI 字号，Full → buffer font）：单行走面板 UI 字体（文件树
            // 行内改名与行标签同族同号，Zed project_panel 对编辑器零覆盖的同款
            // 效果），多行走内置 mono（MCP 粘贴框等 buffer 轨）。字号 ui() 双轨
            //（§5.5 ui(12)，compact 档随宿主行 ui(11)），销债表 #2 的裸 px。
            .font_family(if self.multiline || self.form {
                SharedString::from(crate::editor::markdown::MONO_FAMILY)
            } else {
                crate::appearance::panel_font().family.clone().into()
            })
            .text_size(if self.form {
                gpui::px(15.)
            } else {
                crate::appearance::ui_size(if self.compact { 11. } else { 12. })
            })
            // form（040-2）：内件行高**取整行高**（默认 1.5em = 22.5 → 22.5px
            // 非"整"，44 槽居中后文字偏上 5px——用户实测「placeholder 没有垂直
            // 居中」）。指定 20px 行高后文字在 44px 槽里几何居中。
            .when(self.form, |d| d.line_height(gpui::px(20.)))
            // escape bubbles up from the inner widget (it only unmarks IME
            // composition and propagates) — turn it into the app callback
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                if ev.keystroke.key == "escape" {
                    if let Some(cb) = this.on_escape.take() {
                        cb(cx);
                        this.on_escape = Some(cb);
                        // 只在输入框自己消费 ESC（如收起弹层）时拦截；
                        // 否则放行给外层（弹窗 ESC 关闭）
                        cx.stop_propagation();
                    }
                }
            }))
            .child(input)
    }
}
