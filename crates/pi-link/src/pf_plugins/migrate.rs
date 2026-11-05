use super::*;

/// 090-10 迁移体验：把挂在版本目录下的用户数据 `<pkg>/<ver>/data` 一次性搬到
/// `<pkg>/data`。90-4 的设计本就是 `plugins/<id>/data/`（无版本维度）；此前
/// 代码取 `清单目录/data`，market 版本层布局下等于版本目录——更新一次丢一次。
///
/// 规则：
/// - 只看台账里的包（含停用）；`<pkg>/data` 已存在 → 跳过（幂等；平铺 dev
///   布局 `<pkg>/` 即清单目录，data 本就在位，天然 no-op）。
/// - 源优先台账记录的版本目录；缺则取 mtime 最新且非空的 `<ver>/data`。
/// - **不依赖清单能否解析**：被本版拒载的旧包（schema 过低）也要迁，否则
///   用户点 [更新] 装到新版本目录后仍从空 data 起步。
/// - `fs::rename` 同卷原子；失败只记日志，下次启动重试（.cache os error 32 同款
///   容错）。跑在后台扫描线程、实例装载之前（此时无句柄）。
/// 返回已迁包名（日志 / 单测断言）。
pub fn migrate_data_dirs(root: &Path, ledger: &[LedgerEntry]) -> Vec<String> {
    let mut moved = Vec::new();
    for l in ledger {
        let pkg_dir = root.join(dir_name(&l.name));
        if !pkg_dir.is_dir() {
            continue;
        }
        let target = pkg_dir.join("data");
        if target.exists() {
            continue;
        }
        let Some(src) = pick_versioned_data(&pkg_dir, &l.version) else {
            continue;
        };
        match std::fs::rename(&src, &target) {
            Ok(()) => moved.push(l.name.clone()),
            Err(e) => eprintln!(
                "[pf-plugin] data 迁移失败 {} → {}: {e}（下次启动重试）",
                src.display(),
                target.display()
            ),
        }
    }
    moved
}

/// 版本目录里挑 data 源：台账版本优先；否则 mtime 最新的非空 `<ver>/data`。
/// 只认版本形目录（首字符数字，与清单目录名对账同口径），`assets/`、`pi/`
/// 之类不会被误当版本层。
fn pick_versioned_data(pkg_dir: &Path, ledger_version: &str) -> Option<PathBuf> {
    let preferred = pkg_dir.join(ledger_version).join("data");
    if dir_non_empty(&preferred) {
        return Some(preferred);
    }
    let rd = std::fs::read_dir(pkg_dir).ok()?;
    rd.flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit()))
        })
        .map(|p| p.join("data"))
        .filter(|d| dir_non_empty(d))
        .max_by_key(|d| std::fs::metadata(d).and_then(|m| m.modified()).ok())
}

fn dir_non_empty(d: &Path) -> bool {
    std::fs::read_dir(d)
        .map(|mut rd| rd.next().is_some())
        .unwrap_or(false)
}

/// 清掉 `<pkg>/` 下**不是**台账版本的版本形目录（更新后的旧版本残留）。
///
/// 为什么要清：扫描只认台账版本（宿主 `scan_root`），残留目录本就不装载，但
/// 它占磁盘、带着旧 cwasm，且 `declared_claim_prefixes` 一类全目录遍历还会读到
/// 它的清单。为什么能清：用户数据已在 `<pkg>/data`（包层），版本目录里只有随
/// 版本走的东西。**先救数据再删**：若 `<pkg>/data` 还不存在而该版本目录下有非空
/// `data/`（上版 PF 没迁过），先 rename 上来，绝不让 prune 跑在迁移前面丢数据。
/// 删除 best-effort：旧 cwasm 被运行中实例 mmap 住时 Windows 拒删（os error 32），
/// 留给下次启动再清；返回成功删掉的目录数。
pub fn prune_old_versions(pkg_dir: &Path, keep_version: &str) -> usize {
    let Ok(rd) = std::fs::read_dir(pkg_dir) else {
        return 0;
    };
    let mut removed = 0;
    for e in rd.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        // 只碰版本形目录（首字符数字）；data/、.cache 之类一律不动
        if !p.is_dir() || !name.starts_with(|c: char| c.is_ascii_digit()) || name == keep_version {
            continue;
        }
        let rescue = p.join("data");
        let target = pkg_dir.join("data");
        if !target.exists()
            && dir_non_empty(&rescue)
            && let Err(err) = std::fs::rename(&rescue, &target)
        {
            eprintln!(
                "[pf-plugin] 旧版本 data 抢救失败，保留目录 {}: {err}",
                p.display()
            );
            continue; // 数据没救出来就不删
        }
        match std::fs::remove_dir_all(&p) {
            Ok(()) => removed += 1,
            Err(err) => eprintln!(
                "[pf-plugin] 旧版本目录暂不可删 {}: {err}（下次启动再清）",
                p.display()
            ),
        }
    }
    removed
}

/// 启动期：对台账里每个包清旧版本目录（`prune_old_versions` 的全量版）。跑在
/// `migrate_data_dirs` 之后、`scan` 之前。返回 (包名, 删掉数) 供日志。
pub fn prune_stale_versions(root: &Path, ledger: &[LedgerEntry]) -> Vec<(String, usize)> {
    ledger
        .iter()
        .filter_map(|l| {
            let pkg_dir = root.join(dir_name(&l.name));
            if !pkg_dir.is_dir() {
                return None;
            }
            let n = prune_old_versions(&pkg_dir, &l.version);
            (n > 0).then(|| (l.name.clone(), n))
        })
        .collect()
}

pub(super) fn now_iso() -> String {
    // 无 chrono 依赖：epoch 秒即可（台账时间戳仅排障用）
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("epoch:{secs}")
}
