//! 应用设置（006 界面设置）：`app-settings.json` 的 **app 侧薄壳**（001-S2）。
//!
//! 数据全部住在 pf-context（typed section 合并 + 单写者 + 原子写 + watcher，
//! 见 docs/模块设计/001-PFContext/001-1）：[`app_settings()`] 从 hub 快照组装
//! 兼容视图（UI 调用点签名不变），[`save_app_settings()`] diff 出变化段转
//! `hub.settings().set()`——**整份重建 Map 的旧写入路径已删**（未知键透传由
//! pf-context 读-改-写保证）。getters 群不变。自 workspace.rs 拆出（090 重构）。

use pf_context::sections::{
    Appearance, Chat, DevFlags, Index, Models, Sessions, Shell, Subagents, Updates,
};

mod subagent_prefs;

pub(crate) use subagent_prefs::*;

// FontSpec / CHAT_WIDTH_* re-export：appearance、slider 等调用点路径
// （`workspace::FontSpec` / `workspace::CHAT_WIDTH_*`）不变
pub use pf_context::sections::{
    CHAT_WIDTH_DEFAULT, CHAT_WIDTH_MAX, CHAT_WIDTH_MIN, CHAT_WIDTH_STEP, FontSpec,
};

// ---------------------------------------------------------------------------
// 兼容视图（001-S2）：AppSettings = hub 快照的扁平投影，字段语义同 v1 平铺键
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AppSettings {
    /// theme id ("mist" / "rose" / "one-light" / ... 006 built-ins)
    pub theme: Option<String>,
    /// icon theme id (built-in set is pi-web's own icons)
    pub icon_theme: Option<String>,
    /// chat/session font (≈ zed UI font)
    pub session_font: Option<FontSpec>,
    /// function-panel font
    pub panel_font: Option<FontSpec>,
    /// markdown preview font
    pub markdown_font: Option<FontSpec>,
    /// language index (i18n::TABLE rows)
    pub lang: Option<usize>,
    /// notification sound on/off
    pub sound: Option<bool>,
    /// v60: startup load time window in days (7/14/30) — sessions whose
    /// last activity falls inside the window form the psp list
    pub load_window_days: Option<u64>,
    /// v54 其他页: restore last workspace + session on startup
    pub restore: Option<bool>,
    /// 界面页: 展示思考（关（默认）=新思考块收成一行；开=默认展开全文）
    pub show_thinking: Option<bool>,
    /// 界面页: 文件树 git 标识（023 默认关——清爽目录树；开 = M/A/D/R/U/C
    /// 徽标 + 目录变更点）
    pub git_markers: Option<bool>,
    /// 其他页: 各项目默认显示的会话数量（psp 初始页大小，3-10，默认 10）
    pub session_display_count: Option<u64>,
    /// 其他页: 启动自动折叠阈值（小时）——档位 24/72/168（1/3/7 天），默认 168
    pub auto_collapse_hours: Option<u64>,
    /// @ 文件索引缓存有效期（秒，1-3600，默认 60）
    pub at_index_ttl_secs: Option<u64>,
    /// @ 文件索引缓存的最多项目数（每 cwd 一份，1-100，默认 10）
    pub at_index_max_projects: Option<u64>,
    /// 042：默认模型（设置-模型页下列表五角星）。PF 自有默认——不写 pi 的
    /// settings.json（系统 pi / pi-web 的地盘），spawn 时以 `--model` 落到
    /// 新会话。两键齐才算设置了默认。
    pub default_provider: Option<String>,
    pub default_model: Option<String>,
    /// 081 其他页: 自动更新（默认 on——off 时只提示新版本号 ↑ 不下载）
    pub auto_update: Option<bool>,
    /// 052 其他页: 会话自动清理天数——默认 0 = 关闭（2026-10-18 用户定夺）
    pub auto_clean_days: Option<u64>,
    /// 界面页: Markdown 所见即所得（024 Live Preview 装饰；默认关）
    pub md_live_preview: Option<bool>,
    /// 界面页: 部分操作隐藏（默认 on）
    pub autohide_icons: Option<bool>,
    /// 界面页: 会话内容宽度 px（缺键 = [`CHAT_WIDTH_DEFAULT`]）
    pub chat_content_width: Option<u64>,
    /// 090 排队护栏: 运行中发送方式——"queue"（默认）/"steer"
    pub send_mode: Option<String>,
    /// 044-0 模块设置（兼容视图字段）：当前生效值恒 Some（真源 = hub
    /// `subagents` section，写入口在 subagent_prefs）
    pub subagent_capacity: Option<u64>,
    pub subagent_enabled: Option<bool>,
}

