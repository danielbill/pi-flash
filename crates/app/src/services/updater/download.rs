//! 下载引擎与进度通道（081）：sha256sum 解析 / zip 条目消毒 / 并行解压 /
//! 测速守卫下载（边下边算 SHA-256）/ 镜像逐源切换 / 进度原子量（下载/
//! 解压百分比 + 相位）。自 fetch.rs 切出（itlq.9 超规拆分，纯结构搬移）。

use super::*;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub(crate) fn asset_name(version: &str) -> String {
    format!("pi-flash-{}-win32-x64.zip", version_key_string(version))
}

/// sha256sum 一行解析（`<hash>  file` / `<hash> *file`，与 lib.js 同规则）。
pub(crate) fn parse_hash_for(text: &str, filename: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        let Some((hash, name)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let hash = hash.trim().to_ascii_lowercase();
        if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        let name = name.trim().trim_start_matches('*').trim();
        let name = name.strip_prefix("./").unwrap_or(name);
        if name == filename {
            return Some(hash);
        }
    }
    None
}

/// zip 条目名安全化：拒绝绝对路径 / `..` / 盘符；分隔符归一。返回 None = 跳过。
pub(crate) fn safe_zip_path(dest: &Path, name: &str) -> Option<PathBuf> {
    let name = name.replace('\\', "/");
    if name.starts_with('/') || name.contains(':') {
        return None;
    }
    let mut out = dest.to_path_buf();
    for part in name.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

/// 并行解压。条目互不依赖，按原子下标工作窃取；Windows 瓶颈是 Defender
/// 对每个新建文件的实时扫描，多线程摊薄单文件延迟（单线程 4 万小文件
/// 分钟级，并行后 10s 上下）。进度写 EXTRACT_PCT 供版本号位读取。
/// 任一 Worker 出错即置 failed 停止取活，返回首个错误。
pub(crate) fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
    let f = std::fs::File::open(zip_path).map_err(|e| format!("{e}"))?;
    let total = {
        let probe = zip::ZipArchive::new(f).map_err(|e| format!("{e}"))?;
        probe.len()
    };
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, 8);
    let next = Arc::new(AtomicUsize::new(0));
    let done = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicBool::new(false));
    let first_err: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    std::thread::scope(|s| {
        for _ in 0..workers {
            // 每 worker 独立打开：File::try_clone 复制的句柄共享文件指针，
            // 各线程 seek 会互相踩（「failed to fill whole buffer」实录）
            let f = match std::fs::File::open(zip_path) {
                Ok(f) => f,
                Err(e) => {
                    failed.store(true, Ordering::Relaxed);
                    let mut slot = first_err.lock().unwrap();
                    if slot.is_none() {
                        *slot = Some(format!("{e}"));
                    }
                    break;
                }
            };
            let next = next.clone();
            let done = done.clone();
            let failed = failed.clone();
            let first_err = first_err.clone();
            s.spawn(move || {
                let mut archive = match zip::ZipArchive::new(f) {
                    Ok(a) => a,
                    Err(e) => {
                        failed.store(true, Ordering::Relaxed);
                        let mut slot = first_err.lock().unwrap();
                        if slot.is_none() {
                            *slot = Some(format!("{e}"));
                        }
                        return;
                    }
                };
                loop {
                    if failed.load(Ordering::Relaxed) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= total {
                        return;
                    }
                    let r = (|| -> Result<(), String> {
                        let mut entry = archive.by_index(i).map_err(|e| format!("{e}"))?;
                        let Some(out) = safe_zip_path(dest, entry.name()) else {
                            return Ok(());
                        };
                        if entry.is_dir() {
                            std::fs::create_dir_all(&out).map_err(|e| format!("{e}"))?;
                            return Ok(());
                        }
                        if let Some(parent) = out.parent() {
                            // 并行下兄弟条目可能同时在建同一父目录，
                            // AlreadyExists 视为成功
                            if let Err(e) = std::fs::create_dir_all(parent) {
                                if e.kind() != std::io::ErrorKind::AlreadyExists {
                                    return Err(format!("{e}"));
                                }
                            }
                        }
                        let mut out_f = std::fs::File::create(&out).map_err(|e| format!("{e}"))?;
                        std::io::copy(&mut entry, &mut out_f)
                            .map(|_| ())
                            .map_err(|e| format!("{e}"))
                    })();
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    if total > 0 {
                        EXTRACT_PCT.store(((n * 100) / total).min(100) as u8, Ordering::Relaxed);
                    }
                    if let Err(e) = r {
                        failed.store(true, Ordering::Relaxed);
                        let mut slot = first_err.lock().unwrap();
                        if slot.is_none() {
                            *slot = Some(e);
                        }
                        return;
                    }
                }
            });
        }
    });

    if let Some(e) = first_err.lock().unwrap().take() {
        return Err(e);
    }
    Ok(())
}

