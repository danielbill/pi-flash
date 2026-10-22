//! Windows 任务栏「固定到任务栏」稳定性——081 版本化载荷布局的衍生修复。
//!
//! **症状**（用户实录，每次更新必现）：PF 启动后固定到任务栏，某次更新后
//! 点固定图标就「找不到 pi-flash」，只剩 terminal（npm 薄壳）能启动。
//!
//! **根因**：任务栏固定 lnk 记录的是**固定那一刻**窗口进程的 exe 绝对路径。
//! 081 的启动链让窗口常年运行在版本化目录里（薄壳挑最高版本 /
//! 「点击重启更新」/ 启动交接，落点都是 `payload-<版本>\pi-flash.exe`），
//! 而下一次更新落位后 `cleanup_older` 会删掉旧版本目录——固定 lnk 的目标
//! 随之消失。terminal 能启动，是因为薄壳每次重新挑「版本最高的存活载荷」。
//!
//! **修复**（Windows 标准姿势，Chrome 同款三件套）：
//! 1. 进程级显式 AUMID（`SetCurrentProcessExplicitAppModelID`，必须在首个
//!    窗口创建前调用）——所有版本的 PF 窗口共用同一任务栏身份；
//! 2. 启动时确保开始菜单里有**带同一 AUMID 的稳定入口 lnk**（指向永不
//!    被清理的 `payload\pi-flash.exe`，见 updater::stable_entry_exe）——
//!    explorer 对带显式 AUMID 的窗口执行「固定」时优先认领开始菜单里同
//!    AUMID 的现成快捷方式，固定住的就是稳定入口；
//! 3. 自愈：扫任务栏固定区（`User Pinned\TaskBar`），凡指向版本化载荷
//!    目录的 pi-flash lnk（已死或行将就死）都把目标改写到稳定入口——
//!    老用户的坏 pin 升级后**原地复活**，无需重新固定。
//!
//! 纪律：debug 构建与非安装布局一概不碰文件系统快捷方式（开发跑多少遍都
//! 不许动用户的开始菜单/任务栏）；失败只打 stderr 日志，绝不影响启动。

use std::mem::ManuallyDrop;
use std::path::{Path, PathBuf};

use windows::Win32::Storage::EnhancedStorage::PKEY_AppUserModel_ID;
use windows::Win32::System::Com::StructuredStorage::{
    PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx, IPersistFile,
    STGM_READ,
};
use windows::Win32::System::Variant::VT_LPWSTR;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;
use windows::Win32::UI::Shell::{
    IShellLinkW, SHCNE_UPDATEITEM, SHCNF_PATH, SHChangeNotify, SLGP_RAWPATH,
};
use windows::core::{HSTRING, Interface, PWSTR};

/// AUMID（任务栏身份键）。**铁律：一经发布永不可改**——用户的任务栏固定
/// lnk 与跳转列表里记着它，改了等于全体用户固定全失效。
const AUMID: &str = "dev.piflash.app";

/// CLSID_ShellLink（.lnk 的 COM 类，shell32 自古稳定）；windows 0.61 元数据
/// 未生成 CoClass 包装，直接用 GUID。
const CLSID_SHELLLINK: windows::core::GUID =
    windows::core::GUID::from_u128(0x00021401_0000_0000_c000_000000000046);

/// 开始菜单稳定入口 lnk。
fn start_menu_lnk() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("pi-flash.lnk"),
    )
}

/// 任务栏固定区（pin lnk 的真身目录）。
fn taskbar_pin_dir() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("APPDATA")?)
            .join("Microsoft")
            .join("Internet Explorer")
            .join("Quick Launch")
            .join("User Pinned")
            .join("TaskBar"),
    )
}

/// main() 开窗之前调用。AUMID 必须在首个窗口创建前设置（同步、微秒级）；
/// lnk 兜底与死链自愈是纯磁盘/COM 活，后台线程做，不占启动路径。
pub(crate) fn prepare() {
    set_process_aumid();
    let _ = std::thread::Builder::new()
        .name("win-pin".into())
        .spawn(|| {
            ensure_start_menu_shortcut();
            repair_taskbar_pins();
        });
}

