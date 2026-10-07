//! ExtUi 请求 ↔ 微信文本（060 §4.1 审批桥的微信侧半边）。
//!
//! 链路（app 侧已全部打通，本模块负责微信这一端的**文本映射**）：
//!
//! ```text
//! permission 扩展(tool_call) ── ctx.ui.confirm / select / input
//!       ↓  extension_ui_request
//! Event::ExtensionUi(req)            session/runtime.rs
//!       ↓  cx.emit(SessionEvent::ExtUi)
//! actions_runtime.rs  ├ active → chat.on_ext_ui（桌面弹窗）
//!                     └ parked → runtime.ext_queue
//!       ↓  本模块：req → 微信纯文本（render）
//! 用户回数
//!       ↓  本模块：文本 → ExtUiReply（parse）
//! Command::ExtensionUiResponse { id, value, confirmed, cancelled }
//! ```
//!
//! **回填语义以桌面端为准**（`crates/app/src/ext_ui.rs` 的实际调用）：
//!
//! | 桌面操作 | 回填 |
//! |---|---|
//! | 点第 N 个 Select 选项 | `value = Some(选项原文)`（**不是下标**） |
//! | Confirm 确定 / 取消 | `confirmed = Some(true / false)` |
//! | Input / Editor 提交 | `value = Some(文本)` |
//! | 点外 / ESC / 关闭 | `cancelled = true` |
//!
//! 微信侧必须产出**完全相同**的四类回填 —— 同一条 `ext_respond` 出口、
//! 同一个 pi 队列，桌面弹窗与微信端互不重复消费（§8 档 2 验收）。

use pi_link::protocol::ExtUiMethod;

use crate::format::menu::{option_label, option_index, render_selection, DisplayKind, MenuOption};
use crate::format::messages::Lang;

/// pi 专有、ZCode 文案表里没有的一句提示（本地补充，**不进** ZCode 文案表）。
/// ZCode 的 bot 没有 `input` / `editor` 这两种请求形态，对应文案是空的；
/// 按既有风格补一句，zh-TW 沿用 zh-CN（ZCode 无 zh-TW）。
const LOCAL_TEXT: &[(&str, &str, &str)] = &[(
    "回复文本，0 取消。",
    "回复文本，0 取消。",
    "Reply with text, or 0 to cancel.",
)];

fn local(lang: Lang, ix: usize) -> String {
    let e = &LOCAL_TEXT[ix];
    match lang {
        Lang::En => e.2.to_string(),
        _ => e.0.to_string(),
    }
}

const HINT_TEXT_CANCEL: usize = 0;

/// 是否阻塞型请求（需要微信端回一句才能继续）。
///
/// 对齐 `pi-link/protocol.rs` 的注释：select/confirm/input/editor 期待
/// `extension_ui_response`，其余 fire-and-forget。
pub fn is_blocking(method: &ExtUiMethod) -> bool {
    matches!(
        method,
        ExtUiMethod::Select { .. }
            | ExtUiMethod::Confirm { .. }
            | ExtUiMethod::Input { .. }
            | ExtUiMethod::Editor { .. }
    )
}

/// 把请求渲染成微信纯文本；`None` = 没有可展示内容（桌面专属副作用）。
///
/// `Notify` 非阻塞但**会**返回文本 —— 它就是一条要送到微信的通知。
/// `SetStatus` / `SetWidget` / `SetTitle` / `SetEditorText` 是桌面编辑器的
/// 侧效应，微信端无处可放，返回 `None`。
pub fn render(method: &ExtUiMethod, lang: Lang) -> Option<String> {
    match method {
        ExtUiMethod::Select { title, options } => {
            let opts: Vec<MenuOption> = options.iter().map(|o| MenuOption::new(o)).collect();
            // 通用选择菜单带 0 取消（ZCode formatSelectionFallback 默认分支）
            Some(render_selection(title, &opts, true, None, lang))
        }
        ExtUiMethod::Confirm { title, message } => {
            // 两条固定选项 + 0 取消 —— 与桌面的 确定/取消/点外关闭 三态一一对应。
            // 标题槽可以含换行，把 message 挂在标题里即得到
            //   {title}\n{message}\n0. 取消\n1. 允许\n2. 拒绝\n\n{hint}
            let allow = option_label(lang, DisplayKind::AllowOnce, "");
            let deny = option_label(lang, DisplayKind::RejectOnce, "");
            let opts = [MenuOption::new(&allow), MenuOption::new(&deny)];
            let head = if message.is_empty() {
                title.clone()
            } else {
                format!("{title}\n{message}")
            };
            Some(render_selection(&head, &opts, true, None, lang))
        }
        ExtUiMethod::Input { title, placeholder } => {
            let hint = local(lang, HINT_TEXT_CANCEL);
            Some(match placeholder {
                Some(p) if !p.is_empty() => format!("{title}\n{p}\n\n{hint}"),
                _ => format!("{title}\n\n{hint}"),
            })
        }
        ExtUiMethod::Editor { title, prefill } => {
            let hint = local(lang, HINT_TEXT_CANCEL);
            Some(match prefill {
                Some(p) if !p.is_empty() => format!("{title}\n{p}\n\n{hint}"),
                _ => format!("{title}\n\n{hint}"),
            })
        }
        ExtUiMethod::Notify { message, .. } => Some(message.clone()),
        ExtUiMethod::SetStatus { status_text, .. } => status_text.clone(),
        ExtUiMethod::SetWidget { widget_lines, .. } => {
            widget_lines.as_ref().map(|l| l.join("\n"))
        }
        ExtUiMethod::SetTitle { title } => Some(title.clone()),
        ExtUiMethod::SetEditorText { text } => Some(text.clone()),
    }
}

