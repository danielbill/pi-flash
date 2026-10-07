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
    // ── ZCode messages.ts 整表搬运（按 zh-CN 源串为 key；zh-TW 回退 zh-CN）──
    ("当前 bot 未启用。", "当前 bot 未启用。", "This bot is not enabled."),
    ("Bots 暂不支持群聊，请在私聊中使用。", "Bots 暂不支持群聊，请在私聊中使用。", "Bots do not support group chats yet. Please use a private chat."),
    ("Bots 只允许在私聊中绑定。", "Bots 只允许在私聊中绑定。", "Bots can only bind in a private chat."),
    ("当前 bot 未绑定。请先在 zcode UI 生成绑定码，然后发送 **/bind <code>**。", "当前 bot 未绑定。请先在 zcode UI 生成绑定码，然后发送 **/bind <code>**。", "This bot is not bound. Generate a bind code in the zcode UI, then send **/bind <code>**."),
    ("当前 bot 未启用这个命令。", "当前 bot 未启用这个命令。", "This command is disabled for the current bot."),
    ("没有可用 workspace，请先在 Bots 设置里允许 workspace。", "没有可用 workspace，请先在 Bots 设置里允许 workspace。", "No workspace is available. Allow a workspace in Bots settings first."),
    ("当前聊天上下文的 workspace 已不在授权范围内，请重新选择 **/项目**。", "当前聊天上下文的 workspace 已不在授权范围内，请重新选择 **/项目**。", "The workspace in this chat is no longer authorized. Please select **/workspace** again."),
    ("绑定码无效或已过期，请在 zcode UI 重新生成。", "绑定码无效或已过期，请在 zcode UI 重新生成。", "The bind code is invalid or expired. Generate a new one in the zcode UI."),
    ("绑定失败：bot 不存在。", "绑定失败：bot 不存在。", "Bind failed: bot does not exist."),
    ("绑定成功。发送 **/帮助** 查看可用命令。", "绑定成功。发送 **/帮助** 查看可用命令。", "Bound successfully. Send **/help** to see available commands."),
    ("微信 Bot 已激活。发送 **/帮助** 查看命令，或直接描述你要做的事。", "微信 Bot 已激活。发送 **/帮助** 查看命令，或直接描述你要做的事。", "Weixin bot is active. Send **/help** to see commands, or describe what you want to do."),
    ("ZCode 机器人命令：", "ZCode 机器人命令：", "ZCode bot commands:"),
    ("**/帮助** — 查看这份说明", "**/帮助** — 查看这份说明", "**/help** — Show this guide"),
    ("**/bind <code>** — 绑定当前聊天", "**/bind <code>** — 绑定当前聊天", "**/bind <code>** — Bind this chat"),
    ("**/状态** — 查看工作区、模型和任务状态", "**/状态** — 查看工作区、模型和任务状态", "**/status** — Show workspace, model, and task status"),
    ("/新建 或 /clear — 开始新的任务草稿", "/新建 或 /clear — 开始新的任务草稿", "/new or /clear — Start a new task draft"),
    ("**/项目** — 切换工作区", "**/项目** — 切换工作区", "**/project** — Switch workspace"),
    ("**/模型** — 切换模型", "**/模型** — 切换模型", "**/model** — Switch model"),
    ("**/模式** — 切换运行模式", "**/模式** — 切换运行模式", "**/mode** — Switch run mode"),
    ("**/思考** — 切换思考级别", "**/思考** — 切换思考级别", "**/think** — Switch thought level"),
    ("**/回复** — 切换回复详细程度", "**/回复** — 切换回复详细程度", "**/reply** — Switch reply detail"),
    ("Webhook secret 校验失败。", "Webhook secret 校验失败。", "Webhook secret verification failed."),
    ("处理机器人回调失败：{message}", "处理机器人回调失败：{message}", "Failed to process bot callback: {message}"),
    ("当前任务会话已失效，可能是任务已被清理或机器人消息已过期。请发送 **/new task** 创建新任务后再继续。", "当前任务会话已失效，可能是任务已被清理或机器人消息已过期。请发送 **/new task** 创建新任务后再继续。", "The current task session has expired. It may have been cleaned up, or this bot message is stale. Send **/new task** to create a new task and continue."),
    ("原任务已删除，已为你新建任务。本条消息将在新任务中处理，不会继承原任务的对话上下文。", "原任务已删除，已为你新建任务。本条消息将在新任务中处理，不会继承原任务的对话上下文。", "The previous task was deleted, so I created a new task for you. This message will be processed in the new task without the previous conversation history."),
    ("已收到。", "已收到。", "Received."),
    ("请查看附件并根据内容协助我。", "请查看附件并根据内容协助我。", "Please review the attachment and help based on its content."),
    ("附件处理失败：{message}", "附件处理失败：{message}", "Failed to process attachment: {message}"),
    ("无法下载附件。文件可能已过期、已撤回，或机器人没有读取权限。请重新发送附件后再试。", "无法下载附件。文件可能已过期、已撤回，或机器人没有读取权限。请重新发送附件后再试。", "Could not download the attachment. The file may have expired, been removed, or the bot may not have permission to read it. Please send the attachment again and try once more."),
    ("附件超过 5MB，请压缩后重新发送。", "附件超过 5MB，请压缩后重新发送。", "The attachment exceeds 5MB. Compress it and send it again."),
    ("已取消。", "已取消。", "Cancelled."),
    ("取消", "取消", "Cancel"),
    ("回复数字选择，0 取消。", "回复数字选择，0 取消。", "Reply with a number to choose, or 0 to cancel."),
    ("回复数字选择。", "回复数字选择。", "Reply with a number to choose."),
    ("已进入 {workspacePath} 的新任务草稿。", "已进入 {workspacePath} 的新任务草稿。", "Entered a new task draft in {workspacePath}."),
    ("当前 workspace {workspace}\n选择 workspace", "当前 workspace {workspace}\n选择 workspace", "Current workspace {workspace}\nSelect workspace"),
    ("未找到可用 workspace。", "未找到可用 workspace。", "No available workspace found."),
    ("选择 model", "选择 model", "Select model"),
    ("当前模型 {model}\n选择模型供应商", "当前模型 {model}\n选择模型供应商", "Current model {model}\nSelect model provider"),
    ("当前模型 {model}\n选择模型", "当前模型 {model}\n选择模型", "Current model {model}\nSelect model"),
    ("未找到 model。", "未找到 model。", "Model not found."),
    ("当前会话的模型选择不可用，请使用 /model 重新选择。原选择已保留。", "当前会话的模型选择不可用，请使用 /model 重新选择。原选择已保留。", "The session's model selection is unavailable. Use /model to choose again. Your saved selection has been preserved."),
    ("当前模式 {mode}\n选择模式", "当前模式 {mode}\n选择模式", "Current mode {mode}\nSelect mode"),
    ("未找到模式。", "未找到模式。", "Mode option not found."),
    ("当前任务模式已切换为 {mode}。", "当前任务模式已切换为 {mode}。", "Current task mode changed to {mode}."),
    ("机器人已锁定 **yolo** 运行模式，无法切换。", "机器人已锁定 **yolo** 运行模式，无法切换。", "This bot is locked to **yolo** run mode and cannot be switched."),
    ("当前思考级别 {level}\n选择思考级别", "当前思考级别 {level}\n选择思考级别", "Current thought level {level}\nSelect thought level"),
    ("当前模型不支持思考级别。", "当前模型不支持思考级别。", "The current model does not support thought level."),
    ("当前任务思考级别已切换为 {level}。", "当前任务思考级别已切换为 {level}。", "Current task thought level changed to {level}."),
    ("未找到模型供应商。", "未找到模型供应商。", "Model provider not found."),
    ("当前任务 model 已切换为 {model}。", "当前任务 model 已切换为 {model}。", "Current task model changed to {model}."),
    ("未找到任务。", "未找到任务。", "Task not found."),
    ("已切换到任务：{title}", "已切换到任务：{title}", "Switched to task: {title}"),
    ("当前没有 active task。", "当前没有 active task。", "There is no active task."),
    ("权限请求已过期，请在 zcode UI 中处理。", "权限请求已过期，请在 zcode UI 中处理。", "This permission request has expired. Please handle it in the zcode UI."),
    ("权限请求已处理。", "权限请求已处理。", "Permission request has already been handled."),
    ("已拒绝权限请求。", "已拒绝权限请求。", "Permission request denied."),
    ("已提交权限响应。", "已提交权限响应。", "Permission response submitted."),
    ("问答请求已过期，请在 zcode UI 中处理。", "问答请求已过期，请在 zcode UI 中处理。", "This question request has expired. Please handle it in the zcode UI."),
    ("问答请求已处理。", "问答请求已处理。", "Question request has already been handled."),
    ("已提交问答响应。", "已提交问答响应。", "Question response submitted."),
    ("已取消问答请求。", "已取消问答请求。", "Question request cancelled."),
    ("自定义回答", "自定义回答", "Custom answer"),
    ("请输入自定义回答", "请输入自定义回答", "Enter a custom answer"),
    ("提问", "提问", "Question"),
    ("请审阅此实施计划。", "请审阅此实施计划。", "Review this implementation plan."),
    ("实施计划", "实施计划", "Implementation plan"),
    ("批准", "批准", "Approve"),
    ("退出计划模式并开始实施。", "退出计划模式并开始实施。", "Exit plan mode and start implementation."),
    ("✅ 问答已取消", "✅ 问答已取消", "✅ Questions cancelled"),
    ("跳过", "跳过", "Skip"),
    ("可多选；再次选择会取消，选择“完成”提交。", "可多选；再次选择会取消，选择“完成”提交。", "You can select multiple options; select again to remove, then choose Done."),
    ("也可以直接回复文本作为自定义答案。", "也可以直接回复文本作为自定义答案。", "You can also reply with text as a custom answer."),
    ("正在处理...", "正在处理...", "Working..."),
    ("工具摘要", "工具摘要", "Tool summaries"),
    ("已停止当前任务生成。", "已停止当前任务生成。", "Current task generation stopped."),
    ("未知命令：**/{command}**", "未知命令：**/{command}**", "Unknown command: **/{command}**"),
    ("任务失败：{message}", "任务失败：{message}", "Task failed: {message}"),
    ("当前任务正在运行，稍后再试，或使用 **/停止** 停止当前任务。", "当前任务正在运行，稍后再试，或使用 **/停止** 停止当前任务。", "The current task is still running. Try again later, or use **/stop** to stop the current task."),
    ("当前任务 {task}\n选择任务", "当前任务 {task}\n选择任务", "Current task {task}\nSelect task"),
    ("当前 workspace 没有历史任务。", "当前 workspace 没有历史任务。", "There are no history tasks in the current workspace."),
    ("当前远端项目 {workspacePath} 未连接。请先发送 **/重连**，连接恢复后再重试。上一条请求未执行。", "当前远端项目 {workspacePath} 未连接。请先发送 **/重连**，连接恢复后再重试。上一条请求未执行。", "The remote workspace {workspacePath} is not connected. Send **/reconnect** first, then try again. The previous request was not executed."),
    ("已切换到远端项目 {workspacePath}，但当前未连接。请先发送 **/重连** 后再执行任务。", "已切换到远端项目 {workspacePath}，但当前未连接。请先发送 **/重连** 后再执行任务。", "Switched to {workspacePath}, but the remote workspace is not connected. Send **/reconnect** before running tasks."),
    ("当前远端项目 {workspacePath} 未连接，正在为你重连...", "当前远端项目 {workspacePath} 未连接，正在为你重连...", "The remote workspace {workspacePath} is not connected. Reconnecting now..."),
    ("当前远端项目 {workspacePath} 重连失败：{message}\n上一条请求没有执行。", "当前远端项目 {workspacePath} 重连失败：{message}\n上一条请求没有执行。", "Remote workspace {workspacePath} reconnect failed: {message}\nThe previous request was not executed."),
    ("当前远端项目 {workspacePath} 未连接，但机器人无法访问远端重连服务。请先在 ZCode 打开该远端项目后重试。", "当前远端项目 {workspacePath} 未连接，但机器人无法访问远端重连服务。请先在 ZCode 打开该远端项目后重试。", "The remote workspace {workspacePath} is not connected, but the bot cannot access the remote reconnect service. Open this remote project in ZCode and try again."),
    ("当前 workspace 是本地项目，不需要重连。发送 **/项目** 可切换远端项目。", "当前 workspace 是本地项目，不需要重连。发送 **/项目** 可切换远端项目。", "The current workspace is local and does not need reconnecting. Send **/workspace** to switch to a remote project."),
    ("当前远端项目 {workspacePath} 已连接。", "当前远端项目 {workspacePath} 已连接。", "The remote workspace {workspacePath} is connected."),
    ("当前第三方回复颗粒度 {mode}\n选择第三方回复颗粒度", "当前第三方回复颗粒度 {mode}\n选择第三方回复颗粒度", "Current third-party reply detail {mode}\nSelect third-party reply detail"),
    ("未找到回复颗粒度。", "未找到回复颗粒度。", "Reply detail option not found."),
    ("第三方回复颗粒度已切换为 {mode}。", "第三方回复颗粒度已切换为 {mode}。", "Third-party reply detail changed to {mode}."),
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
