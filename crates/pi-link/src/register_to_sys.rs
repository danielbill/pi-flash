//! 向系统注册（051 / 081）：`%LOCALAPPDATA%\pi-flash\bin` 用户级稳定 bin
//! 目录的地基 + 两个注册物。
//!
//! **地基**：安装树之外的稳定目录（版本化载荷目录随升级漂移、npm 重装整包
//! 替换，都碰不到它）+ HKCU `Environment\Path` **首位**段（判重、保原类型、
//! WM_SETTINGCHANGE 广播、本进程 PATH 同步）。
//!
//! **注册物 ①pif-secret**（051）：auth.json 标记条目去路径化的地基。事故
//! （0.4.0→0.4.1 实录）：旧标记把 helper 的**绝对路径**写进 auth.json，而
//! 该路径 = 版本化载荷目录——升级后旧目录被 `cleanup_older` 清掉，pi 的
//! `!` 命令取 key 全灭。治本：helper 落稳定目录，标记写**零路径裸命令**
//! `!pif-secret get <provider>`，升级/重装/搬家永不再崩。
//!
//! **注册物 ②全局 `pi`**（081）：`pi.exe` 前置转发器落 bin 目录 + 指针
//! `pi-target.json`（node/cli 路径由 PF 按自身/staged 载荷上下文解析写入），
//! 把 vendored 内核注册为系统默认 `pi`。段在 Path **首位**才能 shadow 老
//! npm 全局安装；不卸载不改它，纯遮蔽。
//!
//! pi 侧执行链（vendor/pi `resolve-config-value.js`）：`!` 后整串交
//! `bash -c`（无 Git Bash 回落 `execSync`），都走 PATH 解析；`where`/
//! PATHEXT/MSYS 自动补 `.exe`，裸命令名即可命中。PF spawn 的 pi 继承本进程
//! env（`client.rs` 无 clean env），故 ensure 里同步改本进程 PATH 让子链
//! 立即可用；注册表写回 + 广播让用户自己的新终端也可见。
//!
//! 铁律：**只动自己的段、判重、幂等**。Path 原类型（多为 REG_EXPAND_SZ，含
//! 未展开 `%VAR%`）读原始串、按原类型写回；用户已有段绝不改写，只把我们
//! 自己那一段挪到首位。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

/// 进程内「裸命令模式可用」标志：[`ensure_installed`] 成功后置位。
/// `auth_json::marker_key` 据此决定写裸命令还是回落绝对路径（老行为）。
static PATH_MODE_OK: AtomicBool = AtomicBool::new(false);

/// 标记条目是否可用裸命令形态。
pub fn path_mode_ok() -> bool {
    PATH_MODE_OK.load(Ordering::Relaxed)
}

/// 用户级稳定 bin 目录：`%LOCALAPPDATA%\pi-flash\bin`。
/// 放安装树之外——版本化载荷目录随升级漂移、npm 重装整包替换，都碰不到它。
pub fn stable_bin_dir() -> Option<PathBuf> {
    stable_bin_dir_from(std::env::var_os("LOCALAPPDATA").as_deref())
}

/// `stable_bin_dir` 的纯函数变体（注入 LOCALAPPDATA，供测试）。
pub(crate) fn stable_bin_dir_from(base: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let base = base?;
    if base.is_empty() {
        return None;
    }
    Some(PathBuf::from(base).join("pi-flash").join("bin"))
}

/// helper 源文件：与本进程 exe 同目录（auth_json::helper_exe 同源）。
fn helper_src() -> PathBuf {
    crate::auth_json::helper_exe()
}

/// 确保 pif-secret 在用户级 bin 目录 + PATH 里（幂等，毫秒级）。成功 →
/// 置位 PATH_MODE_OK 并返回 true；任何一步失败保持 false（marker 回落
/// 绝对路径老行为，功能不损）。
#[cfg(windows)]
pub fn ensure_installed() -> bool {
    let ok = ensure_windows();
    PATH_MODE_OK.store(ok, Ordering::Relaxed);
    ok
}

/// 非 Windows：保持老行为（marker 带绝对路径；macOS .app 布局路径稳定）。
#[cfg(not(windows))]
pub fn ensure_installed() -> bool {
    false
}

