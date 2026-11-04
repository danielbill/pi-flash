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

/// 稳定入口目录：`<root>/<me 目录名剥掉版本段>`。npm 布局 me = `payload-<版本>`
/// → `<root>/payload`；me 本身就是 `payload` → 自己。绿色布局同理，调用方按
/// 需自证。
fn stable_entry_dir_of(root: &Path, me: &Path) -> Option<PathBuf> {
    let name = me.file_name()?.to_str()?;
    Some(root.join(base_name(name)))
}

/// 稳定入口自证完整：戳是合法版本且 exe 在位 → 返回该版本。戳在但 exe 缺席
/// = 半残（npm 薄壳 replacePayload 中途炸的典型残骸），不算完整。
fn stable_entry_version(stable_dir: &Path) -> Option<String> {
    let stamp = std::fs::read_to_string(stable_dir.join(".payload-version")).ok()?;
    let ver = stamp.trim().to_string();
    (is_version(&ver) && stable_dir.join(exe_rel()).is_file()).then_some(ver)
}

/// 启动自愈：把稳定入口 `payload\` 刷新到**最新完整载荷**（081 布局里应用内
/// 更新只落版本化目录、稳定入口只由 npm 安装维护，而 npm 薄壳的 replacePayload
/// 先删后建非原子——半途失败留下空/半残稳定入口，从此无人补齐，任务栏 pin 的
/// 目标就此死链）。纪律：
/// - 源 = 版本最高的完整载荷目录；稳定入口已是该版本 → 幂等零动作；
/// - 稳定入口比所有版本化目录都新 → 不降级；
/// - **收尾模式**：稳定入口有同版本 `.payload-stage` 标记（T1 预拷留下）→
///   只补拷被锁跳过的文件，秒级完成；无标记/版本对不上 → 全量复制兜底；
/// - 占用探针：目标 exe 写打开失败 = 有进程跑在稳定入口里（pin 启动的老版本
///   本尊）→ 整体跳过，下次启动重试；
/// - 版本戳完整走完才写——中途失败留下的混合态里旧 exe 仍可启动（交接兜底），
///   失败清单记入标记，下次按清单续拷。
/// 返回是否完成了刷新（true = 稳定入口内容已等于最新完整载荷）。
pub(crate) fn refresh_stable_entry(root: &Path, me: &Path) -> bool {
    let Some(stable_dir) = stable_entry_dir_of(root, me) else {
        return false;
    };
    if crate::services::workspace::same_path(me, &stable_dir) {
        return false; // 自己就跑在稳定入口里：自己锁自己，刷新留给交接后的首启
    }
    let Some((src_ver, src_dir)) = versioned_payloads(root)
        .into_iter()
        .max_by(|a, b| version_key(&a.0).cmp(&version_key(&b.0)))
    else {
        return false; // 没有完整版本化载荷可作源（只能靠 npm 修补）
    };
    match stable_entry_version(&stable_dir) {
        Some(v) if v == src_ver => return false, // 已是最新，零动作
        Some(v) if version_gt(&v, &src_ver) => return false, // 更新，不降级
        _ => {}
    }
    let stable_exe = stable_dir.join(exe_rel());
    if stable_exe.is_file()
        && std::fs::OpenOptions::new()
            .write(true)
            .open(&stable_exe)
            .is_err()
    {
        eprintln!(
            "[updater] 稳定入口被占用（{}），自愈跳过，下次启动重试",
            stable_exe.display()
        );
        return false;
    }
    let outcome: Result<Vec<PathBuf>, ()> = match read_stage_marker(&stable_dir) {
        Some((v, files)) if v == src_ver && !files.is_empty() => {
            let rest = copy_listed(&src_dir, &stable_dir, &files);
            if rest.is_empty() {
                eprintln!("[updater] 预拷收尾：补拷 {} 个锁定文件完成", files.len());
            }
            Ok(rest)
        }
        _ => copy_payload_tree(&src_dir, &stable_dir),
    };
    match outcome {
        Ok(failed) if failed.is_empty() => {
            let _ = std::fs::write(stable_dir.join(".payload-version"), format!("{src_ver}\n"));
            let _ = std::fs::remove_file(stable_dir.join(STAGE_MARKER)); // 全量成功也顺手清旧标记
            eprintln!(
                "[updater] 稳定入口已刷新到 v{src_ver}：{}",
                stable_dir.display()
            );
            true
        }
        Ok(failed) => {
            write_stage_marker(&stable_dir, &src_ver, &failed);
            eprintln!(
                "[updater] 稳定入口刷新不完整（{} 个待补，已记标记，下次启动重试）",
                failed.len()
            );
            false
        }
        Err(()) => {
            // 树遍历失败 = 完整度未知，绝不写清单冒充记账，下次全量重试
            eprintln!("[updater] 稳定入口刷新遍历失败（不写戳不记标记，下次启动重试）");
            false
        }
    }
}

