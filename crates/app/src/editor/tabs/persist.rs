//! tab 带持久化（自 tabs.rs 外移）：快照、落盘、启动回放 + 纯函数
//! tabs_state_from_parts。

use super::*;

/// 纯内核（单测直达）：从 tab 带构件算存档。`term_cwd` 查终端 cwd
///（PanelTab::Term 只存运行期 id，落盘要换 cwd）；瞬态页（Changelog/
/// 市场/搜索）不进存档；超 [`services::workspace::TABS_SAVE_CAP`] 截尾。
pub(super) fn tabs_state_from_parts(
    panel_tabs: &[PanelTab],
    term_cwd: impl Fn(usize) -> Option<PathBuf>,
    active: Option<usize>,
    chat_front: bool,
) -> services::workspace::TabsState {
    let mut tabs = Vec::new();
    let mut orig_ix = Vec::new();
    for (ix, t) in panel_tabs.iter().enumerate() {
        let rec = match t {
            PanelTab::File(p) => Some(services::workspace::SavedTab {
                kind: "file".into(),
                path: p.to_string_lossy().to_string(),
            }),
            PanelTab::Term(id) => term_cwd(*id).map(|cwd| services::workspace::SavedTab {
                kind: "term".into(),
                path: cwd.to_string_lossy().to_string(),
            }),
            _ => None,
        };
        if let Some(rec) = rec {
            orig_ix.push(ix);
            tabs.push(rec);
        }
        if tabs.len() >= services::workspace::TABS_SAVE_CAP {
            break;
        }
    }
    let active = active.and_then(|a| orig_ix.iter().position(|&o| o == a));
    services::workspace::TabsState {
        tabs,
        active,
        chat: chat_front,
    }
}