/// windows 0.61 / windows-sys 0.52 的元数据都没生成这个 shell32 出口，且
/// 实测 SDK 的 shell32.lib 符号表里**根本没有**它（MSDN 标的导入库名有误，
/// 静态声明必 LNK2019）——只能运行时 GetProcAddress 绑定。
fn set_process_aumid() {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    let mut wide: Vec<u16> = AUMID.encode_utf16().collect();
    wide.push(0);
    unsafe {
        let module = GetModuleHandleA(b"shell32.dll\0".as_ptr());
        if module == 0 {
            eprintln!("[win-pin] shell32 未加载，AUMID 没设上");
            return;
        }
        let Some(farproc) = GetProcAddress(
            module,
            b"SetCurrentProcessExplicitAppModelID\0".as_ptr(),
        ) else {
            eprintln!("[win-pin] shell32 缺 SetCurrentProcessExplicitAppModelID，AUMID 没设上");
            return;
        };
        let set_aumid: unsafe extern "system" fn(*const u16) -> i32 =
            std::mem::transmute(farproc);
        let hr = set_aumid(wide.as_ptr());
        if hr.is_negative() {
            eprintln!("[win-pin] AUMID 设置失败 hr=0x{hr:08x}");
        }
    }
}

/// 确保开始菜单有稳定入口 lnk（缺了才建；npm 更新只换 `payload\` 内容
/// 不换路径，已建的恒有效，不必每次重写）。
fn ensure_start_menu_shortcut() {
    let Some(target) = crate::services::updater::stable_entry_exe() else {
        return;
    };
    let Some(lnk) = start_menu_lnk() else { return };
    if lnk.exists() {
        return;
    }
    if let Some(parent) = lnk.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match write_shortcut(&lnk, &target) {
        Ok(()) => notify_shell(&lnk),
        Err(e) => eprintln!("[win-pin] 开始菜单入口 lnk 写入失败：{e}"),
    }
}

/// 自愈历史坏 pin：任务栏固定区里凡指向**版本化载荷目录**的 pi-flash lnk
/// （目录已被 cleanup 删掉 = 死链；还在 = 下次更新必死）都把目标改写到
/// 稳定入口。其余快捷方式（含指向稳定入口/开发 exe 的）一律不碰。
fn repair_taskbar_pins() {
    let Some(dir) = taskbar_pin_dir() else { return };
    let Some(stable) = crate::services::updater::stable_entry_exe() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if !matches!(p.extension(), Some(ext) if ext.eq_ignore_ascii_case("lnk")) {
            continue;
        }
        if read_shortcut_target(&p)
            .filter(|t| pin_points_at_versioned_payload(t, &stable))
            .is_some()
        {
            match write_shortcut(&p, &stable) {
                Ok(()) => notify_shell(&p),
                Err(err) => eprintln!("[win-pin] {} 修复失败：{err}", p.display()),
            }
        }
    }
}

/// pin 目标是否属于「版本化载荷目录里的 pi-flash.exe」：exe 名精确匹配 +
/// 所在目录名带 `-<x.y.z>` 版本段（080/081 布局词汇）+ 不是稳定入口本身。
/// 目录死活不看——被删（死链）与未删（行将就死）都该修。
fn pin_points_at_versioned_payload(target: &str, stable: &Path) -> bool {
    let t = Path::new(target);
    if !t
        .file_name()
        .is_some_and(|n| n.eq_ignore_ascii_case("pi-flash.exe"))
    {
        return false;
    }
    if crate::services::workspace::same_path(t, stable) {
        return false;
    }
    t.parent()
        .and_then(|d| d.file_name())
        .map(|d| d.to_string_lossy())
        .and_then(|d| d.rsplit_once('-').map(|(_, tail)| tail.to_string()))
        .is_some_and(|tail| crate::services::updater::is_version(&tail))
}

fn read_shortcut_target(lnk: &Path) -> Option<String> {
    com(|| unsafe {
        let link: IShellLinkW =
            CoCreateInstance(&CLSID_SHELLLINK, None, CLSCTX_INPROC_SERVER).ok()?;
        let pf: IPersistFile = link.cast().ok()?;
        pf.Load(&hstring_path(lnk), STGM_READ).ok()?;
        let mut buf = [0u16; 1024];
        link.GetPath(&mut buf, std::ptr::null_mut(), SLGP_RAWPATH.0 as u32)
            .ok()?;
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        (len > 0).then(|| String::from_utf16_lossy(&buf[..len]))
    })
}

