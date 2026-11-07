//! 行渲染残件：状态点色调（panel 行/二级菜单 2026-11-07 用户定夺删除；
//! 点色供导航轮卡第三行）。

use super::*;

/// 第三行状态点色调（与列表状态槽同构，唯一例外：运行中用中性灰）。
///
/// background 交付下轮可能先于子代理收尾——运行中的点若用 accent，会让已结束
/// 的轮看起来仍在活动；灰点表示"后台还在跑、尚无结果"，终态再落色（完成
/// NOTICE 青蓝 / 中断·轮次用尽 WARN 橙 / 异常 danger 红）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DotTone {
    Muted,
    Notice,
    Warn,
    Danger,
}

/// 从 agent ToolCall 块推状态点（主会话自足，pi-flash-9eck）：
/// - result details 带 stop_reason → 终态（completed=完成、aborted/max_turns=
///   中断橙、其余=异常红）；
/// - 无 stop_reason 但 result 带错误 → 异常红；
/// - 其余（status=started/running、result 未到）→ 运行中灰。
pub(crate) fn dot_tone_of(block: &Block) -> DotTone {
    let Block::ToolCall {
        details, is_error, ..
    } = block
    else {
        return DotTone::Muted;
    };
    let stop = details
        .as_ref()
        .and_then(|d| d.get("stop_reason"))
        .and_then(|v| v.as_str());
    match stop {
        Some("completed") => DotTone::Notice,
        Some("aborted" | "max_turns") => DotTone::Warn,
        Some(_) => DotTone::Danger,
        None if *is_error => DotTone::Danger,
        None => DotTone::Muted,
    }
}

/// 色调 → 主题色。
pub(crate) fn dot_color(tone: DotTone) -> u32 {
    let t = T();
    match tone {
        DotTone::Muted => t.text_dim,
        DotTone::Notice => crate::appearance::theme::NOTICE,
        DotTone::Warn => crate::appearance::theme::WARN,
        DotTone::Danger => t.danger,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(details: serde_json::Value, is_error: bool) -> Block {
        Block::ToolCall {
            content_index: 0,
            id: "a1".into(),
            name: "agent".into(),
            args: "{}".into(),
            result: String::new(),
            is_error,
            images: Vec::new(),
            duration_s: None,
            details: Some(details),
            args_partial: false,
            result_arrived: false,
        }
    }

    /// background 交付下 turn 先收尾、子代理仍在跑：运行点必须是中性灰，
    /// 不能是 accent（否则已结束的轮看起来仍在活动）。
    #[test]
    fn running_dot_is_muted_not_accent() {
        assert_eq!(
            dot_tone_of(&call(serde_json::json!({"status": "started"}), false)),
            DotTone::Muted
        );
        assert_eq!(
            dot_tone_of(&call(serde_json::json!({"status": "running"}), false)),
            DotTone::Muted
        );
        assert_eq!(
            dot_tone_of(&call(serde_json::json!({}), false)),
            DotTone::Muted
        );
    }

    #[test]
    fn terminal_dots_follow_status_slot() {
        assert_eq!(
            dot_tone_of(&call(
                serde_json::json!({"stop_reason": "completed"}),
                false
            )),
            DotTone::Notice
        );
        assert_eq!(
            dot_tone_of(&call(serde_json::json!({"stop_reason": "aborted"}), false)),
            DotTone::Warn
        );
        assert_eq!(
            dot_tone_of(&call(
                serde_json::json!({"stop_reason": "max_turns"}),
                false
            )),
            DotTone::Warn
        );
        assert_eq!(
            dot_tone_of(&call(serde_json::json!({"stop_reason": "error"}), false)),
            DotTone::Danger
        );
        assert_eq!(
            dot_tone_of(&call(serde_json::json!({}), true)),
            DotTone::Danger
        );
    }
}
