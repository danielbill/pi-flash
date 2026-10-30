//! 插件生命周期状态机 + plugin.log（90-9，`docs/模块设计/90-插件系统/90-9-插件生命周期设计.md`）。
//!
//! 状态机 = 纯函数核心（本文件可单测）；Chat 侧条目只持 [`State`] 字段，
//! 转移由宿主在装载/停用路径调 [`transition`]。plugin.log 落
//! `<插件目录>/plugin.log`（每插件一份，卸载随目录删），行式人可读；
//! `load`/`unload` 必须成对——启动时 [`reconcile_startup`] 据此识别上次
//! 异常退出，不需要任何哨兵文件。
//!
//! 只记 serving 实例的 load/unload：启动期后台验签门（`wasm_hooks::
//! verify_wasm_plugins`）的实例是即验即弃的另一份，不进本日志——否则
//! 每次正常启动都会留下一条永不闭合的 load，崩溃判定全数误报。

use std::path::Path;

use chrono::Local;

/// 宿主持有的插件生命周期状态（90-9 §5）。**宿主事实，插件不可见**（插件
/// 可查的只有执行域实例态 `ctx.service.query`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    Registered,
    Loading,
    Active,
    Unloading,
    Disabled,
    Crashed,
}

/// 生命周期事件：宿主行为 → 状态转移输入。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ev {
    LoadStart,
    LoadOk,
    LoadErr,
    UnloadStart,
    UnloadDone,
}

/// 状态转移（纯函数，90-9 §5 图）：非法转移返回 `None`（调用方保持原状，
/// 只落日志——状态机不 panic，宿主事实以日志为准）。
///
/// `Disabled`/`Crashed` 可再 `LoadStart`：用户手动重新启用是显式意图
/// （90-9 §2 异常处理只说「本次会话禁用」，不剥夺用户手动开闸）。
pub(crate) fn transition(s: State, e: Ev) -> Option<State> {
    use Ev::{LoadErr, LoadOk, LoadStart, UnloadDone, UnloadStart};
    use State::{Active, Crashed, Disabled, Loading, Registered, Unloading};
    Some(match (s, e) {
        (Registered | Disabled | Crashed, LoadStart) => Loading,
        (Loading, LoadOk) => Active,
        (Loading, LoadErr) => Crashed,
        (Active, UnloadStart) => Unloading,
        (Unloading, UnloadDone) => Disabled,
        _ => return None,
    })
}

// ---------------------------------------------------------------------------
// plugin.log（90-9 §7）
// ---------------------------------------------------------------------------

/// 一行日志：`<ts> load   ok    3120ms   注`。公开为纯函数便于单测。
fn format_line(stamp: &str, event: &str, ok: bool, ms: u128, note: &str) -> String {
    let result = if ok { "ok" } else { "ERR" };
    if note.is_empty() {
        format!("{stamp} {event:<6} {result:<5} {ms:>5}ms")
    } else {
        format!("{stamp} {event:<6} {result:<5} {ms:>5}ms   {note}")
    }
}

fn stamp_now() -> String {
    Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()
}

/// 追加一行到 `<插件目录>/plugin.log`。写失败静默——可观测性不反过来卡
/// 宿主路径（铁律：热路径禁止同步重活；这里是一次小 append，量级可忽略）。
fn append(dir: &Path, line: &str) {
    use std::io::Write;
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("plugin.log"))
    else {
        return;
    };
    let _ = f.write_all(format!("{line}\n").as_bytes());
}

/// `load` 事件落日志（ok = 装载+激活全过；note = 错误串/说明）。
pub(crate) fn log_load(dir: &Path, ok: bool, ms: u128, note: &str) {
    append(dir, &format_line(&stamp_now(), "load", ok, ms, note));
}

/// `unload` 事件落日志（停用 / 卸载 / 更新覆盖 / PF 退出四路共用）。
pub(crate) fn log_unload(dir: &Path, ok: bool, ms: u128, note: &str) {
    append(dir, &format_line(&stamp_now(), "unload", ok, ms, note));
}

