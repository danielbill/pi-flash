//! pi-flash — desktop shell for the pi coding agent.
//!
//! v54 shell: topbar 两段（左=收放钮 chrome / 右=内容 tabs+设置+窗口控制）·
//! psp 项目+会话一体列表 · 内容区 chat/term/md 状态机 · statusbar 仅面板段。
//! Layout values come from docs/UI设计/主界面UI设计-2.html (mist tokens).
//! **Windows GUI 子系统（铁律，勿删）**：不写这条，链接出的 exe 是 console
//! subsystem → 进程一启动 Windows 就给它分配控制台 → 机器若以 Windows
//! Terminal 作默认终端宿主，就先弹一个终端窗口（tab 名=exe 名，里面只有
//! WT 自己的一条 iCCP 警告），PF 主窗口才跟上；GUI 程序还白挂一个
//! conhost 子进程。已实测：不写 = 启动即带控制台，写了 = 无 conhost、无
//! 终端窗口。见 docs/agents/engineering.md。
#![windows_subsystem = "windows"]

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use futures::StreamExt;
use gpui::{
    App, Application, Context, FocusHandle, Focusable, KeyBinding, KeyDownEvent, MouseButton,
    ParentElement, Render, SharedString, Styled, WindowOptions, actions, div, prelude::*, px, rgb,
};
use pi_link::protocol::Command;
use pi_link::sessions::{SessionInfo, list_sessions_for_cwd, read_tail_messages};

mod agent_session;
mod appearance;
mod ask_card;
mod assets;
mod automation;
mod content;
mod dialogs;
mod editor;
mod ext_ui;
mod function_panel;
mod i18n;
#[cfg(test)]
mod i18n_audit;
mod market;
mod models_config;
mod remote_control;
mod search;
mod services;
mod session;
mod settings;
mod shell;
mod startup;
mod status_bar;
mod terminal;
mod titlebar;
mod top_panels;
mod ui;
pub(crate) use session::menu::slash_menu_view;
pub(crate) use shell::chat::Chat;
pub(crate) use shell::types::*;
pub(crate) use ui::ComposerInput;

/// 常规（非最大化）窗口尺寸——点窗口「缩小」钮还原到的就是这一档
/// （2026-10-29 用户定案）。逻辑像素（gpui 按 scale_factor 折算），唯一
/// 落点在 `run()` 的建窗参数。
pub(crate) const WINDOW_W: f32 = 1180.;
pub(crate) const WINDOW_H: f32 = 760.;

/// 窗口最小尺寸：宽 = 还原宽（「点缩小钮后多大就别想再拖更小」——再窄聊天
/// 列会被挤到不可用）；高比还原高矮 160（2026-10-29 用户定案 600），给
/// 1366×768 这类老屏幕留出任务栏之后的可用高度。
pub(crate) const WINDOW_W_MIN: f32 = WINDOW_W;
pub(crate) const WINDOW_H_MIN: f32 = 600.;

