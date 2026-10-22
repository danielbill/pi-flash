//! 软件自动更新（081-软件自动更新.md）：揭幕后延迟加载阶段后台检查 GitHub
//! release → on=后台下载→SHA-256 校验→解压落位新载荷目录→inputpanel 右下
//! 版本号位提示「点击重启更新」；off=版本号「x.x.x↑」可点击开 release 页。
//! 新版本首启自动开 topbar「更新日志」tab；新会话页版本号淡字点击 = 手动开
//! 同一页（show_changelog）。
//!
//! # 落位布局（v0.1.11 改版，见 081 §7）
//! ```text
//! <root>/payload/            安装时的载荷（npm 命令与快捷方式的稳定入口）
//! <root>/payload-<版本>/      应用内更新落位的新载荷（版本写在目录名里）
//! ```
//! `<root>` = exe 所在目录的父目录（npm 薄壳 = `<pkg>`，绿色解压包 = 解压目录）。
//!
//! **更新的全部动作 = 解压出一个新目录 + 启动时交接**：
//! 1. 下载解压到 `<root>/<载荷目录名>-<新版本>/`，版本戳 `.payload-version`
//!    最后写（戳在 = 目录完整）——不碰任何在用文件；
//! 2. 任何 exe 启动最前处（开窗之前）扫到「版本更高的完整载荷目录」就 spawn
//!    它的 exe 并退出（交接）。双击旧目录的 exe 也能自动进新版。
//!
//! 0.1.6~0.1.10 的「重启后版本没更新→再次下载」死循环全部源自**整目录
//! rename 撞 Windows 文件句柄**（孤儿 node sidecar / Defender 实时扫描 /
//! Access denied；helper + 标记 + 重试 + 预热堆到 400 行仍在真机翻车）。
//! 新方案不 rename、不等旧进程退出、不用 helper、不写标记文件，从根上不存在
//! 该失败模式；旧版本目录由新版启动时后台清理（rename 当占用探针，删不动就留）。
//!
//! # 网络纪律（设计文档定）
//! 检测 timeout 30s，失败重试 2 次、间隔 10 分钟，全程后台线程，绝不阻塞
//! 主进程；失败静默回落 Idle，下次启动再来。下载源与 npm 薄壳同规则：直连
//! 优先，开传 3s 平均 <128KB/s 即按序切默认镜像（`PI_FLASH_MIRROR` 钉定唯一源 /
//! `PI_FLASH_MIRRORS` 追加候选）。

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 下载/检查仓库坐标（与 npm/bin/lib.js DIRECT_BASE 同源）。
const RELEASES_DOWNLOAD_BASE: &str = "https://github.com/danielbill/pi-flash/releases/download";
/// 默认镜像候选（`<base>/v<version>/<asset>` 同构布局的前缀加速，2026-10 实测
/// 可用，与 npm/bin/lib.js DEFAULT_MIRRORS 同表；失效后 PR 更新两处）。
const DEFAULT_MIRRORS: [&str; 3] = [
    "https://gh-proxy.com/https://github.com/danielbill/pi-flash/releases/download",
    "https://ghfast.top/https://github.com/danielbill/pi-flash/releases/download",
    "https://ghproxy.net/https://github.com/danielbill/pi-flash/releases/download",
];
/// 测速切换（与 npm 薄壳 lib.js 同参数）：开传满 probe（`PI_FLASH_PROBE_MS`，
/// 默认 3s）平均速度仍低于阈值（`PI_FLASH_MIN_KBPS`，默认 128KB/s）即弃源。
const MIN_SPEED_BPS: f64 = 128.0 * 1024.0;
const RELEASES_API_BASE: &str = "https://api.github.com/repos/danielbill/pi-flash/releases";
const UA: &str = "pi-flash-updater";
/// 检测超时（设计文档：30s）。
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// 更新日志现拉超时（用户在等页面，15s 即见「暂时无法连接 GitHub」提示）。
const CHANGELOG_TIMEOUT: Duration = Duration::from_secs(15);
/// 检测失败重试间隔（设计文档：10 分钟 ×2 次）。
const RETRY_INTERVAL: Duration = Duration::from_secs(600);
/// 下载整体预算（zip ≈85MB；连接超时 30s 不封总量）。
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

// ---------------------------------------------------------------------------
// 状态
// ---------------------------------------------------------------------------

/// 版本号位（新会话页 action_bar）的状态机。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum UpdateState {
    /// 无动作：检查中 / 无新版 / 检查失败 / 已是最新。
    Idle,
    /// 后台下载中：转圈 + 「下载 x.x.x 版本中…」（faint）。
    Downloading { version: String },
    /// 新载荷已就位：待重启（「下载完成，点击重启更新」，青蓝）。
    Ready { version: String },
    /// 自动更新 off：有新版（「x.x.x↑」青蓝可点击，开 release 页）。
    Available { version: String },
}

/// 更新日志 tab 的数据（body = markdown；None = 还没拿到）。
#[derive(Debug, Clone)]
pub(crate) struct ChangelogPage {
    pub(crate) version: String,
    pub(crate) body: Option<String>,
    pub(crate) fetching: bool,
    /// 现拉失败（网络不通/被墙，15s 超时）：页面显示「暂时无法连接
    /// GitHub」而非「暂无更新日志」；再点版本号即重试。
    pub(crate) fetch_failed: bool,
}

// ---------------------------------------------------------------------------
// 版本号比较（x.y.z 逐段数值；缺段/非数按 0）
// ---------------------------------------------------------------------------

fn version_key(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.trim().parse().unwrap_or(0))
        .collect()
}