/// 成对判定（90-9 §7）：最后一次 `load` 之后是否再无 `unload`。纯函数，
/// 可单测；无法识别的行跳过（前向兼容：将来加事件名不破坏判定）。
pub(crate) fn has_unclosed_load(content: &str) -> bool {
    let mut open = false;
    for line in content.lines() {
        match line.split_whitespace().nth(1) {
            Some("load") => open = true,
            Some("unload") => open = false,
            _ => {}
        }
    }
    open
}

/// 启动对账：存在未闭合 `load` = 上次异常退出（强杀/崩溃）→ 补一条
/// `unload ERR` 关闭配对，文本带「上次异常退出」供详情页/用户直读。
/// 幂等：补的行本身是 unload，下次启动不会再触发。
pub(crate) fn reconcile_startup(dir: &Path) {
    let Ok(content) = std::fs::read_to_string(dir.join("plugin.log")) else {
        return;
    };
    if has_unclosed_load(&content) {
        append(
            dir,
            &format_line(
                &stamp_now(),
                "unload",
                false,
                0,
                "上次异常退出，未执行 unload",
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: State, e: Ev) -> Option<State> {
        transition(s, e)
    }

    #[test]
    fn state_machine_happy_path() {
        use State::*;
        assert_eq!(t(Registered, Ev::LoadStart), Some(Loading));
        assert_eq!(t(Loading, Ev::LoadOk), Some(Active));
        assert_eq!(t(Active, Ev::UnloadStart), Some(Unloading));
        assert_eq!(t(Unloading, Ev::UnloadDone), Some(Disabled));
    }

    #[test]
    fn state_machine_error_and_rearm() {
        use State::*;
        // load 失败 → Crashed；用户手动重启用可再装载
        assert_eq!(t(Loading, Ev::LoadErr), Some(Crashed));
        assert_eq!(t(Crashed, Ev::LoadStart), Some(Loading));
        // 停用过的插件可再启用（台账开闸 + 懒装载重走 load）
        assert_eq!(t(Disabled, Ev::LoadStart), Some(Loading));
        // 非法转移：注册态没有 unload；Loading 直接 unload 不合法
        assert_eq!(t(Registered, Ev::UnloadStart), None);
        assert_eq!(t(Loading, Ev::UnloadStart), None);
        assert_eq!(t(Active, Ev::LoadStart), None);
    }

    #[test]
    fn line_format_matches_design() {
        assert_eq!(
            format_line("2026-10-30T21:04:12", "load", true, 3120, "首次校验+编译"),
            "2026-10-30T21:04:12 load   ok     3120ms   首次校验+编译"
        );
        assert_eq!(
            format_line("2026-10-31T09:12:07", "load", false, 45, "verify failed"),
            "2026-10-31T09:12:07 load   ERR      45ms   verify failed"
        );
        assert_eq!(
            format_line("2026-10-30T21:44:51", "unload", true, 2, ""),
            "2026-10-30T21:44:51 unload ok        2ms"
        );
    }

    #[test]
    fn pairing_detection() {
        assert!(!has_unclosed_load(""));
        // 设计文档 §7 示例：三行成对
        let ok = "2026-10-30T21:04:12 load   ok    3120ms\n\
                  2026-10-30T21:44:51 unload ok       2ms\n\
                  2026-10-31T09:12:07 load   ERR      45ms   verify failed";
        assert!(has_unclosed_load(ok), "末尾 load 未闭合");
        // 补过对账行（unload ERR 上次异常退出）后即闭合
        let closed = format!(
            "{ok}\n2026-10-31T09:13:00 unload ERR        0ms   上次异常退出，未执行 unload"
        );
        assert!(!has_unclosed_load(&closed));
        // 未知事件行不干扰判定（前向兼容）
        let noise = "2026-10-30T21:04:12 verify ok     3120ms\nload-x ok";
        assert!(!has_unclosed_load(noise));
    }
}