/// T1 半程标记：首行 = 版本，其后 = 因锁定/IO 错未拷入稳定入口的相对路径。
/// 首启自愈读到同版本标记 → 收尾模式只补清单；戳写完即清。
const STAGE_MARKER: &str = ".payload-stage";

fn write_stage_marker(stable_dir: &Path, ver: &str, failed: &[PathBuf]) {
    let mut body = format!("{ver}\n");
    for f in failed {
        body.push_str(&f.to_string_lossy());
        body.push('\n');
    }
    let _ = std::fs::write(stable_dir.join(STAGE_MARKER), body);
}

fn read_stage_marker(stable_dir: &Path) -> Option<(String, Vec<PathBuf>)> {
    let raw = std::fs::read_to_string(stable_dir.join(STAGE_MARKER)).ok()?;
    let mut lines = raw.lines();
    let ver = lines.next()?.trim().to_string();
    if !is_version(&ver) {
        return None;
    }
    Some((ver, lines.map(PathBuf::from).collect()))
}

/// 把载荷树逐文件复制覆盖到 `dst`（非 .exe 先、.exe 后；源里的版本戳不拷——
/// 由调用方在完整走完后写）。任一文件失败不中断，继续拷其余；`Ok(清单)` =
/// 失败文件的相对路径（空 = 全部成功），`Err` = 树遍历失败（完整度未知，
/// 调用方不得写戳/写标记，下次全量重试）。
fn copy_payload_tree(src: &Path, dst: &Path) -> Result<Vec<PathBuf>, ()> {
    let mut dirs = vec![src.to_path_buf()];
    let mut plain: Vec<PathBuf> = Vec::new();
    let mut exes: Vec<PathBuf> = Vec::new();
    while let Some(d) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            return Err(());
        };
        for e in entries.flatten() {
            let Ok(t) = e.file_type() else {
                return Err(());
            };
            let p = e.path();
            if t.is_dir() {
                dirs.push(p);
            } else if p.file_name().is_some_and(|n| n == ".payload-version") {
                // 源戳不拷，目标戳由调用方最后写
            } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("exe")) {
                exes.push(p);
            } else {
                plain.push(p);
            }
        }
    }
    let mut failed = Vec::new();
    for p in plain.iter().chain(exes.iter()) {
        let Ok(rel) = p.strip_prefix(src) else {
            failed.push(p.to_path_buf());
            continue;
        };
        if let Err(e) = copy_one(src, dst, rel) {
            eprintln!("[updater] 复制失败 {}：{e}", p.display());
            failed.push(rel.to_path_buf());
        }
    }
    Ok(failed)
}

/// 按相对路径清单逐个复制（预拷收尾通道）；返回仍失败的（留在标记里重试）。
fn copy_listed(src: &Path, dst: &Path, files: &[PathBuf]) -> Vec<PathBuf> {
    let mut failed = Vec::new();
    for rel in files {
        if let Err(e) = copy_one(src, dst, rel) {
            eprintln!("[updater] 收尾复制失败 {}：{e}", rel.display());
            failed.push(rel.clone());
        }
    }
    failed
}

/// 单文件复制：目标父目录按需创建；目标被锁（运行中镜像）时 Err 返回。
fn copy_one(src: &Path, dst: &Path, rel: &Path) -> std::io::Result<()> {
    let to = dst.join(rel);
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(src.join(rel), &to)?;
    Ok(())
}

