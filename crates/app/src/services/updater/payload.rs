//! 载荷落位与交接（081 §7）：exe 目录推导 / 版本化载荷目录布局 /
//! `next_payload`（staged 新载荷挑选）/ `handoff_if_newer`（启动交接）/
//! 旧载荷后台清理。自 updater.rs 拆出（090 重构）。

use super::*;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

// ---------------------------------------------------------------------------
// 落位布局：载荷目录 = exe 所在目录；安装根 = 它的父目录
// ---------------------------------------------------------------------------

/// 平台载荷 exe 相对载荷目录的路径。
pub(crate) fn exe_rel() -> &'static str {
    if cfg!(windows) {
        "pi-flash.exe"
    } else {
        "pi-flash.app/Contents/MacOS/pi-flash"
    }
}

pub(crate) fn my_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// 载荷目录名剥掉尾部 `-<版本>`：`payload-0.1.11` → `payload`、
/// `pi-flash-0.1.11` → `pi-flash`、`payload` → `payload`（绿色包同理）。
pub(crate) fn base_name(dir_name: &str) -> String {
    match dir_name.rsplit_once('-') {
        Some((base, tail)) if !base.is_empty() && is_version(tail) => base.to_string(),
        _ => dir_name.to_string(),
    }
}

/// 载荷目录是否完整：目录名以 `-<x.y.z>` 结尾 **且** 版本戳与之一致 **且**
/// exe 在位。返回该目录承载的版本。戳是解压完成后最后写的，戳吻合 = 完整
/// （半截下载/中断解压的目录天然落选，无需额外标记文件）。
pub(crate) fn payload_version_of(dir: &Path) -> Option<String> {
    let name = dir.file_name()?.to_str()?;
    let ver = name.rsplit_once('-')?.1;
    if !is_version(ver) {
        return None;
    }
    let stamp = std::fs::read_to_string(dir.join(".payload-version")).ok()?;
    (stamp.trim() == ver && dir.join(exe_rel()).is_file()).then(|| ver.to_string())
}

/// 安装根下所有完整载荷目录：(版本, 目录)。
pub(crate) fn versioned_payloads(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| payload_version_of(&e.path()).map(|v| (v, e.path())))
        .collect()
}

/// 已就位、且确实比 `cur` 新的载荷目录（多份取版本最高者）。
pub(crate) fn staged_newer(root: &Path, cur: &str) -> Option<(String, PathBuf)> {
    versioned_payloads(root)
        .into_iter()
        .filter(|(v, _)| version_gt(v, cur))
        .max_by(|a, b| version_key(&a.0).cmp(&version_key(&b.0)))
}

/// 能切过去的更高版本载荷：排除**自己所在的载荷目录**——它已经是我们了，
/// 纳入会把「点击重启」变成「拉起自己」，也会让交接自己踢自己。
pub(crate) fn pick_next(root: &Path, cur: &str, me: &Path) -> Option<(String, PathBuf)> {
    staged_newer(root, cur).filter(|(_, dir)| dir != me)
}

/// `pick_next` 的运行时入口（自己 = 当前 exe 所在目录）。
pub(crate) fn next_payload() -> Option<(String, PathBuf)> {
    let me = my_exe_dir()?;
    pick_next(me.parent()?, current_version(), &me)
}

/// 稳定入口 exe：081 布局里**永不改名的启动路径**——npm 薄壳与快捷方式的
/// 固定入口 `payload\pi-flash.exe`（版本化载荷 `payload-<版本>\` 剥掉版本段
/// 就是它，cleanup 只删带版本号的目录）。任务栏固定稳定性（win_pin）拿它
/// 当快捷方式目标，pin 才不会随版本目录消亡。
/// 只认安装布局：目录名既不是 `payload`、也不是带完整版本戳的
/// `payload-<版本>`（开发构建 / 绿色解压包）一律 None；debug 构建恒 None
/// ——开发跑多少遍都不许碰用户的开始菜单与任务栏。
pub(crate) fn stable_entry_exe() -> Option<PathBuf> {
    if cfg!(debug_assertions) {
        return None;
    }
    let me = my_exe_dir()?;
    let name = me.file_name()?.to_str()?;
    let stable_dir = if name == "payload" {
        me
    } else if payload_version_of(&me).is_some() {
        me.parent()?.join(base_name(name))
    } else {
        return None;
    };
    let exe = stable_dir.join(exe_rel());
    exe.is_file().then_some(exe)
}

