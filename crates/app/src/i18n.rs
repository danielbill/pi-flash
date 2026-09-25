//! i18n (pi-flash-04k): three languages — zh-CN (source strings, identity),
//! zh-TW, en. `t()` maps a source literal through the table; `tf()` renders
//! `{key}` templates. The language index is runtime-switchable and persisted
//! in the workspace memory file (pi-web i18n parity: user preference).

use std::sync::atomic::{AtomicUsize, Ordering};

/// 0 zh-CN · 1 zh-TW · 2 en
static LANG_IX: AtomicUsize = AtomicUsize::new(0);

pub const LANGS: [&str; 3] = ["zh-CN", "zh-TW", "en"];
pub const LANG_LABELS: [&str; 3] = ["简体中文", "繁體中文", "English"];

pub fn set_lang(ix: usize) {
    LANG_IX.store(ix.min(2), Ordering::Relaxed);
}

pub fn lang_ix() -> usize {
    LANG_IX.load(Ordering::Relaxed)
}

pub fn lang_name() -> &'static str {
    LANGS[lang_ix()]
}

/// (source zh-CN, zh-TW, en)
const TABLE: &[(&str, &str, &str)] = &[
    // chrome / sidebar
    ("pi-flash", "pi-flash", "pi-flash"),
    ("新建", "新建", "New"),
    ("新会话", "新會話", "New session"),
    ("切换后仅显示该项目的会话，并恢复上次打开的会话", "切換後僅顯示該項目的會話，並恢復上次打開的會話", "Show only this project's sessions and restore the last open one"),
    ("完整历史", "完整歷史", "Full history"),
    ("分支", "分支", "Branches"),
    ("主分支", "主分支", "main"),
    ("no git", "no git", "no git"),
    ("暂无分支", "暫無分支", "No branches"),
    ("点击节点：从该用户消息处创建分支新会话", "點擊節點：從該用戶消息處創建分支新會話", "Click a node to fork a new session from that user message"),
    ("新分支", "新分支", "Fork"),
    ("文件浏览器", "文件瀏覽器", "Files"),
    ("压缩", "壓縮", "Compact"),
    ("发送", "發送", "Send"),
    ("消息...输入 / 使用命令，输入 @ 查找文件", "訊息...輸入 / 使用命令，輸入 @ 查找文件", "Message... / for commands, @ for files"),
    ("无活动会话", "無活動會話", "No active session"),
    ("未连接", "未連接", "Disconnected"),
    ("生成标题", "生成標題", "Generate title"),
    // dialogs
    ("重命名会话", "重命名會話", "Rename session"),
    ("session name", "session name", "session name"),
    ("取消", "取消", "Cancel"),
    ("保存", "儲存", "Save"),
    ("更新", "更新", "Update"),
    ("删除", "刪除", "Delete"),
    ("否", "否", "No"),
    ("是", "是", "Yes"),
    ("提交", "提交", "Submit"),
    ("select model", "select model", "select model"),
    ("选择模型", "選擇模型", "Select model"),
    ("选择项目", "選擇項目", "Select project"),
    ("no models match", "no models match", "no models match"),
    // settings tabs
    ("模型", "模型", "Models"),
    ("技能", "技能", "Skills"),
    ("插件", "外掛", "Plugins"),
    ("工具", "工具", "Tools"),
    ("子代理", "子代理", "Subagents"),
    ("通用", "通用", "General"),
    // models tab
    ("已启用模型", "已啟用模型", "Enabled models"),
    ("全部启用", "全部啟用", "Enable all"),
    ("全部停用", "全部停用", "Disable all"),
    ("当前", "當前", "current"),
    ("API Key", "API Key", "API Key"),
    ("API Key 已配置", "API Key 已配置", "API key configured"),
    ("API Key 不能为空", "API Key 不能為空", "API key must not be empty"),
    ("API Key 不能為空", "API Key 不能為空", "API key must not be empty"),
    ("OAuth 已登录", "OAuth 已登入", "Signed in with OAuth"),
    ("凭据", "憑證", "Credentials"),
    ("登录凭据存储于 ~/.pi/agent/auth.json（与 pi 共用）", "登入憑證儲存於 ~/.pi/agent/auth.json（與 pi 共用）", "Credentials are stored in ~/.pi/agent/auth.json (shared with pi)"),
    ("显示", "顯示", "Show"),
    ("隐藏", "隱藏", "Hide"),
    ("ENV 变量、!命令 或明文 key", "ENV 變數、!命令 或明文 key", "env var, !command, or literal key"),
    ("密钥写入 ~/.pi/agent/auth.json（与 pi 共用）；新 provider 的模型需重启 pi-flash 后出现在列表", "密鑰寫入 ~/.pi/agent/auth.json（與 pi 共用）；新 provider 的模型需重啟 pi-flash 後出現在列表", "Key is written to ~/.pi/agent/auth.json (shared with pi); a new provider's models appear after restarting pi-flash"),
    ("未配置", "未配置", "Not configured"),
    ("不能停用最后一个启用的模型", "不能停用最後一個啟用的模型", "Cannot disable the last enabled model"),
    ("写入 settings.json 失败: {e}", "寫入 settings.json 失敗: {e}", "Failed to write settings.json: {e}"),
    ("保存失败: {e}", "儲存失敗: {e}", "Failed to save: {e}"),
    ("项目级 .pi/settings.json 覆盖了 enabledModels，面板只读", "項目級 .pi/settings.json 覆蓋了 enabledModels，面板只讀", "Project .pi/settings.json overrides enabledModels; panel is read-only"),
    ("项目级 settings.json 覆盖了 enabledModels，此面板只读", "項目級 settings.json 覆蓋了 enabledModels，此面板只讀", "Project settings.json overrides enabledModels; this panel is read-only"),
    ("没有匹配的模型", "沒有匹配的模型", "No matching models"),
    // skills tab
    ("没有找到技能", "沒有找到技能", "No skills found"),
    ("没有找到技能（扫描项目 .pi/skills、.agents/skills 与全局目录）", "沒有找到技能（掃描項目 .pi/skills、.agents/skills 與全局目錄）", "No skills found (scans project .pi/skills, .agents/skills and global dirs)"),
    ("对模型可见", "對模型可見", "Visible to model"),
    ("已隐藏（仍可手动调用）", "已隱藏（仍可手動調用）", "Hidden (still invocable manually)"),
    ("写入 SKILL.md 失败: {e}", "寫入 SKILL.md 失敗: {e}", "Failed to write SKILL.md: {e}"),
    // plugins tab
    ("添加插件", "添加外掛", "Add plugin"),
    ("安装", "安裝", "Install"),
    ("安装位置：全局 ~/.pi/agent/{npm,git}；项目 <工作区>/.pi/agent/{npm,git}", "安裝位置：全局 ~/.pi/agent/{npm,git}；項目 <工作區>/.pi/agent/{npm,git}", "Install location: global ~/.pi/agent/{npm,git}; project <workspace>/.pi/agent/{npm,git}"),
    ("npm:@scope/pi-plugin · git:https://... · /绝对路径", "npm:@scope/pi-plugin · git:https://... · /絕對路徑", "npm:@scope/pi-plugin · git:https://... · /absolute/path"),
    ("来源", "來源", "Source"),
    ("全局", "全局", "Global"),
    ("项目", "項目", "Project"),
    ("工作区", "工作區", "Workspace"),
    ("内置", "內置", "Built-in"),
    ("已启用", "已啟用", "Enabled"),
    ("已停用", "已停用", "Disabled"),
    ("已停用（资源不加载）", "已停用（資源不加載）", "Disabled (resources not loaded)"),
    ("已加载", "已加載", "Loaded"),
    ("没有已配置的插件", "沒有已配置的外掛", "No configured plugins"),
    ("移除", "移除", "Remove"),
    ("移除/安装通过 vendored pi CLI 执行（pi remove/install）", "移除/安裝通過 vendored pi CLI 執行（pi remove/install）", "Remove/install runs through the vendored pi CLI (pi remove/install)"),
    ("请输入插件来源（npm: / git: / 本地路径）", "請輸入外掛來源（npm: / git: / 本地路徑）", "Enter a plugin source (npm: / git: / local path)"),
    ("已安装 {source}", "已安裝 {source}", "Installed {source}"),
    ("已移除 {source}", "已移除 {source}", "Removed {source}"),
    ("pi {} 失败: {}", "pi {} 失敗: {}", "pi {} failed: {}"),
    ("删除失败: {e}", "刪除失敗: {e}", "Failed to delete: {e}"),
    // tools tab
    ("工具选择", "工具選擇", "Tool selection"),
    ("全部", "全部", "All"),
    ("默认", "預設", "Default"),
    ("只读", "只讀", "Read-only"),
    ("无", "無", "None"),
    ("不覆盖，pi 默认（全部内置工具）", "不覆蓋，pi 預設（全部內置工具）", "No override; pi default (all built-in tools)"),
    ("read, bash, edit, write", "read, bash, edit, write", "read, bash, edit, write"),
    ("read, grep, find, ls", "read, grep, find, ls", "read, grep, find, ls"),
    ("禁用所有工具", "禁用所有工具", "Disable all tools"),
    ("未设置（pi 默认解析全部工具）", "未設置（pi 預設解析全部工具）", "Unset (pi resolves every tool)"),
    ("[]（无工具）", "[]（無工具）", "[] (no tools)"),
    ("写入 ~/.pi/agent/settings.json 的 defaultTools；新会话生效（与 pi CLI --tools 一致）", "寫入 ~/.pi/agent/settings.json 的 defaultTools；新會話生效（與 pi CLI --tools 一致）", "Writes defaultTools to ~/.pi/agent/settings.json; applies to new sessions (same as pi CLI --tools)"),
    // subagents tab
    ("运行", "運行", "Run"),
    ("运行中", "運行中", "Running"),
    ("运行已结束", "運行已結束", "Run finished"),
    ("已完成", "已完成", "Completed"),
    ("失败", "失敗", "Failed"),
    ("已中止", "已中止", "Aborted"),
    ("中止", "中止", "Stop"),
    ("输出", "輸出", "Output"),
    ("运行", "運行", "Run"),
    ("内置子代理", "內置子代理", "Built-in subagents"),
    ("最大并发 (1-32)", "最大並發 (1-32)", "Max concurrent (1-32)"),
    ("选择一个子代理查看详情并运行；内置子代理由 agents/settings.json 控制", "選擇一個子代理查看詳情並運行；內置子代理由 agents/settings.json 控制", "Pick a subagent to inspect and run; built-ins are controlled by agents/settings.json"),
    ("写入 agents/settings.json 失败: {e}", "寫入 agents/settings.json 失敗: {e}", "Failed to write agents/settings.json: {e}"),
    ("写入 profile 失败: {e}", "寫入 profile 失敗: {e}", "Failed to write profile: {e}"),
    ("子代理启动失败: {e}", "子代理啟動失敗: {e}", "Failed to start subagent: {e}"),
    ("覆盖", "覆蓋", "overridden"),
    ("继承", "繼承", "inherit"),
    ("系统", "系統", "system"),
    // general tab
    ("外观", "外觀", "Appearance"),
    ("主题写入 ~/.pi/agent/settings.json 的 theme 键（与 pi 共用）；vendored pi {}", "主題寫入 ~/.pi/agent/settings.json 的 theme 鍵（與 pi 共用）；vendored pi {}", "Theme is stored in ~/.pi/agent/settings.json's theme key (shared with pi); vendored pi {}"),
    // session list / relative time templates
    ("{} 条消息", "{} 條消息", "{} messages"),
    ("{secs}秒前", "{secs}秒前", "{secs}s ago"),
    ("{n}分钟前", "{n}分鐘前", "{n}m ago"),
    ("{n}小时前", "{n}小時前", "{n}h ago"),
    ("{n}天前", "{n}天前", "{n}d ago"),
    // misc
    ("（无）", "（無）", "(none)"),
    ("退出登录", "退出登入", "Sign out"),
    ("模型: {v}", "模型: {v}", "Model: {v}"),
    ("思考: {v}", "思考: {v}", "Thinking: {v}"),
    ("最大轮数: {v}", "最大輪數: {v}", "Max turns: {v}"),
    // exit banner
    ("Process exited with code {code_text}", "Process exited with code {code_text}", "Process exited with code {code_text}"),
    ("unknown", "unknown", "unknown"),
    // git badges etc stay ASCII
];

