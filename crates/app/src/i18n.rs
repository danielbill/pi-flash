//! i18n (pi-flash-04k): three languages — zh-CN (source strings, identity),
//! zh-TW, en. `t()` maps a source literal through the table; `tf()` renders
//! `{key}` templates. The language index is runtime-switchable and persisted
//! in the workspace memory file (pi-web i18n parity: user preference).

use std::sync::atomic::{AtomicUsize, Ordering};

/// 0 zh-CN · 1 zh-TW · 2 en
static LANG_IX: AtomicUsize = AtomicUsize::new(0);

pub const LANG_LABELS: [&str; 3] = ["简体中文", "繁體中文", "English"];

pub fn set_lang(ix: usize) {
    let ix = ix.min(2);
    LANG_IX.store(ix, Ordering::Relaxed);
    // gpui-component（rust_i18n）词表同步：搜索条按钮文案/tooltip 用组件
    // 自己的 locales/ui.yml（繁体落组件的 zh-HK 桶）
    gpui_component::set_locale(match ix {
        1 => "zh-HK",
        2 => "en",
        _ => "zh-CN",
    });
}

pub fn lang_ix() -> usize {
    LANG_IX.load(Ordering::Relaxed)
}

// ===========================================================================
// 键 = 语义标识 `<域>.<主语>[.<限定>]`，**不再拿中文当键**（2026-10-12 迁移）。
// 旧设计里 zh-CN 源串同时是显示值和查表键，两处必须逐字节相等却没有编译期
// 检查——润色一个字、或新增文案忘了配译文，繁/英界面都会静默回落成中文。
// 语义键让三种语言的文案在表里各自独立：改文案不再动键。
//
// 行内四列：`(key, zh-CN, zh-TW, en)`。**繁/英留空 = 尚未翻译**��此时 tr()
// 回落 zh-CN（与迁移前一致）；漏译清单由 src/i18n_audit.rs 盯着（只减不增）。
//
// 新增文案流程：① i18n_audit 挡住「加了 tr() 却没加表行」；② 表行繁/英空着
// 会被挂进 BACKLOG，补齐后从 BACKLOG 删掉。
// ===========================================================================

/// 关于页信息块：这两个常量现在是**键**，文案在 TABLE 同一行（改文案不必动
/// 键）。about.rs 直接 `tr(ABOUT_*)`。
pub const ABOUT_DESC: &str = "about.tagline";
pub const ABOUT_NOTE: &str = "about.note";

/// (source zh-CN, zh-TW, en)

