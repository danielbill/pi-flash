//! InputPanel 弹出菜单共用件：思考/工具预设档位表 + pill 菜单行渲染。
//!
//! 消费方两处：composer 胶囊菜单（shell/render/pill.rs）与设置·子代理页
//! 表单（模型/思考/工具下拉）——档位文案、行样式、激活判定只有这一份，
//! 两处不许再各自漂移（2026-11-07 子代理表单对齐 InputPanel）。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb};

use super::picker_shell::{PAD, ROW_H};
use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;

/// pill 菜单一行：主文案 + 右侧说明 + ✓ 槽位。
pub(crate) struct PillRow {
    /// 档位键（回调拿它写状态）
    pub key: &'static str,
    /// 主文案（思考/工具菜单 = 键本身；工具 full+ 档显示自定义名）
    pub label: String,
    /// 右侧说明（ui11 text_dim）
    pub desc: String,
    pub active: bool,
}

/// 思考档位表：auto = 「使用 pi 默认设置」（清 override），off/minimal/low/
/// medium/high/xhigh/max 与 pi `ThinkingLevel` 七档一一对应。`cur` = 当前
/// 生效档（"auto" = 未覆盖）。主文案 = 键，右侧 = 中文说明。
pub(crate) fn thinking_rows(cur: &str) -> Vec<PillRow> {
    const ROWS: [(&str, &str); 8] = [
        ("auto", "thinking.use_pi_default"),
        ("off", "thinking.off"),
        ("minimal", "thinking.minimal_reasoning"),
        ("low", "thinking.low_reasoning"),
        ("medium", "thinking.medium_reasoning"),
        ("high", "thinking.high_reasoning"),
        ("xhigh", "thinking.extra_high_reasoning"),
        ("max", "thinking.max_reasoning"),
    ];
    ROWS.iter()
        .map(|(k, desc)| PillRow {
            key: k,
            label: (*k).to_string(),
            desc: tr(desc).to_string(),
            active: *k == cur,
        })
        .collect()
}

/// 工具预设五档表（composer 同款）：键为主文案（custom 档显示 full+ 名），
/// 右侧 = 档位说明。`cur` = 当前预设键（profile「full+」档在子代理侧映射
/// 成 composer 键 `custom` 后传入）。
pub(crate) fn tools_preset_rows(cur: &str) -> Vec<PillRow> {
    const ROWS: [(&str, &str); 5] = [
        ("chat-only", "thinking.chat_only"),
        ("read-only", "tools.preset_readonly_count"),
        ("default", "tools.preset_full_count"),
        // full：内置 7 件 + 精确集（-ne + 个人扩展/内置扩展，包零注入）
        ("full", "extensions.builtin_all_desc"),
        // full+ = full + 本会话选中扩展（默认档；清单按会话保存）
        ("custom", "tools.preset_full_desc"),
    ];
    ROWS.iter()
        .map(|(k, desc)| PillRow {
            key: k,
            // full+ 档显示自定义名（其余档沿用内部键）
            label: if *k == "custom" {
                tr("extensions.full").to_string()
            } else {
                (*k).to_string()
            },
            desc: tr(desc).to_string(),
            active: *k == cur,
        })
        .collect()
}

/// pill 菜单行（✓槽位 + 主文案 + 右侧说明）：composer 胶囊菜单与设置页
/// 下拉共用。`on_pick` 由调用方写入各自状态（composer 写 runtime，设置写
/// 表单字段）；行内点击 stop_propagation，不冒泡到「点外关闭」。
pub(crate) fn pill_row(
    weak: &gpui::WeakEntity<Chat>,
    on_pick: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
    row: PillRow,
) -> AnyElement {
    let t = T();
    let weak = weak.clone();
    let key_id = row.key.replace('/', "+");
    div()
        .id(SharedString::from(format!("pm-row-{key_id}")))
        // 行内只有水平 PAD；上下不塞 padding（行高钉 ROW_H，与扩展菜单
        // 同款节奏，条目间不松散）
        .h(px(ROW_H))
        .px(px(PAD))
        .flex()
        .items_center()
        .gap_2()
        .cursor_pointer()
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .on_mouse_down(MouseButton::Left, move |_, _, cx| {
            cx.stop_propagation();
            let _ = weak.update(cx, |c, cx| on_pick(c, cx));
        })
        .child(
            div()
                .w(px(16.))
                .flex()
                .items_center()
                .justify_center()
                .children(row.active.then(|| crate::ui::icon("check", 13., t.accent))),
        )
        .child(
            div()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(if row.active {
                    gpui::FontWeight::SEMIBOLD
                } else {
                    gpui::FontWeight::NORMAL
                })
                .text_color(rgb(t.text))
                .child(SharedString::from(row.label)),
        )
        .child(
            div()
                .ml_auto()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(row.desc)),
        )
        .into_any_element()
}
