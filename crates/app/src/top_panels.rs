//! 系统提示词 / 工具定义 两个面板 —— **直接复用设置弹窗那套窗体**
//! （`ui::overlay::big_card`：0.7×0.98 大卡片 + chrome 顶条 + ×；外面那层
//! 遮挡/外点/ESC 由 `ui::overlay::layer` 保证）。
//!
//! 内容逐条对齐 pi-web 组件：
//! - [`system_prompt_panel`] ← components/SystemPromptPanel.tsx
//! - [`tool_definitions_panel`] ← components/ToolDefinitionsPanel.tsx
//!   （左侧工具列表走大卡片的 nav 位，与设置弹窗左导航同款）
//!
//! 数据同源：pi 0.86+ 把系统提示词与工具声明写进 transcript 的 system 消息，
//! runtime 在 get_messages 时用 `pi_link::transcript::transcript_system` replay
//! （pi-ai transcript.js 的读法，pi-web 的 `state.systemPrompt` 也是这份重放）。
//!
//! 历史上这里曾是一幅「topbar 下方半屏面板」（pi-web activeTopPanel 的位置），
//! 用户口径改为复用设置弹窗窗体后删除——不要再自造窗体。

use gpui::{App, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::TopPanel;
use crate::appearance::theme::Theme;
use crate::i18n::tr;

/// ⋯ 菜单入口：把两个面画进设置弹窗同款窗体（遮挡/外点/ESC = 浮层基座）。
pub(crate) fn session_info_dialog(
    kind: TopPanel,
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &Theme,
    cx: &App,
) -> gpui::AnyElement {
    let sys = chat.rt().read(cx).sys.clone();
    let title = match kind {
        TopPanel::System => tr("session.system_prompt"),
        TopPanel::Tools => tr("system_prompt.tool_definitions"),
    };
    // 左导航位：工具面板放工具名列表（设置弹窗左导航同款），系统提示词没有
    let nav = match kind {
        TopPanel::System => None,
        TopPanel::Tools => Some(tool_list(
            chat,
            weak,
            sys.as_ref().map(|s| s.tools.as_slice()),
            t,
        )),
    };
    let body = match kind {
        TopPanel::System => {
            system_prompt_panel(sys.as_ref(), chat, weak, &chat.aux.sysprompt_scroll, t)
        }
        TopPanel::Tools => tool_detail(
            selected_tool(chat, sys.as_ref().map(|s| s.tools.as_slice())),
            sys.as_ref().map(|s| s.tools.as_slice()),
            t,
        ),
    };

    let weak_dismiss = weak.clone();
    let dismiss = move |_w: &mut gpui::Window, cx: &mut gpui::App| {
        let _ = weak_dismiss.update(cx, |c, cx| {
            c.dlg.dialog = None;
            cx.notify();
        });
    };
    crate::ui::overlay::layer(true, Some(&chat.dlg.dialog_focus), dismiss.clone())
        .flex()
        .items_center()
        .justify_center()
        .child(crate::ui::overlay::big_card(&title, nav, body, t, dismiss))
        .into_any_element()
}

/// 当前选中的工具：`tool_sel` 命中就用它，否则回落列表首项（pi-web
/// ToolDefinitionsPanel 的 selectedToolName ?? activeTools[0] 同义）。
fn selected_tool<'a>(
    chat: &Chat,
    tools: Option<&'a [pi_link::transcript::ToolDecl]>,
) -> Option<&'a pi_link::transcript::ToolDecl> {
    chat.aux
        .tool_sel
        .as_deref()
        .and_then(|name| tools.and_then(|list| list.iter().find(|t| t.name == name)))
        .or_else(|| tools.and_then(|list| list.first()))
}

/// 空态/未加载态文案（pi-web `.system-prompt-empty` / `.tool-definitions-empty`：
/// 12px、斜体、muted；`padded` = 列表里的 14px/12px 内边距）。
fn empty_state(text: &str, t: &Theme, padded: bool) -> gpui::AnyElement {
    let mut el = div()
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(t.text_muted))
        .italic()
        .child(SharedString::from(text.to_string()));
    el = if padded {
        el.px(px(12.)).py(px(14.))
    } else {
        el.py(px(10.))
    };
    el.into_any_element()
}

mod system_prompt;
mod tools;

use self::system_prompt::system_prompt_panel;
use self::tools::{tool_detail, tool_list};

pub(crate) use system_prompt::bucket_label;