/// 写/改写一个 .lnk：目标与图标 = exe，附 AUMID 属性（explorer 认领固定
/// 与任务栏分组的身份键）。PROPVARIANT 带 Drop（PropVariantClear 会按
/// VT_LPWSTR 释放 pwszVal），SetValue 内部已拷贝，forget 掉防对 Vec 缓冲区
/// 二次释放。
fn write_shortcut(lnk: &Path, target: &Path) -> Result<(), String> {
    com(|| unsafe {
        let link: IShellLinkW = CoCreateInstance(&CLSID_SHELLLINK, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| e.to_string())?;
        link.SetPath(&hstring_path(target))
            .map_err(|e| e.to_string())?;
        link.SetIconLocation(&hstring_path(target), 0)
            .map_err(|e| e.to_string())?;
        link.SetDescription(&HSTRING::from("pi-flash"))
            .map_err(|e| e.to_string())?;

        let mut wide: Vec<u16> = AUMID.encode_utf16().collect();
        wide.push(0);
        let pv = PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_LPWSTR,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 {
                        pwszVal: PWSTR(wide.as_mut_ptr()),
                    },
                }),
            },
        };
        let store: IPropertyStore = link.cast().map_err(|e| e.to_string())?;
        store
            .SetValue(&PKEY_AppUserModel_ID, &pv)
            .map_err(|e| e.to_string())?;
        store.Commit().map_err(|e| e.to_string())?;
        core::mem::forget(pv);
        drop(store);

        let pf: IPersistFile = link.cast().map_err(|e| e.to_string())?;
        pf.Save(&hstring_path(lnk), true).map_err(|e| e.to_string())
    })
}

/// 自起线程的 COM 会话。S_FALSE（已初始化）/RPC_E_CHANGED_MODE（已是 MTA）
/// 都不拦：ShellLink 在两种 apartment 下都可用，失败让调用方拿到具体错误。
fn com<T>(f: impl FnOnce() -> T) -> T {
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    f()
}

fn hstring_path(p: &Path) -> HSTRING {
    HSTRING::from_wide(
        &p.as_os_str()
            .to_string_lossy()
            .encode_utf16()
            .collect::<Vec<_>>(),
    )
}

/// 告诉 explorer 某 lnk 变了——任务栏按钮多半在下次交互时才重读，这只是
/// 让它尽早刷新。
fn notify_shell(p: &Path) {
    let mut wide: Vec<u16> = p.as_os_str().to_string_lossy().encode_utf16().collect();
    wide.push(0);
    unsafe {
        SHChangeNotify(
            SHCNE_UPDATEITEM,
            SHCNF_PATH,
            Some(wide.as_ptr().cast()),
            None,
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 机制核心回归锁：写出的 lnk 读回目标一致，且带约定 AUMID 属性
    /// （explorer「固定认领同 AUMID 快捷方式」全靠它）。
    #[test]
    fn shortcut_roundtrip_with_aumid() {
        let dir = std::env::temp_dir().join(format!("pf-winpin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("payload").join("pi-flash.exe");
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, b"fake").unwrap();
        let lnk = dir.join("pi-flash.lnk");

        write_shortcut(&lnk, &target).expect("写 lnk 失败");
        let got = read_shortcut_target(&lnk).expect("读回失败");
        assert!(
            crate::services::workspace::same_path(Path::new(&got), &target),
            "目标不符：{got}"
        );

        // AUMID 属性读回（GetValue 返回的 PROPVARIANT 由系统分配，Drop 正常清）
        com(|| unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&CLSID_SHELLLINK, None, CLSCTX_INPROC_SERVER).unwrap();
            let pf: IPersistFile = link.cast().unwrap();
            pf.Load(&hstring_path(&lnk), STGM_READ).unwrap();
            let store: IPropertyStore = link.cast().unwrap();
            let pv = store.GetValue(&PKEY_AppUserModel_ID).unwrap();
            assert_eq!(unsafe { pv.Anonymous.Anonymous.vt }, VT_LPWSTR);
            let s = unsafe {
                String::from_utf16_lossy(pv.Anonymous.Anonymous.Anonymous.pwszVal.as_wide())
            };
            assert_eq!(s, AUMID, "lnk 上的 AUMID 属性不符");
        });

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 只修「版本化载荷目录里的 pi-flash.exe」；稳定入口自身、开发 exe、
    /// 别的名字一律不碰。
    #[test]
    fn pin_matching_scope() {
        let stable = Path::new(r"D:\pkg\payload\pi-flash.exe");
        assert!(pin_points_at_versioned_payload(
            r"D:\pkg\payload-0.2.2\pi-flash.exe",
            stable
        ));
        assert!(
            pin_points_at_versioned_payload(r"D:\pkg\pi-flash-0.1.9\pi-flash.exe", stable),
            "绿色版本化目录也算"
        );
        assert!(!pin_points_at_versioned_payload(
            r"D:\pkg\payload\pi-flash.exe",
            stable
        ));
        assert!(
            !pin_points_at_versioned_payload(r"D:\dev\target\release\pi-flash.exe", stable),
            "非版本目录（开发 exe）不碰"
        );
        assert!(!pin_points_at_versioned_payload(
            r"D:\pkg\payload-0.2.2\其他.exe",
            stable
        ));
        assert!(
            !pin_points_at_versioned_payload(r"D:\pkg\update.tmp-del-1234\pi-flash.exe", stable),
            "清理中转目录不是版本段"
        );
    }
}