// composer 覆盖动作（注册为 "Input" 上下文绑定，见 run() 里 bind_keys）：
// ↑/↓ 在菜单态导航补全、空输入态回溯历史，非空多行重新派发组件 MoveUp/
// MoveDown；Tab 在菜单态接受补全；Ctrl+V 截获图片粘贴（composer 附件化，
// 其余输入框经根节点兜底重派发组件 Paste）。组件默认的这些键由此被截获。
// psp 全局三动作（OpenProjectPicker/NewSession/OpenSessionSearch）：无上下
// 文全局绑定 Ctrl+P / Ctrl+N / Ctrl+K；psp 头部三钮的 tooltip 键位 chip 也
// 查这三个动作（键位单一事实源 = keymap，绑定改了 chip 自动跟随）。
// 082 全局两动作（MaximizeSessionList/MaximizeFileTree）：子面板最大化，
// Alt+1 / Alt+2，与 psp/文件树头部最大化钮同一语义。
// 082 视图切换两动作（ViewLeft/ViewRight）：Ctrl+←/→ 切到当前标签左/右
// 邻视图。082 三期用户拍板：这是**壳层专属键**——注册为无上下文（gpui
// keymap 语义「无上下文 = 最深上下文」，任意焦点下都命中，且绑定期先于
// on_key_down 监听器），终端/编辑器/composer 的原键位响应一律截断。
// 必须排在 gpui_component::init 之后（同深度后注册者胜）。分流在渲染根
// on_action：模态/弹层与编辑器搜索面板退回词移动，其余切 tab（边界 =
// no-op 死键）。
// 082 三期 md 预览滚动六动作（Preview*）：预览容器持焦（key_context
// "MdPreview"）时方向/翻页/首尾键滚动虚拟列表，预览不再是键盘死角。
actions!(
    app,
    [
        ComposerUp,
        ComposerDown,
        ComposerTab,
        ComposerPaste,
        FileSave,
        OpenProjectPicker,
        NewSession,
        OpenSessionSearch,
        CopyMessageSelection,
        MaximizeSessionList,
        MaximizeFileTree,
        ViewLeft,
        ViewRight,
        OpenFileSearch,
        OpenProjectSearch,
        PreviewUp,
        PreviewDown,
        PreviewPageUp,
        PreviewPageDown,
        PreviewHome,
        PreviewEnd
    ]
);
use crate::appearance::theme::theme as T;
use ext_ui::render_ext_dialog;
use i18n::tr;
use models_config::EnabledState;
use services::format::*;
use services::git::*;
use services::workspace::*;
use session::messages::{Msg, msgs_from_tail};
use session::runtime::SessionRuntime;
use terminal::TerminalTab;
use ui::TextInput;
use ui::icon;

static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
static PERF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------------------
// state
// ---------------------------------------------------------------------------

/// Rows rendered in the model picker (and the keyboard-selection range);
/// further matches are reachable by typing to filter.
pub(crate) const MODEL_PICKER_ROWS: usize = 12;

/// 崩溃落盘（诊断地基）：gpui 主线程 panic 在无控制台的 GUI 进程里表现为
/// 「窗口没了/卡死」，日志里什么都没有——用户只能看到“崩了”，开发者连
/// 哪个文件哪一行都拿不到（2026-10-18 实际踩过：一次输入路径的借用 panic
/// 耗掉整轮排查）。把 panic 消息 + 位置 + backtrace 写进
/// `<PF 目录>/logs/crash-<ts>.log`，下次启动就能直接读。
///
/// 所有失败一律返回 None：本函数也跑在 panic 处理路径上，不能自己再抛。
pub(crate) fn write_crash_log(info_text: &str) -> Option<PathBuf> {
    let dir = pi_link::paths::ensure_dir()?.join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = dir.join(format!("crash-{ts}.log"));
    let body = format!(
        "pi-flash crash\ntime: {ts}\npid: {}\n\n{info_text}\n\nbacktrace:\n{}\n",
        std::process::id(),
        std::backtrace::Backtrace::force_capture(),
    );
    std::fs::write(&path, body).ok()?;
    Some(path)
}

fn install_crash_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // 先写盘；catch_unwind 兼作最后一道保险（hook 里再 panic 会丢原信息）
        let info_text = info.to_string();
        if let Ok(Some(p)) = std::panic::catch_unwind(move || write_crash_log(&info_text)) {
            eprintln!("[crash] 已写入 {}", p.display());
        }
        default_hook(info);
    }));
}