/// win32 zip 根带 `pi-flash/` 包装目录（release.sh Compress-Archive 行为）；
/// 找到含 pi-flash.exe 的那一层。
pub(crate) fn locate_content_root(extract_dir: &Path) -> Option<PathBuf> {
    for root in [extract_dir.join("pi-flash"), extract_dir.to_path_buf()] {
        if root.join(exe_rel()).is_file() {
            return Some(root);
        }
    }
    None
}

/// 下载进度（0-100），None = 无进度数据（未在下载 / 响应无 content-length）。
/// 下载线程 store、新会话页版本号位渲染 load——spinner 动画每帧触发重绘，
/// 原子读足够，无需实体通知。
static DOWNLOAD_PCT: AtomicU8 = AtomicU8::new(PCT_UNKNOWN);
const PCT_UNKNOWN: u8 = 255;

/// 当前下载进度（0-100），供版本号位 UI 与 pif-ui snapshot 读取。
pub(crate) fn download_percent() -> Option<u8> {
    match DOWNLOAD_PCT.load(Ordering::Relaxed) {
        PCT_UNKNOWN => None,
        pct => Some(pct),
    }
}

/// 写进度：None = 未知（哨兵 255）。已知值钳到 100。
pub(crate) fn set_download_percent(pct: Option<u8>) {
    let v = match pct {
        Some(p) => p.min(100),
        None => PCT_UNKNOWN,
    };
    DOWNLOAD_PCT.store(v, Ordering::Relaxed);
}

/// 流水线相位：版本号位的进度通道二选一（下载 NN% / 解压 NN%）。
/// 解压此前无通道，100% 后单线程解压 4 万小文件（Defender 逐个实时扫描）
/// 要分钟级，UI 只能干等——0.1.8 用户实录「下载完卡一分钟」。
pub(crate) const PHASE_IDLE: u8 = 0;
pub(crate) const PHASE_DOWNLOAD: u8 = 1;
pub(crate) const PHASE_EXTRACT: u8 = 2;

static UPDATE_PHASE: AtomicU8 = AtomicU8::new(PHASE_IDLE);
static EXTRACT_PCT: AtomicU8 = AtomicU8::new(PCT_UNKNOWN);

/// 当前相位（PHASE_*），供版本号位 UI 与 pif-ui snapshot 读取。
pub(crate) fn update_phase() -> u8 {
    UPDATE_PHASE.load(Ordering::Relaxed)
}

/// 解压进度（0-100），None = 无进度数据（不在解压相位）。
pub(crate) fn extract_percent() -> Option<u8> {
    match EXTRACT_PCT.load(Ordering::Relaxed) {
        PCT_UNKNOWN => None,
        pct => Some(pct),
    }
}

/// 切相位并清两条进度通道（UI 永远吃不到上一相位的陈旧百分比）。
pub(crate) fn set_phase(phase: u8) {
    UPDATE_PHASE.store(phase, Ordering::Relaxed);
    DOWNLOAD_PCT.store(PCT_UNKNOWN, Ordering::Relaxed);
    EXTRACT_PCT.store(PCT_UNKNOWN, Ordering::Relaxed);
}