#[cfg(windows)]
fn ensure_windows() -> bool {
    let Some(bin_dir) = stable_bin_dir() else {
        #[cfg(debug_assertions)]
        eprintln!("[register] ensure 失败：LOCALAPPDATA 缺失");
        return false;
    };
    install_helper(&bin_dir);
    if !ensure_on_path(&bin_dir) {
        #[cfg(debug_assertions)]
        eprintln!("[register] ensure 失败：registry/path");
        return false;
    }
    append_process_path(&bin_dir);
    true
}

/// 复制 helper 到 bin 目录。字节相同跳过；目标被占用（pi 正在解析凭据）时
/// 复制失败忽略——helper 只读钥匙库、接口稳定，旧件继续可用，下次启动再试。
#[cfg(windows)]
fn install_helper(bin_dir: &std::path::Path) {
    copy_self_sibling("pif-secret.exe", bin_dir);
}

/// 把本进程 exe 同目录的 `name` 复制进 bin 目录（幂等：字节相同跳过；源缺
/// 席静默——旧载荷没有 pi.exe，全局注册下版起效；目标被占用忽略，下次再试）。
#[cfg(windows)]
fn copy_self_sibling(name: &str, bin_dir: &std::path::Path) {
    let Some(src) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(name)))
    else {
        return;
    };
    if !src.is_file() {
        return;
    }
    if std::fs::create_dir_all(bin_dir).is_err() {
        return;
    }
    let dst = bin_dir.join(name);
    if let (Ok(new), Ok(old)) = (std::fs::read(&src), std::fs::read(&dst)) {
        if new == old {
            return;
        }
    }
    let _ = std::fs::copy(&src, &dst);
}

/// 全局 `pi` shim 开关：release 默认开；debug 默认关（stable_entry_exe 先例：
/// 开发跑多少遍都不许碰用户 shell），`PI_FLASH_PI_SHIM=1` 显式开启供开发验证。
pub fn pi_shim_enabled() -> bool {
    cfg!(not(debug_assertions)) || std::env::var("PI_FLASH_PI_SHIM").is_ok_and(|v| v == "1")
}

/// 把 PF 内置 pi 内核注册为系统全局 `pi`（幂等，毫秒级）：
/// ① PATH：bin 目录段确保在用户 Path **首位**（shadow 老 npm 全局 pi）；
/// ② 复制 `pi.exe`（本进程 exe 同目录 → bin 目录）；
/// ③ 写 `pi-target.json` 指针——`node`/`cli` 由调用方按上下文解析：updater
///    传 **staged 新载荷**（交接后即新内核），启动传当前载荷（自愈漂移）。
/// pi.exe 缺席（旧载荷）→ 复制跳过、指针照写。返回整体是否成功。
#[cfg(windows)]
pub fn ensure_pi_shim(node: &str, cli: &std::path::Path) -> bool {
    if !pi_shim_enabled() {
        return false;
    }
    let Some(bin_dir) = stable_bin_dir() else {
        return false;
    };
    if !ensure_on_path(&bin_dir) {
        return false;
    }
    copy_self_sibling("pi.exe", &bin_dir);
    write_pi_pointer(&bin_dir, node, cli)
}

/// 非 Windows：暂不注册（发版只出 Windows x64）。
#[cfg(not(windows))]
pub fn ensure_pi_shim(_node: &str, _cli: &std::path::Path) -> bool {
    false
}

/// 写 `pi-target.json`（`{"node":…,"cli":…}`，内容不同才写 → 幂等）。
#[cfg(windows)]
fn write_pi_pointer(bin_dir: &std::path::Path, node: &str, cli: &std::path::Path) -> bool {
    let new = format!(
        "{}\n",
        serde_json::to_string_pretty(&serde_json::json!({
            "node": node,
            "cli": cli.to_string_lossy(),
        }))
        .unwrap_or_default()
    );
    let path = bin_dir.join("pi-target.json");
    if std::fs::read_to_string(&path).is_ok_and(|old| old == new) {
        return true;
    }
    std::fs::write(&path, new).is_ok()
}