/// 主题/语言/字体等设置的当前快照视图（001-S2 薄壳）：从 hub typed section
/// 组装，O(1) 零 I/O；渲染期每帧调用安全。
pub fn app_settings() -> AppSettings {
    let st = &pf_context::hub().settings;
    let a = st.get::<Appearance>();
    let c = st.get::<Chat>();
    let se = st.get::<Sessions>();
    let ix = st.get::<Index>();
    let mo = st.get::<Models>();
    let up = st.get::<Updates>();
    let sh = st.get::<Shell>();
    let sub = st.get::<Subagents>();
    AppSettings {
        theme: a.theme.clone(),
        icon_theme: a.icon_theme.clone(),
        session_font: a.session_font.clone(),
        panel_font: a.panel_font.clone(),
        markdown_font: a.markdown_font.clone(),
        lang: a.lang.map(|v| v as usize),
        sound: sh.sound,
        load_window_days: se.load_window_days,
        restore: se.startup_restore,
        show_thinking: c.show_thinking,
        git_markers: c.git_markers,
        session_display_count: se.display_count,
        auto_collapse_hours: se.auto_collapse_hours,
        at_index_ttl_secs: ix.at_index_ttl_secs,
        at_index_max_projects: ix.at_index_max_projects,
        default_provider: mo.default_provider.clone(),
        default_model: mo.default_model.clone(),
        auto_update: up.auto_update,
        auto_clean_days: se.auto_clean_days,
        md_live_preview: c.md_live_preview,
        autohide_icons: sh.autohide_icons,
        chat_content_width: c.content_width,
        send_mode: c.send_mode.clone(),
        subagent_capacity: Some(sub.capacity),
        subagent_enabled: Some(sub.enabled),
    }
}

/// 薄壳写入（001-S2）：diff 当前快照 → 只对**变化段**走 `hub.settings().set()`
/// （读-改-写 / 50ms coalesce / 原子写全在 pf-context）。UI 调用点签名不变；
/// subagents 段不经过这里（写入口 = subagent_prefs，双写 agents/config.json），
/// dev_flags 段 boot-only 只读。
pub fn save_app_settings(s: &AppSettings) {
    let cur = app_settings();
    let st = &pf_context::hub().settings;
    if cur.theme != s.theme
        || cur.icon_theme != s.icon_theme
        || cur.session_font != s.session_font
        || cur.panel_font != s.panel_font
        || cur.markdown_font != s.markdown_font
        || cur.lang != s.lang
    {
        st.set::<Appearance>(|a| {
            a.theme = s.theme.clone();
            a.icon_theme = s.icon_theme.clone();
            a.session_font = s.session_font.clone();
            a.panel_font = s.panel_font.clone();
            a.markdown_font = s.markdown_font.clone();
            a.lang = s.lang.map(|v| v as u64);
        });
    }
    if cur.chat_content_width != s.chat_content_width
        || cur.md_live_preview != s.md_live_preview
        || cur.send_mode != s.send_mode
        || cur.show_thinking != s.show_thinking
        || cur.git_markers != s.git_markers
    {
        st.set::<Chat>(|c| {
            c.content_width = s.chat_content_width;
            c.md_live_preview = s.md_live_preview;
            c.send_mode = s.send_mode.clone();
            c.show_thinking = s.show_thinking;
            c.git_markers = s.git_markers;
        });
    }
    if cur.load_window_days != s.load_window_days
        || cur.restore != s.restore
        || cur.session_display_count != s.session_display_count
        || cur.auto_collapse_hours != s.auto_collapse_hours
        || cur.auto_clean_days != s.auto_clean_days
    {
        st.set::<Sessions>(|se| {
            se.load_window_days = s.load_window_days;
            se.startup_restore = s.restore;
            se.display_count = s.session_display_count;
            se.auto_collapse_hours = s.auto_collapse_hours;
            se.auto_clean_days = s.auto_clean_days;
        });
    }
    if cur.at_index_ttl_secs != s.at_index_ttl_secs
        || cur.at_index_max_projects != s.at_index_max_projects
    {
        st.set::<Index>(|ix| {
            ix.at_index_ttl_secs = s.at_index_ttl_secs;
            ix.at_index_max_projects = s.at_index_max_projects;
        });
    }
    if cur.default_provider != s.default_provider || cur.default_model != s.default_model {
        st.set::<Models>(|m| {
            m.default_provider = s.default_provider.clone();
            m.default_model = s.default_model.clone();
        });
    }
    if cur.auto_update != s.auto_update {
        st.set::<Updates>(|u| u.auto_update = s.auto_update);
    }
    if cur.autohide_icons != s.autohide_icons || cur.sound != s.sound {
        st.set::<Shell>(|sh| {
            sh.autohide_icons = s.autohide_icons;
            sh.sound = s.sound;
        });
    }
}

