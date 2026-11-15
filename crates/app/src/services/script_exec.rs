//! 021 文件树右键【执行】：扩展名 → 启动配方的懒解析器（设计见
//! docs/模块设计/021-目录文件树.md「文件执行」节）。
//!
//! - **配方表回答"怎么启动"**（bat 要 cmd、ps1 要 -ExecutionPolicy
//!   Bypass…）。有意不查系统文件关联：`.js` 关联是 wscript（WSH）方向性
//!   错误，关联表也表达不了附加 flags 与 cwd 语义
//! - **探测链回答"解释器在哪"**，仅 py / sh 两类需要：点击时懒执行 +
//!   会话内正/负缓存；PATH 扫描按驱动器类型只走固定磁盘，UNC/网络/可移
//!   动盘跳过——焊死死网络路径 SMB 秒级超时这一唯一慢源
//! - **执行通道 = PF 内置终端新开 tab**：`cmd /k cd /d "<目录>" && <命令>`。
//!   前置 `cd` 一半是 cwd 语义（bat 的 %~dp0 依赖、相对路径），另一半是
//!   cmd 引号规则护盾——cmd /? 规则 2 只在 /k 后**首字符是引号**时剥首尾
//!   引号，先垫一个裸词 `cd` 整串免疫；PTY 的 working_directory 另行落目录

use std::path::{Path, PathBuf};

/// 可执行扩展名 → 配方种类（菜单可见性判据；大小写归一）
pub(crate) fn exec_kind(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "exe" | "com" => Some("exe"),
        "bat" | "cmd" => Some("bat"),
        "ps1" => Some("ps1"),
        "js" | "mjs" | "cjs" => Some("js"),
        "py" => Some("py"),
        "sh" => Some("sh"),
        _ => None,
    }
}

/// 一次执行所需的一切：菜单展示名 + `cmd /k ` 后面的整串命令
#[derive(Clone)]
pub(crate) struct ExecPlan {
    /// 菜单后缀（`执行 · {label}`）。取解释器名而非版本号——探测不许起
    /// 子进程（`--version` 要 spawn + 闪控制台，与懒解析相悖）
    pub(crate) label: String,
    /// `cmd /k ` 之后的命令串（含 cd 护盾前缀）
    pub(crate) command: String,
}

/// 右键【执行】解析入口：None = 解析失败（菜单置灰带原因）
pub(crate) fn resolve(path: &Path, ws_cwd: Option<&Path>) -> Option<ExecPlan> {
    let kind = exec_kind(path)?;
    let dir = path.parent()?.to_path_buf();
    let file = quote(path);
    let (label, inner) = match kind {
        // exe/com 是自己；bat/cmd 在外层 cmd 实例里直接跑
        "exe" => (String::new(), file),
        "bat" => ("bat".to_string(), file),
        "ps1" => (
            "powershell".to_string(),
            format!("powershell -NoProfile -ExecutionPolicy Bypass -File {file}"),
        ),
        "js" => {
            // pi-link vendor 链：捆绑 node > PI_FLASH_NODE > PATH——PF 发行
            // 版自带 node，此配方零探测
            let node = pi_link::vendor::node_bin();
            ("node".to_string(), format!("{} {file}", q(&node)))
        }
        "py" => match probe_py(ws_cwd)? {
            PyInterp::Launcher(p) => ("py -3".to_string(), format!("{} -3 {file}", quote(&p))),
            PyInterp::Direct(p) => (py_label(&p), format!("{} {file}", quote(&p))),
        },
        "sh" => {
            let bash = probe_sh()?;
            ("bash".to_string(), format!("{} {file}", quote(&bash)))
        }
        _ => return None,
    };
    Some(ExecPlan {
        label,
        command: format!("cd /d {} && {inner}", quote(&dir)),
    })
}

// ---------------------------------------------------------------------------
// 探测链（py / sh；其余配方零探测）
// ---------------------------------------------------------------------------

#[derive(Clone)]
enum PyInterp {
    /// PEP 397 py 启动器（python.org 安装必落 %SystemRoot%，调用带 -3）
    Launcher(PathBuf),
    /// python.exe 直调
    Direct(PathBuf),
}

