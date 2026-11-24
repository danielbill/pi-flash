//! 扩展市场（040 设置-扩展 / 040-1 pi.dev / 090-4 PF 插件）。
//! 列格式 / 空译回落 / audit 解析口径等约定见同目录 mod.rs。

/// (key, zh-CN, zh-TW, en)
#[rustfmt::skip]
pub(super) const TABLE: &[(&str, &str, &str, &str)] = &[
    ("market.btn.install", "安装", "安裝", "Install"),
    ("market.btn.update", "更新", "更新", "Update"),
    // 040-1 Pi 原生扩展市场（pi.dev Package Catalog 移植；页面文案照抄设计稿英文，只收需跟随界面的词）
    ("market.native_tab", "Pi Dev 扩展社区", "Pi Dev 擴展社区", "Pi Dev Packages"),
    ("market.pf.tab", "Pi-Flash 插件", "Pi-Flash 外掛", "Pi-Flash Plugins"),
    ("market.pf.empty", "无可用PF插件", "無可用PF外掛", "No PF plugins available"),
    ("market.pf.scanning", "扫描 Pi-Flash 插件……", "掃描 Pi-Flash 外掛……", "Scanning Pi-Flash plugins…"),
    ("market.pf.updating", "更新中…", "更新中…", "Updating…"),
    ("market.pf.need_newer_pf", "该插件需要 PF {min}，当前 {cur}", "該插件需要 PF {min}，當前 {cur}", "This plugin requires PF {min}, current {cur}"),
    // 090-4 v3：布局对齐 pi market 页后的页内文案（三语齐全）
    ("market.pf.head", "所有插件", "所有外掛", "All plugins"),
    // 090-4 v4：【已安装】页签（管理视图，版式对齐 settings-扩展）
    ("market.pf.tab_installed", "已安装", "已安裝", "Installed"),
    ("market.pf.uninstall_confirm", "卸载插件 {name}？", "卸載外掛 {name}？", "Uninstall plugin {name}?"),
    ("market.pf.enable_failed", "启用失败：未找到插件清单（{name}）", "啟用失敗：未找到外掛清單（{name}）", "Enable failed: manifest not found ({name})"),
    // 090-10 迁移体验：台账启用却被本版 PF 拒载的失效包（旧 schema 等）
    ("market.pf.stale_toast", "{n} 个插件与本版 PF 不兼容，请到「PF 插件市场」更新：{list}", "{n} 個外掛與本版 PF 不相容，請到「PF 外掛市場」更新：{list}", "{n} plugin(s) are incompatible with this PF; update them in PF Plugin Market: {list}"),
    ("market.pf.stale_head", "{n} 个已安装插件与本版 PF 不兼容，需更新：{list}", "{n} 個已安裝外掛與本版 PF 不相容，需更新：{list}", "{n} installed plugin(s) are incompatible with this PF and need an update: {list}"),
    ("market.pf.stale_tag", "需更新", "需更新", "Update needed"),
    ("market.pf.stale_state", "与本版 PF 不兼容（清单 schema {got}），请更新", "與本版 PF 不相容（清單 schema {got}），請更新", "Incompatible with this PF (manifest schema {got}); please update"),
    ("market.pf.stale_invalid", "清单无效，无法装载：{reason}", "清單無效，無法載入：{reason}", "Invalid manifest, cannot load: {reason}"),
    // 插件包内 pi 扩展挂载：忙时会话延迟重启的提示（name = 插件 id）
    ("plugin.ext_wait_idle", "插件 {name} 的扩展将在当前任务完成后生效", "外掛 {name} 的擴充將在當前任務完成後生效", "Extension of plugin {name} will take effect after the current task finishes"),
    // 040-1 页内按钮走英文原串（照设计稿那套 chrome 的语言）
    ("market.install", "install", "install", "install"),
    ("market.prev", "← 上一页", "← 上一頁", "← Previous"),
    ("market.next", "下一页 →", "下一頁 →", "Next →"),
    ("pi_extensions.copy_install", "复制安装命令，做全局安装", "複製安裝命令，做全域安裝", "Copy install command for global installation"),
    ("pi_extensions.from", "从", "從", "From"),
    ("pi_extensions.state", "状态", "狀態", "State"),
];
