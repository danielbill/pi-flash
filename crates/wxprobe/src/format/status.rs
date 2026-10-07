//! /status 状态卡排版（P2 切片 2）。
//!
//! 只移植**纯排版**原语；数据来源（workspace / task service / draft options /
//! 远端连接态）在 P3 的 `pipeline.rs` 里接 pi 会话状态。
//! 逐行对齐 ZCode `botsService.ts:4593-4624` 与 `statusFormatting.ts`。

use super::messages::{t, Lang};

/// ZCode `formatStatusLine`（`botsService.ts:4593`）：`"<标签>: <值>"`。
pub fn status_line(lang: Lang, label: &str, value: &str) -> String {
    format!("{}: {}", t(lang, label), value)
}

/// ZCode `formatStatusStateValue`（`botsService.ts:4602`）。
///
/// 注意 `en-US` 直接返回原始 state（`running` 而非 `Running`）——
/// 这是 ZCode 的原样行为，对拍必须保持，别"顺手修正"。
pub fn status_state_value(lang: Lang, state: &str) -> String {
    if lang == Lang::En {
        return state.to_string();
    }
    match state {
        "draft" => t(lang, "草稿"),
        "remote disconnected" => t(lang, "远端未连接"),
        "running" => t(lang, "⏳ 运行中"),
        "completed" => t(lang, "✅ 已完成"),
        "error" | "failed" => t(lang, "失败"),
        "cancelled" => t(lang, "已取消"),
        "stopped" => t(lang, "已停止"),
        other => other.to_string(),
    }
}

/// ZCode `formatStatusTaskLine`（`statusFormatting.ts:55`）：`"<标签>: <标题> (<id>)"`。
pub fn status_task_line(label: &str, title: &str, task_id: &str) -> String {
    format!("{label}: {title} ({task_id})")
}

/// ZCode `formatTaskRunningDuration`（`statusFormatting.ts:16`）。
///
/// 各级都对**上一级取模**（`% MS_IN_DAY` / `% MS_IN_HOUR` / `% MS_IN_MINUTE`），
/// 所以 25h 输出 `1d 1h` 而不是 `1d 25h`。
pub fn task_running_duration(duration_ms: u64) -> String {
    const MS_IN_SECOND: u64 = 1_000;
    const MS_IN_MINUTE: u64 = 60 * MS_IN_SECOND;
    const MS_IN_HOUR: u64 = 60 * MS_IN_MINUTE;
    const MS_IN_DAY: u64 = 24 * MS_IN_HOUR;

    let days = duration_ms / MS_IN_DAY;
    let hours = (duration_ms % MS_IN_DAY) / MS_IN_HOUR;
    let minutes = (duration_ms % MS_IN_HOUR) / MS_IN_MINUTE;
    let seconds = (duration_ms % MS_IN_MINUTE) / MS_IN_SECOND;

    let mut parts: Vec<String> = Vec::new();
    if days > 0 {
        parts.push(format!("{days}d"));
    }
    if hours > 0 {
        parts.push(format!("{hours}h"));
    }
    if minutes > 0 {
        parts.push(format!("{minutes}m"));
    }
    // ZCode：`seconds > 0 || parts.length === 0` —— 全零时也要输出 "0s"。
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{seconds}s"));
    }
    parts.join(" ")
}

/// P3 会用到的完整 /status 组装（当前只接受已解析好的字段，便于对拍）。
pub fn status_card(lines: &[String]) -> String {
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line_joins_label_and_value() {
        assert_eq!(
            status_line(Lang::ZhCn, "工作区", "/proj/pi-flash"),
            "工作区: /proj/pi-flash"
        );
        assert_eq!(
            status_line(Lang::En, "工作区", "/proj/pi-flash"),
            "Workspace: /proj/pi-flash"
        );
    }

    #[test]
    fn state_value_zh_is_translated_en_is_raw() {
        // zh 侧走译文，en 侧 ZCode 直接透传原始 state（`running` 小写）。
        assert_eq!(status_state_value(Lang::ZhCn, "running"), "⏳ 运行中");
        assert_eq!(status_state_value(Lang::ZhCn, "completed"), "✅ 已完成");
        assert_eq!(status_state_value(Lang::ZhCn, "error"), "失败");
        assert_eq!(status_state_value(Lang::ZhCn, "failed"), "失败");
        assert_eq!(status_state_value(Lang::ZhCn, "draft"), "草稿");
        assert_eq!(status_state_value(Lang::En, "running"), "running");
        assert_eq!(status_state_value(Lang::En, "draft"), "draft");
    }

    #[test]
    fn unknown_state_passes_through() {
        assert_eq!(status_state_value(Lang::ZhCn, "weird"), "weird");
        assert_eq!(status_state_value(Lang::En, "weird"), "weird");
    }

    #[test]
    fn task_line_shows_title_and_id() {
        assert_eq!(
            status_task_line("任务", "重构传输层", "task-42"),
            "任务: 重构传输层 (task-42)"
        );
    }

    #[test]
    fn duration_zero_still_prints_zero_seconds() {
        // ZCode `seconds > 0 || parts.length === 0` → 空 parts 必须补 "0s"。
        assert_eq!(task_running_duration(0), "0s");
        assert_eq!(task_running_duration(999), "0s", "亚秒级同样落到 0s");
    }

    #[test]
    fn duration_each_level_is_modulo_the_previous() {
        assert_eq!(task_running_duration(1_000), "1s");
        assert_eq!(task_running_duration(61_000), "1m 1s");
        assert_eq!(task_running_duration(3_600_000), "1h");
        assert_eq!(
            task_running_duration(5_400_000),
            "1h 30m",
            "1h30m 不能写成 1h 90m"
        );
        assert_eq!(
            task_running_duration(90_000_000),
            "1d 1h",
            "25h 必须取模成 1d 1h，不能是 1d 25h"
        );
        assert_eq!(task_running_duration(86_400_000), "1d");
        assert_eq!(
            task_running_duration(90_061_001),
            "1d 1h 1m 1s",
            "全级取模"
        );
    }

    #[test]
    fn card_is_newline_joined() {
        let card = status_card(&[
            status_line(Lang::ZhCn, "工作区", "pi-flash"),
            "------".to_string(),
            status_line(Lang::ZhCn, "状态", &status_state_value(Lang::ZhCn, "running")),
        ]);
        assert_eq!(card, "工作区: pi-flash\n------\n状态: ⏳ 运行中");
    }
}