/// HKCU `Environment\Path` 确保我们的段在**首位**（shadow 既有同名命令——
/// 全局 `pi` 必须赢过老 npm 全局安装；判重、保原类型写回）。
#[cfg(windows)]
fn ensure_on_path(bin_dir: &std::path::Path) -> bool {
    use windows_registry::{CURRENT_USER, Type};

    // open() 默认只读（options().read()）——写 Path 必须显式要 KEY_WRITE。
    let Ok(key) = CURRENT_USER.options().read().write().open("Environment") else {
        #[cfg(debug_assertions)]
        eprintln!("[register] open(Environment) 失败");
        return false;
    };
    let dir = match bin_dir.to_str() {
        Some(d) => d.to_string(),
        None => return false,
    };
    // 读原始值：String::try_from 对 REG_SZ / REG_EXPAND_SZ 都给**未展开**串，
    // `%VAR%` 原样保留；其他类型（multi_sz 等）不碰——宁可不用 PATH 模式。
    let (current, expand) = match key.get_value("Path") {
        Ok(raw) => match (String::try_from(raw.clone()), raw.ty()) {
            (Ok(s), Type::ExpandString) => (s, true),
            (Ok(s), Type::String) => (s, false),
            _ => {
                #[cfg(debug_assertions)]
                eprintln!("[register] Path 值类型不识别");
                return false;
            }
        },
        Err(e) => {
            #[cfg(debug_assertions)]
            eprintln!("[register] Path 读取失败（按空处理）：{e}");
            (String::new(), true)
        } // 无 Path 值 → 按系统惯例新建 REG_EXPAND_SZ
    };
    // 已首位 → 无需写回；否则摘旧位 + 前插（只动我们自己的段）。
    let Some(new) = path_with_segment_front(&current, &dir) else {
        return true;
    };
    let written = if expand {
        key.set_expand_string("Path", &new)
    } else {
        key.set_string("Path", &new)
    };
    if let Err(e) = written {
        #[cfg(debug_assertions)]
        eprintln!("[register] Path 写回失败：{e}");
        return false;
    }
    // 广播让 Explorer / 已开终端立即重读环境（否则新壳要等重登录才见新段）。
    broadcast_env_change();
    true
}

/// `WM_SETTINGCHANGE`("Environment") 广播，2s 超时跳过挂死窗口。
#[cfg(windows)]
fn broadcast_env_change() {
    const WM_SETTINGCHANGE: u32 = 0x001A;
    const SMTO_ABORTIFHUNG: u32 = 0x0002;
    use windows_sys::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SendMessageTimeoutW};
    // "Environment" UTF-16 + NUL
    let param: Vec<u16> = "Environment\0".encode_utf16().collect();
    unsafe {
        SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            0,
            param.as_ptr() as isize,
            SMTO_ABORTIFHUNG,
            2000,
            std::ptr::null_mut(),
        );
    }
}

/// 本进程 PATH 追加（pi 子进程继承，PF 链路立即可解析）。edition 2024 下
/// `set_var` 为 unsafe：仅启动后台线程跑一次、且判重后才写；std 侧有内部
/// 锁，Rust 并发读安全，FFI 缓存风险在 Windows 上无（子进程 env 在
/// CreateProcess 时快照）。
#[cfg(windows)]
fn append_process_path(bin_dir: &std::path::Path) {
    let Some(dir) = bin_dir.to_str() else {
        return;
    };
    let current = std::env::var("PATH").unwrap_or_default();
    if path_has_segment(&current, dir) {
        return;
    }
    let new = if current.is_empty() {
        dir.to_string()
    } else {
        format!("{current};{dir}")
    };
    unsafe { std::env::set_var("PATH", new) };
}

/// PATH 值按 `;` 分段后是否已含 `dir`：大小写不敏感、忽略空段、段尾分隔符
/// 不敏感。纯函数供测试；注册表里的 `%VAR%` 段永远不等于绝对路径 dir，
/// 不会误判重。
pub(crate) fn path_has_segment(path_value: &str, dir: &str) -> bool {
    let want = dir.trim_end_matches(['\\', '/']).to_ascii_lowercase();
    if want.is_empty() {
        return false;
    }
    path_value.split(';').any(|seg| {
        let seg = seg.trim().trim_end_matches(['\\', '/']);
        !seg.is_empty() && seg.to_ascii_lowercase() == want
    })
}

