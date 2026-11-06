//! 落位管线（081）：下载 + SHA-256 校验（sidecar → SHA256SUMS 兜底）+ 解压
//! 到新载荷目录 `<root>/<基名>-<版本>/`（版本戳最后写）+ 触达预热 + 稳定
//! 入口预拷 + pif-secret/pi 指针注册 + 更新日志缓存。自 fetch.rs 切出
//!（itlq.9 超规拆分，纯结构搬移）。

use super::*;
use std::path::Path;

/// 下载 + SHA-256 校验（sidecar → SHA256SUMS 兜底，080 §3.2 同序）+ 解压到
/// 新的载荷目录 `<root>/<基名>-<版本>/`（版本戳最后写）。成功返回版本号。
/// 落位目标就是「更新后要运行的目录」——没有 rename、没有标记文件。
pub(crate) fn stage_update(rel: &ReleaseInfo) -> Result<String, String> {
    let me = my_exe_dir().ok_or("无法定位当前载荷目录")?;
    stage_update_in(&me, rel)
}

/// 显式传入**当前载荷目录**的版本（测试注入用：测试进程的 current_exe 指向
/// target/，不隔离）。下载源仍走 download_bases()（测试用 PI_FLASH_MIRROR
/// 定向本地假源）。
pub(crate) fn stage_update_in(me: &Path, rel: &ReleaseInfo) -> Result<String, String> {
    let root = me.parent().ok_or("无法定位安装根目录")?.to_path_buf();
    let ver = version_key_string(&rel.version);
    let stem = base_name(&me.file_name().unwrap_or_default().to_string_lossy());
    let target = root.join(format!("{stem}-{ver}"));
    if target == me {
        return Err(format!("目标目录就是当前运行目录（v{ver}），拒绝覆盖"));
    }
    let tmp = root.join(format!("update.tmp-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(tmp.join("x")).map_err(|e| format!("{e}"))?;

    let result = (|| -> Result<(), String> {
        let asset = asset_name(&ver);
        let zip_path = tmp.join(&asset);

        set_phase(PHASE_DOWNLOAD);
        // 下载（测速自动换源：直连慢/挂就按序切镜像），边下边算 SHA-256
        let (zip_hash, chosen) = download_mirrored(&asset, &zip_path, &ver)?;

        // 校验和：直连优先钉定官方哈希（zip 走镜像时防串包）→ 命中源兜底；
        // sidecar 404 → SHA256SUMS 兜底
        let expected = {
            let mut found = None;
            let mut bases = vec![RELEASES_DOWNLOAD_BASE.to_string()];
            if chosen != RELEASES_DOWNLOAD_BASE {
                bases.push(chosen);
            }
            'outer: for b in &bases {
                for suffix in [format!("{asset}.sha256"), "SHA256SUMS".to_string()] {
                    match fetch_text(&format!("{b}/v{ver}/{suffix}")) {
                        Ok(text) => {
                            if let Some(h) = parse_hash_for(&text, &asset) {
                                found = Some(h);
                                break 'outer;
                            }
                        }
                        Err(_) => continue, // 404 / 网络抖动 → 试下一个来源
                    }
                }
            }
            found.ok_or_else(|| format!("release 资产里找不到 {asset} 的 SHA-256，放弃安装"))?
        };
        if zip_hash != expected {
            return Err(format!("SHA-256 不符（期望 {expected}，实际 {zip_hash}）"));
        }

        set_phase(PHASE_EXTRACT);
        extract_zip(&zip_path, &tmp.join("x"))?;
        let content =
            locate_content_root(&tmp.join("x")).ok_or("载荷结构异常：找不到 pi-flash.exe")?;

        // 上一轮半截的同版本目录先清（解压中断/被清），再 rename 成最终目录
        let _ = std::fs::remove_dir_all(&target);
        std::fs::rename(&content, &target).map_err(|e| format!("载荷落位失败：{e}"))?;

        // 安装阶段把成本花完：每个文件读一遍，然后才写版本戳。有戳 = 装好且
        // 成本已付（点重启、以后每次打开都是秒开）；没戳 = 没装（交接不用它，
        // 下次重装）。
        touch_tree(&target);
        std::fs::write(target.join(".payload-version"), format!("{ver}\n"))
            .map_err(|e| format!("写版本戳失败：{e}"))?;

        // T1 预拷（ab0c 优化，release only）：趁老进程锁不住大部分文件，后台
        // 把新载荷尽量刷进稳定入口 `payload\`——锁住的记 `.payload-stage` 标
        // 记，首启 refresh_stable_entry 按清单收尾。debug 构建不碰安装树（与
        // win_pin / 自愈同纪律），e2e 夹具也不受扰动。
        if !cfg!(debug_assertions) {
            let precopy_root = root.clone();
            let precopy_me = me.to_path_buf();
            let precopy_target = target.clone();
            let precopy_ver = ver.clone();
            let _ = std::thread::Builder::new()
                .name("precopy".into())
                .spawn(move || {
                    precopy_stable_entry(&precopy_root, &precopy_me, &precopy_target, &precopy_ver);
                });
        }

        // 051：新载荷落位后顺手确保 pif-secret 稳定安装（幂等毫秒级）——
        // 版本化载荷目录随升级漂移，helper 的归宿永远是用户级 PATH 目录。
        let _ = pi_link::register_to_sys::ensure_installed();
        // 081：内置 pi 内核注册为系统全局 `pi`——指针直接指向 **staged 新载
        // 荷**（交接后用户敲 `pi` 即新内核）。旧包没有 pi.exe → 复制静默跳
        // 过、指针照写，下版起效。
        let node = {
            let bundled = target.join("node.exe");
            if bundled.is_file() {
                bundled.to_string_lossy().into_owned()
            } else {
                pi_link::vendor::node_bin()
            }
        };
        let _ = pi_link::register_to_sys::ensure_pi_shim(
            &node,
            &target
                .join(pi_link::vendor::VENDOR_DIR_NAME)
                .join(pi_link::vendor::NODE_MODULES_REL),
        );

        // 更新日志缓存（重启后的更新日志页直接读本地）
        if !rel.body.is_empty() {
            if let Some(p) = pi_link::paths::changelog_cache_file(&ver) {
                if let Some(parent) = p.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let _ = std::fs::write(&p, &rel.body);
            }
        }
        Ok(())
    })();

    set_phase(PHASE_IDLE);
    let _ = std::fs::remove_dir_all(&tmp);
    result.map(|_| ver)
}