/// 微信入站文本 → 回填。
///
/// 编号解析复用 [`crate::format::menu::option_index`]，也就是 ZCode
/// `permission.respond` 那套 `Number.parseInt(v, 10) - 1` 语义 ——
/// 越界 / `NaN` 一律 [`ExtUiReply::Invalid`]，由调用方重发一次提示。
#[allow(clippy::too_many_lines)]
pub fn parse(method: &ExtUiMethod, text: &str) -> ExtUiReply {
    match method {
        ExtUiMethod::Select { options, .. } => {
            if text.trim() == "0" {
                return ExtUiReply::Cancelled;
            }
            match option_index(text) {
                Some(i) if i < options.len() => {
                    ExtUiReply::Select(options[i].clone())
                }
                _ => ExtUiReply::Invalid,
            }
        }
        ExtUiMethod::Confirm { .. } => match option_index(text) {
            // 1 → 第一项「允许」→ 确定；2 → 第二项「拒绝」→ 取消
            Some(0) => ExtUiReply::Confirm(true),
            Some(1) => ExtUiReply::Confirm(false),
            Some(_) | None => {
                if text.trim() == "0" {
                    ExtUiReply::Cancelled
                } else {
                    ExtUiReply::Invalid
                }
            }
        },
        ExtUiMethod::Input { .. } | ExtUiMethod::Editor { .. } => {
            let s = text.trim();
            if s == "0" {
                ExtUiReply::Cancelled
            } else if s.is_empty() {
                ExtUiReply::Invalid
            } else {
                ExtUiReply::Text(s.to_string())
            }
        }
        // fire-and-forget：不该有回填
        _ => ExtUiReply::NotApplicable,
    }
}