/// 我所在载荷目录自证一致：目录名里的版本（若有）必须等于我自己的版本。
/// 不一致 = 被复制/篡改的载荷（例如旧 exe 被塞进 `payload-9.9.9/`）——那种
/// 情况下交接会 A 让位 B、B 又让位 A，变成进程 spawn 风暴，直接拒绝交接。
/// 绿色包目录名不含版本 → None → 无从证伪，照常交接。
pub(crate) fn self_version_matches(me: &Path) -> bool {
    match payload_version_of(me) {
        Some(v) => v == current_version(),
        None => true,
    }
}

/// 启动最前处（main 开窗之前）调用：更高版本的完整载荷已就位就 spawn 它的
/// exe 并返回 true（调用方随即退出，用户只感觉这次启动慢半拍）。顺序：
/// 先起后台清理线程（删旧版本目录，不阻塞开窗），再判断交接。
pub(crate) fn handoff_if_newer() -> bool {
    let Some(me) = my_exe_dir() else {
        return false;
    };
    let Some(root) = me.parent().map(Path::to_path_buf) else {
        return false;
    };
    std::thread::spawn({
        let root = root.clone();
        let me = me.clone();
        move || cleanup_older(root, me)
    });
    if !self_version_matches(&me) {
        eprintln!(
            "[updater] {} 的版本戳与进程版本不符，跳过启动交接",
            me.display()
        );
        return false;
    }
    let Some((ver, dir)) = pick_next(&root, current_version(), &me) else {
        return false;
    };
    let exe = dir.join(exe_rel());
    eprintln!("[updater] 交接给已就位的新载荷 v{ver}：{}", exe.display());
    // cwd 交接（2026-10-17 bugfix）：双击/快捷方式直启时本进程 cwd = 自己的
    // 载荷目录（老 `payload\`）——这对子进程是垃圾 cwd：子 exe 已换到
    // `payload-<新版本>\`，继承 cwd ≠ 子 exe 目录，Chat::new 的「exe 目录
    // 启动」探针失灵 → 老 payload 目录被当成用户项目注册（打开项目即注
    // 册）、__last 被污染，此后每次启动都从 payload 起。父进程 cwd == 自己
    // exe 目录（双击签名）时，显式给子进程 current_dir = 它自己的载荷目
    // 录，让它同样命中 exe 目录启动兜底；终端/薄壳场景 cwd 是用户语义
    // （shell 所在项目），保持继承不动。
    let double_click =
        std::env::current_dir().is_ok_and(|c| crate::services::workspace::same_path(&c, &me));
    let mut cmd = std::process::Command::new(&exe);
    if double_click {
        cmd.current_dir(&dir);
    }
    match cmd.spawn() {
        Ok(_) => true,
        Err(e) => {
            eprintln!(
                "[updater] 交接失败（{e}），继续以 v{} 启动",
                current_version()
            );
            false
        }
    }
}

/// 启动时初始状态：已有更高版本的完整载荷 = Ready（交接没能拉起来时的兜底，
/// 提示可点击重试），否则 Idle。
pub(crate) fn initial_state() -> UpdateState {
    match next_payload() {
        Some((version, _)) => UpdateState::Ready { version },
        None => UpdateState::Idle,
    }
}

