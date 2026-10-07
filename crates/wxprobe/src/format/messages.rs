//! 文案表（P2 切片 2）。
//!
//! 形状对齐 `crates/app/src/i18n.rs` 的 `TABLE: &[(zh-CN, zh-TW, en)]`，
//! **zh-CN 源字面量即 key** —— 这样 P3 把本表搬进 `app::i18n` 时是机械替换，
//! 不需要改任何调用点的 key。
//!
//! 与 ZCode 的差异：ZCode bot 只有 zh-CN / en 两种 locale，且用语义 id
//! （`statusWorkspace`）做 key；这里用 zh-CN 源串做 key 并补 zh-TW 列。
//! zh-TW 目前整表抄 zh-CN（ZCode 无此语言），P3 接 `app::i18n` 时补齐。

/// 语言序号与 `app::i18n::LANG_IX` 一致（0 zh-CN · 1 zh-TW · 2 en），
/// P3 换成全局索引时只需改 `current()`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    ZhCn = 0,
    ZhTw = 1,
    En = 2,
}

impl Lang {
    pub fn from_ix(ix: usize) -> Lang {
        match ix {
            1 => Lang::ZhTw,
            2 => Lang::En,
            _ => Lang::ZhCn,
        }
    }
}

/// (zh-CN 源, zh-TW, en)
static TABLE: &[(&str, &str, &str)] = &[
    // ── /status 状态卡（ZCode `botsService.ts:4593-4624` 的 label id）──
    ("工作区", "工作區", "Workspace"),
    ("模型", "模型", "Model"),
    ("任务", "任務", "Task"),
    ("状态", "狀態", "State"),
    ("已工作", "已工作", "Worked"),
    ("进展", "進展", "Progress"),
    // ── 状态值 ──
    ("草稿", "草稿", "draft"),
    ("远端未连接", "遠端未連接", "remote disconnected"),
    ("⏳ 运行中", "⏳ 執行中", "Running"),
    ("✅ 已完成", "✅ 已完成", "Completed"),
    ("失败", "失敗", "Failed"),
    ("已取消", "已取消", "cancelled"),
    ("已停止", "已停止", "stopped"),
    // ── 断连提示（ZCode `remoteDisconnectedStatus`，需 {workspacePath} 模板）──
    (
        "当前远端项目 {workspacePath} 未连接。请发送 **/重连** 恢复连接。",
        "目前遠端項目 {workspacePath} 未連接。請發送 **/重連** 恢復連接。",
        "The remote workspace {workspacePath} is not connected. Send **/reconnect** to restore the connection.",
    ),
    // ── replyFormatter 的 formatterMessages（ZCode 单独一张表，此处按 zh-CN 源串合并）──
    // 注意 en 值与 messages.ts 同名项一致（`⏳ 运行中`→Running、`失败`→Failed），
    // 合并不会引入语义冲突。
    ("工具调用：", "工具調用：", "Tool calls:"),
    ("完成", "完成", "Completed"),
    ("已拒绝", "已拒絕", "Denied"),
    ("等待中", "等待中", "Pending"),
    ("需要权限：", "需要權限：", "Permission required:"),
    ("写入中", "寫入中", "Writing"),
    ("更新中", "更新中", "Updating"),
    ("删除中", "刪除中", "Deleting"),
    ("编辑中", "編輯中", "Editing"),
    ("变更摘要", "變更摘要", "Change summary"),
    ("还有 {count} 个工具调用", "還有 {count} 個工具調用", "{count} more tool calls"),
    ("还有 {count} 个文件", "還有 {count} 個文件", "{count} more files"),
];

/// 查表；未命中时回退 key 本身（zh-CN 源串），永远不 panic。
pub fn t(lang: Lang, key: &str) -> String {
    match TABLE.iter().find(|entry| entry.0 == key) {
        Some(entry) => match lang {
            Lang::ZhCn => entry.0,
            Lang::ZhTw => entry.1,
            Lang::En => entry.2,
        }
        .to_string(),
        None => key.to_string(),
    }
}

/// `{name}` 模板替换，对齐 ZCode `formatBotMessage` 的 `replaceAll("{k}", v)`。
pub fn tf(lang: Lang, key: &str, vars: &[(&str, &str)]) -> String {
    let mut out = t(lang, key);
    for (name, value) in vars {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zh_cn_is_the_identity_language() {
        for entry in TABLE {
            assert_eq!(t(Lang::ZhCn, entry.0), entry.0, "zh-CN 必须原样返回");
        }
    }

    #[test]
    fn english_table_matches_zcode_wording() {
        assert_eq!(t(Lang::En, "工作区"), "Workspace");
        assert_eq!(t(Lang::En, "已工作"), "Worked");
        assert_eq!(t(Lang::En, "进展"), "Progress");
        assert_eq!(t(Lang::En, "⏳ 运行中"), "Running");
        assert_eq!(t(Lang::En, "✅ 已完成"), "Completed");
    }

    #[test]
    fn unknown_key_falls_back_to_source_literal() {
        assert_eq!(t(Lang::En, "不存在的 key"), "不存在的 key");
    }

    #[test]
    fn template_replaces_all_occurrences() {
        assert_eq!(
            tf(Lang::ZhCn, "{a} 与 {a}", &[("a", "X")]),
            "X 与 X",
            "对齐 ZCode 的 replaceAll"
        );
    }

    #[test]
    fn lang_index_round_trips_with_app_scheme() {
        assert_eq!(Lang::from_ix(0), Lang::ZhCn);
        assert_eq!(Lang::from_ix(1), Lang::ZhTw);
        assert_eq!(Lang::from_ix(2), Lang::En);
        assert_eq!(Lang::from_ix(9), Lang::ZhCn, "越界回退 zh-CN");
    }
}
