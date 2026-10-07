//! P3 步骤 2 —— 命令分发的**纯排版侧**（档 1：`/帮助` `/状态` `/新建`）。
//!
//! 需要 `Chat` / 会话状态的部分留在 [`crate::remote_control`]（`Action::{Status,New}`），
//! 这里只做「状态 → 文案」，方便单测。
//!
//! 对齐 ZCode `botsService.ts`：
//! * `/帮助` → `BOT_MENU_ORDER` 顺序 + `messages` 的 `help*` 句
//! * `/状态` → `createStatusReply` 的卡片结构
//! * `/新建` → 运行中回 `taskRunning`，否则建草稿后**回状态卡**（`case "new"`）

use wxprobe::command::BOT_MENU_ORDER;
use wxprobe::format::messages::{t, Lang};
use wxprobe::format::status::{status_card, status_line, status_state_value, status_task_line};

/// 品牌标题 —— ZCode 原文是「ZCode 机器人命令：」，产品里**换成本应用名**
/// （2026-10-07 用户拍板）。有意的本地化偏差，**不进** ZCode 文案表，
/// 与 `extui::LOCAL_TEXT` 同类；zh-TW 沿用 zh-CN。
pub(crate) fn help_title(lang: Lang) -> String {
    match lang {
        Lang::En => "pi-flash bot commands:".to_string(),
        _ => "pi-flash 机器人命令：".to_string(),
    }
}

/// `/帮助`：标题 + 按 `BOT_MENU_ORDER` 的 9 条说明（每条都是文案表里的现成句子）。
pub(crate) fn help_text(lang: Lang) -> String {
    let lines: Vec<String> = BOT_MENU_ORDER
        .iter()
        .map(|key| t(lang, help_line_key(key)))
        .collect();
    format!("{}\n{}", help_title(lang), lines.join("\n"))
}

/// `BOT_MENU_ORDER` 的键 → 文案表的 zh-CN 源串。
///
/// 表是**按 zh-CN 源串作键**的（P2 决定），所以这里必须做一次映射。
fn help_line_key(order_key: &str) -> &'static str {
    match order_key {
        "help" => "**/帮助** — 查看这份说明",
        "status" => "**/状态** — 查看工作区、模型和任务状态",
        "new" => "/新建 或 /clear — 开始新的任务草稿",
        "workspace" => "**/项目** — 切换工作区",
        "model" => "**/模型** — 切换模型",
        "mode" => "**/模式** — 切换运行模式",
        "thoughtLevel" => "**/思考** — 切换思考级别",
        "reply" => "**/回复** — 切换回复详细程度",
        "bind" => "**/bind <code>** — 绑定当前聊天",
        // BOT_MENU_ORDER 是 [String;9]，上面已穷举；表里缺句时 t() 会回退 key
        _ => "",
    }
}

/// 会话状态快照 → `/状态` 卡片所需字段（与 `Chat` 解耦，便于单测）。
pub(crate) struct StatusInput<'a> {
    pub workspace: String,
    pub model: String,
    /// `Some` 才渲染任务行（首条 prompt 落盘前是草稿，没有任务名）
    pub task: Option<(String, String)>,
    /// ZCode 的 state 词表：`draft` / `running` / `completed` / ...
    pub state: &'a str,
    /// 运行时长（ms）；`None` = 不渲染「已工作」行
    pub elapsed_ms: Option<u64>,
    /// 「进展」行；空串不渲染
    pub progress: String,
}

/// ZCode `createStatusReply` 的卡片结构（`------` 把工作区块与任务块分开）。
pub(crate) fn status_text(lang: Lang, s: &StatusInput<'_>) -> String {
    let mut lines = vec![
        status_line(lang, &t(lang, "工作区"), &s.workspace),
        status_line(lang, &t(lang, "模型"), &s.model),
        "------".to_string(),
    ];
    if let Some((title, id)) = &s.task {
        lines.push(status_task_line(&t(lang, "任务"), title, id));
    }
    lines.push(status_line(
        lang,
        &t(lang, "状态"),
        &status_state_value(lang, s.state),
    ));
    if let Some(ms) = s.elapsed_ms {
        lines.push(status_line(
            lang,
            &t(lang, "已工作"),
            &wxprobe::format::status::task_running_duration(ms),
        ));
    }
    if !s.progress.is_empty() {
        lines.push(status_line(lang, &t(lang, "进展"), &s.progress));
    }
    status_card(&lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> StatusInput<'static> {
        StatusInput {
            workspace: "pi-flash".into(),
            model: "glm-5.3".into(),
            task: Some(("重构传输层".into(), "task-42".into())),
            state: "running",
            elapsed_ms: Some(5_400_000),
            progress: "正在编辑 crates/wxprobe/src/wire.rs".into(),
        }
    }

    #[test]
    fn help_lists_all_nine_in_menu_order() {
        let s = help_text(Lang::ZhCn);
        let head = s.lines().next().unwrap();
        assert_eq!(head, help_title(Lang::ZhCn));
        let body: Vec<&str> = s.lines().skip(1).collect();
        assert_eq!(body.len(), BOT_MENU_ORDER.len(), "9 条一条不少");
        // 顺序 = BOT_MENU_ORDER
        assert!(body[0].contains("/帮助"));
        assert!(body[1].contains("/状态"));
        assert!(body[2].contains("/新建"));
        assert!(body[8].contains("/bind <code>"));
    }

    #[test]
    fn help_is_localized_but_keeps_command_names() {
        let en = help_text(Lang::En);
        assert!(en.starts_with(&help_title(Lang::En)));
        assert!(en.lines().any(|l| l == "**/help** — Show this guide"));
        // 括号里的命令名不翻译
        assert!(en.contains("/新建 或 /clear") || en.contains("/new or /clear"));
    }

    #[test]
    fn status_card_matches_zcode_layout() {
        let got = status_text(Lang::ZhCn, &sample());
        let lines: Vec<&str> = got.lines().collect();
        assert_eq!(lines[0], "工作区: pi-flash");
        assert_eq!(lines[1], "模型: glm-5.3");
        assert_eq!(lines[2], "------");
        assert_eq!(lines[3], "任务: 重构传输层 (task-42)");
        assert_eq!(lines[4], "状态: ⏳ 运行中");
        assert!(lines[5].starts_with("已工作: "));
        assert!(lines[6].starts_with("进展: "));
    }

    #[test]
    fn status_omits_optional_lines() {
        let s = StatusInput {
            workspace: "w".into(),
            model: "m".into(),
            task: None,
            state: "draft",
            elapsed_ms: None,
            progress: String::new(),
        };
        let got = status_text(Lang::ZhCn, &s);
        let lines: Vec<&str> = got.lines().collect();
        assert_eq!(lines.len(), 4, "无任务/时长/进展时只剩 3 行 + 分隔线");
        assert_eq!(lines[3], "状态: 草稿");
        assert!(!got.contains("已工作"));
        assert!(!got.contains("进展"));
    }

    #[test]
    fn english_state_passes_through_raw() {
        // §5 保真点：en 侧 status_state_value 透传原始 state（小写）
        let s = StatusInput {
            state: "running",
            ..sample()
        };
        let got = status_text(Lang::En, &s);
        assert!(got.lines().any(|l| l == "State: running"), "{got}");
    }
}