/// 后台清理：删掉比当前版本旧的版本化载荷目录，以及超过 1 小时未更新的
/// `update.tmp-*` 半截下载。删目录前先 `rename` 当**占用探针**——目录里有
/// 进程在用的文件时 Windows rename 必败（0.1.6~0.1.10 的死循环正是这条
/// 语义造成的），失败即跳过，绝不删活目录；下次启动再试。全程忽略错误。
pub(crate) fn cleanup_older(root: PathBuf, me: PathBuf) {
    for (ver, dir) in versioned_payloads(&root) {
        if dir == me || !version_gt(current_version(), &ver) {
            continue;
        }
        let trash = root.join(format!("update.tmp-del-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&trash);
        if std::fs::rename(&dir, &trash).is_ok() {
            let _ = std::fs::remove_dir_all(&trash);
        }
    }
    let Ok(entries) = std::fs::read_dir(&root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // 老落位方案（0.1.6~0.1.10 的整目录 swap 时代）留下的垃圾：helper 日志、
        // staged 标记、swap 中转目录。用户从那些版本升级过来时它们会永久占盘
        // （`payload.old-*` 一份就是 300MB），且旧代码里没人清。
        // 注意：`payload.stage-<版本>` **不在此列**——它本身就是一份完整载荷，
        // 走上面的版本规则（更新的拿它交接、更旧的才删）。
        if name == "update-swap.log"
            || name == "update-staged.json"
            || name.starts_with("payload.old-")
        {
            remove_path(&entry.path());
            continue;
        }
        if !name.starts_with("update.tmp-") {
            // 落位成功、版本戳还没写就被杀（`payload-<版本>/`：里面有 exe，但没有
            // 戳）：它不是「完整载荷」（交接和上面的版本清理都看不见它），也不是
            // 那批老垃圾，会永久占盘 300MB。超 1 小时才删——免得误删另一个实例刚
            // rename 完、马上要写戳的那个目录。
            let looks_like_payload = name
                .rsplit_once('-')
                .map(|(_, v)| is_version(v))
                .unwrap_or(false)
                && entry.path().join(exe_rel()).is_file();
            if looks_like_payload && payload_version_of(&entry.path()).is_none() {
                if older_than_an_hour(&entry) {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
            continue;
        }
        if older_than_an_hour(&entry) {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// 目录/文件超过 1 小时没被动过（拿不到时间戳就当作是旧的）。
pub(crate) fn older_than_an_hour(entry: &std::fs::DirEntry) -> bool {
    entry
        .metadata()
        .and_then(|m| m.modified())
        .map(|t| {
            t.elapsed()
                .map(|a| a > Duration::from_secs(3600))
                .unwrap_or(true)
        })
        .unwrap_or(true)
}

/// 删一个残留时目录/文件都可能（老垃圾里两者都有）。
pub(crate) fn remove_path(p: &Path) {
    if p.is_dir() {
        let _ = std::fs::remove_dir_all(p);
    } else {
        let _ = std::fs::remove_file(p);
    }
}

/// 把目录里每个文件读一遍（并行，最多 8 线程）：安装阶段就把「首次打开」的
/// 成本花掉（杀软/文件系统对新载荷一万多个文件的首次扫描），这样点击重启、
/// 以及以后每次打开新版本都跟平时一样秒开——启动路径一行代码都不加。
/// 单文件失败只跳过：这只是优化，不影响安装正确性。返回（读到的文件数, 毫秒）。
pub(crate) fn touch_tree(dir: &Path) -> (usize, u128) {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(e.path()),
                Ok(t) if t.is_file() => {
                    if e.metadata().map(|m| m.len() > 0).unwrap_or(false) {
                        files.push(e.path());
                    }
                }
                _ => {}
            }
        }
    }
    let t0 = std::time::Instant::now();
    let next = AtomicUsize::new(0);
    let done = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(p) = files.get(i) else { break };
                    let mut buf = [0u8; 4096];
                    if let Ok(mut f) = std::fs::File::open(p) {
                        if f.read(&mut buf).is_ok() {
                            done.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
            });
        }
    });
    let ms = t0.elapsed().as_millis();
    let n = done.load(Ordering::Relaxed);
    eprintln!(
        "[updater] 安装收尾：读过 {n}/{} 个文件，{ms} ms",
        files.len()
    );
    (n, ms)
}