/// 测速参数（env 可调，与 npm/bin/lib.js 同名变量同默认值；测试注入更小的
/// probe/阈值以秒级完成）。
pub(crate) fn speed_probe() -> Duration {
    Duration::from_millis(
        std::env::var("PI_FLASH_PROBE_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000),
    )
}

pub(crate) fn min_speed_bps() -> f64 {
    std::env::var("PI_FLASH_MIN_KBPS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|kbps| kbps * 1024.0)
        .unwrap_or(MIN_SPEED_BPS)
}

/// 下载到文件，边下边算 SHA-256（单遍）。开传满 probe 平均速度仍低于 min_bps
/// 即 Err（测速不过，由上层换源）。返回 hex 摘要。
pub(crate) fn download_and_hash_cfg(
    url: &str,
    dest: &Path,
    probe: Duration,
    min_bps: f64,
) -> Result<String, String> {
    use sha2::Digest;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(CHECK_TIMEOUT)
        .timeout(DOWNLOAD_TIMEOUT)
        .build();
    let resp = agent
        .get(url)
        .set("User-Agent", UA)
        .call()
        .map_err(|e| format!("{e}"))?;
    let total: u64 = resp
        .header("content-length")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    set_download_percent((total > 0).then_some(0));
    let mut reader = resp.into_reader();
    let mut out = std::fs::File::create(dest).map_err(|e| format!("{e}"))?;
    let mut h = sha2::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut got: u64 = 0;
    let mut last_pct: u8 = 0;
    let t0 = std::time::Instant::now();
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("{e}"))?;
        if n == 0 {
            break;
        }
        got += n as u64;
        let elapsed = t0.elapsed();
        if elapsed >= probe {
            let speed = got as f64 / elapsed.as_secs_f64();
            if speed < min_bps {
                return Err(format!(
                    "测速不过（{}KB/s < {}KB/s）",
                    (speed / 1024.0) as u64,
                    (min_bps / 1024.0) as u64
                ));
            }
        }
        if total > 0 {
            let pct = ((got as f64 / total as f64) * 100.0).min(100.0) as u8;
            if pct != last_pct {
                set_download_percent(Some(pct));
                last_pct = pct;
            }
        }
        h.update(&buf[..n]);
        out.write_all(&buf[..n]).map_err(|e| format!("{e}"))?;
    }
    Ok(format!("{:x}", h.finalize()))
}

/// 按候选源逐个试下载（每源一次，测速不过/网络错误都换下一个），全灭才报错。
/// 返回（zip 的 SHA-256，命中源 base——校验和钉官方优先时兜底用）。
pub(crate) fn download_mirrored(
    asset: &str,
    dest: &Path,
    ver: &str,
) -> Result<(String, String), String> {
    download_mirrored_cfg(
        asset,
        dest,
        ver,
        &download_bases(),
        speed_probe(),
        min_speed_bps(),
    )
}

pub(crate) fn download_mirrored_cfg(
    asset: &str,
    dest: &Path,
    ver: &str,
    bases: &[String],
    probe: Duration,
    min_bps: f64,
) -> Result<(String, String), String> {
    let mut errors = Vec::new();
    for base in bases {
        eprintln!("[updater] 下载源：{base}");
        match download_and_hash_cfg(&format!("{base}/v{ver}/{asset}"), dest, probe, min_bps) {
            Ok(h) => return Ok((h, base.clone())),
            Err(e) => {
                eprintln!("[updater] 源不可用（{e}），换下一个");
                errors.push(format!("{base}：{e}"));
            }
        }
    }
    Err(format!(
        "所有下载源均失败：{}；可设 PI_FLASH_MIRROR=<base> 指定源，或到 GitHub release 页手动下载",
        errors.join("；")
    ))
}

pub(crate) fn fetch_text(url: &str) -> Result<String, String> {
    let resp = check_agent()
        .get(url)
        .set("User-Agent", UA)
        .call()
        .map_err(|e| format!("{e}"))?;
    resp.into_string().map_err(|e| format!("{e}"))
}