/// a 是否严格大于 b。
pub(crate) fn version_gt(a: &str, b: &str) -> bool {
    let (ka, kb) = (version_key(a), version_key(b));
    let n = ka.len().max(kb.len());
    for i in 0..n {
        let (x, y) = (
            ka.get(i).copied().unwrap_or(0),
            kb.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

pub(crate) fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn version_key_string(v: &str) -> String {
    v.trim().trim_start_matches('v').to_string()
}

/// `x.y.z` 形状（≥2 段、全数字）。用来在目录名里认出「版本」段。
pub(crate) fn is_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

// ---------------------------------------------------------------------------
// 落位布局：载荷目录 = exe 所在目录；安装根 = 它的父目录
// ---------------------------------------------------------------------------

/// 平台载荷 exe 相对载荷目录的路径。
fn exe_rel() -> &'static str {
    if cfg!(windows) {
        "pi-flash.exe"
    } else {
        "pi-flash.app/Contents/MacOS/pi-flash"
    }
}

fn my_exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

/// 载荷目录名剥掉尾部 `-<版本>`：`payload-0.1.11` → `payload`、
/// `pi-flash-0.1.11` → `pi-flash`、`payload` → `payload`（绿色包同理）。
fn base_name(dir_name: &str) -> String {
    match dir_name.rsplit_once('-') {
        Some((base, tail)) if !base.is_empty() && is_version(tail) => base.to_string(),
        _ => dir_name.to_string(),
    }
}

/// 载荷目录是否完整：目录名以 `-<x.y.z>` 结尾 **且** 版本戳与之一致 **且**
/// exe 在位。返回该目录承载的版本。戳是解压完成后最后写的，戳吻合 = 完整
/// （半截下载/中断解压的目录天然落选，无需额外标记文件）。
fn payload_version_of(dir: &Path) -> Option<String> {
    let name = dir.file_name()?.to_str()?;
    let ver = name.rsplit_once('-')?.1;
    if !is_version(ver) {
        return None;
    }
    let stamp = std::fs::read_to_string(dir.join(".payload-version")).ok()?;
    (stamp.trim() == ver && dir.join(exe_rel()).is_file()).then(|| ver.to_string())
}

/// 安装根下所有完整载荷目录：(版本, 目录)。
fn versioned_payloads(root: &Path) -> Vec<(String, PathBuf)> {
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
fn staged_newer(root: &Path, cur: &str) -> Option<(String, PathBuf)> {
    versioned_payloads(root)
        .into_iter()
        .filter(|(v, _)| version_gt(v, cur))
        .max_by(|a, b| version_key(&a.0).cmp(&version_key(&b.0)))
}

/// 能切过去的更高版本载荷：排除**自己所在的载荷目录**——它已经是我们了，
/// 纳入会把「点击重启」变成「拉起自己」，也会让交接自己踢自己。
fn pick_next(root: &Path, cur: &str, me: &Path) -> Option<(String, PathBuf)> {
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
fn self_version_matches(me: &Path) -> bool {
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
fn cleanup_older(root: PathBuf, me: PathBuf) {
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
fn older_than_an_hour(entry: &std::fs::DirEntry) -> bool {
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
fn remove_path(p: &Path) {
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
fn touch_tree(dir: &Path) -> (usize, u128) {
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

// ---------------------------------------------------------------------------
// release 拉取
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct ReleaseInfo {
    pub version: String,
    pub body: String,
}

fn api_base() -> String {
    std::env::var("PI_FLASH_RELEASES_API")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| RELEASES_API_BASE.to_string())
}

/// 候选下载源：`PI_FLASH_MIRROR` 指定唯一源（跳过测速切换）；否则直连优先 +
/// `PI_FLASH_MIRRORS` 追加 + 默认镜像（与 npm/bin/lib.js downloadBases 同规则）。
fn download_bases() -> Vec<String> {
    if let Ok(m) = std::env::var("PI_FLASH_MIRROR") {
        let m = m.trim().trim_end_matches('/').to_string();
        if !m.is_empty() {
            return vec![m];
        }
    }
    let extra = std::env::var("PI_FLASH_MIRRORS")
        .map(|s| {
            s.split([',', ' ', ';'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_end_matches('/').to_string())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut bases = vec![RELEASES_DOWNLOAD_BASE.to_string()];
    bases.extend(extra);
    bases.extend(DEFAULT_MIRRORS.iter().map(|s| s.to_string()));
    bases
}

fn check_agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(CHECK_TIMEOUT).build()
}

/// latest release（含 body = 更新日志 markdown）。tag 形如 v1.2.3。timeout
/// 由调用方定：更新检查走设计文档的 30s，更新日志现拉走 15s。
fn fetch_release(url: &str, timeout: Duration) -> Result<ReleaseInfo, String> {
    let resp = ureq::AgentBuilder::new()
        .timeout(timeout)
        .build()
        .get(url)
        .set("User-Agent", UA)
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| format!("{e}"))?;
    // 注意：ureq 2 的 into_string 返回 io::Result（与 call 的 ureq::Error 不同型）
    let text = resp.into_string().map_err(|e| format!("{e}"))?;
    let json: serde_json::Value = serde_json::from_str(&text).map_err(|e| format!("{e}"))?;
    let tag = json
        .get("tag_name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or("release 缺 tag_name")?;
    Ok(ReleaseInfo {
        version: version_key_string(tag),
        body: json
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
    })
}

fn fetch_latest() -> Result<ReleaseInfo, String> {
    fetch_release(&format!("{}/latest", api_base()), CHECK_TIMEOUT)
}

/// 按 tag 拉更新日志（更新日志页缓存缺失时的现拉通道）。15s 超时：被墙/
/// 断网时尽快落「暂时无法连接 GitHub」提示，不让用户对着转圈干等。
fn fetch_release_body(version: &str) -> Result<String, String> {
    let url = format!("{}/tags/v{}", api_base(), version_key_string(version));
    fetch_release(&url, CHANGELOG_TIMEOUT).map(|r| r.body)
}

// ---------------------------------------------------------------------------
// 下载 → 校验 → 落位
// ---------------------------------------------------------------------------

fn asset_name(version: &str) -> String {
    format!("pi-flash-{}-win32-x64.zip", version_key_string(version))
}

/// sha256sum 一行解析（`<hash>  file` / `<hash> *file`，与 lib.js 同规则）。
fn parse_hash_for(text: &str, filename: &str) -> Option<String> {
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
fn safe_zip_path(dest: &Path, name: &str) -> Option<PathBuf> {
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
fn extract_zip(zip_path: &Path, dest: &Path) -> Result<(), String> {
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
fn locate_content_root(extract_dir: &Path) -> Option<PathBuf> {
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
fn set_download_percent(pct: Option<u8>) {
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
fn set_phase(phase: u8) {
    UPDATE_PHASE.store(phase, Ordering::Relaxed);
    DOWNLOAD_PCT.store(PCT_UNKNOWN, Ordering::Relaxed);
    EXTRACT_PCT.store(PCT_UNKNOWN, Ordering::Relaxed);
}

/// 测速参数（env 可调，与 npm/bin/lib.js 同名变量同默认值；测试注入更小的
/// probe/阈值以秒级完成）。
fn speed_probe() -> Duration {
    Duration::from_millis(
        std::env::var("PI_FLASH_PROBE_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(3000),
    )
}

fn min_speed_bps() -> f64 {
    std::env::var("PI_FLASH_MIN_KBPS")
        .ok()
        .and_then(|v| v.parse::<f64>().ok())
        .map(|kbps| kbps * 1024.0)
        .unwrap_or(MIN_SPEED_BPS)
}

/// 下载到文件，边下边算 SHA-256（单遍）。开传满 probe 平均速度仍低于 min_bps
/// 即 Err（测速不过，由上层换源）。返回 hex 摘要。
fn download_and_hash_cfg(
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
fn download_mirrored(asset: &str, dest: &Path, ver: &str) -> Result<(String, String), String> {
    download_mirrored_cfg(
        asset,
        dest,
        ver,
        &download_bases(),
        speed_probe(),
        min_speed_bps(),
    )
}

fn download_mirrored_cfg(
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

fn fetch_text(url: &str) -> Result<String, String> {
    let resp = check_agent()
        .get(url)
        .set("User-Agent", UA)
        .call()
        .map_err(|e| format!("{e}"))?;
    resp.into_string().map_err(|e| format!("{e}"))
}

/// 下载 + SHA-256 校验（sidecar → SHA256SUMS 兜底，080 §3.2 同序）+ 解压到
/// 新的载荷目录 `<root>/<基名>-<版本>/`（版本戳最后写）。成功返回版本号。
/// 落位目标就是「更新后要运行的目录」——没有 rename、没有标记文件。
fn stage_update(rel: &ReleaseInfo) -> Result<String, String> {
    let me = my_exe_dir().ok_or("无法定位当前载荷目录")?;
    stage_update_in(&me, rel)
}

/// 显式传入**当前载荷目录**的版本（测试注入用：测试进程的 current_exe 指向
/// target/，不隔离）。下载源仍走 download_bases()（测试用 PI_FLASH_MIRROR
/// 定向本地假源）。
fn stage_update_in(me: &Path, rel: &ReleaseInfo) -> Result<String, String> {
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

// ---------------------------------------------------------------------------
// last_seen（新版本首启判定）
// ---------------------------------------------------------------------------

fn read_last_seen() -> Option<String> {
    let raw = std::fs::read_to_string(pi_link::paths::update_state_file()?).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    v.get("last_seen_version")
        .and_then(|x| x.as_str())
        .map(str::to_string)
}

fn write_last_seen(version: &str) {
    let Some(p) = pi_link::paths::update_state_file() else {
        return;
    };
    if let Some(parent) = p.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = pi_link::config::write_json(&p, &serde_json::json!({ "last_seen_version": version }));
}

fn read_changelog_cache(version: &str) -> Option<String> {
    std::fs::read_to_string(pi_link::paths::changelog_cache_file(version)?)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

// ---------------------------------------------------------------------------
// Chat 接线
// ---------------------------------------------------------------------------

impl crate::Chat {
    /// 调试/开发不跑更新（PI_FLASH_UPDATE_CHECK=1 可强制，0 可关）。
    fn checks_enabled() -> bool {
        match std::env::var("PI_FLASH_UPDATE_CHECK").as_deref() {
            Ok("1") => true,
            Ok("0") => false,
            _ => !cfg!(debug_assertions),
        }
    }

    /// 揭幕后的两个延迟加载钩子（010 §延迟加载「版本更新检查」）：
    /// ① 新版本首启 → 自动开「更新日志」tab；② 后台检查更新。
    /// 关闭时（debug 默认）全程不动，含 last_seen 记账——dev 构建跑一版就
    /// 写 last_seen=新版，正式版更新后首启便永远不算「新版本首启」。
    pub(crate) fn spawn_startup_update_hooks(&mut self, cx: &mut gpui::Context<Self>) {
        if !Self::checks_enabled() {
            return;
        }
        let cur = current_version().to_string();
        match read_last_seen() {
            Some(seen) if version_gt(&cur, &seen) => {
                self.show_changelog(cur.clone(), cx);
                write_last_seen(&cur);
            }
            None => write_last_seen(&cur), // 首次运行：只记账，不开页
            _ => {}
        }
        self.spawn_update_check(cx);
    }

    /// 打开某版本的更新日志页（新版本首启钩子与新会话页版本号点击共用）：
    /// 内存已有同版本页 = 正文在就直接开；拉取中不打断；两者皆无（上次
    /// 拉取失败）置回 fetching 重试。否则读本地缓存装页，缺失则后台按
    /// tag 现拉（只展示不回写缓存——看一次的页面）。
    pub(crate) fn show_changelog(&mut self, version: String, cx: &mut gpui::Context<Self>) {
        let mut need_fetch = false;
        if self
            .aux
            .changelog
            .as_ref()
            .is_some_and(|p| p.version == version)
        {
            if let Some(p) = self.aux.changelog.as_mut() {
                if p.body.is_none() && !p.fetching {
                    p.fetching = true;
                    need_fetch = true;
                }
            }
        } else {
            let cached = read_changelog_cache(&version);
            need_fetch = cached.is_none();
            self.aux.changelog = Some(ChangelogPage {
                version: version.clone(),
                fetching: need_fetch,
                body: cached,
                fetch_failed: false,
            });
        }
        self.open_changelog_tab(cx);
        if need_fetch {
            self.spawn_changelog_fetch(version, cx);
        }
    }

    fn spawn_changelog_fetch(&mut self, version: String, cx: &mut gpui::Context<Self>) {
        cx.spawn(async move |this, cx| {
            let body = cx
                .background_executor()
                .spawn(async move { fetch_release_body(&version) })
                .await;
            let _ = this.update(cx, |c, cx| {
                if let Some(p) = &mut c.aux.changelog {
                    match body {
                        // 空正文 = release 无 notes，走「暂无更新日志」，不算连接失败
                        Ok(b) => {
                            if !b.trim().is_empty() {
                                p.body = Some(b);
                            }
                            p.fetch_failed = false;
                        }
                        Err(_) => p.fetch_failed = true,
                    }
                    p.fetching = false;
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 打开（或聚焦）topbar「更新日志」tab。
    pub(crate) fn open_changelog_tab(&mut self, cx: &mut gpui::Context<Self>) {
        match self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, crate::PanelTab::Changelog))
        {
            Some(ix) => self.activate_panel_tab(ix, cx),
            None => {
                self.ws.panel_tabs.push(crate::PanelTab::Changelog);
                let ix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
            }
        }
        self.set_content_view(crate::ContentView::Changelog);
        cx.notify();
    }

    /// Ready 提示点击（「下载完成，点击重启更新」）：persist UI → spawn 新版
    /// 载荷目录里的 exe（新目录，零锁）→ 本进程退出。spawn 不起来就原地不动，
    /// 提示留给下次启动的交接。
    pub(crate) fn restart_to_apply_update(&mut self, cx: &mut gpui::Context<Self>) {
        self.persist_ui();
        let Some((version, dir)) = next_payload() else {
            // 没有可切的更高版本（已被清/自己就是最新）→ 回落 Idle
            self.aux.update = UpdateState::Idle;
            cx.notify();
            return;
        };
        let exe = dir.join(exe_rel());
        eprintln!("[updater] 重启切换到 v{version}：{}", exe.display());
        if std::process::Command::new(&exe).spawn().is_ok() {
            cx.quit();
        }
    }

    /// 后台检查更新（on=下载落位新载荷目录，off=只提示 ↑）。检测失败重试 2
    /// 次、间隔 10 分钟（设计文档定），全程不阻塞主进程，最终失败静默。
    pub(crate) fn spawn_update_check(&mut self, cx: &mut gpui::Context<Self>) {
        if !Self::checks_enabled() {
            return;
        }
        // 已有就位的新载荷：不花网络，等重启
        if matches!(self.aux.update, UpdateState::Ready { .. }) {
            return;
        }
        cx.spawn(async move |this, cx| {
            // ① 检测（3 次尝试，间隔 10 分钟）
            let mut latest: Option<ReleaseInfo> = None;
            for attempt in 0..3 {
                let r = cx
                    .background_executor()
                    .spawn(async { fetch_latest() })
                    .await;
                match r {
                    Ok(rel) => {
                        latest = Some(rel);
                        break;
                    }
                    Err(e) => {
                        eprintln!("[updater] 检查更新失败（{e}）");
                        if attempt < 2 {
                            cx.background_executor().timer(RETRY_INTERVAL).await;
                        }
                    }
                }
            }
            let Some(rel) = latest else { return };
            if !version_gt(&rel.version, current_version()) {
                return; // 已是最新
            }
            // ② 自动更新开关（读最新设置：用户可能中途改）
            let auto = this
                .update(cx, |_, _| crate::services::workspace::auto_update())
                .unwrap_or(true);
            if !auto {
                let _ = this.update(cx, |c, cx| {
                    c.aux.update = UpdateState::Available {
                        version: rel.version.clone(),
                    };
                    cx.notify();
                });
                return;
            }
            // ③ 后台下载 → 落位新载荷目录
            let _ = this.update(cx, |c, cx| {
                c.aux.update = UpdateState::Downloading {
                    version: rel.version.clone(),
                };
                cx.notify();
            });
            let outcome = cx
                .background_executor()
                .spawn(async move { stage_update(&rel) })
                .await;
            let _ = this.update(cx, |c, cx| {
                c.aux.update = match &outcome {
                    Ok(v) => UpdateState::Ready { version: v.clone() },
                    Err(e) => {
                        eprintln!("[updater] 下载更新失败：{e}");
                        UpdateState::Idle
                    }
                };
                cx.notify();
            });
        })
        .detach();
    }
}

/// 打开该版本的 GitHub release 页（off 模式点击「x.x.x↑」）。
pub(crate) fn open_release_page(version: &str) {
    let url = format!(
        "https://github.com/danielbill/pi-flash/releases/tag/v{}",
        version_key_string(version)
    );
    open_url(&url);
}

fn open_url(url: &str) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        let _ = url;
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试 spawn powershell 必须 CREATE_NO_WINDOW——否则每跑一轮 cargo test
    /// 就弹一个 PowerShell 控制台窗抢屏幕（Compress-Archive 有进度条输出，
    /// 控制台会实体化成可见窗口；agent 代跑测试时尤甚）。
    fn hidden_powershell() -> std::process::Command {
        let mut cmd = std::process::Command::new("powershell.exe");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        cmd
    }

    /// 独立临时目录（测试进程共用 target/，不能污染真安装根）。
    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "pf-upd-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 造一个「完整载荷目录」：exe（假文件）+ 版本戳。
    fn fake_payload(dir: &Path, ver: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(exe_rel()), b"fake-exe").unwrap();
        std::fs::write(dir.join(".payload-version"), format!("{ver}\n")).unwrap();
    }

    #[test]
    fn version_compare() {
        assert!(version_gt("1.2.3", "1.2.2"));
        assert!(version_gt("v1.2.3", "1.2.2"));
        assert!(version_gt("1.10.0", "1.9.9"), "逐段数值比较，不是字典序");
        assert!(version_gt("0.2.0", "0.1.99"));
        assert!(!version_gt("1.2.3", "1.2.3"));
        assert!(!version_gt("1.2", "1.2.0"), "缺段按 0");
        assert!(!version_gt("0.1.0", "0.2.0"));
        assert!(!version_gt("abc", "0.0.1"), "非数段按 0");
    }

    #[test]
    fn base_name_strips_version_suffix() {
        assert_eq!(base_name("payload-0.1.11"), "payload");
        assert_eq!(base_name("pi-flash-0.1.11"), "pi-flash");
        assert_eq!(base_name("payload"), "payload");
        assert_eq!(base_name("pi-flash"), "pi-flash", "绿色包基名不能被吃掉");
        assert_eq!(base_name("my-app"), "my-app", "非数字尾巴不算版本");
        assert_eq!(base_name("pi-flash-0.1"), "pi-flash");
    }

    /// 交接自证：目录名版本必须等于进程版本，否则拒绝交接（防 spawn 风暴）。
    #[test]
    fn self_version_guard() {
        let root = temp_dir("selfver");
        let mine = root.join(format!("payload-{}", current_version()));
        fake_payload(&mine, current_version());
        assert!(self_version_matches(&mine), "版本一致应放行");
        let faked = root.join("payload-9.9.9");
        fake_payload(&faked, "9.9.9");
        assert!(!self_version_matches(&faked), "戳与进程版本不符应拒交");
        assert!(
            self_version_matches(&root.join("payload")),
            "无版本名照常交接"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 布局核心：目录名带版本 + 戳吻合 + exe 在位 = 可交接的新载荷；半截目录、
    /// 名字与戳不符、非版本目录一律落选。
    #[test]
    fn staged_newer_picks_complete_payload_only() {
        let root = temp_dir("layout");
        std::fs::create_dir_all(root.join("payload")).unwrap();
        fake_payload(&root.join("payload-0.1.10"), "0.1.10");
        // 半截：目录名对、戳缺失
        let broken = root.join("payload-0.1.11");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join(exe_rel()), b"fake-exe").unwrap();
        // 名字与戳不符
        fake_payload(&root.join("payload-0.1.12"), "0.1.10");

        let got = staged_newer(&root, "0.1.9").unwrap();
        assert_eq!(got.0, "0.1.10", "只有完整目录算数");
        assert_eq!(got.1, root.join("payload-0.1.10"));
        assert!(staged_newer(&root, "0.1.10").is_none(), "同版本不算更新");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 清理：只删比自己旧的完整载荷目录，且不碰基名无版本的安装目录 /
    /// 自己所在目录 / 更高的新载荷。版本全部相对 `current_version()` 推导：
    /// cleanup_older 比的是编译期 crate 版本，写死具体号必随版本上涨碎掉
    ///（0.1.12 起 0.1.11 已不算「更新的」）。
    #[test]
    fn cleanup_removes_only_older_payloads() {
        let root = temp_dir("cleanup");
        let cur = current_version();
        let older = "0.0.9"; // 任何现实版本之下
        let newer = format!("9{cur}"); // 首段 9 > 0，恒在当前版本之上
        std::fs::create_dir_all(root.join("payload")).unwrap();
        fake_payload(&root.join(format!("payload-{older}")), older);
        fake_payload(&root.join(format!("payload-{cur}")), cur);
        fake_payload(&root.join(&format!("payload-{newer}")), &newer);
        cleanup_older(root.clone(), root.join(format!("payload-{cur}")));
        assert!(
            !root.join(format!("payload-{older}")).exists(),
            "旧版本目录被清"
        );
        assert!(root.join(format!("payload-{cur}")).exists(), "自己不动");
        assert!(root.join(format!("payload-{newer}")).exists(), "更新的不动");
        assert!(root.join("payload").exists(), "安装目录（无版本）不动");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 选择：排除自己所在目录（否则「点击重启」会拉起自己）。
    #[test]
    fn pick_next_excludes_self() {
        let root = temp_dir("pick-next");
        let me = root.join("payload-0.1.10");
        let next = root.join("payload-0.1.11");
        fake_payload(&me, "0.1.10");
        fake_payload(&next, "0.1.11");
        assert_eq!(pick_next(&root, "0.1.10", &me).unwrap().1, next);
        assert!(
            pick_next(&root, "0.1.10", &next).is_none(),
            "自己就是最新的 → 没有可切的更高版本"
        );
        assert_eq!(
            pick_next(&root, "0.1.10", &root.join("payload")).unwrap().1,
            next,
            "安装目录（无版本名）不是任何人"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 老落位方案（0.1.6~0.1.10 整目录 swap 时代）的残留必须被启动清理带走：
    /// helper 日志 / staged 标记 / `payload.old-*`（升级过来时一份就是 300MB）。
    /// `payload.stage-<版本>` 是例外——那是完整载荷，走版本规则。
    #[test]
    fn cleanup_removes_legacy_swap_leftovers() {
        let root = temp_dir("legacy");
        let me = root.join("payload");
        fake_payload(&me, "0.1.10");
        let old = root.join("payload.old-20261014215500");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join(exe_rel()), b"x").unwrap();
        std::fs::write(root.join("update-swap.log"), b"log").unwrap();
        std::fs::write(root.join("update-staged.json"), b"{}").unwrap();
        let old_stage = root.join("payload.stage-0.1.8");
        fake_payload(&old_stage, "0.1.8");

        cleanup_older(root.clone(), me.clone());

        assert!(
            !old.exists(),
            "payload.old-* 应被清（升级过来的 300MB 孤儿）"
        );
        assert!(
            !root.join("update-swap.log").exists(),
            "老 helper 日志应被清"
        );
        assert!(!root.join("update-staged.json").exists(), "老标记应被清");
        assert!(!old_stage.exists(), "更旧的 staging 走版本规则清掉");
        assert!(me.exists(), "当前载荷不动");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// **清理绝不删活目录**（0.1.6~0.1.10 死循环的教训）：目录里有进程
    /// share-none 持有文件时，`rename` 探针必败 → 跳过本次；占用释放后
    /// 再跑一次即可删掉。若改用 `remove_dir_all` 硬删，就会把正在运行的
    /// 旧载荷从磁盘上掏空。
    #[cfg(windows)]
    #[test]
    fn cleanup_skips_live_payload_dir() {
        let root = temp_dir("cleanup-live");
        let live = root.join("payload-0.1.8");
        let me = root.join("payload-0.1.10");
        fake_payload(&live, "0.1.8");
        fake_payload(&me, "0.1.10");

        // 占用者：share-none 打开 live 内的文件，抓住后写哨兵文件（测试侧靠它
        // 同步，不靠碰运气的 sleep）
        let sentinel = root.join("holding.txt");
        let mut holder = hidden_powershell()
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "$f=[System.IO.File]::Open('{}','Open','Read','None'); \
                     Set-Content -LiteralPath '{}' -Value ok; \
                     Start-Sleep -Seconds 30",
                    live.join(exe_rel()).display(),
                    sentinel.display()
                ),
            ])
            .spawn()
            .unwrap();
        for _ in 0..50 {
            if sentinel.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(sentinel.exists(), "占用者未就位（powershell 启动失败？）");

        cleanup_older(root.clone(), me.clone());
        assert!(live.exists(), "活目录（文件被占用）绝不能删");
        assert!(me.exists(), "自己不动");

        let _ = holder.kill();
        let _ = holder.wait();
        std::thread::sleep(Duration::from_millis(300));
        cleanup_older(root.clone(), me.clone());
        assert!(!live.exists(), "占用释放后应能删掉");
        assert!(me.exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn hash_line_parse() {
        let text = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  pi-flash-0.1.2-win32-x64.zip\n\
                    011f68a3a5e3c1fda41e7f68a9a4f1654eeab5d5f4d19c8e13a7d411d1a25b35 *other.zip\n";
        assert_eq!(
            parse_hash_for(text, "pi-flash-0.1.2-win32-x64.zip").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert!(parse_hash_for(text, "missing.zip").is_none());
        assert!(parse_hash_for("garbage line\n", "x.zip").is_none());
    }

    #[test]
    fn zip_path_sanitize() {
        let dest = Path::new("D:\\pkg");
        assert!(safe_zip_path(dest, "pi-flash/vendor/pi/x.js").is_some());
        assert!(safe_zip_path(dest, "pi-flash\\vendor\\x.js").is_some());
        assert!(safe_zip_path(dest, "/abs/path").is_none(), "绝对路径拒绝");
        assert!(safe_zip_path(dest, "a/../../etc").is_none(), "上跳拒绝");
        assert!(safe_zip_path(dest, "C:\\evil").is_none(), "盘符拒绝");
        assert!(safe_zip_path(dest, "a/./b").is_some(), "点段折叠");
    }

    // --- 下载引擎功能测试（本地裸 TCP 假源，真下载真哈希真守卫） ------------

    /// 起一次性 HTTP 服务返回 base；trickle_ms = 每 1KB 间隔（模拟慢源）；
    /// with_length = false 时无 content-length（close 分隔）。
    fn serve(body: Vec<u8>, trickle_ms: Option<u64>, with_length: bool) -> String {
        use std::io::Write as _;
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((mut s, _)) = listener.accept() else {
                return;
            };
            let mut req = [0u8; 4096];
            let _ = std::io::Read::read(&mut s, &mut req);
            let len_head = if with_length {
                format!("Content-Length: {}\r\n", body.len())
            } else {
                String::new()
            };
            let _ = write!(s, "HTTP/1.1 200 OK\r\n{len_head}Connection: close\r\n\r\n");
            for chunk in body.chunks(1024) {
                if s.write_all(chunk).is_err() {
                    return; // 测试端测速中止断开属预期
                }
                if let Some(ms) = trickle_ms {
                    std::thread::sleep(Duration::from_millis(ms));
                }
            }
        });
        format!("http://{addr}")
    }

    /// 已关闭的端口（连接拒绝），当挂掉的源。
    fn dead_base() -> String {
        use std::net::TcpListener;
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        format!("http://{addr}")
    }

    /// 单源下载的落盘目标（每次开头清一次；**测试结束必须自己删**——
    /// 之前只在开头清，每次 `cargo test` 都在 %TEMP% 漏三份最多 256KB 的残渣）。
    fn test_dest(name: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("pf-upd-test-{name}-{}", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    /// 顺序跑三种场景（共用全局百分比原子量，避免并行互扰）：
    /// ① 测速守卫弃慢源；② 死源在前自动切换，哈希正确、百分比到 100；
    /// ③ 无 content-length 成功但无百分比。
    #[test]
    fn download_speed_guard_and_failover() {
        let _guard = PROGRESS_LOCK.lock().unwrap();
        use sha2::Digest as _;
        let probe = Duration::from_millis(300);
        let min_bps = 128.0 * 1024.0;
        let (d_slow, d_fo, d_nl) = (test_dest("slow"), test_dest("fo"), test_dest("nl"));

        // ① 20KB/s 慢源 → 快速中止，错误信息含"测速不过"
        let slow = serve(vec![0xAB; 64 * 1024], Some(50), true);
        let t0 = std::time::Instant::now();
        let r = download_and_hash_cfg(&format!("{slow}/v0.0.1/x.zip"), &d_slow, probe, min_bps);
        assert!(r.is_err(), "慢源应被测速守卫拒绝");
        assert!(r.unwrap_err().contains("测速不过"));
        assert!(
            t0.elapsed() < Duration::from_secs(5),
            "应秒级中止而非拖到整体超时"
        );

        // ② 死端口在前 → 切到可用源；命中源正确、SHA-256 正确、百分比 100
        let body = vec![0x5Au8; 256 * 1024];
        let good = serve(body.clone(), None, true);
        let bases = vec![dead_base(), good.clone()];
        let (hash, chosen) =
            download_mirrored_cfg("x.zip", &d_fo, "0.0.1", &bases, probe, min_bps).unwrap();
        assert_eq!(chosen, good, "应命中可用源");
        let mut h = sha2::Sha256::new();
        h.update(&body);
        assert_eq!(hash, format!("{:x}", h.finalize()), "边下边算哈希正确");
        assert_eq!(download_percent(), Some(100));

        // ③ 无 content-length：下载成功但百分比保持未知
        let stream = serve(b"no-length-body".to_vec(), None, false);
        let (_h3, chosen3) =
            download_mirrored_cfg("x.zip", &d_nl, "0.0.1", &[stream.clone()], probe, min_bps)
                .unwrap();
        assert_eq!(chosen3, stream);
        assert_eq!(download_percent(), None, "无 content-length 无百分比");

        for d in [&d_slow, &d_fo, &d_nl] {
            let _ = std::fs::remove_file(d);
        }
    }

    /// 真实网络端到端（发版前手动跑一次）：
    /// `cargo test -p app updater_real_download -- --ignored --nocapture`
    #[test]
    #[ignore = "真实网络下载整个载荷（约 85MB），手动验证用"]
    fn updater_real_download() {
        let rel = fetch_latest().unwrap_or_else(|e| panic!("fetch_latest 失败：{e}"));
        let asset = asset_name(&rel.version);
        let dir = std::env::temp_dir().join(format!("pf-upd-real-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join(&asset);
        let (hash, chosen) = download_mirrored(&asset, &dest, &rel.version)
            .unwrap_or_else(|e| panic!("下载失败：{e}"));
        eprintln!(
            "v{} 命中源：{chosen}\nsha256={hash}\n文件：{}",
            rel.version,
            dest.display()
        );
        assert_eq!(download_percent(), Some(100));
        assert!(dest.metadata().unwrap().len() > 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- 更新管线端到端（假源 + 真 zip，无 GUI 无 powershell） ----------------
    // 进度全局量（DOWNLOAD_PCT/EXTRACT_PCT/UPDATE_PHASE）跨测试共享：凡断言
    // 或触发它们的用例一律持锁，防 cargo 并行线程互扰。

    static PROGRESS_LOCK: Mutex<()> = Mutex::new(());

    /// 多请求静态文件站（serve() 只接一条连接；管线要 zip、sidecar、SUMS 多
    /// 次请求）。URL 路径直接映射 root 下相对文件，缺失 404。
    fn serve_dir(root: &Path) -> String {
        use std::io::{Read as _, Write as _};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let root = root.to_path_buf();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { return };
                let mut req = [0u8; 4096];
                if s.read(&mut req).is_err() {
                    continue;
                }
                let req = String::from_utf8_lossy(&req);
                let Some(path) = req.split_whitespace().nth(1) else {
                    continue;
                };
                match std::fs::read(root.join(path.trim_start_matches('/'))) {
                    Ok(data) => {
                        let head = format!(
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            data.len()
                        );
                        let _ = s.write_all(head.as_bytes());
                        let _ = s.write_all(&data);
                    }
                    Err(_) => {
                        let _ = s.write_all(
                            b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found",
                        );
                    }
                }
            }
        });
        format!("http://{addr}")
    }

    /// 真 zip 夹具（复刻 release.sh：Compress-Archive 包装目录 pi-flash/），
    /// 附 sidecar 与 SHA256SUMS，落 site/v<ver>/。
    fn build_release_fixture(site: &Path, ver: &str, exe_bytes: &[u8], marker: &str) {
        let fix_root = site.join(format!("fix-{ver}"));
        let pkg = fix_root.join("pi-flash");
        std::fs::create_dir_all(&pkg).unwrap();
        std::fs::write(pkg.join(exe_rel()), exe_bytes).unwrap();
        std::fs::write(pkg.join("marker.txt"), marker).unwrap();

        let ver_dir = site.join(format!("v{ver}"));
        std::fs::create_dir_all(&ver_dir).unwrap();
        let zip = ver_dir.join(format!("pi-flash-{ver}-win32-x64.zip"));
        let ps = format!(
            "Compress-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
            pkg.display(),
            zip.display()
        );
        let out = hidden_powershell()
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "夹具压缩失败：{}",
            String::from_utf8_lossy(&out.stderr)
        );
        use sha2::Digest as _;
        let mut h = sha2::Sha256::new();
        h.update(std::fs::read(&zip).unwrap());
        let hash = format!("{:x}", h.finalize());
        let name = zip.file_name().unwrap().to_string_lossy().to_string();
        std::fs::write(
            ver_dir.join(format!("{name}.sha256")),
            format!("{hash}  {name}\n"),
        )
        .unwrap();
        std::fs::write(ver_dir.join("SHA256SUMS"), format!("{hash}  {name}\n")).unwrap();
    }

    /// 用户实录链路（无 GUI 主防线）：假源 latest → stage 落位新载荷目录
    /// → 目录/戳/内容齐全 → `staged_newer` 认账（启动交接与 Ready 态的数据源）
    /// → 清理旧版本目录时不动它。
    #[cfg(windows)]
    #[test]
    fn update_pipeline_end_to_end() {
        let _guard = PROGRESS_LOCK.lock().unwrap();
        let root = temp_dir("pipe-e2e");
        let me = root.join("payload"); // 当前载荷目录（模拟 npm 薄壳布局）
        fake_payload(&me, "0.1.8");

        let ver = "9.9.9";
        let site = root.join("site");
        build_release_fixture(&site, ver, b"new", "payload-v9.9.9\n");
        let base = serve_dir(&site);
        // SAFETY：活跃测试里只有本测试与 bad_hash 用例读 PI_FLASH_MIRROR，
        // 其余下载测试走 _cfg 显式注入，无并行读者
        unsafe { std::env::set_var("PI_FLASH_MIRROR", &base) };

        let rel = ReleaseInfo {
            version: ver.to_string(),
            body: String::new(),
        };
        let staged = stage_update_in(&me, &rel).unwrap();
        assert_eq!(staged, ver);

        let target = root.join(format!("payload-{ver}"));
        assert_eq!(
            std::fs::read_to_string(target.join(".payload-version")).unwrap(),
            format!("{ver}\n")
        );
        assert_eq!(
            std::fs::read(target.join("marker.txt")).unwrap(),
            b"payload-v9.9.9\n",
            "载荷内容为新夹具"
        );
        assert_eq!(
            staged_newer(&root, "0.1.8").map(|(v, _)| v),
            Some(ver.to_string()),
            "启动交接认账"
        );
        // 旧版本清理不碰新载荷；旧载荷目录（无版本名）也不动
        cleanup_older(root.clone(), me.clone());
        assert!(target.exists(), "新载荷目录不能被清理碰掉");
        assert!(me.exists(), "安装目录不动");
        let tmp_left: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("update.tmp-"))
            .collect();
        assert!(tmp_left.is_empty(), "半截下载目录已清：{tmp_left:?}");

        unsafe { std::env::remove_var("PI_FLASH_MIRROR") };
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 哈希不符的失败路径：stage 报 SHA-256 错；旧载荷原样，无新目录/无 tmp 残留。
    #[cfg(windows)]
    #[test]
    fn stage_update_bad_hash_leaves_no_trace() {
        let _guard = PROGRESS_LOCK.lock().unwrap();
        let root = temp_dir("pipe-bad");
        let me = root.join("payload");
        fake_payload(&me, "0.1.8");

        let ver = "9.9.9";
        let site = root.join("site");
        build_release_fixture(&site, ver, b"new", "payload-v9.9.9\n");
        // 篡改 sidecar 与 SUMS 为错哈希（64 位十六进制保证能被解析后比对）
        let bad = format!("{}\n", "0".repeat(64));
        std::fs::write(
            site.join(format!("v{ver}/pi-flash-{ver}-win32-x64.zip.sha256")),
            &bad,
        )
        .unwrap();
        std::fs::write(site.join(format!("v{ver}/SHA256SUMS")), &bad).unwrap();
        let base = serve_dir(&site);
        // SAFETY：见 update_pipeline_end_to_end
        unsafe { std::env::set_var("PI_FLASH_MIRROR", &base) };

        let rel = ReleaseInfo {
            version: ver.to_string(),
            body: String::new(),
        };
        let err = stage_update_in(&me, &rel).unwrap_err();
        assert!(err.contains("SHA-256"), "应报校验不符：{err}");

        assert_eq!(
            std::fs::read_to_string(me.join(".payload-version")).unwrap(),
            "0.1.8\n",
            "旧载荷原样"
        );
        assert!(
            !root.join(format!("payload-{ver}")).exists(),
            "无新载荷目录"
        );
        let residue: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("update.tmp-"))
            .collect();
        assert!(residue.is_empty(), "tmp 已清：{residue:?}");

        unsafe { std::env::remove_var("PI_FLASH_MIRROR") };
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 半截残留目录（上次解压中断：有目录、无戳）必须落选，且下次落位要把它
    /// 换成完整的（而不是因目录已存在而失败）。
    #[cfg(windows)]
    #[test]
    fn stage_replaces_stale_incomplete_target() {
        let _guard = PROGRESS_LOCK.lock().unwrap();
        let root = temp_dir("stage-stale");
        let me = root.join("payload");
        fake_payload(&me, "0.1.9");

        let ver = "9.9.9";
        let stale = root.join(format!("payload-{ver}"));
        std::fs::create_dir_all(&stale).unwrap();
        std::fs::write(stale.join(exe_rel()), b"half-extracted").unwrap();
        std::fs::write(stale.join("junk.txt"), b"leftover").unwrap();
        assert!(
            staged_newer(&root, "0.1.9").is_none(),
            "无版本戳 = 半截 → 落选（绝不能拿来交接）"
        );

        let site = root.join("site");
        build_release_fixture(&site, ver, b"new", "payload-v9.9.9\n");
        let base = serve_dir(&site);
        // SAFETY：见 update_pipeline_end_to_end
        unsafe { std::env::set_var("PI_FLASH_MIRROR", &base) };

        let rel = ReleaseInfo {
            version: ver.to_string(),
            body: String::new(),
        };
        stage_update_in(&me, &rel).unwrap();

        assert_eq!(
            std::fs::read_to_string(stale.join(".payload-version")).unwrap(),
            format!("{ver}\n")
        );
        assert_eq!(
            std::fs::read(stale.join("marker.txt")).unwrap(),
            b"payload-v9.9.9\n"
        );
        assert!(!stale.join("junk.txt").exists(), "旧半截内容被整目录替换掉");
        assert_eq!(
            staged_newer(&root, "0.1.9").map(|(v, _)| v),
            Some(ver.to_string())
        );

        unsafe { std::env::remove_var("PI_FLASH_MIRROR") };
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 拒绝落位到当前运行目录（防止自覆盖）。
    #[test]
    fn stage_refuses_running_dir() {
        let root = temp_dir("self-target");
        let me = root.join("payload-0.1.10");
        fake_payload(&me, "0.1.10");
        let rel = ReleaseInfo {
            version: "0.1.10".to_string(),
            body: String::new(),
        };
        let err = stage_update_in(&me, &rel).unwrap_err();
        assert!(err.contains("当前运行目录"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    /// 并行解压树完整性：Compress-Archive 造真 zip（嵌套目录 + PS 5.1 的
    /// 反斜杠条目名——safe_zip_path 归一化正为此），并行解压后逐文件比对
    /// 内容；进度终值必须到 100（pif-ui snapshot progress 的数据源）。
    #[cfg(windows)]
    #[test]
    fn extract_zip_parallel_tree_intact() {
        let _guard = PROGRESS_LOCK.lock().unwrap();
        let dir = temp_dir("extract-par");
        let src = dir.join("pi-flash");
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        for i in 0..120 {
            let rel = if i % 3 == 0 {
                format!("vendor/pi/deep/dir{i}/module_{i}.js")
            } else if i % 3 == 1 {
                format!("vendor/node_modules/pkg{i}/lib/index.js")
            } else {
                format!("assets/i18n/msg_{i}.json")
            };
            let body = format!("payload {i}: {}", "x".repeat(i * 37 % 900));
            let p = src.join(&rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, &body).unwrap();
            files.push((rel, body.into_bytes()));
        }
        let zip_path = dir.join("t.zip");
        let ps = format!(
            "Compress-Archive -Path '{}' -DestinationPath '{}' -Force",
            src.display(),
            zip_path.display()
        );
        let out = hidden_powershell()
            .args(["-NoProfile", "-Command", &ps])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "Compress-Archive 失败：{}",
            String::from_utf8_lossy(&out.stderr)
        );

        let dest = dir.join("out");
        extract_zip(&zip_path, &dest).unwrap();
        // Compress-Archive -Path <目录> 连包装目录一起进 zip：条目名带
        // pi-flash/ 前缀（与 release.sh 产物同形，locate_content_root 消费）
        for (rel, body) in &files {
            let got = std::fs::read(dest.join("pi-flash").join(rel.replace('/', "\\")))
                .unwrap_or_else(|e| panic!("{rel}: {e}"));
            assert_eq!(&got, body, "{rel} 内容不符");
        }
        assert_eq!(extract_percent(), Some(100), "解压进度终值应为 100");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// 真机计时（`cargo test -p app --bin pi-flash extract_real_dist -- --ignored
    /// --nocapture`）：对 dist/ 里最新真 zip 跑并行解压，打印耗时与条目数。
    /// 基线对照 = PowerShell Expand-Archive（单线程）。dist/ 不存在时跳过。
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn extract_real_dist_zip_timing() {
        // cargo test 的 cwd 是 crate 根；CARGO_MANIFEST_DIR = <ws>/crates/app，
        // 剥两级到工作区根（同 vendor.rs base_candidates 惯例）
        let dist = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("dist");
        let mut best: Option<(PathBuf, std::time::Duration)> = None;
        if let Ok(entries) = std::fs::read_dir(&dist) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("pi-flash-") && name.ends_with("-win32-x64.zip") {
                    let score = e
                        .metadata()
                        .and_then(|m| m.modified())
                        .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default())
                        .unwrap_or_default();
                    if best.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                        best = Some((e.path(), score));
                    }
                }
            }
        }
        let Some((zip_path, _)) = best else {
            eprintln!("dist/ 无 zip，跳过");
            return;
        };
        let dest = std::env::temp_dir().join(format!("pf-extract-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        let t0 = std::time::Instant::now();
        extract_zip(&zip_path, &dest).unwrap();
        let dt = t0.elapsed();
        let count = count_files(&dest);
        eprintln!(
            "并行解压 {:?}：{} 个文件，{:?}（{:?}/s）",
            zip_path.file_name().unwrap(),
            count,
            dt,
            (count as f64 / dt.as_secs_f64()) as u64
        );
        let _ = std::fs::remove_dir_all(&dest);
    }

    fn count_files(root: &Path) -> usize {
        let mut n = 0;
        if let Ok(entries) = std::fs::read_dir(root) {
            for e in entries.flatten() {
                if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    n += count_files(&e.path());
                } else {
                    n += 1;
                }
            }
        }
        n
    }
}