// (key, zh-CN, zh-TW, en)
//
// rustfmt 跳过本 const：i18n_audit 按行解析（行 trim 后以 '(' 起 + 切 4 列），
// 表行必须**一行一条**；rustfmt 会把超宽表行折成 5 行 → audit 只解析到零头直接
// panic。豁免只针对这个 const，本文件其余部分照常自动格式化。
#[rustfmt::skip]
const TABLE: &[(&str, &str, &str, &str)] = &[
    // chrome / sidebar
    // compaction card（pi-web CompactionMessageView parity）
    ("compaction.done", "会话已压缩", "會話已壓縮", "Conversation compacted"),
    ("compaction.preset_locked", "运行中不能更换工具预设", "執行中不能更換工具預設", "Cannot change the tool preset while the agent is running"),
    ("compaction.summary_intro", "此处之前的会话历史已压缩为以下摘要：", "此處之前的會話歷史已壓縮為以下摘要：", "The conversation history before this point was compacted into the following summary:"),
    ("compaction.no_summary", "（无摘要）", "（無摘要）", "(no summary)"),
    ("compaction.file_context", "文件上下文：{details}", "檔案脈絡：{details}", "File context: {details}"),
    ("compaction.reads", "{n} 读取", "{n} 讀取", "{n} read"),
    ("compaction.writes", "{n} 修改", "{n} 修改", "{n} modified"),
    ("compaction.modified_files", "修改文件", "修改檔案", "Modified files"),
    ("compaction.read_files", "读取文件", "讀取檔案", "Read files"),
    ("app.name", "pi-flash", "pi-flash", "pi-flash"),
    // 040-1 Pi 扩展市场（标准页 + topbar 三横菜单）
    ("market.tab", "Pi扩展市场", "Pi擴展市場", "Pi Extension Market"),
    ("titlebar.menu", "菜单", "選單", "Menu"),
    ("market.search_placeholder", "搜索扩展名称或说明", "搜尋擴展名稱或說明", "Search extensions by name or description"),
    ("market.search", "搜索", "搜尋", "Search"),
    ("market.filter.not_installed", "未安装", "未安裝", "Not installed"),
    ("market.filter.installed", "已安装", "已安裝", "Installed"),
    ("market.update_all", "全部更新", "全部更新", "Update all"),
    ("market.btn.install", "安装", "安裝", "Install"),
    ("market.btn.update", "更新", "更新", "Update"),
    ("market.btn.remove", "卸载", "卸載", "Uninstall"),
    ("market.type.extension", "扩展", "擴展", "Extension"),
    ("market.type.skill", "技能", "技能", "Skill"),
    ("market.type.prompt", "提示词", "提示詞", "Prompt"),
    ("market.type.theme", "主题", "主題", "Theme"),
    ("market.type.subagent", "子代理", "子代理", "Subagent"),
    // 040-2 Pi 原生扩展市场（pi.dev Package Catalog 移植；页面文案本身照抄
    // 设计稿英文，这里只收需要跟随界面的那几个词）
    ("market.native_tab", "Pi 扩展市场", "Pi 擴展市場", "Pi Market"),
    ("market.pf.tab", "Pi-Flash 插件", "Pi-Flash 外掛", "Pi-Flash Plugins"),
    ("market.pf.empty", "已全部安装，没有可更新的插件", "已全部安裝，沒有可更新的插件", "Everything is installed and up to date"),
    ("market.pf.installing", "安装中…", "安裝中…", "Installing…"),
    ("market.pf.updating", "更新中…", "更新中…", "Updating…"),
    ("market.pf.confirm", "再点一次确认卸载", "再點一次確認卸載", "Click again to confirm uninstall"),
    ("market.pf.removed", "已卸载 {name}", "已卸載 {name}", "Uninstalled {name}"),
    ("market.pf.failed", "操作失败：{err}", "操作失敗：{err}", "Operation failed: {err}"),
    ("market.pf.need_newer_pf", "该插件需要 PF {min}，当前 {cur}", "該插件需要 PF {min}，當前 {cur}", "This plugin requires PF {min}, current {cur}"),
    // 090-4 v3：布局对齐 pi market 页后的页内文案（三语齐全）
    ("market.pf.head", "所有插件", "所有外掛", "All plugins"),
    // 040-2 页内按钮走英文原串（照设计稿那套 chrome 的语言）
    ("market.install", "install", "install", "install"),
    ("market.copied", "copied", "copied", "copied"),
    ("market.type.unknown", "—", "—", "—"),
    ("market.empty", "没有匹配的扩展，换个关键词试试", "沒有符合的擴展，換個關鍵字試試", "No matching extensions — try another keyword"),
    ("market.installed_summary", "个已安装", "個已安裝", "installed"),
    ("market.empty_installed", "尚未安装任何扩展，去「未安装」页签逛逛", "尚未安裝任何擴展，去「未安裝」頁籤逛逛", "No extensions installed yet — browse the Not installed tab"),
    ("market.offline", "网络不可用，显示本地缓存的 {n} 条", "網路無法使用，顯示本機快取的 {n} 筆", "Network unavailable — showing {n} cached entries"),
    ("market.loading", "加载中…", "載入中…", "Loading…"),
    ("market.per_month", "/月", "/月", "/mo"),
    ("market.sort_downloads", "最多下载", "最多下載", "Most downloads"),
    ("market.sort_updated", "最近更新", "最近更新", "Recently updated"),
    ("market.sort_name", "名称 A→Z", "名稱 A→Z", "Name A→Z"),
    ("market.results", "个结果", "個結果", "results"),
    ("market.only_ext", "只展示扩展（技能/提示词/主题不列出）", "只顯示擴展（技能/提示詞/主題不列出）", "Extensions only (skills, prompts and themes are not listed)"),
    ("market.prev", "← 上一页", "← 上一頁", "← Previous"),
    ("market.next", "下一页 →", "下一頁 →", "Next →"),
    ("market.today", "今天", "今天", "today"),
    ("market.days_ago", "{n} 天前", "{n} 天前", "{n}d ago"),
    ("market.load_failed", "加载失败：{e}", "載入失敗：{e}", "Load failed: {e}"),
    ("market.retry", "重试", "重試", "Retry"),
    ("action.new", "新建", "新建", "New"),
    ("session.new", "新会话", "新會話", "New session"),
    // 012 新会话页（newSession）
    ("new_session.greeting", "Hi，打算让我做点什么？", "Hi，打算讓我做點什麼？", "Hi, what shall we work on?"),
    ("session.open_project", "打开项目", "開啟專案", "Open project"),
    ("session.project_switch_hint", "切换后仅显示该项目的会话，并恢复上次打开的会话", "切換後僅顯示該項目的會話，並恢復上次打開的會話", "Show only this project's sessions and restore the last open one"),
    ("session.full_history", "完整历史", "完整歷史", "Full history"),
    ("branch.title", "分支", "分支", "Branches"),
    ("branch.main", "主分支", "主分支", "main"),
    ("git.none", "no git", "no git", "no git"),
    ("session.no_branches", "暂无分支", "暫無分支", "No branches"),
    ("branch.node_hint", "点击节点：从该用户消息处创建分支新会话", "點擊節點：從該用戶消息處創建分支新會話", "Click a node to fork a new session from that user message"),
    ("session.fork", "新分支", "新分支", "Fork"),
    ("session.copied", "已复制", "已複製", "Copied"),
    ("session.creating", "创建中…", "建立中…", "Creating…"),
    ("tool.file_browser", "文件浏览器", "文件瀏覽器", "Files"),
    ("action.compact", "压缩上下文", "壓縮上下文", "Compact context"),
    ("action.send", "发送", "發送", "Send"),
    ("composer.placeholder", "消息...输入 / 使用命令，输入 @ 查找文件", "訊息...輸入 / 使用命令，輸入 @ 查找文件", "Message... / for commands, @ for files"),
    ("session.no_active_session", "无活动会话", "無活動會話", "No active session"),
    ("status.disconnected", "未连接", "未連接", "Disconnected"),
    ("session.generate_title", "生成标题", "生成標題", "Generate title"),
    // dialogs
    // 004 打开项目菜单（projectManager）
    ("project.search_placeholder", "搜索项目…", "搜尋項目…", "Search projects…"),
    ("action.open_folder", "打开文件夹", "開啟資料夾", "Open folder"),
    ("project.idle_30d", "最近 30 天没有打开过的项目", "最近 30 天沒有開啟過的項目", "No projects opened in the last 30 days"),
    ("project.no_match", "没有匹配的项目", "沒有符合的項目", "No matching projects"),
    ("action.rename_session", "重命名会话", "重命名會話", "Rename session"),
    ("action.session_name", "session name", "session name", "session name"),
    ("action.cancel", "取消", "取消", "Cancel"),
    ("action.save", "保存", "儲存", "Save"),
    ("action.update", "更新", "更新", "Update"),
    ("action.delete", "删除", "刪除", "Delete"),
    ("action.no", "否", "否", "No"),
    ("action.yes", "是", "是", "Yes"),
    ("action.submit", "提交", "提交", "Submit"),
    // 问卷卡（ask_card.rs，ask_user_question 整卷 tab 卡）
    ("ask.question", "问题", "問題", "Question"),
    ("ask.confirm", "确 认", "確 認", "Confirm"),
    ("ask.cancel_all", "取消整卷", "取消整卷", "Cancel all"),
    ("ask.next", "下一题", "下一題", "Next"),
    ("ask.advance_submit", "跳题/提交", "跳題/提交", "Skip/Submit"),
    ("ask.direct_pick", "直选", "直選", "Pick"),
    ("ask.other_placeholder", "自定义答案…", "自訂答案…", "Custom answer…"),
    ("ask.no_preview", "（无预览）", "（無預覽）", "(no preview)"),
    ("model.select_placeholder", "select model", "select model", "select model"),
    ("model.select_title", "选择模型", "選擇模型", "Select model"),
    ("action.filter_models", "过滤模型...", "過濾模型...", "filter models..."),
    ("action.select_project", "选择项目", "選擇項目", "Select project"),
    ("action.no_models_match", "no models match", "no models match", "no models match"),
    // 系统提示词面板 token 分析块（7 大类）
    ("tokens.total", "总计", "總計", "Total"),
    ("tokens.global_prompt", "全局提示词", "全域提示詞", "Global prompt"),
    ("tokens.system_tools", "系统工具", "系統工具", "System tools"),
    ("tokens.project_prompt", "项目提示词", "專案提示詞", "Project prompt"),
    // settings tabs
    ("settings.tab.models", "模型", "模型", "Models"),
    ("settings.tab.skills", "技能", "技能", "Skills"),
    ("settings.tab.extensions", "扩展", "擴展", "Extensions"),
    ("settings.tab.agents", "子代理", "子代理", "Subagents"),
    ("settings.tab.tools", "工具", "工具", "Tools"),
    ("settings.tab.general", "通用", "通用", "General"),
    // models tab
    ("models.enabled_models", "已启用模型", "已啟用模型", "Enabled models"),
    ("models.enable_all", "全部启用", "全部啟用", "Enable all"),
    ("models.disable_all", "全部停用", "全部停用", "Disable all"),
    ("models.current", "当前", "當前", "current"),
    ("models.api_key", "API Key", "API Key", "API Key"),
    ("models.api_key_configured", "API Key 已配置", "API Key 已配置", "API key configured"),
    ("models.api_key_required", "API Key 不能为空", "API Key 不能為空", "API key must not be empty"),
    ("models.signed_in_with_oauth", "OAuth 已登录", "OAuth 已登入", "Signed in with OAuth"),
    ("models.credentials", "凭据", "憑證", "Credentials"),
    ("models.credentials_hint", "登录凭据存储于 ~/.pi/agent/auth.json（与 pi 共用）", "登入憑證儲存於 ~/.pi/agent/auth.json（與 pi 共用）", "Credentials are stored in ~/.pi/agent/auth.json (shared with pi)"),
    ("action.show", "显示", "顯示", "Show"),
    ("action.hide", "隐藏", "隱藏", "Hide"),
    ("models.api_key_hint", "密钥写入 ~/.pi/agent/auth.json（与 pi 共用）；新 provider 的模型需重启 pi-flash 后出现在列表", "密鑰寫入 ~/.pi/agent/auth.json（與 pi 共用）；新 provider 的模型需重啟 pi-flash 後出現在列表", "Key is written to ~/.pi/agent/auth.json (shared with pi); a new provider's models appear after restarting pi-flash"),
    ("models.not_configured", "未配置", "未配置", "Not configured"),
    ("models.cannot_disable_last", "不能停用最后一个启用的模型", "不能停用最後一個啟用的模型", "Cannot disable the last enabled model"),
    ("settings.write_failed", "写入 settings.json 失败: {e}", "寫入 settings.json 失敗: {e}", "Failed to write settings.json: {e}"),
    ("models.failed_to_save", "保存失败: {e}", "儲存失敗: {e}", "Failed to save: {e}"),
    ("models.project_override_dotpi", "项目级 .pi/settings.json 覆盖了 enabledModels，面板只读", "項目級 .pi/settings.json 覆蓋了 enabledModels，面板只讀", "Project .pi/settings.json overrides enabledModels; panel is read-only"),
    ("models.project_override", "项目级 settings.json 覆盖了 enabledModels，此面板只读", "項目級 settings.json 覆蓋了 enabledModels，此面板只讀", "Project settings.json overrides enabledModels; this panel is read-only"),
    ("models.no_matching_models", "没有匹配的模型", "沒有匹配的模型", "No matching models"),
    // skills tab
    ("skills.no_skills_found", "没有找到技能", "沒有找到技能", "No skills found"),
    ("skills.empty_hint", "没有找到技能（扫描项目 .pi/skills、.agents/skills 与全局目录）", "沒有找到技能（掃描項目 .pi/skills、.agents/skills 與全局目錄）", "No skills found (scans project .pi/skills, .agents/skills and global dirs)"),
    ("skills.visible_to_model", "对模型可见", "對模型可見", "Visible to model"),
    ("skills.hidden_hint", "已隐藏（仍可手动调用）", "已隱藏（仍可手動調用）", "Hidden (still invocable manually)"),
    ("skills.write_failed", "写入 SKILL.md 失败: {e}", "寫入 SKILL.md 失敗: {e}", "Failed to write SKILL.md: {e}"),
    // extensions tab（040）
    ("extensions.install", "安装", "安裝", "Install"),
    ("extensions.installing", "安装中", "安裝中", "Installing…"),
    ("extensions.install_hint", "从 https://pi.dev/packages 复制安装命令，做全局安装", "從 https://pi.dev/packages 複製安裝命令，做全域安裝", "Copy an install command from https://pi.dev/packages and install globally"),
    ("extensions.global_defaults", "配置全局默认扩展", "配置全域預設擴展", "Global default extensions"),
    // 082 扩展更新
    ("extensions.update_all", "更新全部", "更新全部", "Update all"),
    ("extensions.updating_all", "更新中…", "更新中…", "Updating…"),
    ("extensions.update", "更新", "更新", "Update"),
    ("extensions.updated_one", "扩展已更新，重启会话后生效", "擴展已更新，重啟會話後生效", "Extension updated. Restart the session to apply"),
    ("extensions.updated_all", "全部扩展已更新，重启会话后生效", "全部擴展已更新，重啟會話後生效", "All extensions updated. Restart the session to apply"),
    ("new_session.pi_extension_updates", "Pi 扩展更新", "Pi 擴展更新", "Pi extension updates"),
    ("about.description", "说明", "說明", "Description"),
    ("extensions.path", "路径", "路徑", "Path"),
    ("extensions.source", "来源", "來源", "Source"),
    ("scope.global", "全局", "全局", "Global"),
    ("scope.project", "项目", "項目", "Project"),
    ("scope.workspace", "工作区", "工作區", "Workspace"),
    ("scope.builtin", "内置", "內置", "Built-in"),
    ("status.enabled", "已启用", "已啟用", "Enabled"),
    ("status.disabled", "已停用", "已停用", "Disabled"),
    ("status.loaded", "已加载", "已加載", "Loaded"),
    ("extensions.not_configured", "没有已配置的扩展", "沒有已配置的擴展", "No configured extensions"),
    ("extensions.not_installed", "没有已安装的扩展", "沒有已安裝的擴展", "No extensions installed"),
    ("action.remove", "移除", "移除", "Remove"),
    ("extensions.fetch_models", "获取模型", "獲取模型", "Fetch models"),
    ("extensions.fetching", "获取中…", "獲取中…", "Fetching…"),
    ("extensions.remove_key_confirm", "删除 {name} 的 API Key，并不再显示？", "刪除 {name} 的 API Key，並不再顯示？", "Delete the {name} API key and hide this provider?"),
    ("extensions.fetch_key_rejected", "获取失败：key 被拒（HTTP {s}）", "獲取失敗：key 被拒（HTTP {s}）", "Fetch failed: key rejected (HTTP {s})"),
    ("extensions.fetch_failed", "获取失败：{e}", "獲取失敗：{e}", "Fetch failed: {e}"),
    ("action.confirm", "确认", "確認", "Confirm"),
    ("extensions.uninstall", "确认卸载", "確認卸載", "Uninstall"),
    ("extensions.uninstall_confirm", "卸载扩展 {name}？", "卸載擴展 {name}？", "Uninstall extension {name}?"),
    ("extensions.source_placeholder", "请输入扩展来源，或粘贴 pi install 安装命令", "請輸入擴展來源，或貼上 pi install 安裝命令", "Enter a source or paste a pi install command"),
    ("extensions.installed", "已安装 {source}", "已安裝 {source}", "Installed {source}"),
    ("extensions.removed", "已移除 {source}", "已移除 {source}", "Removed {source}"),
    ("extensions.pi_failed", "pi {} 失败: {}", "pi {} 失敗: {}", "pi {} failed: {}"),
    ("extensions.remove_failed", "删除失败: {e}", "刪除失敗: {e}", "Failed to delete: {e}"),
    ("extensions.remove_failed_msg", "删除失败：{msg}", "刪除失敗：{msg}", "Failed to delete: {msg}"),
    ("extensions.ok", "知道了", "知道了", "OK"),
    // tools tab
    ("tools.tool_selection", "工具选择", "工具選擇", "Tool selection"),
    ("tools.all", "全部", "全部", "All"),
    ("tools.default", "默认", "預設", "Default"),
    ("tools.read_only", "只读", "只讀", "Read-only"),
    ("tools.none", "无", "無", "None"),
    ("tools.pi_default_hint", "不覆盖，pi 默认（全部内置工具）", "不覆蓋，pi 預設（全部內置工具）", "No override; pi default (all built-in tools)"),
    ("tools.read_bash_edit_write", "read, bash, edit, write", "read, bash, edit, write", "read, bash, edit, write"),
    ("tools.read_grep_find_ls", "read, grep, find, ls", "read, grep, find, ls", "read, grep, find, ls"),
    ("tools.disable_all_tools", "禁用所有工具", "禁用所有工具", "Disable all tools"),
    ("tools.unset_hint", "未设置（pi 默认解析全部工具）", "未設置（pi 預設解析全部工具）", "Unset (pi resolves every tool)"),
    ("tools.no_tools", "[]（无工具）", "[]（無工具）", "[] (no tools)"),
    ("tools.default_tools_hint", "写入 ~/.pi/agent/settings.json 的 defaultTools；新会话生效（与 pi CLI --tools 一致）", "寫入 ~/.pi/agent/settings.json 的 defaultTools；新會話生效（與 pi CLI --tools 一致）", "Writes defaultTools to ~/.pi/agent/settings.json; applies to new sessions (same as pi CLI --tools)"),
    // 会话统计行
    ("stats.output", "输出", "輸出", "Output"),
    // general tab（v60 界面页）
    ("settings.tab.appearance", "界面", "介面", "Appearance"),
    ("settings.theme", "主题", "主題", "Themes"),
    // ---- 083 项目搜索 ----
    ("shortcuts.project_search", "搜索文件内容（项目搜索）", "搜尋檔案內容（專案搜尋）", "Search in project files"),
    ("search.tab", "搜索", "搜尋", "Search"),
    ("search.close", "收起搜索", "收起搜尋", "Close search"),
    ("search.match_count", "{n} 处", "{n} 處", "{n} matches"),
    ("search.placeholder", "搜索全部文件…", "搜尋全部檔案…", "Search all files…"),
    ("search.replace_placeholder", "替换为…", "取代為…", "Replace with…"),
    ("search.include_placeholder", "包含：如 src/**/*.rs", "包含：如 src/**/*.rs", "Include: e.g. src/**/*.rs"),
    ("search.exclude_placeholder", "排除：如 vendor/*, *.lock", "排除：如 vendor/*, *.lock", "Exclude: e.g. vendor/*, *.lock"),
    ("search.case_sensitive", "区分大小写", "區分大小寫", "Match case"),
    ("search.whole_word", "整词匹配", "整詞符合", "Match whole word"),
    ("search.regex", "正则表达式", "正規表達式", "Use regular expression"),
    ("search.filters", "文件过滤", "檔案篩選", "Toggle file filters"),
    ("search.replace", "替换", "取代", "Toggle replace"),
    ("search.replace_next", "替换下一处", "取代下一處", "Replace next match"),
    ("search.replace_all", "全部替换", "全部取代", "Replace all matches"),
    ("search.prev", "上一个匹配", "上一個符合", "Select previous match"),
    ("search.next", "下一个匹配", "下一個符合", "Select next match"),
    ("search.include_ignored", "同时搜索被忽略的文件", "同時搜尋被忽略的檔案", "Also search ignored files"),
    ("search.path_match", "文件名匹配", "檔名符合", "file name"),
    ("search.hint_idle", "输入内容以搜索整个项目（文件名与文件内容一起搜）", "輸入內容以搜尋整個專案（檔名與檔案內容一起搜尋）", "Type to search the whole project (file names and contents)"),
    ("search.searching", "正在搜索…", "正在搜尋…", "Searching…"),
    ("search.no_results", "没有匹配结果", "沒有符合結果", "No results found"),
    ("search.landing_title", "搜索所有文件", "搜尋所有檔案", "Search All Files"),
    ("search.landing_hint", "开始输入以搜索。更多选项：", "開始輸入以搜尋。更多選項：", "Start typing to search. For more options:"),
    ("search.landing_paths", "包含/排除指定路径", "包含/排除指定路徑", "Include/exclude specific paths"),
    ("search.landing_replace", "查找并替换", "尋找並取代", "Find and replace"),
    ("search.landing_regex", "使用正则匹配", "使用正規表達式符合", "Match with regex"),
    ("search.landing_case", "区分大小写", "區分大小寫", "Match case"),
    ("search.landing_whole_word", "整词匹配", "整詞符合", "Match whole words"),
    ("search.no_results_body", "项目中没有找到与查询匹配的内容", "專案中沒有找到與查詢符合的內容", "No results found in this project for the provided query"),
    ("search.loading_project", "正在载入项目…", "正在載入專案…", "Loading project…"),
    ("search.no_project", "没有打开的项目，无法搜索", "沒有開啟的專案，無法搜尋", "No project open to search"),
    ("search.replaced", "已替换 {files} 个文件中的 {hits} 处", "已取代 {files} 個檔案中的 {hits} 處", "Replaced {hits} matches in {files} files"),
    ("search.replace_skipped_dirty", "（跳过 {n} 个有未保存修改的文件）", "（略過 {n} 個有未儲存修改的檔案）", " (skipped {n} unsaved files)"),
    ("search.replace_none", "没有可替换的文件", "沒有可取代的檔案", "Nothing to replace"),
    ("settings.fonts", "字体", "字體", "Fonts"),
    ("settings.language", "语言", "語言", "Language"),
    ("settings.chat_font", "会话字体", "會話字體", "Chat font"),
    ("settings.panel_font", "面板字体", "面板字體", "Panel font"),
    ("settings.file_font", "文档字体", "文档字體", "File font"),
    ("size.small", "小", "小", "Small"),
    ("size.medium", "中", "中", "Medium"),
    ("size.large", "大", "大", "Large"),
    ("size.extra_large", "特大", "特大", "Extra large"),
    ("settings.search_fonts", "搜索字体…", "搜尋字型…", "Search fonts…"),
    ("settings.no_matching_fonts", "无匹配字体", "無符合字型", "No matching fonts"),
    ("settings.notification_sound", "提示音", "提示音", "Notification sound"),
    ("settings.sound_hint", "agent 运行结束播放系统提示音", "agent 執行結束播放系統提示音", "Play a system sound when the agent run finishes"),
    ("settings.load_time_window", "加载时间窗口", "載入時間範圍", "Load time window"),
    ("settings.load_window_hint", "加载最近几天内活跃的会话", "載入最近幾天內活躍的會話", "Load sessions active within the window"),
    ("settings.session_display_count", "会话显示数", "會話顯示數", "Session display count"),
    ("settings.auto_collapse_time", "自动折叠时间", "自動摺疊時間", "Auto-collapse time"),
    ("settings.auto_collapse_hint", "启动时，超时未活动的项目默认折叠", "啟動時，超時未活動的專案預設摺疊", "Collapse projects with no recent activity at startup"),
    ("settings.send_mode", "运行中发送方式", "執行中發送方式", "Send behavior while running"),
    ("settings.send_mode_hint", "设置Agent运行时用户发言为排队发送或立刻插话", "设置Agent运行时用户发言为排队发送或立刻插话", "Queue or Steer"),
    ("settings.send_queue", "排队", "排隊", "Queue"),
    ("settings.send_steer", "插话", "插話", "Steer"),
    ("settings.session_count_hint", "各项目默认显示的会话数量", "各項目預設顯示的會話數量", "Sessions shown per project by default"),
    ("settings.auto_clean_title", "自动清理会话", "自動清理會話", "Auto-clean sessions"),
    ("settings.auto_clean_hint", "设置会话停止活动N天后自动删除，移入回收站，置顶会话除外", "設置會話停止活動N天後自動刪除，移入資源回收筒，置頂會話除外", "Delete sessions after N days of inactivity,moves to the recycle bin, pinned excluded"),
    ("misc.auto_clean_off", "关闭", "關閉", "Off"),
    ("misc.auto_clean_days", "{n} 天", "{n} 天", "{n} days"),
    ("settings.show_more", "显示更多", "顯示更多", "Show more"),
    // session list / relative time templates
    ("session.messages", "{} 条消息", "{} 條消息", "{} messages"),
    // 033 导航面板总结栏：改「icon 数字」四组（v7x），无文案不走过 i18n。
    // time.s_ago 已删（2026-11：弹窗时间不数秒，time_ago 最小粒度到分钟）
    ("time.m_ago", "{n}分钟前", "{n}分鐘前", "{n}m ago"),
    ("time.h_ago", "{n}小时前", "{n}小時前", "{n}h ago"),
    ("time.d_ago", "{n}天前", "{n}天前", "{n}d ago"),
    // misc
    ("value.none", "（无）", "（無）", "(none)"),
    ("auth.sign_out", "退出登录", "退出登入", "Sign out"),
    // 界面页显示开关（自「其他」页挪入，无小节标题——用户定夺）
    // 界面页：会话内容宽度（消息列 + composer 胶囊同源；pi-web chatContentWidth）
    ("settings.chat_width", "会话内容宽度", "會話內容寬度", "Chat content width"),
    ("settings.chat_width_hint", "消息与输入框的最大宽度", "訊息與輸入框的最大寬度", "Max width shared by messages and the composer"),
    ("settings.show_thinking", "展示思考", "展示思考", "Show thinking"),
    ("settings.show_thinking_hint", "思考默认展开或收起", "思考預設展開或收起", "thinking blocks expand by default or collapse to one line"),
    // 界面页：部分操作隐藏（dock 两操作栏不常用 icon 自动隐藏总开关）
    ("settings.autohide_icons", "部分操作隐藏", "部分操作隱藏", "Auto-hide small icons"),
    ("settings.autohide_icons_hint", "设置面板上的小icon隐藏，只当鼠标悬浮时展示", "設定面板上的小icon隱藏，只當滑鼠懸浮時展示", "Small panel icons stay hidden until you hover the panel"),
    // 081 自动更新（其他页开关 + 新会话页版本号位 + 更新日志 tab）
    ("about.auto_update", "自动更新", "自動更新", "Auto-update"),
    ("settings.auto_update_hint", "允许Pi-Flash自动更新", "允許Pi-Flash自動更新", "Let Pi-Flash update itself automatically"),
    ("about.downloading_v", "下载 {v} 版本中…", "下載 {v} 版本中…", "Downloading v{v}…"),
    ("about.extracting_v", "解压安装中…", "解壓安裝中…", "Extracting & installing…"),
    ("update.ready_click", "下载完成，点击重启更新", "下載完成，點擊重啟更新", "Download complete — click to restart and update"),
    ("about.changelog", "更新日志", "更新日誌", "Changelog"),
    ("about.fetching_changelog", "正在获取更新日志…", "正在獲取更新日誌…", "Fetching changelog…"),
    ("about.no_changelog_available", "暂无更新日志", "暫無更新日誌", "No changelog available"),
    ("about.github_unreachable", "暂时无法连接 GitHub", "暫時無法連線 GitHub", "Can't reach GitHub right now"),
    ("about.about", "关于", "關於", "About"),
    ("about.author", "作者", "作者", "Author"),
    ("about.author_name", "不正常赛博捅讯社", "不正常賽博捅訊社", "不正常赛博捅讯社"),
    ("about.version", "PF版本", "PF 版本", "Version"),
    ("about.kernel", "内核版本", "內核版本", "Kernel"),
    ("about.tagline", "Windows上Pi的极速Shell + 文件管理编辑", "Windows上Pi的極速Shell + 檔案管理編輯", "A lightning-fast Pi Shell for Windows + file management & editing"),
    ("about.note", "Pi-Flash将保持轻巧专注速度体验", "Pi-Flash將保持輕巧專注速度體驗", "The Pi-Flash kernel stays lean and focused on speed"),
    // 文件树 git 标识开关（设置-界面）
    ("settings.file_tree_git_markers", "文件树 Git 标识", "檔案樹 Git 標識", "File tree Git markers"),
    ("settings.git_markers_hint", "文件树显示 git 修改徽标与目录变更点", "檔案樹顯示 git 修改徽標與目錄變更點", "Show git modification badges and directory change dots in the file tree"),
    // 023 fileView（标签栏 + 菜单 / 导航栏 / 冲突横幅 / 确认弹窗）
    ("file_view.pick_in_tree", "在左侧文件树中选择一个文件", "在左側檔案樹中選擇一個檔案", "Pick a file in the tree on the left"),
    ("file_view.open_file", "打开文件…", "開啟檔案…", "Open file…"),
    ("file_view.new_file", "新建文件", "新增檔案", "New file"),
    ("file_view.name_label", "文件名（可含子目录）", "檔案名（可含子目錄）", "File name (subdirs allowed)"),
    ("file_view.create_hint", "在项目根下创建；Enter 确认，Esc 取消", "在專案根下建立；Enter 確認，Esc 取消", "Created under the project root; Enter to confirm, Esc to cancel"),
    ("file_view.too_large", "文件超过 10MB，不打开", "檔案超過 10MB，不開啟", "File exceeds 10MB, not opening"),
    ("file_view.read_failed", "读取失败", "讀取失敗", "Read failed"),
    // 无法打开灰屏（二进制占位 tab，仿 Zed Could not open file）
    ("file_view.unsupported_type", "无法打开 {ext} 类型文件", "無法開啟 {ext} 類型的檔案", "Cannot open {ext} files"),
    ("file_view.binary_unsupported", "二进制文件不受支持", "不支援二進位檔案", "Binary files are not supported"),
    ("file_view.open_in_default_app", "在默认应用中打开", "以預設應用程式開啟", "Open in Default App"),
    ("file_view.save_failed", "保存失败", "儲存失敗", "Save failed"),
    // 051-apikey管理
    ("conflict.configured_vault", "已配置 · 系统凭据库", "已設定 · 系統憑證庫", "Configured · OS vault"),
    ("conflict.configured_file", "已配置 · 文件存储", "已設定 · 檔案儲存", "Configured · file storage"),
    ("conflict.vault_unavailable", "系统凭据库不可用，已降级为文件存储", "系統憑證庫不可用，已降級為檔案儲存", "Credential vault unavailable — fell back to file storage"),
    ("conflict.create_failed", "创建失败", "建立失敗", "Create failed"),
    ("conflict.path_escapes_root", "路径越出项目根", "路徑越出專案根", "Path escapes the project root"),
    ("status.saved", "已保存", "已儲存", "Saved"),
    ("conflict.modified_on_disk", "文件已在磁盘上被修改（本地有未保存修改）", "檔案已在磁碟上被修改（本地有未儲存修改）", "File changed on disk (you have unsaved edits)"),
    ("conflict.deleted_on_disk", "文件已从磁盘消失", "檔案已從磁碟消失", "File disappeared from disk"),
    ("conflict.reload", "重新加载", "重新載入", "Reload"),
    ("conflict.keep_my_version", "保留我的版本", "保留我的版本", "Keep my version"),
    ("conflict.keep_buffer", "保留缓冲", "保留緩衝", "Keep buffer"),
    ("conflict.unsaved_count", "未保存的修改", "未儲存的修改", "Unsaved changes"),
    ("conflict.unsaved_list", "有未保存的修改：", "有未儲存的修改：", "Unsaved changes:"),
    ("conflict.unsaved_n_files", "有未保存的修改：{n} 个文件", "有未儲存的修改：{n} 個檔案", "Unsaved changes in {n} files"),
    ("conflict.close_without_saving", "不保存关闭", "不儲存關閉", "Close without saving"),
    ("conflict.save_and_close", "保存并关闭", "儲存並關閉", "Save and close"),
    // topbar 标签右键菜单（zed tab_bar 菜单移植；无 Close Clean，会话
    // 首标签恒不可关——菜单里没有 Close、Close Left 恒灰）
    ("tab.close", "关闭", "關閉", "Close"),
    ("tab.close_others", "关闭其他标签", "關閉其他標籤", "Close Others"),
    ("tab.close_left", "关闭左侧标签", "關閉左側標籤", "Close Left"),
    ("tab.close_right", "关闭右侧标签", "關閉右側標籤", "Close Right"),
    ("tab.close_all", "关闭全部标签", "關閉全部標籤", "Close All"),
    ("conflict.no_files", "（无文件）", "（無檔案）", "(no files)"),
    ("conflict.initializing_editor", "编辑器初始化中…", "編輯器初始化中…", "Initializing editor…"),
    ("conflict.file_closed", "文件已关闭", "檔案已關閉", "File closed"),
    // 021 文件树操作栏 + 右键菜单
    ("file_tree.reveal_in_explorer", "在资源管理器中打开", "在檔案資源管理器中開啟", "Open in Explorer"),
    ("file_tree.open_in_terminal", "在终端中打开", "在終端機中開啟", "Open in terminal"),
    ("file_tree.open_in_browser", "在浏览器中打开", "在瀏覽器中開啟", "Open in browser"),
    ("file_tree.execute", "执行", "執行", "Execute"),
    ("file_tree.ext_menu", "扩展菜单", "擴充選單", "Extensions"),
    ("file_tree.execute_no_interpreter", "未找到解释器", "找不到解譯器", "no interpreter found"),
    ("file_tree.copy_path", "复制路径", "複製路徑", "Copy path"),
    ("action.copy", "复制", "複製", "Copy"),
    ("action.paste", "粘贴", "貼上", "Paste"),
    ("action.cut", "剪切", "剪下", "Cut"),
    ("action.select_all", "全选", "全選", "Select All"),
    // 031 输入面板右键菜单（用户定案：redo 文案「恢复」）
    ("action.undo", "撤销", "撤銷", "Undo"),
    ("action.redo", "恢复", "恢復", "Redo"),
    ("file_view.preview", "预览", "預覽", "Preview"),
    ("file_tree.insert_at_mention", "@到会话输入框", "@到會話輸入框", "Mention in composer"),
    ("file_tree.rename", "改名", "改名", "Rename"),
    ("file_tree.new_folder", "新建目录", "新增資料夾", "New folder"),
    ("action.rename", "重命名", "重新命名", "Rename"),
    ("field.name", "名称", "名稱", "Name"),
    ("action.create", "创建", "建立", "Create"),
    ("tree.name_required", "名称不能为空", "名稱不能為空", "Name cannot be empty"),
    ("tree.name_has_separator", "名称不能包含路径分隔符", "名稱不能包含路徑分隔符", "Name cannot contain path separators"),
    ("file_tree.already_exists", "已存在", "已存在", "Already exists"),
    ("tree.exists_named", "「{name}」已存在，请换一个名称", "「{name}」已存在，請換一個名稱", "\"{name}\" already exists at this location, please choose a different name"),
    ("tree.name_has_whitespace", "名称首尾包含空白字符", "名稱首尾包含空白字元", "Name contains leading or trailing whitespace"),
    ("tree.name_illegal_segment", "名称不能为空或包含 .. 等非法段", "名稱不能為空或包含 .. 等非法段", "Name cannot be empty or contain invalid segments like \"..\""),
    ("tree.delete_confirm", "删除 {name}？（移入回收站）", "刪除 {name}？（移入回收站）", "Delete {name}? (moves to the recycle bin)"),
    ("tree.delete_confirm_dirty", "删除 {name}？{n} 个打开文件的未保存更改将丢失（移入回收站）", "刪除 {name}？{n} 個開啟檔案的未保存變更將遺失（移至資源回收筒）", "Delete {name}? Unsaved changes in {n} open file(s) will be lost (moves to the recycle bin)"),
    ("file_tree.search_files", "搜索文件…", "搜尋檔案…", "Search files…"),
    ("file_tree.search_hint", "输入文件名关键词搜索当前项目", "輸入檔名關鍵詞搜尋當前專案", "Type a file-name keyword to search the current project"),
    ("file_tree.scanning_files", "正在扫描文件…", "正在掃描檔案…", "Scanning files…"),
    ("file_tree.files", "{n} 个文件", "{n} 個檔案", "{n} files"),
    ("file_tree.no_changes", "暂无变更", "暫無變更", "No changes"),
    ("file_tree.show_changes_only", "只看变更", "只看變更", "Show changes only"),
    ("file_tree.maximize", "最大化文件树", "最大化文件樹", "Maximize file tree"),
    ("file_tree.switch_project", "切换项目", "切換專案", "Switch project"),
    ("tree.delete_root_denied", "不能删除项目根目录", "不能刪除專案根目錄", "Cannot delete the project root"),
    ("tree.deleting", "正在删除「{name}」…（大目录移入回收站可能较久）", "正在刪除「{name}」…（大目錄移至資源回收筒可能較久）", "Deleting \"{name}\"… (large folders may take a while to move to the recycle bin)"),
    ("file_tree.rename_failed", "重命名失败: {e}", "重新命名失敗: {e}", "Rename failed: {e}"),
    ("file_tree.move_failed", "移动失败: {e}", "移動失敗: {e}", "Move failed: {e}"),
    // 021 Pin 收藏夹
    ("file_tree.pin", "Pin", "Pin", "Pin"),
    ("file_tree.unpin", "取消固定", "取消釘選", "Unpin"),
    ("file_tree.pin_favorites", "Pin 收藏夹", "Pin 收藏夾", "Pin favorites"),
    ("file_tree.back_to_tree", "返回目录树", "返回目錄樹", "Back to file tree"),
    ("file_tree.no_pins_view", "暂无收藏", "暫無收藏", "No pins"),
    // 会话功能栏（session::session_fn_bar，原 topbar ⋯ 菜单三入口）
    ("session.open_terminal", "打开终端", "開啟終端機", "Open terminal"),
    ("session.open_dir", "会话目录", "會話目錄", "Sessions folder"),
    ("session.system_prompt", "系统提示词", "系統提示詞", "System prompt"),
    ("session.session_tools", "会话工具集", "會話工具集", "Session tools"),
    // top panel：系统提示词（pi-web system.*）
    ("system_prompt.tool_definitions", "工具定义", "工具定義", "Tool definitions"),
    ("system_prompt.empty_disabled", "系统提示词为空（工具已禁用）", "系統提示詞為空（工具已停用）", "System prompt is empty (tools are disabled)"),
    ("system_prompt.loading", "系统提示词加载中…", "系統提示詞載入中…", "Loading system prompt…"),
    // top panel：工具定义（pi-web tools.*）
    ("tool_defs.not_loaded", "工具定义尚未加载", "工具定義尚未載入", "Tool definitions have not loaded yet"),
    ("tool_defs.no_active_tools", "没有启用的工具", "沒有啟用的工具", "No active tools"),
    ("tool_defs.description", "描述", "描述", "Description"),
    ("tool_defs.params", "参数", "參數", "Parameters"),
    ("tool_defs.parameters", "{count} 个参数", "{count} 個參數", "{count} parameters"),
    ("tool_defs.tool_declarations", "调用声明", "調用聲明", "Tool declarations"),
    ("tool_defs.items", "{n} 条", "{n} 條", "{n} items"),
    ("tool_defs.no_parameters", "无参数", "無參數", "No parameters"),
    ("tool_defs.required", "必填", "必填", "Required"),
    ("tool_defs.optional", "可选", "選填", "Optional"),
    ("tool_defs.allowed", "可选值", "允許值", "Allowed"),
    ("tool_defs.default", "默认值", "預設值", "Default"),
    ("tool_defs.model", "模型: {v}", "模型: {v}", "Model: {v}"),
    ("tool_defs.thinking", "思考: {v}", "思考: {v}", "Thinking: {v}"),
    ("tool_defs.max_turns", "最大轮数: {v}", "最大輪數: {v}", "Max turns: {v}"),
    // editor pill menus
    ("thinking.use_pi_default", "使用 pi 默认设置", "使用 pi 預設設置", "Use pi default"),
    ("thinking.off", "关闭推理", "關閉推理", "Reasoning off"),
    ("thinking.minimal_reasoning", "最低限度推理", "最低程度推理", "Minimal reasoning"),
    ("thinking.low_reasoning", "低强度推理", "低強度推理", "Low reasoning"),
    ("thinking.medium_reasoning", "中等强度推理", "中等強度推理", "Medium reasoning"),
    ("thinking.high_reasoning", "高强度推理", "高強度推理", "High reasoning"),
    ("thinking.extra_high_reasoning", "超高强度推理", "超高強度推理", "Extra-high reasoning"),
    ("thinking.max_reasoning", "最高强度推理", "最高強度推理", "Max reasoning"),
    ("tools.from_default_tools", "取自 settings.json 的 defaultTools", "取自 settings.json 的 defaultTools", "From settings.json defaultTools"),
    ("thinking.chat_only", "仅聊天", "僅聊天", "Chat only"),
    ("tools.preset_readonly_count", "4 个只读内置工具", "4 個只讀內置工具", "4 read-only built-in tools"),
    ("tools.preset_full_count", "4 个内置工具", "4 個內置工具", "4 built-in tools"),
    ("thinking.all_built_in_tools", "全部内置工具", "全部內置工具", "All built-in tools"),
    // 034/040 会话扩展清单（2026-10 改版：档名 full+、勾选即生效）
    ("extensions.full", "full+", "full+", "full+"),
    ("tools.preset_full_desc", "full + 自定义扩展", "full + 自訂擴展", "full + custom extensions"),
    ("extensions.choose_extensions", "选择扩展", "選擇擴展", "Choose extensions"),
    ("extensions.builtin_all_desc", "全部内置工具，无扩展", "全部內置工具，无擴展", "All built-in tools ,no extensions"),
    ("extensions.list_next_turn", "扩展清单将在下一轮生效", "擴展清單將在下一輪生效", "The extension list takes effect on the next turn"),
    ("extensions.list_save_failed", "扩展清单保存失败: {e}", "擴展清單儲存失敗: {e}", "Failed to save the extension list: {e}"),
    // 034 扩展按钮 + 勾选菜单
    ("extensions.only_full_preset", "扩展只能在 full+ 档勾选", "擴展只能在 full+ 檔勾選", "Extensions can only be picked in the full+ preset"),
    // 040 说明 token 预览（延迟加载）
    ("tokens.prompt_size", "说明大小", "說明大小", "Prompt size"),
    ("tokens.sum", "合计：", "合計：", "Total:"),
    ("nav.work_details", "工作详情 · {m} 条消息 · {t} 次工具调用", "工作詳情 · {m} 條消息 · {t} 次工具調用", "Work details · {m} messages · {t} tool calls"),
    ("nav.live_tail_skipped", "…本回合还有 {n} 条消息（轮末在工作详情展开）", "…本回合還有 {n} 條消息（輪末在工作詳情展開）", "…{n} more messages this turn (expand in work details when done)"),
    ("mcp.enabled_count", "已开启 {n}/{total}", "已開啟 {n}/{total}", "{n}/{total} enabled"),
    ("models.registry_read_failed", "读取 Provider 注册表失败：{e}", "讀取 Provider 註冊表失敗：{e}", "Failed to read Provider registry: {e}"),
    ("models.available_count", "可用模型 {enabled}/{total}", "可用模型 {enabled}/{total}", "Available models {enabled}/{total}"),
    ("models.free_only", "只看free", "只看free", "Free only"),
    ("models.json_read_failed", "无法读取 models.json，为避免覆盖已禁用保存：{e}", "無法讀取 models.json，為避免覆蓋已停用儲存：{e}", "Cannot read models.json; save skipped to avoid overwriting disabled entries: {e}"),
    ("skills.failed_to_toggle_count", "{total} 个技能中有 {count} 个未能更改", "{total} 個技能中有 {count} 個未能更改", "{count} of {total} skills failed to toggle"),
    ("status.running_tools_with_extra", "正在运行 {names}（另有 {count} 个）...", "正在運行 {names}（另有 {count} 個）...", "Running {names} (and {count} more)..."),
    ("messages.no_output", "（无输出）", "（無輸出）", "(no output)"),
    // 031 @ 文件检索 + ! shell 命令
    ("composer.files", "文件", "檔案", "Files"),
    ("composer.slash_commands", "斜杠命令", "斜槓命令", "Slash commands"),
    ("composer.no_matching_files", "没有匹配的文件", "沒有符合的檔案", "No matching files"),
    ("statusbar.no_project", "未选择项目", "未選擇專案", "No project"),
    ("composer.shell_local_only", "Shell · 输出仅本地（!!）", "Shell · 輸出僅本機（!!）", "Shell · output stays local (!!)"),
    ("composer.shell_to_model", "Shell · 输出发给模型", "Shell · 輸出傳送給模型", "Shell · output sent to model"),
    ("composer.running_hint", "执行中 · Esc 中止", "執行中 · Esc 中止", "Running · Esc to abort"),
    ("composer.shell_busy", "会话忙，无法执行 shell 命令", "會話忙碌，無法執行 shell 命令", "Session is busy — can't run a shell command"),
    ("composer.shell_empty", "命令为空：! 后面接 shell 命令", "命令為空：! 後面接 shell 命令", "Empty command — type a shell command after !"),
    ("status.compacting", "压缩中…", "壓縮中…", "Compacting…"),
    ("status.auto_retry", "出错，{secs}s 后自动重试（第 {attempt}/{max} 次）", "出錯，{secs}s 後自動重試（第 {attempt}/{max} 次）", "Error — retrying in {secs}s (attempt {attempt}/{max})"),
    ("status.pi_exited", "pi 进程已退出", "pi 程式已結束", "pi process exited"),
    ("status.pi_exited_code", "pi 进程已退出（code {code}）", "pi 程式已結束（code {code}）", "pi process exited (code {code})"),
    ("status.auto_compacting", "上下文自动压缩中…", "上下文自動壓縮中…", "Auto-compacting context…"),
    ("status.auto_compact_failed", "自动压缩失败：{error}", "自動壓縮失敗：{error}", "Auto-compaction failed: {error}"),
    ("status.summarization_retry", "上下文溢出，摘要重试：{secs}s 后第 {attempt}/{max} 次", "上下文溢出，摘要重試：{secs}s 後第 {attempt}/{max} 次", "Context overflow — summary retry in {secs}s (attempt {attempt}/{max})"),
    ("status.extension_error", "扩展错误：{error}", "擴充錯誤：{error}", "Extension error: {error}"),
    ("messages.tool_interrupted", "pi 进程中断，工具调用未返回", "pi 程式中斷，工具呼叫未返回", "pi process died before the tool returned"),
    ("composer.compacting_wait", "上下文压缩中，请稍等……", "上下文壓縮中，請稍等……", "Compacting context, please wait…"),
    ("composer.generating_title", "生成标题…", "生成標題…", "Generating title…"),
    ("composer.title_set", "已生成标题: {title}", "已生成標題: {title}", "Title set: {title}"),
    ("composer.title_failed", "标题生成失败: {e}", "標題生成失敗: {e}", "Title generation failed: {e}"),
    // exit banner
    ("exit.process_exited", "Process exited with code {code_text}", "Process exited with code {code_text}", "Process exited with code {code_text}"),
    ("exit.unknown", "unknown", "unknown", "unknown"),
    // git badges etc stay ASCII
    // ---- 以下为存量漏译：繁/英待补，tr() 回落中文 ----
    ("actions.edit", "编辑", "編輯", "Edit"),
    ("branch.detour_note", "*此对话曾短暂探索另一分支后返回，摘要如下：*", "", ""),
    ("composer.compacting_wait2", "上下文压缩中，请等待……", "", ""),
    ("composer.hint_esc", "ESC 叫停", "", ""),
    ("composer.hint_queue", "排队发送，ESC叫停", "排队发送，ESC叫停", "Queue follow-up · ESC to stop"),
    ("composer.hint_steer", "指引介入 · ESC叫停", "指引介入 · ESC 叫停", "Steer insert · ESC to stop"),
    ("composer.hint_shortcuts", "/skill，@文件，Shift回车换行", "", ""),
    ("custom_models.add", "添加自定义 Provider", "添加自定義 Provider", "Add custom provider"),
    ("custom_models.base_url_hint", "留空 Base URL 使用 pi 内置端点；API Key 支持 ENV 变量", "留空 Base URL 使用 pi 內置端點；API Key 支持 ENV 變量", "Leave Base URL blank to use pi built-in endpoint; API Key supports ENV variables"),
    ("custom_models.configure", "配置 Provider", "配置 Provider", "Configure provider"),
    ("custom_models.endpoint_hint", "OpenAI / Anthropic 兼容端点；留空 Base URL 使用 pi 内置端点", "OpenAI / Anthropic 兼容端點；留空 Base URL 使用 pi 內置端點", "OpenAI / Anthropic compatible endpoint; leave Base URL blank to use the pi built-in endpoint"),
    ("custom_models.exists", "已存在名为「{name}」的 Provider", "已存在名為「{name}」的 Provider", "A provider named \"{name}\" already exists"),
    ("custom_models.field_context", "上下文窗口（tokens）", "上下文窗口（tokens）", "Context window (tokens)"),
    ("custom_models.field_id", "ID", "ID", "ID"),
    ("custom_models.field_name", "Provider 名称", "Provider 名稱", "Provider name"),
    ("custom_models.key_hint", "ENV_VAR_NAME、!shell-command 或明文 key", "ENV_VAR_NAME、!shell-command 或明文 key", "ENV_VAR_NAME, !shell-command, or a plaintext key"),
    ("custom_models.model_id_required", "模型 ID 不能为空", "模型 ID 不能為空", "Model ID must not be empty"),
    ("custom_models.name_invalid", "Provider 名称只能使用字母、数字、_ 和 -", "Provider 名稱只能使用字母、數字、_ 和 -", "Provider name may only contain letters, digits, _ and -"),
    ("custom_models.name_required", "Provider 名称不能为空", "Provider 名稱不能為空", "Provider name must not be empty"),
    ("custom_models.new_model", "新模型", "新模型", "New model"),
    ("custom_models.reasoning_badge", "推理模型（T 徽标）", "推理模型（T 徽標）", "Reasoning model (T badge)"),
    ("dock.mcp", "MCP", "MCP", "MCP"),
    ("dock.other", "其他", "其他", "Other"),
    ("extensions.source_example", "例：pi install npm:pi-web-access", "例：pi install npm:pi-web-access", "e.g. pi install npm:pi-web-access"),
    ("file_tree.search", "搜索文件", "搜尋檔案", "Search files"),
    ("file_view.image_read_failed", "图片读取失败", "", ""),
    ("git.clean", "工作区干净", "工作區乾淨", "Working tree clean"),
    ("git.commit_placeholder", "提交信息（提交已暂存更改）", "", ""),
    ("git.no_commits", "暂无提交", "", ""),
    ("git.pushed", "推送完成", "", ""),
    ("git.pushing", "推送中…", "", ""),
    ("git.stage_all", "Stage All", "Stage All", "Stage All"),
    ("git.staged", "提交已暂存", "", ""),
    ("mcp.closed", "已关闭", "", ""),
    ("mcp.failed_count", "{count} 个服务器未能更改", "", ""),
    ("mcp.field_name", "MCP名称", "MCP名稱", "MCP name"),
    ("mcp.form_placeholder", "粘贴 JSON / url / 命令行 / pi mcp add...", "粘貼 JSON / url / 命令行 / pi mcp add...", "Paste JSON / url / command line / pi mcp add..."),
    ("mcp.form_title", "MCP配置", "MCP配置", "MCP configuration"),
    ("mcp.name_invalid", "服务器名称只能使用字母、数字、_ 和 -", "伺服器名稱只能使用字母、數字、_ 和 -", "Server name can only use letters, digits, _ and -"),
    ("mcp.next_turn", "MCP 清单将在下一轮生效", "MCP 清單將在下一輪生效", "MCP list takes effect on next turn"),
    ("mcp.none_configured", "没有已配置的 MCP 服务器", "沒有已配置的 MCP 伺服器", "No MCP servers configured"),
    ("mcp.save_failed", "MCP 保存失败: {e}", "MCP 保存失敗: {e}", "MCP save failed: {e}"),
    ("mcp.saved_next_session", "已保存，对新会话生效", "已保存，對新會話生效", "Saved, takes effect on new sessions"),
    ("mcp.select", "选择 MCP", "選擇 MCP", "Select MCP"),
    ("menu.no_command_found", "未找到扩展、提示词或技能命令", "", ""),
    ("menu.tab_enter_insert", "Tab / Enter 插入", "", ""),
    ("messages.before", "之前", "", ""),
    ("messages.extended_hidden", "扩展消息（不显示）", "", ""),
    ("messages.generating_args", "正在生成参数...", "", ""),
    ("messages.too_large", "消息内容过大，已按纯文本显示", "", ""),
    ("messages.truncated", "回复因达到模型输出长度上限而被截断。发送一条后续消息以继续。", "", ""),
    ("model.add_provider", "添加 Provider", "添加 Provider", "Add provider"),
    ("model.count", "{n} models", "{n} models", "{n} models"),
    ("model.keep_one", "至少需要保留一个启用的模型", "至少需要保留一個啟用的模型", "At least one model must stay enabled"),
    ("model.key_not_in_vault", "密钥不在系统凭据库，请重新保存", "密鑰不在系統憑據庫，請重新保存", "Key is not in the system credential vault; please save it again"),
    ("model.loading_registry", "正在读取 Provider 注册表…", "正在讀取 Provider 註冊表…", "Loading provider registry…"),
    ("model.no_provider_match", "没有匹配的 Provider", "沒有匹配的 Provider", "No matching providers"),
    ("model.oauth_unavailable", "OAuth 登录暂未接入，订阅服务商暂时无法在此配置", "OAuth 登入暫未接入，訂閱服務商暫時無法在此配置", "OAuth sign-in is not available yet; subscription providers can't be configured here"),
    ("model.search_provider", "搜索 Provider...", "搜尋 Provider…", "Search providers…"),
    ("pi_extensions.copy_install", "复制安装命令，做全局安装", "複製安裝命令，做全域安裝", "Copy install command for global installation"),
    ("pi_extensions.from", "从", "從", "From"),
    ("pi_extensions.state", "状态", "狀態", "State"),
    ("psp.confirm_delete", "确认删除", "", ""),
    ("psp.delete_project", "删除所有会话移除项目", "删除所有会话移除项目", "delete all sessions and remove project"),
    ("psp.list_mode", "列表方式", "", ""),
    ("psp.maximize_session_list", "最大化会话列表", "最大化會話列表", "Maximize session list"),
    ("psp.messages_count", "{n} 条消息", "", ""),
    ("psp.new_session", "新建会话", "新建會話", "New session"),
    ("psp.new_session_in_project", "在此项目新建会话", "在此專案新建會話", "New session in project"),
    ("psp.no_sessions", "无会话", "", ""),
    ("psp.open_in_file_browser", "在文件浏览器中打开", "", ""),
    ("psp.open_project", "打开项目", "開啟專案", "Open project"),
    ("psp.open_session_dir", "在资源管理器中打开会话目录", "在檔案總管中開啟會話目錄", "Open sessions folder in Explorer"),
    ("psp.pin", "置顶", "置頂", "Pin"),
    ("psp.pinned_group", "置顶", "置頂", "Pinned"),
    ("psp.project_deleted", "项目已删除（{n} 个会话）", "", ""),
    ("psp.project_groups", "项目分组列表", "", ""),
    ("psp.recent_sessions", "最近会话列表", "", ""),
    ("psp.search_sessions", "搜索会话", "搜尋會話", "Search sessions"),
    ("psp.session_label", "会话", "會話", "Sessions"),
    ("psp.sessions_count", "{n} 个会话", "{n} 個會話", "{n} sessions"),
    ("session.autoclean_done", "已自动清理 {n} 个超期会话（移入回收站）", "已自動清理 {n} 個超期會話（移入資源回收筒）", "Auto-cleaned {n} stale session(s) (moved to recycle bin)"),
    ("psp.sort", "排序", "排序", "Sort"),
    ("psp.sort_by_time", "按更新时间排序", "", ""),
    ("psp.sort_manual", "手动排序", "", ""),
    ("psp.sort_mode", "排序方式", "", ""),
    ("psp.split_tip", "拖拽调整上下比例（双击复位 50%）", "拖曳調整上下比例（雙擊復位 50%）", "Drag to resize (double-click to reset)"),
    ("psp.switch_to_flat", "切换为最近会话列表", "切換為最近會話列表", "Switch to recent sessions"),
    ("psp.switch_to_grouped", "切换为项目分组列表", "切換為專案分組列表", "Switch to project groups"),
    ("psp.unpin", "取消置顶", "取消置頂", "Unpin"),
    ("remote.panel_title", "微信通信", "微信通信", "WeChat"),
    ("remote.qr_confirm", "已扫码，请在手机上确认…", "已掃碼，請在手機上確認…", "Scanned, please confirm on your phone…"),
    ("remote.qr_desc", "用手机微信扫码，绑定这台机器的 pi-flash。", "用手機微信掃碼，綁定這台機器的 pi-flash。", "Scan with WeChat to bind this pi-flash machine."),
    ("remote.qr_hint", "用手机微信「扫一扫」对屏扫码；约 2 分钟后过期。", "用手機微信「掃一掃」對屏掃碼；約 2 分鐘後過期。", "Scan the on-screen QR code with WeChat; expires in about 2 minutes."),
    ("remote.qr_install_hint", "若微信提示未安装：请先在微信安装「微信ClawBot」插件，再重新扫码。", "若微信提示未安裝：請先在微信安裝「微信ClawBot」外掛，再重新掃碼。", "If WeChat says the bot is not installed: install the \"WeChat ClawBot\" plugin in WeChat first, then scan again."),
    // 060 七态生命周期（2026-10-24 用户定夺）：①未扫描 ②微信侧出错
    // ③重新扫码更新重建 ④我方创建出错 ⑤创建成功 ⑥启用中 ⑦关停中
    ("remote.st_absent", "未创建", "未建立", "Not created"),
    ("remote.st_rebinding", "更新中", "更新中", "Updating"),
    ("remote.st_write_failed", "状态写入失败", "狀態寫入失敗", "State write failed"),
    ("remote.title_error", "扫码出错", "掃碼出錯", "Scan failed"),
    ("remote.title_rebind", "更新绑定", "更新綁定", "Rebinding"),
    ("remote.title_create_failed", "创建出错", "建立出錯", "Creation failed"),
    ("remote.title_running", "渠道运行中", "頻道運行中", "Channel running"),
    ("remote.hint_rebind", "重新扫码后自动替换旧 Bot 并重启渠道。", "重新掃碼後自動替換舊 Bot 並重啟頻道。", "After a re-scan the old bot is replaced and the channel restarts automatically."),
    ("remote.hint_starting", "pf-wxbot 创建成功，正在启动渠道…", "pf-wxbot 建立成功，正在啟動頻道…", "pf-wxbot created. Starting the channel…"),
    ("remote.hint_create_failed", "pf-wxbot 创建失败：{err}", "pf-wxbot 建立失敗：{err}", "pf-wxbot creation failed: {err}"),
    ("remote.qr_loading", "正在获取二维码…", "正在獲取二維碼…", "Getting QR code…"),
    ("remote.qr_retry", "扫码连接失败，请刷新重扫。", "掃碼連接失敗，請刷新重掃。", "Scan connection failed. Refresh and try again."),
    // 002-微信通信（pi-flash-plugins）版式文案
    ("remote.hint_absent", "请用微信扫描二维码，创建Pi-Flash微信机器人", "請用微信掃描二維碼，建立Pi-Flash微信機器人", "Scan the QR code with WeChat to create the Pi-Flash bot"),
    ("remote.hint_wx_error", "微信侧返回错误，请确认微信已安装「ClawBot插件」", "微信側返回錯誤，請確認微信已安裝「ClawBot外掛」", "WeChat returned an error. Make sure the ClawBot plugin is installed in WeChat"),
    ("remote.qr_created", "Pi-Flash微信机器人已创建", "Pi-Flash微信機器人已建立", "The Pi-Flash WeChat bot has been created"),
    ("remote.qr_timeout", "扫描超时，请点击重新扫码", "掃描逾時，請點擊重新掃碼", "Scan timed out. Tap to scan again"),
    ("remote.qr_regen", "重新生成二维码", "重新產生二維碼", "Regenerate QR code"),
    ("remote.qr_title", "扫码绑定", "掃碼綁定", "Scan to bind"),
    ("remote.select_provider", "选择模型供应商", "選擇模型供應商", "Select model provider"),
    ("remote.select_thinking", "选择思考档位", "選擇思考檔位", "Select thinking tier"),
    ("remote.switch_session", "切换会话", "切換會話", "Switch session"),
    ("remote.switch_workspace", "切换工作区", "切換工作區", "Switch workspace"),
    ("session_search.tab", "会话查询", "會話查詢", "Session Search"),
    ("session_search.title", "搜索会话", "搜尋會話", "Search Sessions"),
    ("session_search.no_match_hint", "换个关键词，或确认会话已落盘", "換個關鍵詞，或確認會話已落盤", "Try another keyword, or make sure the session is saved"),
    ("session_search.hint", "输入关键词，搜索全部会话内容（跨项目）", "輸入關鍵詞，搜尋全部會話內容（跨專案）", "Type a keyword to search all sessions (every project)"),
    ("session_search.no_match", "没有匹配结果", "沒有符合結果", "No matching results"),
    ("session_search.placeholder", "搜索会话内容…", "搜尋會話內容…", "Search session content…"),
    ("session_search.searching", "搜索中…", "搜尋中…", "Searching…"),
    ("settings.data_dir", "工作区数据目录", "工作區數據目錄", "Workspace data directory"),
    ("settings.memory_dir", "会话记忆 / 布局状态存放位置", "會話記憶 / 佈局狀態存放位置", "Session memory / layout state location"),
    ("settings.restore_hint", "启动时恢复上次激活的项目与会话", "啟動時恢復上次激活的項目與會話", "Restore the last active project and session at startup"),
    ("settings.restore_title", "启动恢复", "啟動恢復", "Startup restore"),
    // 082 设置「快捷键」页
    ("shortcuts.count", "{n} 条", "{n} 條", "{n} keys"),
    ("shortcuts.group_file_tree", "文件树", "檔案樹", "File tree"),
    ("shortcuts.group_global", "全局", "全域", "Global"),
    ("shortcuts.group_markdown", "Markdown 预览", "Markdown 預覽", "Markdown preview"),
    ("shortcuts.group_terminal", "终端", "終端", "Terminal"),
    ("shortcuts.md_scroll", "滚动预览", "滾動預覽", "Scroll the preview"),
    ("shortcuts.md_scroll_hint", "↑/↓ 三行，PgUp/PgDn 翻页，Home/End 首尾（预览持焦时生效）", "↑/↓ 三行，PgUp/PgDn 翻頁，Home/End 首尾（預覽持焦時生效）", "↑/↓ three lines, PgUp/PgDn page, Home/End ends (when the preview has focus)"),
    ("shortcuts.rename_hint", "选中文件或目录时", "選中檔案或目錄時", "When a file or folder is selected"),
    ("shortcuts.search_files", "搜索文件", "搜尋檔案", "Search files"),
    ("shortcuts.term_word_jump_hint", "直接发送到终端；Ctrl+←/→ 已留给切标签（任意焦点可切）", "直接發送到終端；Ctrl+←/→ 已留給切標籤（任意焦點可切）", "Sent to the terminal; Ctrl+←/→ is reserved for tab switching under any focus"),
    ("shortcuts.term_word_jump_left", "跳词向左", "跳詞向左", "Jump word left"),
    ("shortcuts.term_word_jump_right", "跳词向右", "跳詞向右", "Jump word right"),
    ("shortcuts.title", "键盘快捷键", "鍵盤快捷鍵", "Keyboard Shortcuts"),
    ("shortcuts.view_left", "向左切换视图", "向左切換視圖", "Switch view left"),
    ("shortcuts.view_right", "向右切换视图", "向右切換視圖", "Switch view right"),
    ("stats.cache_hit_rate", "缓存命中率", "快取命中率", "Cache hit rate"),
    ("stats.cache_read", "缓存读", "快取讀", "Cache read"),
    ("stats.cache_write", "缓存写", "快取寫", "Cache write"),
    ("stats.context_capacity", "上下文容量", "上下文容量", "Context usage"),
    ("stats.context_tokens", "上下文 token", "上下文 token", "Context tokens"),
    ("stats.cost", "费用", "費用", "Cost"),
    ("stats.duration", "用时", "用時", "Duration"),
    ("stats.input", "输入", "輸入", "Input"),
    ("stats.messages", "消息", "訊息", "Messages"),
    ("stats.no_usage", "会话尚未产生用量。", "會話尚未產生用量。", "No usage recorded for this session yet."),
    ("stats.share", "使用比例", "使用比例", "Share"),
    ("stats.tokens_total", "Token 累计", "Token 累計", "Total tokens"),
    ("stats.total_cost", "累计费用", "累計費用", "Total cost"),
    ("status.running_tool", "正在运行 {name}...", "正在運行 {name}...", "Running {name}..."),
    ("status.running_tools", "正在运行 {names}...", "正在運行 {names}...", "Running {names}..."),
    ("status.queued", "已排队（本轮结束后发送）", "已排隊（本輪結束後發送）", "Queued (sends after the current turn)"),
    ("status.queued_n", "已排队 {count} 条（本轮结束后发送）", "已排隊 {count} 條（本輪結束後發送）", "Queued: {count} (sends after the current turn)"),
    ("queue.send_now", "立即", "立即", "Send now"),
    ("queue.send_now_tip", "中止当前回复，这条立刻作为新一轮发出", "中止當前回覆，這條立刻作為新一輪發出", "Abort the current reply and send this as a new turn now"),
    ("queue.full", "缓冲栏已有排队消息，请先处理卡片；或 Ctrl+Enter 立即打断", "緩衝欄已有排隊訊息，請先處理卡片；或 Ctrl+Enter 立即打斷", "Buffer already holds a queued message — handle the card first, or Ctrl+Enter to interrupt now"),
    ("queue.edit", "撤回编辑（内容回输入框）", "撤回編輯（內容回輸入框）", "Withdraw to composer for editing"),
    ("queue.remove", "移除排队消息", "移除排隊訊息", "Remove queued message"),
    ("queue.withdraw_conflict", "请先发送或清空当前草稿，再编辑队列消息。", "請先發送或清空當前草稿，再編輯排隊訊息。", "Send or clear the current draft before editing a queued message."),
    ("queue.pushed_back", "排队消息暂未发出（会话忙），已放回队列", "排隊訊息暫未發出（會話忙），已放回佇列", "Queued message not sent yet (session busy) — put back in queue"),
    ("status.steered", "已插话（下一次模型调用前注入）", "已插話（下一次模型調用前注入）", "Steered (injected before the next model call)"),
    ("status.recalled_queued", "已撤回 {count} 条排队消息（文本回到输入框）", "已撤回 {count} 條排隊訊息（文字回到輸入框）", "Recalled {count} queued message(s) back to the composer"),
    ("status.steer_dropped", "已中止：{count} 条插话未送达模型，已从队列撤下", "已中止：{count} 條插話未送達模型，已從佇列撤下", "Aborted: {count} steered message(s) never reached the model and were dropped"),
    ("status.send_failed", "发送失败", "發送失敗", "Send failed"),
    ("status.waiting_model", "正在等待模型...", "正在等待模型...", "Waiting for model..."),
    ("terminal.none", "暂无终端会话", "暫無終端會話", "No terminal sessions"),
    ("titlebar.expand_sidebar", "展开侧栏", "展開側欄", "Expand sidebar"),
    ("titlebar.toggle_sidebar", "收起 / 展开侧栏", "收起 / 展開側欄", "Collapse / expand sidebar"),
    ("tools.preset_rebound", "工具预设: {k} (会话进程已重绑)", "工具預設: {k} (會話進程已重綁)", "Tool preset: {k} (session process rebound)"),
    ("remote.enable", "启用微信 Bot", "啟用微信 Bot", "Enable WeChat Bot"),
    ("remote.enable_hint", "关停会立刻停掉长轮询线程并释放轮询锁（无悬挂线程）。", "關閉會立刻停掉長輪詢線程並釋放輪詢鎖（無懸掛線程）。", "Turning off stops the long-poll thread and releases the lock immediately (no dangling threads)."),
    ("remote.channel", "渠道：{status}", "頻道：{status}", "Channel: {status}"),
    ("remote.status_running", "运行中", "運行中", "Running"),
    ("remote.status_stopped", "已停止（有 token）", "已停止（有 token）", "Stopped (has token)"),
    ("remote.status_not_started", "未启动", "未啟動", "Not started"),
    ("remote.poll_error", "最近错误：{err}", "最近錯誤：{err}", "Last error: {err}"),
    ("remote.binding", "绑定：{status}", "綁定：{status}", "Binding: {status}"),
    ("remote.bound_scanned", "已扫码", "已掃碼", "Scanned"),
    ("remote.bound_not_scanned", "未扫码", "未掃碼", "Not scanned"),
    ("remote.bind_code", "绑定码：{code}", "綁定碼：{code}", "Binding code: {code}"),
    ("remote.bound_session", "已绑定会话：{k}", "已綁定會話：{k}", "Bound session: {k}"),
    ("remote.bound_unbound", "未绑定 —— 在微信里发 /bind 上面这串数字", "未綁定 —— 在微信裡發 /bind 上面這串數字", "Not bound — send /bind <code> in WeChat"),
    ("remote.field_status", "状态", "狀態", "Status"),
    ("remote.field_binding", "绑定", "綁定", "Binding"),
    ("remote.qr_begin", "获取二维码", "取得二維碼", "Get QR code"),
    ("remote.qr_refresh", "刷新重扫", "刷新重掃", "Refresh & rescan"),
    ("remote.qr_done", "✅ 绑定成功", "✅ 綁定成功", "✅ Bound successfully"),
    ("remote.qr_help", "发送 /帮助 查看可用命令。", "發送 /幫助 查看可用命令。", "Send /help to see available commands."),
    ("remote.qr_expired", "二维码已过期，请刷新重扫", "二維碼已過期，請刷新重掃", "QR code expired. Refresh and rescan."),
    // 060 微信通信页两栏重构（2026-10-24）：左「扫码连接」/右「Bot 渠道」；
    // 渠道卡只保留微信（060 范围无飞书/Lark/Telegram）
    ("remote.scan_title", "手机扫码连接", "手機掃碼連接", "Scan with Phone"),
    ("remote.scan_desc", "用手机微信扫码，在微信里使用这个工作区。", "用手機微信掃碼，在微信裡使用這個工作區。", "Scan with WeChat to use this workspace on your phone."),
    ("remote.scan_wait", "等待手机连接", "等待手機連接", "Waiting for phone"),
    ("remote.scan_wait_hint", "在微信里给 Bot 发消息即可连接。", "在微信裡給 Bot 發訊息即可連接。", "Send the bot a message on WeChat to connect."),
    ("remote.scan_stopped", "渠道已停止", "頻道已停止", "Channel stopped"),
    ("remote.scan_stopped_hint", "启动渠道后恢复与微信的连接。", "啟動頻道後恢復與微信的連接。", "Start the channel to resume the WeChat connection."),
    ("remote.scan_begin_hint", "获取二维码后，用微信扫码绑定。", "取得二維碼後，用微信掃碼綁定。", "Get a QR code, then scan it with WeChat to bind."),
    ("remote.scan_stop", "停止", "停止", "Stop"),
    ("remote.scan_start", "启动", "啟動", "Start"),
    ("remote.st_getting", "获取中", "獲取中", "Getting"),
    ("remote.st_ready", "已就绪", "已就緒", "Ready"),
    ("remote.st_bound", "已绑定", "已綁定", "Bound"),
    ("remote.st_expired", "已过期", "已過期", "Expired"),
    ("remote.st_failed", "失败", "失敗", "Failed"),
    ("remote.st_disabled", "未启用", "未啟用", "Disabled"),
    ("remote.bot_title", "使用 Bot Channel", "使用 Bot Channel", "Use a Bot Channel"),
    ("remote.bot_desc", "连接聊天 Bot，适合更长时间的移动端访问。", "連接聊天 Bot，適合更長時間的移動端存取。", "Connect a chat bot for longer mobile access."),
    ("remote.wx_name", "微信", "微信", "WeChat"),
    ("remote.wx_desc", "在微信上安装「微信ClawBot」插件，使用微信远程指挥Pi-Flash工作。", "在微信上安裝「微信ClawBot」外掛，使用微信遠程指揮Pi-Flash工作。", "Install the \"WeChat ClawBot\" plugin on WeChat to command Pi-Flash remotely."),
    ("remote.channel_enable", "启用渠道", "啟用頻道", "Enable channel"),
    ("remote.channel_disable", "停用渠道", "停用頻道", "Disable channel"),
    ("remote.bot_mgmt", "机器人管理", "機器人管理", "Bot management"),
    ("remote.alive_hint", "Bot 随本应用存活：退出 pi-flash 即停止。", "Bot 隨本應用存活：退出 pi-flash 即停止。", "The bot lives with this app: it stops when pi-flash exits."),
    ("settings.filter_models", "筛选模型…", "篩選模型…", "Filter models…"),
    ("settings.filter_enabled", "筛选启用的模型…", "篩選已啟用的模型…", "Filter enabled models…"),
    ("settings.model_display_name", "显示名（可选）", "顯示名（可選）", "Display name (optional)"),
    ("settings.tab.mcp", "MCP", "MCP", "MCP"),
    ("settings.tab.misc", "其他", "其他", "Misc"),
    ("settings.tab.shortcuts", "快捷键", "快捷鍵", "Shortcuts"),
    ("settings.tab.about", "关于", "關於", "About"),
    ("custom_models.base_url", "Base URL", "Base URL", "Base URL"),
    ("custom_models.api_key", "API Key", "API Key", "API Key"),
    ("custom_models.api", "API", "API", "API"),
    ("skills.description_tokens", "描述：{desc} · 正文：{tokens} tokens", "描述：{desc} · 正文：{tokens} tokens", "Description: {desc} · Body: {tokens} tokens"),
    ("skills.injected", "注入系统提示词；关闭后取消注入，靠手动调用", "注入系統提示詞；關閉後取消注入，靠手動調用", "Injected system prompt; turn off to cancel injection, invoke manually"),
    ("skills.not_injected", "未注入系统提示词，靠手动调用", "未注入系統提示詞，靠手動調用", "No system prompt injected, invoke manually"),
    ("misc.window_7d", "7 天", "7 天", "7 days"),
    ("misc.window_14d", "14 天", "14 天", "14 days"),
    ("misc.window_30d", "30 天", "30 天", "30 days"),
    ("misc.collapse_1d", "1 天", "1 天", "1 day"),
    ("misc.collapse_3d", "3 天", "3 天", "3 days"),
    ("misc.collapse_7d", "7 天", "7 天", "7 days"),
    ("dialogs.subscription_services", "订阅服务", "訂閱服務", "Subscription services"),
    ("dialogs.search_results", "{} 条结果", "{} 條結果", "{} results"),
    ("file_tree.no_files", "暂无文件", "暫無檔案", "No files"),
    ("theme.mist", "雾青", "霧青", "Mist"),
    ("theme.rose", "蔷薇", "蔷薇", "Rose"),
    ("theme.latte", "薰衣草", "薰衣草", "Latte"),
    ("theme.kitchen", "秋叶", "秋葉", "Autumn Leaves"),
    ("theme.pine", "松林", "松林", "Pine"),
    ("git.view_diff", "View Diff", "檢視差異", "View Diff"),
    ("action.close", "×", "×", "×"),
    ("action.confirm_mark", "✓", "✓", "✓"),
    ("status.alert", "!", "!", "!"),
    ("session.separator", "|", "|", "|"),
    ("session.shortcut_a", "A", "A", "A"),
    ("session.slash_prefix", "/", "/", "/"),
    ("shortcuts.plus", "+", "+", "+"),
    ("loading.ellipsis", "...", "...", "..."),
    ("markdown.unnamed", "未命名", "未命名", "Untitled"),
    ("send.shell_aborted", "shell 中止", "shell 中止", "shell aborted"),
    ("send.shell_done", "shell 完成", "shell 完成", "shell done"),
    ("send.shell_exit", "shell 退出码 {code}", "shell 退出碼 {code}", "shell exit code {code}"),
    ("custom_models.reasoning_letter", "T", "T", "T"),
    ("remote.qr_gen_failed", "二维码生成失败：{e}", "二維碼產生失敗：{e}", "Failed to generate QR code: {e}"),
    // -- 子代理页（044-1 §2）
    ("agents.builtin_group", "内置", "內建", "Built-in"),
    ("agents.custom_group", "自定义", "自訂", "Custom"),
    ("agents.new", "新建", "新增", "New"),
    ("agents.name", "名称", "名稱", "Name"),
    ("agents.desc", "描述", "描述", "Description"),
    ("agents.model", "模型", "模型", "Model"),
    ("agents.tools", "工具权限", "工具權限", "Tool access"),
    ("agents.prompt", "系统提示", "系統提示", "System prompt"),
    ("agents.prompt_ph", "追加到默认人格的提示词（留空则不追加）", "追加到預設人格的提示詞（留空則不追加）", "Prompt appended to the default persona (leave empty for none)"),
    ("agents.max_turns", "最大轮次", "最大輪次", "Max turns"),
    ("agents.inject_agents_md", "注入AGENTS.md", "注入AGENTS.md", "Inject AGENTS.md"),
    ("agents.inherit", "继承", "繼承", "Inherit"),
    ("agents.inherit_tools", "继承委派者", "繼承委派者", "Inherit from delegator"),
    ("agents.unlimited", "不限", "不限", "Unlimited"),
    ("agents.specify", "指定", "指定", "Custom"),
    ("agents.save", "保存", "儲存", "Save"),
    ("agents.delete", "删除", "刪除", "Delete"),
    ("agents.copy_custom", "复制为自定义", "複製為自訂", "Copy to custom"),
    ("agents.readonly_builtin", "内置 · 只读", "內建 · 唯讀", "Built-in · read-only"),
    ("agents.overrides_builtin", "覆盖内置", "覆蓋內建", "Overrides built-in"),
    ("agents.broken", "此定义已损坏，无法解析", "此定義已損毀，無法解析", "This definition is broken and cannot be parsed"),
    ("agents.open_fix", "打开文件修复", "開啟檔案修復", "Open file to fix"),
    ("agents.empty", "还没有自定义子代理——点上方「新建」创建一个", "還沒有自訂子代理——點上方「新增」建立一個", "No custom subagents yet — click New above to create one"),
    ("agents.saved", "已保存", "已儲存", "Saved"),
    ("agents.name_invalid", "名称只能含字母、数字、连字符与下划线", "名稱只能含字母、數字、連字號與底線", "Name may only contain letters, digits, hyphens and underscores"),
    // -- 子代理面板（044-1 §1）
    ("subagent.panel", "子代理面板", "子代理面板", "Subagent panel"),
    ("subagent.empty", "本会话还没有子代理——在对话里让我委派任务即可", "本會話還沒有子代理——在對話裡讓我委派任務即可", "No subagents in this session yet — delegate a task in the conversation"),
    ("subagent.open_child", "打开子会话", "開啟子會話", "Open child session"),
    ("subagent.running", "运行中", "執行中", "Running"),
    ("subagent.done", "完成", "完成", "Done"),
    ("subagent.aborted", "已中止", "已中止", "Aborted"),
    ("subagent.max_turns", "轮次用尽", "輪次用盡", "Out of turns"),
    ("subagent.failed", "失败", "失敗", "Failed"),
    ("subagent.n_agents", "个子代理", "個子代理", "sub-agents"),
    ("subagent.total", "合计", "合計", "Total"),
    ("subagent.n_running", "个运行中", "個執行中", "running"),
    ("subagent.just_now", "刚刚", "剛剛", "just now"),
    ("subagent.ago_secs", "秒前", "秒前", "s ago"),
    ("subagent.ago_mins", "分钟前", "分鐘前", "m ago"),
    ("subagent.ago_hours", "小时前", "小時前", "h ago"),
];