/// 把 PATH 值里的 `dir` 段放到**首位**：已首位 → `None`（无需写回）；否则
/// 摘除全部旧位（幂等防重复）后前插，其余段原序原样保留。纯函数供测试；
/// 归一化规则同 [`path_has_segment`]。空 Path 直接给 `dir`。
pub(crate) fn path_with_segment_front(path_value: &str, dir: &str) -> Option<String> {
    let want = dir.trim_end_matches(['\\', '/']).to_ascii_lowercase();
    if want.is_empty() {
        return None;
    }
    let norm = |s: &str| s.trim().trim_end_matches(['\\', '/']).to_ascii_lowercase();
    if path_value
        .split(';')
        .next()
        .is_some_and(|first| norm(first) == want)
    {
        return None;
    }
    let kept: Vec<&str> = path_value.split(';').filter(|s| norm(s) != want).collect();
    if path_value.trim().is_empty() {
        return Some(dir.to_string());
    }
    Some(format!("{dir};{}", kept.join(";")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_segment_match_is_normalized() {
        let p = r"C:\Users\x\AppData\Local\pi-flash\bin";
        // 完全一致 / 大小写 / 尾反斜杠 / 尾分号 / 空段
        assert!(path_has_segment(p, p));
        assert!(path_has_segment(
            r"c:\users\x\appdata\local\pi-flash\bin",
            p
        ));
        assert!(path_has_segment(&format!("{p}\\"), p));
        assert!(path_has_segment(&format!("A;B;;{p};"), p));
        // 未含
        assert!(!path_has_segment(r"C:\Windows\System32", p));
        // 前缀相同不等于段相同（防误判重）
        assert!(!path_has_segment(
            r"C:\Users\x\AppData\Local\pi-flash\bin2",
            p
        ));
        // %VAR% 段（未展开）绝不等于绝对路径
        assert!(!path_has_segment(r"%LOCALAPPDATA%\pi-flash\bin", p));
        // 空入参
        assert!(!path_has_segment("", p));
        assert!(!path_has_segment("a;b", ""));
    }

    #[cfg(windows)]
    #[test]
    fn stable_bin_dir_layout() {
        use std::ffi::OsStr;
        let dir = stable_bin_dir_from(Some(OsStr::new(r"C:\u\appdata\local"))).unwrap();
        assert!(dir.ends_with(r"pi-flash\bin"), "{dir:?}");
        // 空 / 缺失 LOCALAPPDATA → None
        assert!(stable_bin_dir_from(Some(OsStr::new(""))).is_none());
        assert!(stable_bin_dir_from(None).is_none());
    }

    #[test]
    fn path_front_insert_move_and_noop() {
        let dir = r"C:\u\AppData\Local\pi-flash\bin";
        // 缺席 → 前插，其余原序保留
        assert_eq!(
            path_with_segment_front(r"C:\Windows;D:\node", dir).as_deref(),
            Some(format!("{dir};C:\\Windows;D:\\node").as_str())
        );
        // 已首位 → None（无需写回）
        assert_eq!(
            path_with_segment_front(&format!("{dir};C:\\Windows"), dir),
            None
        );
        // 大小写 / 尾分隔符归一后已首位 → None
        assert_eq!(
            path_with_segment_front("c:\\U\\APPDATA\\LOCAL\\PI-FLASH\\BIN\\;C:\\Windows", dir),
            None
        );
        // 空值 → 直接给 dir
        assert_eq!(path_with_segment_front("", dir).as_deref(), Some(dir));
        // 重复段（异常历史）→ 摘全部 + 前插一份；空段原样保留
        assert_eq!(
            path_with_segment_front(&format!("a;;{dir};b;{dir}\\"), dir).as_deref(),
            Some(format!("{dir};a;;b").as_str())
        );
        // %VAR% 段绝不匹配绝对路径（不误摘）
        assert_eq!(
            path_with_segment_front(r"%LOCALAPPDATA%\pi-flash\bin", dir).as_deref(),
            Some(format!("{dir};%LOCALAPPDATA%\\pi-flash\\bin").as_str())
        );
        // 空入参 → None
        assert_eq!(path_with_segment_front("a;b", ""), None);
    }

    #[cfg(windows)]
    #[test]
    fn pi_pointer_idempotent() {
        let dir = std::env::temp_dir().join(format!(
            "pf-shim-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cli = dir.join("cli.js");
        assert!(write_pi_pointer(&dir, "node", &cli));
        let first = std::fs::read_to_string(dir.join("pi-target.json")).unwrap();
        // 同内容二写 → 幂等
        assert!(write_pi_pointer(&dir, "node", &cli));
        assert_eq!(
            std::fs::read_to_string(dir.join("pi-target.json")).unwrap(),
            first
        );
        // 内容变化 → 更新
        assert!(write_pi_pointer(&dir, "node2", &cli));
        assert_ne!(
            std::fs::read_to_string(dir.join("pi-target.json")).unwrap(),
            first
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
