//! 稳定入口自愈与预拷（081 布局）：`payload\` 稳定入口的半残自愈
//! （refresh_stable_entry）/ T1 预拷（precopy_stable_entry）/ 半程标记
//! （`.payload-stage`）/ 逐文件复制原语。自 payload.rs 切出
//!（itlq.9 超规拆分，纯结构搬移）。

use super::*;
use std::path::{Path, PathBuf};

/// 稳定入口目录：`<root>/<me 目录名剥掉版本段>`。npm 布局 me = `payload-<版本>`
/// → `<root>/payload`；me 本身就是 `payload` → 自己。绿色布局同理，调用方按
/// 需自证。
pub(crate) fn stable_entry_dir_of(root: &Path, me: &Path) -> Option<PathBuf> {
    let name = me.file_name()?.to_str()?;
    Some(root.join(base_name(name)))
}

/// 稳定入口自证完整：戳是合法版本且 exe 在位 → 返回该版本。戳在但 exe 缺席
/// = 半残（npm 薄壳 replacePayload 中途炸的典型残骸），不算完整。
pub(crate) fn stable_entry_version(stable_dir: &Path) -> Option<String> {
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
pub(crate) const STAGE_MARKER: &str = ".payload-stage";

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
pub(crate) fn repoint_pi_shim_to_stable(stable_dir: &Path) {
    let node = stable_dir.join("node.exe");
    let cli = stable_dir
        .join(pi_link::vendor::VENDOR_DIR_NAME)
        .join(pi_link::vendor::NODE_MODULES_REL);
    if node.is_file() && cli.is_file() {
        let _ = pi_link::register_to_sys::ensure_pi_shim(&node.to_string_lossy(), &cli);
    }
}
