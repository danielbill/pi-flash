//! `pif-secret` 稳定安装（051）：auth.json 标记条目去路径化的地基。
//!
//! 事故（0.4.0→0.4.1 实录）：旧标记把 `pif-secret.exe` 的**绝对路径**写进
//! auth.json，而该路径 = 版本化载荷目录（`payload-<版本>\`）——升级后旧目录
//! 被 `cleanup_older` 清掉，pi 的 `!` 命令取 key 全灭。治本：helper 落到
//! 安装树之外的**用户级稳定目录**（`%LOCALAPPDATA%\pi-flash\bin`）并加入
//! HKCU PATH，标记条目从此写**零路径裸命令** `!pif-secret get <provider>`
//! ——升级/重装/搬家永不再崩。
//!
//! pi 侧执行链（vendor/pi `resolve-config-value.js`）：`!` 后整串交
//! `bash -c`（无 Git Bash 回落 `execSync`），都走 PATH 解析；`where`/
//! PATHEXT/MSYS 自动补 `.exe`，裸命令名即可命中。PF spawn 的 pi 继承本进程
//! env（`client.rs` 无 clean env），故 ensure 里同步追加本进程 PATH 让子链
//! 立即可用；注册表写回 + WM_SETTINGCHANGE 广播让用户自己的新终端也可见。
//!
//! 铁律：**只追加、判重、幂等**。Path 原类型（多为 REG_EXPAND_SZ，含未展开
//! `%VAR%`）读原始串、按原类型写回；用户已有段绝不改写。

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
        eprintln!("[secret-bin] ensure 失败：LOCALAPPDATA 缺失");
        return false;
    };
    install_helper(&bin_dir);
    if !ensure_on_path(&bin_dir) {
        #[cfg(debug_assertions)]
        eprintln!("[secret-bin] ensure 失败：registry/path");
        return false;
    }
    append_process_path(&bin_dir);
    true
}

/// 复制 helper 到 bin 目录。字节相同跳过；目标被占用（pi 正在解析凭据）时
/// 复制失败忽略——helper 只读钥匙库、接口稳定，旧件继续可用，下次启动再试。
#[cfg(windows)]
fn install_helper(bin_dir: &std::path::Path) {
    let src = helper_src();
    if !src.is_file() {
        return;
    }
    let Some(name) = src.file_name() else {
        return;
    };
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

/// HKCU `Environment\Path` 追加 bin 目录（判重、保原类型写回）。
#[cfg(windows)]
fn ensure_on_path(bin_dir: &std::path::Path) -> bool {
    use windows_registry::{CURRENT_USER, Type};

    // open() 默认只读（options().read()）——写 Path 必须显式要 KEY_WRITE。
    let Ok(key) = CURRENT_USER.options().read().write().open("Environment") else {
        #[cfg(debug_assertions)]
        eprintln!("[secret-bin] open(Environment) 失败");
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
                eprintln!("[secret-bin] Path 值类型不识别");
                return false;
            }
        },
        Err(e) => {
            #[cfg(debug_assertions)]
            eprintln!("[secret-bin] Path 读取失败（按空处理）：{e}");
            (String::new(), true)
        } // 无 Path 值 → 按系统惯例新建 REG_EXPAND_SZ
    };
    if path_has_segment(&current, &dir) {
        return true;
    }
    let mut new = current;
    if !new.is_empty() && !new.ends_with(';') {
        new.push(';');
    }
    new.push_str(&dir);
    let written = if expand {
        key.set_expand_string("Path", &new)
    } else {
        key.set_string("Path", &new)
    };
    if let Err(e) = written {
        #[cfg(debug_assertions)]
        eprintln!("[secret-bin] Path 写回失败：{e}");
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
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST,
    };
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
}