/// T1 预拷（ab0c 优化）：落位后趁热把新载荷尽量刷进稳定入口——老进程锁得住
/// 的只有它自己的运行镜像（跑在稳定入口时 = pi-flash.exe，node.exe 视会话而
/// 定），vendor 等万计文件此时无主，现在拷走，首启就只剩大文件收尾。纪律与
/// 自愈同源：逐文件失败记账不中断；**无一失败 → 直接写戳完成迁移**（老进程
/// 跑在版本化目录时的常见情形，首启零复制）；有剩余 → 写 `.payload-stage`
/// 标记，首启 refresh_stable_entry 收尾。进程中途死掉 → 无戳无标记 → 首启
/// 全量复制兜底（不劣化）。
pub(crate) fn precopy_stable_entry(root: &Path, me: &Path, target: &Path, ver: &str) {
    let Some(stable_dir) = stable_entry_dir_of(root, me) else {
        return;
    };
    if crate::services::workspace::same_path(target, &stable_dir) {
        return; // 对称守卫：目标是版本化目录，理论不可能，防呆
    }
    match stable_entry_version(&stable_dir) {
        Some(v) if v == ver => return,            // 已有实例收过尾
        Some(v) if version_gt(&v, ver) => return, // 不降级
        _ => {}
    }
    let failed = match copy_payload_tree(target, &stable_dir) {
        Ok(f) => f,
        Err(()) => {
            eprintln!("[updater] 载荷预拷遍历失败（不写戳不记标记，首启全量兜底）");
            return;
        }
    };
    if failed.is_empty() {
        let _ = std::fs::write(stable_dir.join(".payload-version"), format!("{ver}\n"));
        let _ = std::fs::remove_file(stable_dir.join(STAGE_MARKER));
        // 直接迁移完成 → 指针同步收敛稳定入口（版本化目录随时可被清理）
        repoint_pi_shim_to_stable(&stable_dir);
        eprintln!(
            "[updater] 载荷预拷：稳定入口直接迁移到 v{ver}（{}）",
            stable_dir.display()
        );
    } else {
        write_stage_marker(&stable_dir, ver, &failed);
        eprintln!(
            "[updater] 载荷预拷：{} 个文件待首启补拷（{}）",
            failed.len(),
            stable_dir.display()
        );
    }
}

