//! Extension UI surface (pi-web ExtensionWidgets + blocking dialog
//! parity)：SetWidget 渲染 + 阻塞式 Select/Confirm/Input/Editor 弹窗，
//! 含协议动作 on_ext_ui / ext_respond（P2 自 ext_ui_actions.rs 并入）。
use crate::*;

use gpui::SharedString;

use crate::Chat;

/// One extension widget block (mono lines, pi-web widget rendering).
pub(crate) fn render_ext_widget(
    lines: &[String],
    t: &crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let text: String = lines.join("\n");
    div()
        .w_full()
        .px_3()
        .py_2()
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.tool_bg))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .text_size(crate::appearance::ui_size(11.))
        .text_color(rgb(t.text_muted))
        .child(SharedString::from(text))
        .into_any_element()
}

/// 标题栏短标题：首个非空行（长文本进信息面板）
fn bar_title_of(s: &str) -> String {
    s.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

/// Blocking extension UI dialog（通用壳：标题栏 + 信息/操作双面板 + 底部按钮）。
///
/// 挂 session/mod.rs 会话域居中浮层槽（chat-wrap 中栏，与 composer/问卷卡
/// 同一居中基准，遮罩只盖会话区），不挂全窗根。
///
/// 尺寸 600×600（小窗口按视口收缩），信息与操作各占剩余高度一半、各自
/// 滚动。扩展标题可长到 40+ 行（pi-usage 的 `/usage`「View all」会把所有
/// provider 列出来）：旧版不限高，弹窗比小窗口还高 → 卡片铺满视口，Close
/// 被顶到屏幕外、点背板又落在卡片上（stop_propagation）→ 弹窗根本关不掉，
/// 命令相位跟着一直亮。
mod actions;
mod dialog;

pub(crate) use dialog::render_ext_dialog;
