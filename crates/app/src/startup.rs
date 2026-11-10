//! 010-启动：启动阶段唯一入口。
//!
//! 1. **启动页**：参考 codex（docs/模块设计/image.bmp）——近黑背景、logo 居中、
//!    普通窗口不最大化；pi 附着（或超时兜底，见 main.rs 的启动闸门）后由 Chat
//!    揭幕，落点即启动恢复的既有分流：有上次会话 → 030 会话界面，无 → 012
//!    新会话页。
//! 2. **全局态一次性装载**（010-启动.md §1-§8）：启动时把后续所有界面要用的
//!    全局量从硬盘装进内存（模型清单 / 命令 / 插件 / mcp / 默认项 / 项目上下文），
//!    渲染路径只读内存，**不扫盘、不发 RPC**；会话进程答到的 RPC 只做覆盖
//!    （并回写 `catalog-cache.json`，见 §4.1）。
//!
//! 自有文件全部落 `~/.pi-flash/`（`pi_link::paths`），`~/.pi/agent/` 只留给 pi。

use std::time::Duration;

use gpui::{WindowControlArea, div, prelude::*, px, rgb};

/// 最短展示：从启动页首次绘制起算（见 `mark_splash_painted`）。低于这个数
/// 观感是「闪一下黑」而不是启动页。
pub(crate) const MIN_SPLASH: Duration = Duration::from_millis(500);
/// 就绪兜底：pi 起不来（缺 node / 被占用等）也不能把用户困在启动页。
pub(crate) const SPLASH_TIMEOUT: Duration = Duration::from_millis(3000);

static SPLASH_PAINT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// 主窗口句柄（main.rs 建窗即登记，pi-flash-weia）：启动恢复的终端 tab
/// 要 window（测字宽/焦点），异步恢复任务经它进 window 上下文——与
/// automation 同款模式。
static MAIN_WINDOW: std::sync::OnceLock<gpui::AnyWindowHandle> = std::sync::OnceLock::new();

pub(crate) fn note_main_window(handle: gpui::AnyWindowHandle) {
    let _ = MAIN_WINDOW.set(handle);
}

pub(crate) fn main_window() -> Option<gpui::AnyWindowHandle> {
    MAIN_WINDOW.get().copied()
}

/// 启动页首帧绘制时刻（splash 渲染分支调用；OnceLock 只记第一次）。
pub(crate) fn mark_splash_painted() {
    let _ = SPLASH_PAINT.set(std::time::Instant::now());
}

/// 启动页已绘制时长——None = 还没画出来（忙机首帧可晚于 pi 附着，
/// 揭幕闸门必须等这个有值，否则用户一帧启动页都见不到）。
pub(crate) fn splash_paint_age() -> Option<Duration> {
    SPLASH_PAINT.get().map(|t| t.elapsed())
}

/// 全幅启动页。取色自参考图：底 0x202020、logo 0xa1a1a1。顶部 40px 注册
/// 拖拽区——client-side titlebar 下没有原生拖拽带，否则启动页拖不动窗口
///（关闭走 Alt+F4 / 任务栏；启动页通常亚秒级，控制钮不画）。
pub(crate) fn splash_view() -> gpui::Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(0x202020))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(40.))
                .window_control_area(WindowControlArea::Drag),
        )
        // 盒 106 = 原始 88 的 1.2 倍（用户定的 +20%）；墨迹占盒 51.7%（v5 裸标）⇒ 实际墨迹高 ~55px
        .child(crate::ui::icon("logo-marks", 106., 0xa1a1a1))
}

// ---------------------------------------------------------------------------
// 启动第一步：目录迁移 + 首启种子（010-启动.md §4 / §10）
// ---------------------------------------------------------------------------

/// 启动第一步（`main()` 最前面调用，必须早于任何读盘者：workspace 记忆缓存 /
/// recents 单例 / sessions 扫描器都是进程级惰性单例）：
/// 1. 建 `~/.pi-flash/`；旧 `~/.pi/agent/pi-flash-*.json` 原样搬家
/// 2. recents 清单仍为空 → 显式种子（按会话 mtime 序落盘）
/// 3. 密钥/自定义 provider 收编（051）：全部**复制式**——auth.json 明文进
///    凭据库 + pf-auth.json，pi models.json 的自定义条目复制进 providers.json；
///    pi 的两个文件字节不动。必须先于任何会话 spawn（-e 扩展 + env 注入
///    依赖账本与凭据库就绪）
pub(crate) fn boot() {
    // 本函数所有 eprintln 只在 debug 构建打印：release 包必须 stdout/stderr 零输出
    // （包内烟测门禁；0.3.2 实录——升级机上 migrate/purge 任一条命中就把烟测打红）。
    let moved = pi_link::paths::migrate_legacy_files();
    if moved > 0 && cfg!(debug_assertions) {
        eprintln!("[startup] migrated {moved} legacy pi-flash file(s) → ~/.pi-flash/");
    }
    // dev-flags（重型模块发版排除）状态一眼可见：关掉的功能为什么没了。
    // 只在 debug 构建打印：release 包必须 stdout/stderr 零输出（包内烟测门禁，
    // 0.2.1 实录——默认关态的这行状态本身就把烟测打红），且开发机上可能
    // 放着全关的 dev-flags.json，「文件存在才打印」也躲不开。
    if cfg!(debug_assertions) {
        eprintln!("[startup] dev-flags: {}", pi_link::dev_flags::summary());
    }
    // 存量清洗（幂等）：注册表里落在当前安装根内的历史脏条目——0.1.x 更新
    // 交接 spawn 不设 cwd，继承的老 payload 目录曾被当项目写进来（「payload
    // 项目删了每次启动又回来」的根因，pi_link::paths::is_app_install_path）。
    // 放在 boot 第一步：注册表 with_registry 惰性单例首读就是清洗后的盘面。
    let purged = pi_link::projects::purge_app_paths();
    if purged > 0 && cfg!(debug_assertions) {
        eprintln!("[startup] purged {purged} app-owned path(s) from project registry");
    }
    // 工作区记忆同批清洗：脏 __last（启动恢复目标被钉死在载荷目录 = 每次双
    // 击都落回 payload 的另一半根因，恢复路径已在 Chat::new 消毒）+ 载荷目
    // 录名下的每项目记忆；__ 前缀的非路径键（布局/UI 状态）不动
    let purged_ws = crate::services::workspace::purge_app_install_memory();
    if purged_ws > 0 && cfg!(debug_assertions) {
        eprintln!("[startup] purged {purged_ws} app-owned key(s) from workspace memory");
    }
    pi_link::recents::ensure_seeded();
    // 042-1 一次性迁移：PF 私有账本（providers.json / pf-auth.json）→ pi 的
    // models.json + auth.json 标记条目；旧文件改名 .bak，幂等。失败只记日志，
    // 绝不阻塞启动（下次启动重试）。
    for line in pi_link::migrate_0421::run() {
        if cfg!(debug_assertions) {
            eprintln!("[startup] migrate: {line}");
        }
    }
}

// ---------------------------------------------------------------------------
// 子模块（按启动阶段）：全局态装载 / 启动后台泵 / 启动恢复
// ---------------------------------------------------------------------------

mod globals;
mod pumps;
mod restore;

pub(crate) use globals::{
    Globals, ProjectCtx, load_globals, load_project, load_skills, merge_commands, merge_models,
    purge_ghosts, write_cache,
};
pub(crate) use pumps::spawn_boot_tasks;
pub(crate) use restore::{restorable_session, spawn_boot_restore};