fn main() {
    let _ = T0.set(std::time::Instant::now());
    install_crash_hook();
    // 081 启动交接：双击/快捷方式直启 exe 也认新版本。开窗之前若安装根下
    // 已有「版本更高的完整载荷目录」（应用内更新只解压不 rename），就 spawn
    // 它的 exe 并让位——用户只感觉这次启动慢半拍；拉不起来则照常启动旧版
    // （Ready 提示可点重试）。零 rename/零 helper/零锁。
    if services::updater::handoff_if_newer() {
        return;
    }
    // 任务栏固定稳定性（win_pin）：显式 AUMID 必须在首个窗口创建前设置；
    // 开始菜单稳定入口 lnk + 任务栏死链自愈走后台线程，不占启动路径。
    #[cfg(windows)]
    services::win_pin::prepare();
    // 启动第一步（010-启动.md §4/§10）：建 ~/.pi-flash/、旧 pi-flash-*.json 搬家、
    // recents 首启种子。必须早于任何读盘者（workspace 记忆 / recents / 会话扫描器
    // 都是进程级惰性单例）。
    startup::boot();
    // [perf] 仪器总开关：PI_FLASH_PERF=1 才开（1452 RENDER_ON 同款门控）。
    // 此处曾被无条件 store(true)，每次启动都往 stderr 打 [perf] 行，release 烟测必炸。
    PERF.store(
        std::env::var("PI_FLASH_PERF").is_ok(),
        std::sync::atomic::Ordering::Relaxed,
    );
    // theme: PI_FLASH_THEME (dev override) > app_settings.json（pi-flash 专
    // 属配置，绝不碰 pi 的 settings.json——pi 的 settings schema 有自己的
    // theme 键，写入未知主题名会让 pi 每次启动报错）
    if std::env::var("PI_FLASH_THEME")
        .ok()
        .and_then(|n| crate::appearance::theme::set_by_name(&n).then_some(()))
        .is_none()
    {
        if let Some(name) = app_settings().theme {
            crate::appearance::theme::set_by_name(&name);
        }
    }
    // language: persisted workspace-memory preference；无偏好默认 zh-CN。
    // set_lang 内同步 gpui-component 的 rust_i18n locale（搜索条文案/tooltip）
    i18n::set_lang(load_lang_pref().unwrap_or(0));
    Application::new()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            // JetBrains Mono 随二进制打包：全应用唯一内置 mono（会话区 mono
            // + 终端网格；三档字重 + Italic 覆盖 400/600/700 与斜体）。
            // 注册失败仅回退系统字体，不致命。
            cx.text_system()
                .add_fonts(vec![
                    std::borrow::Cow::Borrowed(
                        include_bytes!("../../../assets/fonts/JetBrainsMono-Regular.ttf")
                            .as_slice(),
                    ),
                    std::borrow::Cow::Borrowed(
                        include_bytes!("../../../assets/fonts/JetBrainsMono-SemiBold.ttf")
                            .as_slice(),
                    ),
                    std::borrow::Cow::Borrowed(
                        include_bytes!("../../../assets/fonts/JetBrainsMono-Bold.ttf").as_slice(),
                    ),
                    std::borrow::Cow::Borrowed(
                        include_bytes!("../../../assets/fonts/JetBrainsMono-Italic.ttf").as_slice(),
                    ),
                ])
                .expect("embedded mono fonts are valid TTF");
            // 系统字体目录（设置页字体下拉数据源，字母序；一次性枚举）
            crate::appearance::init_font_catalog(cx);
            // 面板字号 → 全局 UI 缩放（ui_size 基准）
            crate::appearance::sync_ui_scale();
            // gpui-component (widget library powering TextInput): global
            // init + token mapping from the active app theme
            gpui_component::init(cx);
            // 消息划选一期：TextView 链接点击统一路由（组件默认 open_url 开
            // 浏览器；本地文件链接须走应用内文件 tab，规则同自研渲染器）
            gpui_component::text::set_link_handler(
                std::sync::Arc::new(|url: &str, cx: &mut gpui::App| {
                    crate::editor::markdown::dispatch_md_link(url, cx);
                }),
                cx,
            );
            // 消息划选：inline code span 里「形似本地文件路径」的内容标成
            // 链接（旧渲染器 inline_code_box chip parity，仅 code span——
            // 裸文本路径旧渲染器也不可点），点击仍走上面的 link handler
            gpui_component::text::set_path_link_detector(
                std::sync::Arc::new(|s: &str| crate::editor::markdown::as_file_path(s)),
                cx,
            );
            // composer 覆盖绑定：同深度("Input")后注册者优先，必须排在
            // gpui_component::init 之后（其绑定含 up/down/tab→组件移动/
            // 缩进）。被截获的键由 composer 的 on_action 处理（菜单导航/
            // 历史回溯/补全接受），非空多行时重新派发 MoveUp/MoveDown。
            cx.bind_keys([
                KeyBinding::new("up", ComposerUp, Some("Input")),
                KeyBinding::new("down", ComposerDown, Some("Input")),
                KeyBinding::new("tab", ComposerTab, Some("Input")),
                // 图片粘贴：同深度后注册者优先（覆盖组件 ctrl-v→Paste），
                // composer 内层处理图片附件，其余输入框由根节点兜底重派发
                KeyBinding::new("ctrl-v", ComposerPaste, Some("Input")),
                // 消息划选：抢占组件 ctrl-c→input::Copy（gpui 键盘派发绑定
                // 先于 on_key_down 监听器，纯监听器兜底够不到 Input 焦点场景；
                // composer 空选区时组件 Copy 直接 return 不写剪贴板但吞键）。
                // 分流在渲染根 on_action：有消息选区复制消息，否则原样转发
                // input::Copy，输入框选区照常复制
                KeyBinding::new("ctrl-c", CopyMessageSelection, Some("Input")),
                // 023 fileView：编辑器里 Ctrl+S 保存（composer 等其他 Input
                // 上下文内无人监听该 action，自然空转）
                KeyBinding::new("ctrl-s", FileSave, Some("Input")),
                // 082 四期（Zed Pane 模式，替代三期无上下文版）：双绑定锚定——
                // Some("ContentTabs") 命中内容区任意后代焦点（终端/预览/中性
                // 锚点，同 gpui 前缀匹配 keymap/context.rs:253）；Some("Input")
                // 同深度后注册抢组件词移动（composer/编辑器内也切，082 二期
                // 语义）。模态是根的子、不在 content-col 子树 → 路径无该上下
                // 文 → 模态内词移动不受影响（天然隔离，Zed RunModal 反向可
                // 参）。终端跳词仍走 Ctrl+Shift+←/→（修饰符精确匹配不互斥）
                KeyBinding::new("ctrl-left", ViewLeft, Some("ContentTabs")),
                KeyBinding::new("ctrl-left", ViewLeft, Some("Input")),
                KeyBinding::new("ctrl-right", ViewRight, Some("ContentTabs")),
                KeyBinding::new("ctrl-right", ViewRight, Some("Input")),
                // 082 三期 md 预览滚动（MdPreview 上下文 = 预览容器持焦时
                // 生效；组件未占这些键——Input/List 上下文的同名键互不干扰）
                KeyBinding::new("up", PreviewUp, Some("MdPreview")),
                KeyBinding::new("down", PreviewDown, Some("MdPreview")),
                KeyBinding::new("pageup", PreviewPageUp, Some("MdPreview")),
                KeyBinding::new("pagedown", PreviewPageDown, Some("MdPreview")),
                KeyBinding::new("home", PreviewHome, Some("MdPreview")),
                KeyBinding::new("end", PreviewEnd, Some("MdPreview")),
                // psp 全局三键（无上下文 = 任意焦点可用；组件输入框未占这三个键）
                // 禁注册 cmd-*：gpui 的 cmd 是 platform 修饰键（≠ctrl 别名），
                // 后注册会赢得 tooltip 键位反查，Windows 上显示成 Win+P
                KeyBinding::new("ctrl-p", OpenProjectPicker, None),
                KeyBinding::new("ctrl-n", NewSession, None),
                KeyBinding::new("ctrl-k", OpenSessionSearch, None),
                // 全局文件搜索（Ctrl+L）：与文件树工具栏「搜索文件」钮同一
                // 方法 = 083 搜索面板（文件+内容同搜）。2026-10-21 起旧弹窗
                // 退役，两条快捷键同一面板
                KeyBinding::new("ctrl-l", OpenFileSearch, None),
                // 083 项目搜索（Ctrl+Shift+F，Zed project_search::ToggleFocus
                // 同键；None 上下文任意焦点可用，再按一次回到查询框）
                KeyBinding::new("ctrl-shift-f", OpenProjectSearch, None),
                // 082 子面板最大化两键（无上下文 = 任意焦点可用，与上面三键
                // 同款；tooltip 键位 chip 反查这里）
                KeyBinding::new("alt-1", MaximizeSessionList, None),
                KeyBinding::new("alt-2", MaximizeFileTree, None),
            ]);
            appearance::sync_gpui_tokens(cx);
            // startup restore (§4)：每次启动默认最大化（位置不持久化——
            // gpui Windows 的外框/客户区坐标在存取间不对称，每个周期漂移
            // 一个边框宽）。取消最大化后的尺寸仍保存，供会话内还原参考。
            // 010-启动：先以普通窗口呈现启动页（不全屏），揭幕后由 render
            // 的 pending_zoom 补最大化——0.2.2 建窗即 Maximized 的延迟处理
            // 实测不生效，显式 zoom 是项目已验证路径（从建窗回调后移）。
            let _restored = get_window_state();
            // 还原尺寸（WINDOW_W/H）与最小尺寸（WINDOW_W/H_MIN）分开：
            // 宽度同值（还原宽就是最小宽），高度允许再矮 160px。
            let bounds = gpui::Bounds::centered(None, gpui::size(px(WINDOW_W), px(WINDOW_H)), cx);
            let window_bounds = gpui::WindowBounds::Windowed(bounds);
            let automation_chat: std::sync::OnceLock<gpui::WeakEntity<Chat>> =
                std::sync::OnceLock::new();
            let window_handle = cx
                .open_window(
                    WindowOptions {
                        window_bounds: Some(window_bounds),
                        // 最小尺寸（逻辑像素，gpui 按 scale_factor 换算后交给
                        // WM_GETMINMAXINFO）：宽同还原宽、高超矮一档
                        window_min_size: Some(gpui::size(px(WINDOW_W_MIN), px(WINDOW_H_MIN))),
                        titlebar: Some(gpui::TitlebarOptions {
                            title: Some("pi-flash".into()),
                            // client-side title bar (v54 topbar 两段, window
                            // control hitboxes registered by titlebar.rs)
                            appears_transparent: true,
                            ..Default::default()
                        }),
                        ..Default::default()
                    },
                    |window, cx| {
                        // gpui-component widgets require their Root as the window
                        // root view (renders their context-menu/popover layers)
                        let chat = cx.new(Chat::new);
                        let weak = chat.downgrade();
                        let _ = automation_chat.set(weak.clone());
                        // persist window bounds + shell layout on close so the
                        // startup restore layer has data (§4)
                        window.on_window_should_close(cx, move |window, cx| {
                            let b = window.bounds();
                            save_window_state(&WindowState {
                                x: f64::from(b.origin.x),
                                y: f64::from(b.origin.y),
                                w: f64::from(b.size.width),
                                h: f64::from(b.size.height),
                                maximized: window.is_maximized(),
                            });
                            if let Some(chat) = weak.upgrade() {
                                chat.update(cx, |chat, _cx| chat.persist_ui());
                            }
                            true
                        });
                        cx.new(|cx| gpui_component::Root::new(chat.into(), window, cx))
                    },
                )
                .unwrap();
            // UI 自动化服务（pi-flash-2kq）：PI_FLASH_AUTOMATION=<port|auto|1>
            // 显式开启，缺省关闭——agent 调试用，不抢真实屏幕/鼠标。
            if let (Some(weak), Ok(spec)) =
                (automation_chat.get(), std::env::var("PI_FLASH_AUTOMATION"))
            {
                if let Err(e) = automation::start(cx, window_handle.into(), weak.clone(), &spec) {
                    eprintln!("pi-flash automation: {e}");
                }
            }
            cx.activate(true);
        });
}

#[cfg(test)]
mod crash_log_tests {
    use super::write_crash_log;

    /// 崩溃落盘契约（诊断地基）：正文必须含 panic 消息与 backtrace 段，
    /// 且路径落在 <PF 目录>/logs/crash-*.log。回归锁防「hook 在但写不出」。
    #[test]
    fn crash_log_written_with_message_and_backtrace() {
        let path = write_crash_log("boom: 输入路径重入借用").expect("写盘失败");
        assert!(path.to_string_lossy().contains("logs"));
        assert!(
            path.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("crash-")
        );
        let body = std::fs::read_to_string(&path).expect("读回失败");
        assert!(
            body.contains("boom: 输入路径重入借用"),
            "缺 panic 消息: {body}"
        );
        assert!(body.contains("backtrace:"), "缺 backtrace 段: {body}");
        std::fs::remove_file(&path).ok();
    }
}
