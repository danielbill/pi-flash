//! 行渲染残件：状态点色（二级菜单 2026-11-07 用户定夺删除——原本住在这里
//! 的 panel_row/统计段/状态标记随浮层一起退役；点色供导航轮卡第三行）。

use super::*;

/// 第三行状态点色调（与列表状态槽同构，唯一例外：运行中用中性灰）。
///
/// background 交付下轮可能先于子代理收尾——运行中的点若用 accent，会让已结束
/// 的轮看起来仍在活动；灰点表示"后台还在跑、尚无结果"，终态再落色（完成
/// NOTICE 青蓝 / 中断·轮次用尽 WARN 橙 / 异常 danger 红）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum DotTone {
    Muted,
    Notice,
    Warn,
    Danger,
}

pub(super) fn row_dot_tone(row: &CatalogRow) -> DotTone {
    match row.status_kind() {
        StatusKind::Running => DotTone::Muted,
        StatusKind::Done => DotTone::Notice,
        StatusKind::Aborted | StatusKind::MaxTurns => DotTone::Warn,
        StatusKind::Error => DotTone::Danger,
    }
}

/// 色调 → 主题色。
pub(super) fn row_dot_color(row: &CatalogRow) -> u32 {
    let t = T();
    match row_dot_tone(row) {
        DotTone::Muted => t.text_dim,
        DotTone::Notice => crate::appearance::theme::NOTICE,
        DotTone::Warn => crate::appearance::theme::WARN,
        DotTone::Danger => t.danger,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(phase: &str, stop: Option<&str>) -> CatalogRow {
        CatalogRow {
            child_id: "sub-1".into(),
            provider: "spawn".into(),
            label: None,
            profile: None,
            depth: 1,
            phase: phase.into(),
            stop_reason: stop.map(Into::into),
            error_code: None,
            session_file: None,
            parent_tool_call_id: Some("call-1".into()),
            usage_total: None,
            turns: None,
            agent_messages: None,
            tool_calls: None,
            started_at: String::new(),
            updated_at: String::new(),
        }
    }

    /// background 交付下 turn 先收尾、子代理仍在跑：运行点必须是中性灰，
    /// 不能是 accent（否则已结束的轮看起来仍在活动）。
    #[test]
    fn running_dot_is_muted_not_accent() {
        assert_eq!(row_dot_tone(&row("active", None)), DotTone::Muted);
        assert_eq!(row_dot_tone(&row("provisioning", None)), DotTone::Muted);
    }

    #[test]
    fn terminal_dots_follow_status_slot() {
        assert_eq!(
            row_dot_tone(&row("completed", Some("completed"))),
            DotTone::Notice
        );
        assert_eq!(row_dot_tone(&row("interrupted", None)), DotTone::Warn);
        assert_eq!(
            row_dot_tone(&row("completed", Some("aborted"))),
            DotTone::Warn
        );
        assert_eq!(
            row_dot_tone(&row("completed", Some("max_turns"))),
            DotTone::Warn
        );
        assert_eq!(row_dot_tone(&row("failed", None)), DotTone::Danger);
        assert_eq!(
            row_dot_tone(&row("completed", Some("error"))),
            DotTone::Danger
        );
    }
}