/// 001-4：自我调试开关（设置-更多【自我调试】，默认 on）——pfctx_manager
/// 工具是否注入 agent 会话（spawn 门控 + 切换重绑，见 extensions/pfctx-manager）。
pub fn agent_self_debug() -> bool {
    pf_context::hub()
        .settings
        .get::<pf_context::sections::Agent>()
        .self_debug
}

/// 042：默认模型读取（两键齐生效；键不存在 = 未设置默认）。
pub fn default_model_pref() -> Option<(String, String)> {
    let s = app_settings();
    Some((s.default_provider?, s.default_model?))
}

/// 090 排队护栏：运行中发送方式（设置-其他）。返回 "queue"（默认，含缺键
/// /非法值）或 "steer"。
pub fn send_mode_pref() -> &'static str {
    match app_settings().send_mode.as_deref() {
        Some("steer") => "steer",
        _ => "queue",
    }
}

/// 090：发送方式写入（设置页两档分段按钮）。
pub fn save_send_mode(mode: &str) {
    let mut s = app_settings();
    s.send_mode = Some(mode.to_string());
    save_app_settings(&s);
}

/// 042：默认模型写入（设置页五角星 / inputpanel 星标共用；None = 取消）。
/// 落盘即生效——新会话 spawn 读最新值。
pub fn set_default_model_pref(pref: Option<(String, String)>) {
    let mut s = app_settings();
    match pref {
        Some((p, m)) => {
            s.default_provider = Some(p);
            s.default_model = Some(m);
        }
        None => {
            s.default_provider = None;
            s.default_model = None;
        }
    }
    save_app_settings(&s);
}

/// Startup load time window in days (settings-其他「加载时间窗口」档位
/// 7/14/30, default 7) — the recents list filters on it.
pub fn load_window_days() -> u64 {
    match app_settings().load_window_days.unwrap_or(7) {
        14 => 14,
        30 => 30,
        _ => 7,
    }
}

/// Sessions whose tail is read into memory at startup (fixed, not a
/// setting — the load window is the user-facing knob).
pub const TAIL_PRELOAD: usize = 10;

/// v54 其他: startup restore toggle.
/// 启动恢复开关：on = 落最近活动项目并**直接加载**该项目最后会话；
/// off = 落同一项目但停在新会话页（项目记忆与开关无关，恒生效）。
/// 默认 on（「启动恢复最后会话」是默认体验；显式关过的用户保留其设置）。
pub fn startup_restore() -> bool {
    app_settings().restore.unwrap_or(true)
}

/// 其他页: 各项目默认显示的会话数量（psp 初始页大小，3-10，默认 10）。
pub fn session_display_count() -> usize {
    app_settings()
        .session_display_count
        .unwrap_or(10)
        .clamp(3, 10) as usize
}

/// 其他页: 启动自动折叠阈值（小时；档位 24/72/168 = 1/3/7 天，默认 168）。
pub fn auto_collapse_hours() -> u64 {
    match app_settings().auto_collapse_hours.unwrap_or(168) {
        24 => 24,
        72 => 72,
        _ => 168,
    }
}

/// @ 文件索引缓存有效期（秒；默认 60，clamp 1-3600）。
pub fn at_index_ttl_secs() -> u64 {
    app_settings()
        .at_index_ttl_secs
        .unwrap_or(60)
        .clamp(1, 3600)
}