/// py 探测链：项目 venv（每次现查，只 1~2 个 stat，切项目即换）→
/// py.exe → PATH python.exe（跳过 WindowsApps 假别名）。全局段走缓存。
fn probe_py(ws_cwd: Option<&Path>) -> Option<PyInterp> {
    if let Some(cwd) = ws_cwd {
        // venv 里的依赖几乎总比全局 python 正确——编码工具的第一优先
        for name in [".venv", "venv"] {
            let p = cwd.join(name).join("Scripts").join("python.exe");
            if p.is_file() {
                return Some(PyInterp::Direct(p));
            }
        }
    }
    cached_py(|| {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let p = PathBuf::from(root).join("py.exe");
            if p.is_file() {
                return Some(PyInterp::Launcher(p));
            }
        }
        find_in_path("python.exe", &is_store_alias_dir).map(PyInterp::Direct)
    })
}

/// sh 探测链：PATH bash.exe（排除 System32——那是 WSL 启动器）→ Git for
/// Windows / scoop 固定安装位。
fn probe_sh() -> Option<PathBuf> {
    cached_sh(|| {
        find_in_path("bash.exe", &is_under_system32).or_else(|| {
            [
                ("ProgramFiles", r"Git\bin\bash.exe"),
                ("ProgramFiles(x86)", r"Git\bin\bash.exe"),
                ("LOCALAPPDATA", r"Programs\Git\bin\bash.exe"),
                ("USERPROFILE", r"scoop\apps\git\current\bin\bash.exe"),
            ]
            .iter()
            .find_map(|(var, tail)| {
                let p = std::env::var_os(var).map(|base| PathBuf::from(base).join(tail));
                p.filter(|p| p.is_file())
            })
        })
    })
}

// 缓存用两个独立静态而非共享 HashMap——py/sh 返回类型不同，独立静态
// 换来类型安全 + 负缓存（None 也记，连点不重扫）。

fn cached_py(f: impl FnOnce() -> Option<PyInterp>) -> Option<PyInterp> {
    static CACHE: std::sync::LazyLock<std::sync::Mutex<Option<Option<PyInterp>>>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
    let mut slot = CACHE.lock().ok()?;
    slot.get_or_insert_with(f).clone()
}

fn cached_sh(f: impl FnOnce() -> Option<PathBuf>) -> Option<PathBuf> {
    static CACHE: std::sync::LazyLock<std::sync::Mutex<Option<Option<PathBuf>>>> =
        std::sync::LazyLock::new(|| std::sync::Mutex::new(None));
    let mut slot = CACHE.lock().ok()?;
    slot.get_or_insert_with(f).clone()
}

/// PATH 逐目录找 `file`；`skip` 排除伪解释器目录（WindowsApps / System32），
/// 非固定磁盘（UNC/网络/可移动）整目录跳过
fn find_in_path(file: &str, skip: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|d| !d.as_os_str().is_empty())
        .filter(|d| !skip(d))
        .filter(|d| fixed_local_dir(d))
        .map(|d| d.join(file))
        .find(|c| c.is_file())
}

/// Microsoft Store 执行别名目录（假 python3/python 点了弹商店）
fn is_store_alias_dir(dir: &Path) -> bool {
    dir.components()
        .any(|c| c.as_os_str().eq_ignore_ascii_case("WindowsApps"))
}

/// System32 子树（bash.exe 同名 WSL 启动器驻地）
fn is_under_system32(dir: &Path) -> bool {
    let low = dir.to_string_lossy().to_ascii_lowercase();
    std::env::var("SystemRoot")
        .map(|root| format!("{}\\system32", root.to_ascii_lowercase()))
        .map(|s32| low.starts_with(&s32))
        .unwrap_or(false)
}

/// 只扫固定磁盘：UNC（`\\`开头）与非 `X:` 形态一律不扫；盘符根用
/// GetDriveTypeW 判 DRIVE_FIXED——死网络路径的 metadata 会等 SMB 超时（秒
/// 级），这是懒解析唯一可能的慢源，在此焊死
#[cfg(windows)]
fn fixed_local_dir(dir: &Path) -> bool {
    use windows_sys::Win32::Storage::FileSystem::GetDriveTypeW;
    // windows-sys 0.52 把 DRIVE_* 常量放在 WindowsProgramming 组
    use windows_sys::Win32::System::WindowsProgramming::DRIVE_FIXED;
    let s = dir.to_string_lossy();
    if s.starts_with("\\\\") {
        return false;
    }
    let mut cs = s.chars();
    match cs.next() {
        Some(c) if c.is_ascii_alphabetic() && cs.next() == Some(':') => {}
        _ => return false,
    }
    let root = format!("{}\\", &s[..2]);
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY：wide 是合法 NUL 结尾 UTF-16；GetDriveTypeW 仅读该串
    unsafe { GetDriveTypeW(wide.as_ptr()) == DRIVE_FIXED }
}

