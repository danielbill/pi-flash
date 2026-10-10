//! i18n (pi-flash-04k): three languages — zh-CN (source strings, identity),
//! zh-TW, en. `t()` maps a source literal through the table; `tf()` renders
//! `{key}` templates. The language index is runtime-switchable and persisted
//! in the workspace memory file (pi-web i18n parity: user preference).

use std::sync::atomic::{AtomicUsize, Ordering};

/// 0 zh-CN · 1 zh-TW · 2 en
static LANG_IX: AtomicUsize = AtomicUsize::new(0);

pub const LANG_LABELS: [&str; 3] = ["简体中文", "繁體中文", "English"];

pub fn set_lang(ix: usize) {
    LANG_IX.store(ix.min(2), Ordering::Relaxed);
}

pub fn lang_ix() -> usize {
    LANG_IX.load(Ordering::Relaxed)
}


/// (source zh-CN, zh-TW, en)
const TABLE: &[(&str, &str, &str)] = &[
    // chrome / sidebar
    // compaction card（pi-web CompactionMessageView parity）
    ("会话已压缩", "會話已壓縮", "Conversation compacted"),
    ("运行中不能更换工具预设", "執行中不能更換工具預設", "Cannot change the tool preset while the agent is running"),
    ("此处之前的会话历史已压缩为以下摘要：", "此處之前的會話歷史已壓縮為以下摘要：", "The conversation history before this point was compacted into the following summary:"),
    ("（无摘要）", "（無摘要）", "(no summary)"),
    ("文件上下文：{details}", "檔案脈絡：{details}", "File context: {details}"),
    ("{n} 读取", "{n} 讀取", "{n} read"),
    ("{n} 修改", "{n} 修改", "{n} modified"),
    ("修改文件", "修改檔案", "Modified files"),
    ("读取文件", "讀取檔案", "Read files"),
    ("pi-flash", "pi-flash", "pi-flash"),
    ("新建", "新建", "New"),
    ("新会话", "新會話", "New session"),
    // 012 新会话页（newSession）
    ("Hi，打算让我做点什么？", "Hi，打算讓我做點什麼？", "Hi, what shall we work on?"),
    ("打开项目", "開啟專案", "Open project"),
    ("切换后仅显示该项目的会话，并恢复上次打开的会话", "切換後僅顯示該項目的會話，並恢復上次打開的會話", "Show only this project's sessions and restore the last open one"),
    ("完整历史", "完整歷史", "Full history"),
    ("分支", "分支", "Branches"),
    ("主分支", "主分支", "main"),
    ("no git", "no git", "no git"),
    ("暂无分支", "暫無分支", "No branches"),
    ("点击节点：从该用户消息处创建分支新会话", "點擊節點：從該用戶消息處創建分支新會話", "Click a node to fork a new session from that user message"),
    ("新分支", "新分支", "Fork"),
    ("已复制", "已複製", "Copied"),
    ("创建中…", "建立中…", "Creating…"),
    ("文件浏览器", "文件瀏覽器", "Files"),
    ("压缩", "壓縮", "Compact"),
    ("发送", "發送", "Send"),
    ("消息...输入 / 使用命令，输入 @ 查找文件", "訊息...輸入 / 使用命令，輸入 @ 查找文件", "Message... / for commands, @ for files"),
    ("无活动会话", "無活動會話", "No active session"),
    ("未连接", "未連接", "Disconnected"),
    ("生成标题", "生成標題", "Generate title"),
    // dialogs
    // 004 打开项目菜单（projectManager）
    ("搜索项目…", "搜尋項目…", "Search projects…"),
    ("打开文件夹", "開啟資料夾", "Open folder"),
    ("最近 30 天没有打开过的项目", "最近 30 天沒有開啟過的項目", "No projects opened in the last 30 days"),
    ("没有匹配的项目", "沒有符合的項目", "No matching projects"),
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
    ("过滤模型...", "過濾模型...", "filter models..."),
    ("选择项目", "選擇項目", "Select project"),
    ("no models match", "no models match", "no models match"),
    // 系统提示词面板 token 分析块（7 大类）
    ("总计", "總計", "Total"),
    ("全局提示词", "全域提示詞", "Global prompt"),
    ("系统工具", "系統工具", "System tools"),
    ("项目提示词", "專案提示詞", "Project prompt"),
    // settings tabs
    ("模型", "模型", "Models"),
    ("技能", "技能", "Skills"),
    ("扩展", "擴展", "Extensions"),
    ("工具", "工具", "Tools"),
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
    // extensions tab（040）
    ("安装", "安裝", "Install"),
    ("安装中", "安裝中", "Installing…"),
    ("从 https://pi.dev/packages 复制安装命令，做全局安装", "從 https://pi.dev/packages 複製安裝命令，做全域安裝", "Copy an install command from https://pi.dev/packages and install globally"),
    ("配置全局默认扩展", "配置全域預設擴展", "Global default extensions"),
    ("说明", "說明", "Description"),
    ("路径", "路徑", "Path"),
    ("来源", "來源", "Source"),
    ("全局", "全局", "Global"),
    ("项目", "項目", "Project"),
    ("工作区", "工作區", "Workspace"),
    ("内置", "內置", "Built-in"),
    ("已启用", "已啟用", "Enabled"),
    ("已停用", "已停用", "Disabled"),
    ("已加载", "已加載", "Loaded"),
    ("没有已配置的扩展", "沒有已配置的擴展", "No configured extensions"),
    ("没有已安装的扩展", "沒有已安裝的擴展", "No extensions installed"),
    ("移除", "移除", "Remove"),
    ("确认", "確認", "Confirm"),
    ("确认卸载", "確認卸載", "Uninstall"),
    ("卸载扩展 {name}？", "卸載擴展 {name}？", "Uninstall extension {name}?"),
    ("请输入扩展来源，或粘贴 pi install 安装命令", "請輸入擴展來源，或貼上 pi install 安裝命令", "Enter a source or paste a pi install command"),
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
    // 会话统计行
    ("输出", "輸出", "Output"),
    // general tab（v60 界面页）
    ("界面", "介面", "Appearance"),
    ("主题", "主題", "Themes"),
    ("字体", "字體", "Fonts"),
    ("语言", "語言", "Language"),
    ("会话字体", "會話字體", "Chat font"),
    ("面板字体", "面板字體", "Panel font"),
    ("文档字体", "文档字體", "File font"),
    ("小", "小", "Small"),
    ("中", "中", "Medium"),
    ("大", "大", "Large"),
    ("特大", "特大", "Extra large"),
    ("搜索字体…", "搜尋字型…", "Search fonts…"),
    ("无匹配字体", "無符合字型", "No matching fonts"),
    ("提示音", "提示音", "Notification sound"),
    ("agent 运行结束播放系统提示音", "agent 執行結束播放系統提示音", "Play a system sound when the agent run finishes"),
    ("加载时间窗口", "載入時間範圍", "Load time window"),
    ("加载最近几天内活跃的会话", "載入最近幾天內活躍的會話", "Load sessions active within the window"),
    ("会话显示数", "會話顯示數", "Session display count"),
    ("各项目默认显示的会话数量", "各項目預設顯示的會話數量", "Sessions shown per project by default"),
    ("显示更多", "顯示更多", "Show more"),
    // session list / relative time templates
    ("{} 条消息", "{} 條消息", "{} messages"),
    // 033 导航面板总结栏
    ("{n}条消息：用户{u}条 助手{a}条 工具调用{t}次", "{n}條訊息：用户 {u} 條 助理{a}條 工具呼叫{t}次", "{n}messages:{u}user {a} assistant,{t}tool calls"),
    ("{secs}秒前", "{secs}秒前", "{secs}s ago"),
    ("{n}分钟前", "{n}分鐘前", "{n}m ago"),
    ("{n}小时前", "{n}小時前", "{n}h ago"),
    ("{n}天前", "{n}天前", "{n}d ago"),
    // misc
    ("（无）", "（無）", "(none)"),
    ("退出登录", "退出登入", "Sign out"),
    // 界面页显示开关（自「其他」页挪入，无小节标题——用户定夺）
    ("展示思考", "展示思考", "Show thinking"),
    ("开启时思考块默认展开全文，关闭时默认收起为一行", "開啟時思考塊預設展開全文，關閉時預設收起為一行", "When on, thinking blocks expand by default; when off, they collapse to one line"),
    // 文件树 git 标识开关（设置-界面）
    ("文件树 Git 标识", "檔案樹 Git 標識", "File tree Git markers"),
    ("文件树显示 git 修改徽标与目录变更点", "檔案樹顯示 git 修改徽標與目錄變更點", "Show git modification badges and directory change dots in the file tree"),
    // 023 fileView（标签栏 + 菜单 / 导航栏 / 冲突横幅 / 确认弹窗）
    ("在左侧文件树中选择一个文件", "在左側檔案樹中選擇一個檔案", "Pick a file in the tree on the left"),
    ("打开文件…", "開啟檔案…", "Open file…"),
    ("新建文件", "新增檔案", "New file"),
    ("文件名（可含子目录）", "檔案名（可含子目錄）", "File name (subdirs allowed)"),
    ("在项目根下创建；Enter 确认，Esc 取消", "在專案根下建立；Enter 確認，Esc 取消", "Created under the project root; Enter to confirm, Esc to cancel"),
    ("文件超过 10MB，不打开", "檔案超過 10MB，不開啟", "File exceeds 10MB, not opening"),
    ("二进制文件，不打开", "二進位檔案，不開啟", "Binary file, not opening"),
    ("读取失败", "讀取失敗", "Read failed"),
    ("保存失败", "儲存失敗", "Save failed"),
    // 051-apikey管理
    ("已配置 · 系统凭据库", "已設定 · 系統憑證庫", "Configured · OS vault"),
    ("已配置 · 文件存储", "已設定 · 檔案儲存", "Configured · file storage"),
    ("系统凭据库不可用，已降级为文件存储", "系統憑證庫不可用，已降級為檔案儲存", "Credential vault unavailable — fell back to file storage"),
    ("创建失败", "建立失敗", "Create failed"),
    ("路径越出项目根", "路徑越出專案根", "Path escapes the project root"),
    ("已保存", "已儲存", "Saved"),
    ("文件已在磁盘上被修改（本地有未保存修改）", "檔案已在磁碟上被修改（本地有未儲存修改）", "File changed on disk (you have unsaved edits)"),
    ("文件已从磁盘消失", "檔案已從磁碟消失", "File disappeared from disk"),
    ("重新加载", "重新載入", "Reload"),
    ("保留我的版本", "保留我的版本", "Keep my version"),
    ("保留缓冲", "保留緩衝", "Keep buffer"),
    ("未保存的修改", "未儲存的修改", "Unsaved changes"),
    ("有未保存的修改：", "有未儲存的修改：", "Unsaved changes:"),
    ("取消", "取消", "Cancel"),
    ("不保存关闭", "不儲存關閉", "Close without saving"),
    ("保存并关闭", "儲存並關閉", "Save and close"),
    ("（无文件）", "（無檔案）", "(no files)"),
    ("编辑器初始化中…", "編輯器初始化中…", "Initializing editor…"),
    ("文件已关闭", "檔案已關閉", "File closed"),
    // topbar ⋯ 更多菜单（三个入口）
    ("打开终端", "開啟終端機", "Open terminal"),
    ("此会话系统提示词", "此會話系統提示詞", "Session system prompt"),
    ("此会话加载工具", "此會話載入工具", "Session loaded tools"),
    // top panel：系统提示词（pi-web system.*）
    ("系统提示词", "系統提示詞", "System prompt"),
    ("工具定义", "工具定義", "Tool definitions"),
    ("系统提示词为空（工具已禁用）", "系統提示詞為空（工具已停用）", "System prompt is empty (tools are disabled)"),
    ("系统提示词加载中…", "系統提示詞載入中…", "Loading system prompt…"),
    // top panel：工具定义（pi-web tools.*）
    ("工具定义尚未加载", "工具定義尚未載入", "Tool definitions have not loaded yet"),
    ("没有启用的工具", "沒有啟用的工具", "No active tools"),
    ("描述", "描述", "Description"),
    ("参数", "參數", "Parameters"),
    ("{count} 个参数", "{count} 個參數", "{count} parameters"),
    ("调用声明", "調用聲明", "Tool declarations"),
    ("{n} 条", "{n} 條", "{n} items"),
    ("无参数", "無參數", "No parameters"),
    ("必填", "必填", "Required"),
    ("可选", "選填", "Optional"),
    ("可选值", "允許值", "Allowed"),
    ("默认值", "預設值", "Default"),
    ("模型: {v}", "模型: {v}", "Model: {v}"),
    ("思考: {v}", "思考: {v}", "Thinking: {v}"),
    ("最大轮数: {v}", "最大輪數: {v}", "Max turns: {v}"),
    // editor pill menus
    ("使用 pi 默认设置", "使用 pi 預設設置", "Use pi default"),
    ("关闭推理", "關閉推理", "Reasoning off"),
    ("最低限度推理", "最低程度推理", "Minimal reasoning"),
    ("低强度推理", "低強度推理", "Low reasoning"),
    ("中等强度推理", "中等強度推理", "Medium reasoning"),
    ("高强度推理", "高強度推理", "High reasoning"),
    ("超高强度推理", "超高強度推理", "Extra-high reasoning"),
    ("最高强度推理", "最高強度推理", "Max reasoning"),
    ("取自 settings.json 的 defaultTools", "取自 settings.json 的 defaultTools", "From settings.json defaultTools"),
    ("仅聊天", "僅聊天", "Chat only"),
    ("4 个只读内置工具", "4 個只讀內置工具", "4 read-only built-in tools"),
    ("4 个内置工具", "4 個內置工具", "4 built-in tools"),
    ("全部内置工具", "全部內置工具", "All built-in tools"),
    // 034/040 会话扩展清单（2026-10 改版：档名 full+、勾选即生效）
    ("full+", "full+", "full+"),
    ("full + 自定义扩展", "full + 自訂擴展", "full + custom extensions"),
    ("选择扩展", "選擇擴展", "Choose extensions"),
    ("全部内置工具（不装任何扩展）", "全部內置工具（不裝任何擴展）", "All built-in tools (no extensions loaded)"),
    ("扩展清单将在下一轮生效", "擴展清單將在下一輪生效", "The extension list takes effect on the next turn"),
    ("扩展清单保存失败: {e}", "擴展清單儲存失敗: {e}", "Failed to save the extension list: {e}"),
    // 034 扩展按钮 + 勾选菜单
    ("扩展只能在 full+ 档勾选", "擴展只能在 full+ 檔勾選", "Extensions can only be picked in the full+ preset"),
    // 040 说明 token 预览（延迟加载）
    ("说明大小", "說明大小", "Prompt size"),
    ("合计：", "合計：", "Total:"),
    // 031 @ 文件检索 + ! shell 命令
    ("文件", "檔案", "Files"),
    ("斜杠命令", "斜槓命令", "Slash commands"),
    ("没有匹配的文件", "沒有符合的檔案", "No matching files"),
    ("Shell · 输出仅本地（!!）", "Shell · 輸出僅本機（!!）", "Shell · output stays local (!!)"),
    ("Shell · 输出发给模型", "Shell · 輸出傳送給模型", "Shell · output sent to model"),
    ("执行中 · Esc 中止", "執行中 · Esc 中止", "Running · Esc to abort"),
    ("会话忙，无法执行 shell 命令", "會話忙碌，無法執行 shell 命令", "Session is busy — can't run a shell command"),
    ("命令为空：! 后面接 shell 命令", "命令為空：! 後面接 shell 命令", "Empty command — type a shell command after !"),
    ("压缩中…", "壓縮中…", "Compacting…"),
    ("上下文压缩中，请稍等……", "上下文壓縮中，請稍等……", "Compacting context, please wait…"),
    ("生成标题…", "生成標題…", "Generating title…"),
    ("已生成标题: {title}", "已生成標題: {title}", "Title set: {title}"),
    ("标题生成失败: {e}", "標題生成失敗: {e}", "Title generation failed: {e}"),
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

    /// tests mutate the process-global language; serialize them
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn identity_for_source_language() {
        let _g = LOCK.lock();
        set_lang(0);
        assert_eq!(tr("保存"), "保存");
        assert_eq!(tr("something new"), "something new");
    }

    #[test]
    fn translations_for_other_languages() {
        let _g = LOCK.lock();
        set_lang(1);
        assert_eq!(tr("保存"), "儲存");
        assert_eq!(tr("新分支"), "新分支");
        assert_eq!(tr("创建中…"), "建立中…");
        assert_eq!(tr("Hi，打算让我做点什么？"), "Hi，打算讓我做點什麼？");
        assert_eq!(tr("搜索项目…"), "搜尋項目…");
        assert_eq!(tr("打开文件夹"), "開啟資料夾");
        set_lang(2);
        assert_eq!(tr("保存"), "Save");
        assert_eq!(tr("新分支"), "Fork");
        assert_eq!(tr("创建中…"), "Creating…");
        assert_eq!(tr("Hi，打算让我做点什么？"), "Hi, what shall we work on?");
        assert_eq!(tr("搜索项目…"), "Search projects…");
        assert_eq!(tr("打开文件夹"), "Open folder");
        assert_eq!(tr("not in table"), "not in table");
        set_lang(0);
    }

    #[test]
    fn template_replacement() {
        let _g = LOCK.lock();
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