/// 微信入站的一条回填结果。映射到
/// `Command::ExtensionUiResponse { id, value, confirmed, cancelled }` 由 app 侧完成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtUiReply {
    /// Select：`value = Some(选项原文)`
    Select(String),
    /// Confirm：`confirmed = Some(bool)`
    Confirm(bool),
    /// Input / Editor：`value = Some(文本)`
    Text(String),
    /// `cancelled = true`（回 0，对应桌面的点外 / ESC）
    Cancelled,
    /// 解析不出来 —— 调用方应当把提示**重发一次**再等
    Invalid,
    /// 非阻塞请求不该有回填
    NotApplicable,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn select(options: &[&str]) -> ExtUiMethod {
        ExtUiMethod::Select {
            title: "选模型".into(),
            options: options.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn select_renders_with_cancel_line() {
        let m = select(&["claude", "gpt"]);
        let got = render(&m, Lang::ZhCn).unwrap();
        assert_eq!(
            got,
            "选模型\n0. 取消\n1. claude\n2. gpt\n\n回复数字选择，0 取消。"
        );
    }

    #[test]
    fn confirm_renders_two_options_and_message() {
        let m = ExtUiMethod::Confirm {
            title: "允许执行？".into(),
            message: "cargo build".into(),
        };
        let got = render(&m, Lang::ZhCn).unwrap();
        assert_eq!(
            got,
            "允许执行？\ncargo build\n0. 取消\n1. 允许\n2. 拒绝\n\n回复数字选择，0 取消。"
        );
        assert_eq!(
            render(&m, Lang::En).unwrap(),
            "允许执行？\ncargo build\n0. Cancel\n1. Allow\n2. Deny\n\nReply with a number to choose, or 0 to cancel."
        );
    }

    #[test]
    fn input_shows_placeholder_and_editor_shows_prefill() {
        let inp = ExtUiMethod::Input {
            title: "问个问题".into(),
            placeholder: Some("在这里输入".into()),
        };
        assert_eq!(
            render(&inp, Lang::ZhCn).unwrap(),
            "问个问题\n在这里输入\n\n回复文本，0 取消。"
        );
        let empty = ExtUiMethod::Input {
            title: "问".into(),
            placeholder: None,
        };
        assert_eq!(render(&empty, Lang::ZhCn).unwrap(), "问\n\n回复文本，0 取消。");

        let ed = ExtUiMethod::Editor {
            title: "编辑".into(),
            prefill: Some("line1".into()),
        };
        assert!(render(&ed, Lang::ZhCn).unwrap().starts_with("编辑\nline1\n"));
    }

    #[test]
    fn fire_and_forget_still_renders_notify() {
        let n = ExtUiMethod::Notify {
            message: "任务完成".into(),
            notify_type: Some("success".into()),
        };
        assert_eq!(render(&n, Lang::ZhCn).unwrap(), "任务完成");
        assert!(!is_blocking(&n));
        assert_eq!(parse(&n, "1"), ExtUiReply::NotApplicable);
    }

    #[test]
    fn blocking_kinds_match_pi_protocol() {
        assert!(is_blocking(&select(&["a"])));
        assert!(is_blocking(&ExtUiMethod::Confirm {
            title: "t".into(),
            message: "m".into()
        }));
        assert!(is_blocking(&ExtUiMethod::Input {
            title: "t".into(),
            placeholder: None
        }));
        assert!(is_blocking(&ExtUiMethod::Editor {
            title: "t".into(),
            prefill: None
        }));
        assert!(!is_blocking(&ExtUiMethod::SetTitle { title: "x".into() }));
        assert!(!is_blocking(&ExtUiMethod::SetStatus {
            status_key: "k".into(),
            status_text: None
        }));
    }

    #[test]
    fn select_reply_returns_option_text_not_index() {
        // 桌面端点选项回的是**选项原文**（ext_ui.rs:61 `ext_respond(Some(v))`）
        let m = select(&["claude", "gpt"]);
        assert_eq!(parse(&m, "1"), ExtUiReply::Select("claude".into()));
        assert_eq!(parse(&m, "2"), ExtUiReply::Select("gpt".into()));
        assert_eq!(parse(&m, " 2 "), ExtUiReply::Select("gpt".into()), "parseInt 跳空白");
        assert_eq!(parse(&m, "0"), ExtUiReply::Cancelled);
        assert_eq!(parse(&m, "3"), ExtUiReply::Invalid, "越界");
        assert_eq!(parse(&m, "abc"), ExtUiReply::Invalid, "NaN");
        assert_eq!(parse(&m, "-1"), ExtUiReply::Invalid);
    }

    #[test]
    fn confirm_reply_maps_one_to_true_two_to_false() {
        let m = ExtUiMethod::Confirm {
            title: "t".into(),
            message: "m".into(),
        };
        assert_eq!(parse(&m, "1"), ExtUiReply::Confirm(true));
        assert_eq!(parse(&m, "2"), ExtUiReply::Confirm(false));
        assert_eq!(parse(&m, "0"), ExtUiReply::Cancelled, "0 = 点外/ESC 关闭");
        assert_eq!(parse(&m, "3"), ExtUiReply::Invalid);
        assert_eq!(parse(&m, ""), ExtUiReply::Invalid);
    }

    #[test]
    fn input_reply_trims_and_rejects_zero_and_blank() {
        let m = ExtUiMethod::Input {
            title: "t".into(),
            placeholder: None,
        };
        assert_eq!(parse(&m, "  帮我读一下 README  "), ExtUiReply::Text("帮我读一下 README".into()));
        assert_eq!(parse(&m, "0"), ExtUiReply::Cancelled);
        assert_eq!(parse(&m, "   "), ExtUiReply::Invalid, "空文本不能提交");
        // 正文真的是 "0" 的场景无法与取消区分 —— 与桌面 ESC 语义取齐（取消优先）
        assert_eq!(parse(&m, "00"), ExtUiReply::Text("00".into()));
    }
}