/// @ 文件索引缓存的最多项目数（每 cwd 一份；默认 10，clamp 1-100）。
pub fn at_index_max_projects() -> usize {
    app_settings()
        .at_index_max_projects
        .unwrap_or(10)
        .clamp(1, 100) as usize
}

/// 界面页: 展示思考（关（默认）=新思考块收成一行；开=默认展开全文）。
pub fn show_thinking() -> bool {
    app_settings().show_thinking.unwrap_or(false)
}

/// 界面页: 部分操作隐藏（dock 会话控制栏「列表切换」与文件树操作栏五颗
/// 功能 icon 常态透明，hover 各自操作栏才显影；默认 on）。渲染期每帧读，
/// hub 快照 O(1)，开销可忽略。
pub fn autohide_icons() -> bool {
    app_settings().autohide_icons.unwrap_or(true)
}

/// 开发旗标查询（001-S2：OnceLock 快照退役，改走 hub `dev_flags` boot-only
/// 段；键 = pi-link `dev_flags::KNOWN`，env 覆盖最高优先；改动重启生效）。
pub fn dev_flag_enabled(key: &str) -> bool {
    pf_context::hub().settings.get::<DevFlags>().flag(key)
}

/// subagent 功能总闸（开发旗标，默认开（044-0）；dev-flags.json /
/// PI_FLASH_SUBAGENTS 可强关）。只管设置页导航可见性；运行时生效开关 =
/// 本旗标 && 用户面开关（`subagent_module_active`）。
pub fn subagents_enabled() -> bool {
    dev_flag_enabled("subagents")
}

/// topbar「菜单」三横钮是否渲染：**恒显**（2026-10-23 用户禁令：不许删菜单 icon）。
pub fn market_menu_enabled() -> bool {
    true
}

/// 文件树 git 标识开关（设置-界面；默认关 = 清爽目录树，023 定案）。
pub fn git_markers() -> bool {
    app_settings().git_markers.unwrap_or(false)
}

/// 081 其他页: 自动更新开关（默认 on）。
pub fn auto_update() -> bool {
    app_settings().auto_update.unwrap_or(true)
}

/// 052 其他页: 会话自动清理天数（默认 0 = 关闭——2026-10-18 用户定夺：自动删
/// 会话太凶，显式设置后才生效）。
pub fn auto_clean_days() -> u64 {
    app_settings().auto_clean_days.unwrap_or(0).min(100)
}

/// 界面页: Markdown 所见即所得（024 Live Preview 装饰；默认关 = md 源码态
/// 走普通编辑器路径。已知回车崩溃与标题行距问题未修，显式开启才挂装饰）。
pub fn md_live_preview() -> bool {
    app_settings().md_live_preview.unwrap_or(false)
}

// ---- 会话内容宽度（pi-web `--chat-content-max-width` parity）------------
//
// 单一内容列宽：消息列、composer 胶囊、新会话页内容带、ask 问卷卡全部读
// 这里。阈值与常量（CHAT_WIDTH_*）住在 pf-context chat section（上方
// re-export），clamp 唯一落点 = section `validate`。
//
// 会话内容宽度（px，已钳制；渲染期多次读取，O(1) 快照读）。
pub fn chat_content_width() -> f32 {
    clamp_chat_width(
        app_settings()
            .chat_content_width
            .unwrap_or(CHAT_WIDTH_DEFAULT as u64),
    ) as f32
}

fn clamp_chat_width(v: u64) -> u32 {
    (v as u32).clamp(CHAT_WIDTH_MIN, CHAT_WIDTH_MAX)
}

/// 拖动中的实时预览（001-S2：走 hub `set_preview` 通道——只改内存快照不落
/// 盘，松手再 [`save_app_settings_now`] 落一次）。
pub fn preview_chat_content_width(width: u32) {
    pf_context::hub()
        .settings
        .set_preview::<Chat>(|c| c.content_width = Some(clamp_chat_width(width as u64) as u64));
}

/// 把内存态（含拖拽预览后的值）落盘（001-S2：预览段转 dirty + 同步 flush）。
pub fn save_app_settings_now() {
    let _ = pf_context::hub().settings.save_now();
}

#[cfg(test)]
mod tests;