/// 自愈完成后把全局 pi 指针收敛到稳定入口：版本化目录会被扩展清理删掉，指针
/// 钉版本目录迟早悬空。`ensure_pi_shim` 内容不变时零写入，幂等。
fn repoint_pi_shim_to_stable(stable_dir: &Path) {
    let node = stable_dir.join("node.exe");
    let cli = stable_dir
        .join(pi_link::vendor::VENDOR_DIR_NAME)
        .join(pi_link::vendor::NODE_MODULES_REL);
    if node.is_file() && cli.is_file() {
        let _ = pi_link::register_to_sys::ensure_pi_shim(&node.to_string_lossy(), &cli);
    }
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
    let next = if self_version_matches(&me) {
        pick_next(&root, current_version(), &me)
    } else {
        eprintln!(
            "[updater] {} 的版本戳与进程版本不符，跳过启动交接",
            me.display()
        );
        None
    };
    let handing_off = next.is_some();
    std::thread::spawn({
        let root = root.clone();
        let me = me.clone();
        move || {
            // 稳定入口自愈 + pi 指针收敛，先于扩展清理（清理要删掉已被自愈
            // 吸收的版本化目录，源必须先拷完）。交接场景跳过——新版首启（跑
            // 在自己的版本目录里）会做同一件事，快死的进程里复制半途被杀只
            // 留半截。debug 构建不碰安装树（开发跑多少遍都不许动用户的安装
            // 布局，与 win_pin 同纪律）。
            if !handing_off && !cfg!(debug_assertions) {
                let _ = refresh_stable_entry(&root, &me);
                if let Some(stable) =
                    stable_entry_dir_of(&root, &me).filter(|d| stable_entry_version(d).is_some())
                {
                    repoint_pi_shim_to_stable(&stable);
                }
            }
            cleanup_older(root, me);
        }
    });
    let Some((ver, dir)) = next else {
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
    // 扩展判据（ab0c）：稳定入口完整时，版本 ≤ 稳定入口戳的版本化目录都是
    // 冗余副本（内容已被自愈吸收进稳定入口），一并清掉——稳态归一为一份。
    // rename 占用探针（下方）天然保护运行中/被锁的目录。
    let redundant_upto = stable_entry_dir_of(&root, &me).and_then(|d| stable_entry_version(&d));
    for (ver, dir) in versioned_payloads(&root) {
        let outdated = version_gt(current_version(), &ver);
        let redundant = redundant_upto
            .as_ref()
            .is_some_and(|s| !version_gt(&ver, s));
        if dir == me || (!outdated && !redundant) {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pf-upd-{}-{}-{}",
            tag,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 造一份完整载荷：exe + 若干普通文件 + 戳（最后写，与真实布局同序）。
    fn make_payload(dir: &Path, ver: &str, extra: &[&str]) {
        std::fs::create_dir_all(dir).unwrap();
        let mut files: Vec<&str> = vec!["pi-flash.exe", "vendor/pi/x.js"];
        files.extend_from_slice(extra);
        for f in files {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, format!("{f} @ {ver}")).unwrap();
        }
        std::fs::write(dir.join(".payload-version"), format!("{ver}\n")).unwrap();
    }

    /// 真镜像锁（windows）：把系统 cmd.exe 复制成 `exe` 并运行——运行中镜像
    /// 的写打开必败（ERROR_SHARING_VIOLATION）。std 打开文件自带全共享模式，
    /// 测试里持写句柄模拟不出镜像锁，必须真跑。返回 None = 环境不可用，调用
    /// 方跳过用例。
    #[cfg(windows)]
    fn spawn_image_lock(exe: &Path) -> Option<std::process::Child> {
        std::fs::create_dir_all(exe.parent().unwrap()).ok()?;
        let cmd = std::env::var("SystemRoot")
            .map(|r| std::path::PathBuf::from(r).join("System32/cmd.exe"))
            .unwrap_or_else(|_| std::path::PathBuf::from(r"C:\Windows\System32\cmd.exe"));
        std::fs::copy(&cmd, exe).ok()?;
        let mut child = std::process::Command::new(exe)
            .args(["/c", "ping", "-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        // 轮询等镜像节真正建立（全量并行下 CPU 抢占，固定 sleep 会翻车）：
        // 写打开失败 = 锁已生效；5s 仍锁不上视为环境异常
        for _ in 0..50 {
            if std::fs::OpenOptions::new().write(true).open(exe).is_err() {
                return Some(child);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let _ = child.kill();
        let _ = child.wait();
        eprintln!("[test] 镜像锁 5s 未建立，跳过用例");
        None
    }

    #[test]
    fn stable_entry_version_requires_stamp_and_exe() {
        let root = temp_root("sev");
        let stable = root.join("payload");
        assert_eq!(stable_entry_version(&stable), None, "啥都没有");

        std::fs::create_dir_all(&stable).unwrap();
        std::fs::write(stable.join(".payload-version"), "0.4.2\n").unwrap();
        assert_eq!(stable_entry_version(&stable), None, "只有戳没有 exe = 半残");

        std::fs::write(stable.join(exe_rel()), b"exe").unwrap();
        assert_eq!(stable_entry_version(&stable).as_deref(), Some("0.4.2"));

        std::fs::write(stable.join(".payload-version"), "垃圾\n").unwrap();
        assert_eq!(stable_entry_version(&stable), None, "非版本戳不认");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refresh_copies_newest_and_stamps_last() {
        let root = temp_root("refresh");
        let me = root.join("payload-0.4.2");
        make_payload(&me, "0.4.2", &["extensions/a.json"]);
        // 稳定入口半残：有目录没内容（npm replacePayload 炸后的典型残骸）
        std::fs::create_dir_all(root.join("payload")).unwrap();

        assert!(refresh_stable_entry(&root, &me));
        let stable = root.join("payload");
        assert_eq!(stable_entry_version(&stable).as_deref(), Some("0.4.2"));
        assert_eq!(
            std::fs::read_to_string(stable.join("pi-flash.exe")).unwrap(),
            "pi-flash.exe @ 0.4.2"
        );
        assert!(stable.join("vendor/pi/x.js").is_file());
        assert!(stable.join("extensions/a.json").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refresh_noop_when_already_current() {
        let root = temp_root("noop");
        let me = root.join("payload-0.4.2");
        make_payload(&me, "0.4.2", &[]);
        make_payload(&root.join("payload"), "0.4.2", &[]);

        assert!(!refresh_stable_entry(&root, &me), "已是最新 = 零动作");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refresh_never_downgrades() {
        let root = temp_root("downgrade");
        let me = root.join("payload-0.4.2");
        make_payload(&me, "0.4.2", &[]);
        // 稳定入口更新（npm 刚装的新壳载荷），版本化目录是旧的
        make_payload(&root.join("payload"), "0.5.0", &[]);

        assert!(!refresh_stable_entry(&root, &me), "不降级");
        assert_eq!(
            std::fs::read_to_string(root.join("payload/pi-flash.exe")).unwrap(),
            "pi-flash.exe @ 0.5.0"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[cfg(windows)]
    fn refresh_skips_when_target_locked() {
        let root = temp_root("locked");
        let me = root.join("payload-0.4.2");
        make_payload(&me, "0.4.2", &[]);
        let stable = root.join("payload");
        // 老版本跑在稳定入口：镜像锁 → 整体跳过
        let Some(mut child) = spawn_image_lock(&stable.join(exe_rel())) else {
            eprintln!("[test] 镜像锁不可用，跳过用例");
            return;
        };
        std::fs::write(stable.join(".payload-version"), "0.4.0\n").unwrap();

        assert!(!refresh_stable_entry(&root, &me), "被占用整体跳过");
        assert_eq!(
            stable_entry_version(&stable).as_deref(),
            Some("0.4.0"),
            "旧内容原封不动"
        );
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refresh_incomplete_writes_no_stamp() {
        let root = temp_root("partial");
        let me = root.join("payload-0.4.2");
        make_payload(&me, "0.4.2", &["node.exe"]);
        let stable = root.join("payload");
        std::fs::create_dir_all(&stable).unwrap();
        // 目标侧埋雷：node.exe 位置是目录 → 源文件复制必败 → 不完整
        std::fs::create_dir_all(stable.join("node.exe")).unwrap();

        assert!(!refresh_stable_entry(&root, &me), "有失败 = 不完整");
        assert_eq!(
            stable_entry_version(&stable),
            None,
            "不完整绝不写戳冒充完整载荷"
        );
        assert!(
            stable.join("pi-flash.exe").is_file(),
            "非 exe 已先拷成功（旧 exe 仍可启动走交接）"
        );
        let marker = std::fs::read_to_string(stable.join(STAGE_MARKER)).unwrap();
        assert!(
            marker.starts_with("0.4.2\n") && marker.contains("node.exe"),
            "失败清单入标记（首行版本 + 相对路径），下次按清单续拷"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cleanup_deletes_redundant_versioned_dirs() {
        let root = temp_root("cleanup");
        let me = root.join("payload-9.9.9");
        make_payload(&me, "9.9.9", &[]);
        make_payload(&root.join("payload"), "9.9.9", &[]); // 稳定入口 = 9.9.9
        make_payload(&root.join("payload-0.4.1"), "0.4.1", &[]);
        make_payload(&root.join("payload-8.8.8"), "8.8.8", &[]);

        cleanup_older(root.clone(), me.clone());
        assert!(
            !root.join("payload-0.4.1").exists(),
            "≤ 稳定入口戳 → 冗余删除"
        );
        assert!(
            !root.join("payload-8.8.8").exists(),
            "≤ 稳定入口戳 → 冗余删除"
        );
        assert!(me.exists(), "自己所在的目录永不删（运行中）");
        assert!(
            root.join("payload").exists(),
            "稳定入口本体不在版本化清单里"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cleanup_keeps_everything_without_stable_stamp() {
        let root = temp_root("cleanup2");
        let me = root.join("payload-9.9.9");
        make_payload(&me, "9.9.9", &[]);
        make_payload(&root.join("payload-0.0.1"), "0.0.1", &[]);
        // 没有稳定入口目录 → 扩展判据不生效
        make_payload(&root.join("payload-9.9.8"), "9.9.8", &[]);

        cleanup_older(root.clone(), me.clone());
        assert!(
            root.join("payload-9.9.8").exists(),
            "无稳定入口戳时扩展判据不删（outdated 判据对 9.9.8 不成立）"
        );
        assert!(me.exists());
        // payload-0.0.1 会被 outdated 判据删（current_version() 必大于 0.0.1），
        // 与扩展判据无关，不在此断言。
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn precopy_direct_migration_when_target_idle() {
        let root = temp_root("precopy");
        let me = root.join("payload-0.4.2");
        make_payload(&me, "0.4.2", &["node.exe"]);
        let target = me.clone();
        let stable = root.join("payload");
        std::fs::create_dir_all(&stable).unwrap(); // 空（被炸残骸）

        // 稳定入口无主（老进程跑在版本化目录）→ T1 直接完成迁移
        precopy_stable_entry(&root, &me, &target, "0.4.2");
        assert_eq!(
            stable_entry_version(&stable).as_deref(),
            Some("0.4.2"),
            "无一被锁 → 直接迁移写戳"
        );
        assert!(!stable.join(STAGE_MARKER).exists(), "完整迁移不留标记");
        assert!(stable.join("node.exe").is_file());
        assert!(stable.join("vendor/pi/x.js").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    #[cfg(windows)]
    fn precopy_marker_then_refresh_finishes() {
        let root = temp_root("precmk");
        let me = root.join("payload-0.4.2");
        make_payload(&me, "0.4.2", &["node.exe"]);
        let stable = root.join("payload");
        // 老进程本尊跑在稳定入口：pi-flash.exe 镜像锁
        let Some(mut child) = spawn_image_lock(&stable.join(exe_rel())) else {
            eprintln!("[test] 镜像锁不可用，跳过用例");
            return;
        };

        precopy_stable_entry(&root, &me, &me.clone(), "0.4.2");
        assert_eq!(stable_entry_version(&stable), None, "有锁定剩余 = 不写戳");
        assert!(stable.join("node.exe").is_file(), "未锁文件已预拷就位");
        let marker = std::fs::read_to_string(stable.join(STAGE_MARKER)).unwrap();
        assert!(marker.starts_with("0.4.2\n"), "标记首行 = 版本");
        assert!(marker.contains("pi-flash.exe"), "锁定文件在清单里");

        // 老进程退出 → 首启自愈按清单收尾（只补 pi-flash.exe，不再全量）
        let _ = child.kill();
        let _ = child.wait();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(refresh_stable_entry(&root, &me), "收尾完成");
        assert_eq!(stable_entry_version(&stable).as_deref(), Some("0.4.2"));
        assert!(!stable.join(STAGE_MARKER).exists(), "收尾后清标记");
        assert_eq!(
            std::fs::read_to_string(stable.join("pi-flash.exe")).unwrap(),
            "pi-flash.exe @ 0.4.2"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn refresh_marker_version_mismatch_falls_back_to_full() {
        let root = temp_root("precmm");
        let me = root.join("payload-0.4.2");
        make_payload(&me, "0.4.2", &[]);
        let stable = root.join("payload");
        std::fs::create_dir_all(&stable).unwrap();
        // 陈旧标记（版本对不上）→ 必须走全量而非按清单
        std::fs::write(stable.join(STAGE_MARKER), "0.3.9\npi-flash.exe\n").unwrap();

        assert!(refresh_stable_entry(&root, &me));
        assert_eq!(stable_entry_version(&stable).as_deref(), Some("0.4.2"));
        assert!(!stable.join(STAGE_MARKER).exists(), "全量成功清旧标记");
        let _ = std::fs::remove_dir_all(&root);
    }
}
