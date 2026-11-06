//! 探查/测速/目录状态枚举（042）：McProbe、MjSpeedState、MjTestState、
//! McCatalogState。

/// 042：provider 通路探查状态（保存 key 自动一次 + 详情页【重新探查】）。
/// Failed 带已格式化的失败文案（与详情错误条同文，空目录占位复用）。
#[derive(Debug, Clone)]
pub(crate) enum McProbe {
    Running,
    Done,
    Failed(String),
}

/// 042-1 自定义商「测速」状态（单槽：详情页同屏只有一个）。测速通过 =
/// 自动保存表单 + 自动拉模型落目录；通过前【保存】【+模型】禁用。
/// `seeded` = 打开已有商时的初始通过态（非实测，不显示结果行）。
#[derive(Debug, Clone)]
pub(crate) enum MjSpeedState {
    Running,
    Passed {
        latency_ms: u64,
        models: usize,
        seeded: bool,
    },
    /// 通了但没解出模型条目：已保存，提示手动 +模型
    PassedEmpty,
    Failed(String),
}

impl MjSpeedState {
    /// 保存 / +模型 的可用判据（042-1：测速未通过前禁用）。
    pub(crate) fn allows_edit(&self) -> bool {
        matches!(
            self,
            MjSpeedState::Passed { .. } | MjSpeedState::PassedEmpty { .. }
        )
    }
}

/// 042 M2 自定义模型「测试」状态（pi-web ModelDetail test 同款：进行中 /
/// 通过（延迟 + HTTP 状态 + 回复文本）/ 失败文案）。
#[derive(Debug, Clone)]
pub(crate) enum MjTestState {
    Running,
    Ok {
        latency_ms: u64,
        status: u16,
        text: String,
    },
    Err(String),
}

/// 042 M2 models.dev 目录拉取状态（「填入模型信息」按钮与结果行）。
#[derive(Debug, Clone)]
pub(crate) enum MjCatalogState {
    Idle,
    Running,
    /// Done：回填成功（成功不占提示栏，用户定稿；只有失败才提示）
    Done,
    Err(String),
}