#[cfg(not(windows))]
fn fixed_local_dir(_dir: &Path) -> bool {
    true
}

// ---------------------------------------------------------------------------
// 拼装辅助
// ---------------------------------------------------------------------------

/// 路径加引号（NTFS 文件名不可能含 `"`，直包即可）
fn quote(p: &Path) -> String {
    format!("\"{}\"", p.display())
}

/// 046 脚本任务的子进程配方：解释器 program + argv（**不经** cmd /k 外壳；
/// bat/cmd 走 `cmd /C`）。与 [`resolve`] 共享探测链与 node 路径，两处配方
/// 语义不许漂移（resolve 管「终端里跑」，本函数管「子进程跑」，扩展名
/// 支持集以 exec_kind 为唯一事实源）。
/// 返回 (program, args)；cwd 恒 = 脚本所在目录（调用方落 current_dir）。
pub(crate) fn spawn_plan(path: &Path, ws_cwd: Option<&Path>) -> Option<(PathBuf, Vec<String>)> {
    let kind = exec_kind(path)?;
    let file = path.to_string_lossy().to_string();
    match kind {
        // exe/com 直接自身启动（com 由 loader 接管）
        "exe" => Some((path.to_path_buf(), Vec::new())),
        "bat" => {
            let com = std::env::var_os("COMSPEC")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("cmd"));
            Some((com, vec!["/C".into(), file]))
        }
        "ps1" => Some((
            PathBuf::from("powershell"),
            vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                file,
            ],
        )),
        "js" => Some((PathBuf::from(pi_link::vendor::node_bin()), vec![file])),
        "py" => match probe_py(ws_cwd)? {
            PyInterp::Launcher(p) => Some((p, vec!["-3".into(), file])),
            PyInterp::Direct(p) => Some((p, vec![file])),
        },
        "sh" => Some((probe_sh()?, vec![file])),
        _ => None,
    }
}

/// 裸命令名（node_bin 可能是 "node" 也可能是绝对路径）仅含空格才包引号
fn q(name: &str) -> String {
    if name.contains(' ') {
        format!("\"{name}\"")
    } else {
        name.to_string()
    }
}

/// 解释器展示名：python.exe → "python"（py 启动器/venv 另有专属 label）
fn py_label(p: &Path) -> String {
    p.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("python")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_kind_maps_case_insensitively() {
        assert_eq!(exec_kind(Path::new("A.BAT")), Some("bat"));
        assert_eq!(exec_kind(Path::new("x.Script.PS1")), Some("ps1"));
        assert_eq!(exec_kind(Path::new("app.EXE")), Some("exe"));
        assert_eq!(exec_kind(Path::new("noext")), None);
        assert_eq!(exec_kind(Path::new("readme.txt")), None);
    }

    /// cmd 引号规则护盾：`cd` 垫词让 /k 后首字符不是引号，规则 2（剥首尾
    /// 引号）整串免疫；空格路径全程保持引号
    #[test]
    fn bat_plan_has_cd_shield_and_quoted_paths() {
        let dir = std::env::temp_dir().join(format!("pf_exec_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("run me.bat");
        std::fs::write(&f, b"@echo off").unwrap();
        let plan = resolve(&f, None).expect("bat 配方零探测，恒可解析");
        assert_eq!(plan.label, "bat");
        assert!(plan.command.starts_with("cd /d \""));
        assert!(plan.command.contains("&& \""));
        assert!(plan.command.ends_with("run me.bat\""));
        std::fs::remove_file(&f).ok();
        std::fs::remove_dir(&dir).ok();
    }

    #[test]
    fn ps1_carries_bypass_flag() {
        let dir = std::env::temp_dir().join(format!("pf_exec_test2_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("a.ps1");
        std::fs::write(&f, b"echo hi").unwrap();
        let plan = resolve(&f, None).unwrap();
        assert!(
            plan.command.contains("-ExecutionPolicy Bypass -File \""),
            "got: {}",
            plan.command
        );
        std::fs::remove_file(&f).ok();
        std::fs::remove_dir(&dir).ok();
    }
}
