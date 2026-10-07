//! 档 2 的**数字选择菜单状态机**（060-c）。
//!
//! 对齐 ZCode `pendingSelectionsByContext`：菜单挂起时，入站的数字**优先**
//! 被菜单吃掉，不落进模型；回 `0` 或解析失败各有明确出路。
//!
//! 渲染复用 ZCode 的 [`render_selection`]（微信纯文本编号菜单），
//! 编号解析复用 `permission.respond` 那套 `Number.parseInt(v,10)-1` 语义
//! （`menu::option_index`），与权限审批共用同一条解析路径。

use wxprobe::format::menu::{option_index, render_selection, MenuOption};
use wxprobe::format::messages::Lang;

/// 菜单种类 —— 决定选中后由 `Chat` 执行什么动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuKind {
    /// `/思考` 8 档
    Think,
    /// `/模型` 第一层：供应商
    ModelProvider,
    /// `/模型` 第二层：该供应商下的模型（`sub` = provider）
    Model,
    /// `/任务` 会话列表
    Task,
    /// `/项目` 工作区列表
    Project,
}


/// 挂起中的菜单。
#[derive(Debug, Clone)]
pub struct PendingMenu {
    pub kind: MenuKind,
    /// 第二层菜单的上下文（`Model` 层的 provider）
    pub sub: Option<String>,
    /// `(展示名, 载荷)`，与编号一一对应
    pub options: Vec<(String, String)>,
    /// 已渲染的提示；编号解析失败时**原样重发**
    pub prompt: String,
}

/// `resolve` 的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// 选中第 N 项（载荷）
    Selected(String),
    /// 回 0 取消
    Cancelled,
    /// 解析不出来 —— 调用方应重发 `prompt` 并**保留**菜单
    Retry,
}

impl PendingMenu {
    /// 渲染成微信文本并生成挂起菜单。`options` 为 `(展示名, 载荷)`。
    pub fn build(
        kind: MenuKind,
        sub: Option<String>,
        title: &str,
        options: Vec<(String, String)>,
        lang: Lang,
    ) -> (String, Self) {
        let labels: Vec<MenuOption> = options
            .iter()
            .map(|(label, _)| MenuOption::new(label))
            .collect();
        // 通用选择菜单带 0 取消（ZCode formatSelectionFallback 默认分支）
        let prompt = render_selection(title, &labels, true, None, lang);
        (
            prompt.clone(),
            Self {
                kind,
                sub,
                options,
                prompt,
            },
        )
    }

    /// 解析一次回数。
    pub fn resolve(&self, text: &str) -> Outcome {
        if text.trim() == "0" {
            return Outcome::Cancelled;
        }
        match option_index(text) {
            Some(i) if i < self.options.len() => Outcome::Selected(self.options[i].1.clone()),
            _ => Outcome::Retry,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wxprobe::format::messages::t;

    fn two() -> Vec<(String, String)> {
        vec![
            ("claude-sonnet".to_string(), "claude-sonnet-4".to_string()),
            ("gpt-5".to_string(), "gpt-5".to_string()),
        ]
    }

    #[test]
    fn build_renders_numbered_menu_with_cancel() {
        let (text, m) = PendingMenu::build(
            MenuKind::ModelProvider,
            None,
            "选择模型",
            two(),
            Lang::ZhCn,
        );
        assert_eq!(
            text,
            "选择模型\n0. 取消\n1. claude-sonnet\n2. gpt-5\n\n回复数字选择，0 取消。"
        );
        assert_eq!(m.options.len(), 2);
        assert_eq!(m.prompt, text, "重发用的就是这份渲染结果");
    }

    #[test]
    fn resolve_returns_payload_not_index() {
        let (_, m) = PendingMenu::build(MenuKind::Think, None, "t", two(), Lang::ZhCn);
        assert_eq!(m.resolve("1"), Outcome::Selected("claude-sonnet-4".into()));
        assert_eq!(m.resolve(" 2 "), Outcome::Selected("gpt-5".into()));
    }

    #[test]
    fn zero_cancels_and_garbage_retries() {
        let (_, m) = PendingMenu::build(MenuKind::Task, None, "t", two(), Lang::ZhCn);
        assert_eq!(m.resolve("0"), Outcome::Cancelled);
        assert_eq!(m.resolve("3"), Outcome::Retry, "越界要重发提示而不是静默");
        assert_eq!(m.resolve("abc"), Outcome::Retry, "NaN 同上");
        assert_eq!(m.resolve("-1"), Outcome::Retry);
    }

    #[test]
    fn empty_menu_never_selects() {
        let (_, m) = PendingMenu::build(MenuKind::Task, None, "空", vec![], Lang::ZhCn);
        assert_eq!(m.resolve("1"), Outcome::Retry);
    }

    #[test]
    fn cancelled_message_exists_in_table() {
        // 取消回执用 ZCode 文案表现成句子，不新造
        assert_eq!(t(Lang::ZhCn, "已取消。"), "已取消。");
        assert_eq!(t(Lang::En, "已取消。"), "Cancelled.");
    }
}