/// Translate a **semantic key** for the active language.
///
/// - 键不存在 → 原样返回（未知字符串穿透，新 UI 优雅降级）
/// - **译文为空 → 回落 zh-CN**（存量漏译的行为与迁移前一致：繁/英界面显示
///   中文，而不是空白、也不是裸键名。漏译清单由 src/i18n_audit.rs 盯着）
pub fn tr<'a>(s: &'a str) -> &'a str {
    let row = TABLE.iter().find(|e| e.0 == s);
    match lang_ix() {
        0 => row.map(|e| e.1).unwrap_or(s),
        ix => row
            .map(|e| match ix {
                1 => e.2,
                _ => e.3,
            })
            .filter(|t| !t.is_empty())
            .or_else(|| row.map(|e| e.1))
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
        assert_eq!(tr("action.save"), "保存");
        assert_eq!(tr("something.new"), "something.new");
    }

    #[test]
    fn translations_for_other_languages() {
        let _g = LOCK.lock();
        set_lang(1);
        assert_eq!(tr("action.save"), "儲存");
        assert_eq!(tr("session.fork"), "新分支");
        assert_eq!(tr("session.creating"), "建立中…");
        assert_eq!(tr("new_session.greeting"), "Hi，打算讓我做點什麼？");
        assert_eq!(tr("project.search_placeholder"), "搜尋項目…");
        assert_eq!(tr("action.open_folder"), "開啟資料夾");
        set_lang(2);
        assert_eq!(tr("action.save"), "Save");
        assert_eq!(tr("session.fork"), "Fork");
        assert_eq!(tr("session.creating"), "Creating…");
        assert_eq!(tr("new_session.greeting"), "Hi, what shall we work on?");
        assert_eq!(tr("project.search_placeholder"), "Search projects…");
        assert_eq!(tr("action.open_folder"), "Open folder");
        assert_eq!(tr("not.in.table"), "not.in.table");
        set_lang(0);
    }

    /// 存量漏译：繁/英单元格为空 → 回落 zh-CN（**不能**空白、不能回落裸键名）
    #[test]
    fn untranslated_falls_back_to_zh() {
        let _g = LOCK.lock();
        set_lang(2);
        assert_eq!(tr("git.stage_all"), "Stage All");
        set_lang(1);
        assert_eq!(tr("git.stage_all"), "Stage All");
        set_lang(0);
    }

    /// 键与文案解耦：改译文不动键（迁移的核心收益）
    #[test]
    fn key_is_independent_of_copy() {
        let _g = LOCK.lock();
        set_lang(0);
        assert_eq!(tr("settings.show_thinking"), "展示思考");
        set_lang(2);
        assert_eq!(tr("settings.show_thinking"), "Show thinking");
        set_lang(0);
    }

    #[test]
    fn template_replacement() {
        let _g = LOCK.lock();
        set_lang(2);
        assert_eq!(
            tf("extensions.installed", &[("source", "npm:x".to_string())]),
            "Installed npm:x"
        );
        set_lang(0);
        assert_eq!(
            tf("extensions.installed", &[("source", "npm:x".to_string())]),
            "已安装 npm:x"
        );
    }
}