/// Translate a source (zh-CN) literal for the active language. Unknown
/// strings pass through unchanged, so new UI degrades gracefully.
pub fn tr<'a>(s: &'a str) -> &'a str {
    match lang_ix() {
        0 => s,
        ix => TABLE
            .iter()
            .find(|e| e.0 == s)
            .map(|e| match ix {
                1 => e.1,
                _ => e.2,
            })
            .unwrap_or(s),
    }
}

/// Render a `{key}` template in the active language.
pub fn tf(template: &str, pairs: &[(&str, String)]) -> String {
    let mut out = tr(template).to_string();
    for (k, v) in pairs {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_for_source_language() {
        set_lang(0);
        assert_eq!(tr("保存"), "保存");
        assert_eq!(tr("something new"), "something new");
    }

    #[test]
    fn translations_for_other_languages() {
        set_lang(1);
        assert_eq!(tr("保存"), "儲存");
        assert_eq!(tr("新分支"), "新分支");
        set_lang(2);
        assert_eq!(tr("保存"), "Save");
        assert_eq!(tr("新分支"), "Fork");
        assert_eq!(tr("not in table"), "not in table");
        set_lang(0);
    }

    #[test]
    fn template_replacement() {
        set_lang(2);
        assert_eq!(
            tf("已安装 {source}", &[("source", "npm:x".to_string())]),
            "Installed npm:x"
        );
        set_lang(0);
        assert_eq!(
            tf("已安装 {source}", &[("source", "npm:x".to_string())]),
            "已安装 npm:x"
        );
    }
}
