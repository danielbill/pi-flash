//! Pi 扩展更新检测（082-扩展更新.md）：Rust 移植 pi-web `lib/plugin-updates.ts`。
//!
//! 为什么不用 pi 的 SDK `checkForAvailableUpdates()`：那是进程内 API，PF 是
//! RPC 消费方吃不到（RPC 协议无此方法，`pi update` CLI 也没有 --check 旗标）。
//! 检测本质 = 「读包清单 + spawn 探测命令」，照 pi-web 逻辑纯 Rust 实现：
//! - npm：实装版本（`<install>/package.json`）vs `npm view <spec> version`
//!   最新版 semver 对比（来源带 `^` 范围时取 maxSatisfying）；
//! - git：本地 HEAD vs `git ls-remote origin <upstream>`。
//! 检测零写入、不碰 node_modules。
//!
//! Windows 陷阱（pi-web node-cli 同款）：PATH 上的 `npm` 实为 `npm.cmd`，
//! 直接 spawn 会被拒（CVE-2024-27980）——定位 `<node 目录>/node_modules/npm/
//! bin/npm-cli.js` 用 PF 自带 node 直跑，找不到退裸 `npm`。
//!
//! 执行侧不在这：更新 = `cli.js update --extensions [--extension <src>]`，
//! 走 settings/plugins.rs 的 `mc_cli_op` 管道（与安装/卸载同路，op 泵收尾）。
//!
//! v1 边界：只查全局 settings.json `packages`（与扩展页 global only 对齐）；
//! npm 钉精确版本 / git 钉 ref / 本地路径 → unsupported；`PI_OFFLINE=1` 全
//! 部 error；pnpm 等非标准布局读不到实装版本按 error 容错。

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use crate::Chat;
use crate::i18n::tr;

// ---------------------------------------------------------------------------
// 状态与结果类型
// ---------------------------------------------------------------------------

/// 单包检测状态（pi-web PluginUpdateState 同名四态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PkgUpdateState {
    /// 有新版
    UpdateAvailable,
    /// 已是最新
    UpToDate,
    /// 钉死版本/ref 或本地路径，无法自动检测
    Unsupported,
    /// 检测失败（未安装/网络/命令错误）
    Error,
}

/// 检测状态（`aux.pi_extension_updates`）：checking=true 时 results 是上一轮旧值。
#[derive(Debug, Clone, Default)]
pub(crate) struct UpdatesState {
    pub(crate) results: Vec<PkgUpdateResult>,
    pub(crate) checking: bool,
}

impl UpdatesState {
    /// 可更新包数（新会话页通知与「更新全部(n)」的 n）。
    pub(crate) fn available(&self) -> usize {
        self.results
            .iter()
            .filter(|r| r.state == PkgUpdateState::UpdateAvailable)
            .count()
    }

    /// 单包当前检测状态（设置页行内箭头 / 详情按钮提示用）。
    pub(crate) fn state_of(&self, source: &str) -> Option<PkgUpdateState> {
        self.results
            .iter()
            .find(|r| r.source == source)
            .map(|r| r.state)
    }
}

/// 单包结果（pi-web PluginUpdateResult 对应）。
#[derive(Debug, Clone)]
pub(crate) struct PkgUpdateResult {
    pub(crate) source: String,
    pub(crate) display_name: String,
    pub(crate) state: PkgUpdateState,
}

fn mk(source: &str, state: PkgUpdateState) -> PkgUpdateResult {
    PkgUpdateResult {
        source: source.to_string(),
        display_name: pi_link::skills::display_source(source).to_string(),
        state,
    }
}

// ---------------------------------------------------------------------------
// 检测主体（同步，跑后台线程）
// ---------------------------------------------------------------------------

const CMD_TIMEOUT: Duration = Duration::from_secs(10);

/// 检测全局 settings.json `packages` 全部条目，结果顺序与清单一致。
pub(crate) fn check_all() -> Vec<PkgUpdateResult> {
    let sources: Vec<String> = pi_link::config::read_packages(&pi_link::config::settings_path())
        .unwrap_or_default()
        .iter()
        .map(|e| pi_link::skills::entry_source(e))
        .collect();
    if sources.is_empty() {
        return Vec::new();
    }
    if offline() {
        return sources
            .iter()
            .map(|s| mk(s, PkgUpdateState::Error))
            .collect();
    }
    let agent_dir = pi_link::config::agent_dir();
    std::thread::scope(|scope| {
        let handles: Vec<_> = sources
            .iter()
            .map(|s| scope.spawn(|| check_one(&agent_dir, s)))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    })
}

fn check_one(agent_dir: &Path, source: &str) -> PkgUpdateResult {
    if let Some(npm) = parse_npm_source(source) {
        // 钉死精确版本 → 无法自动检测（pi-web isPluginSourceCheckable 同判）
        if npm
            .version
            .as_deref()
            .is_some_and(|v| semver::Version::parse(v).is_ok())
        {
            return mk(source, PkgUpdateState::Unsupported);
        }
        return check_npm(agent_dir, source, npm);
    }
    if is_git_source(source) {
        if has_git_ref(source) {
            return mk(source, PkgUpdateState::Unsupported);
        }
        return check_git(source);
    }
    // 本地路径等：原地目录，没有「更新」概念
    mk(source, PkgUpdateState::Unsupported)
}

fn check_npm(agent_dir: &Path, source: &str, npm: NpmSource) -> PkgUpdateResult {
    let installed = pi_link::skills::package_install_dir_in(agent_dir, source);
    let current = match read_installed_version(&installed) {
        Ok(v) => v,
        Err(_) => return mk(source, PkgUpdateState::Error),
    };
    // npm view 以主目录为 cwd（用户 .npmrc 的 registry 生效；与项目无关）
    let mut cmd = match npm_invocation(&["view", &npm.spec, "version", "--json"]) {
        Ok(c) => c,
        Err(_) => return mk(source, PkgUpdateState::Error),
    };
    cmd.current_dir(home_dir());
    let out = match run_timeout(cmd, CMD_TIMEOUT) {
        Ok(o) => o,
        Err(_) => return mk(source, PkgUpdateState::Error),
    };
    let range = npm
        .version
        .as_deref()
        .and_then(|v| semver::VersionReq::parse(v).ok());
    match latest_version(&out, range.as_ref()) {
        Ok(latest) => {
            let state = if latest > current {
                PkgUpdateState::UpdateAvailable
            } else {
                PkgUpdateState::UpToDate
            };
            mk(source, state)
        }
        Err(_) => mk(source, PkgUpdateState::Error),
    }
}

fn check_git(source: &str) -> PkgUpdateResult {
    let dir = pi_link::skills::package_install_dir(source);
    if !dir.is_dir() {
        return mk(source, PkgUpdateState::Error);
    }
    let git = |args: &[&str]| {
        let mut cmd = Command::new("git");
        cmd.args(args)
            .current_dir(&dir)
            .env("GIT_TERMINAL_PROMPT", "0");
        run_timeout(cmd, CMD_TIMEOUT)
    };
    // 本地 HEAD vs 上游：先解析 @{upstream}，解析不到退 HEAD（pi-web 同款）
    let local = match git(&["rev-parse", "HEAD"]) {
        Ok(o) => o.trim().to_string(),
        Err(_) => return mk(source, PkgUpdateState::Error),
    };
    let upstream = git(&["rev-parse", "--abbrev-ref", "@{upstream}"])
        .map(|o| o.trim().to_string())
        .unwrap_or_default();
    let head = match upstream.strip_prefix("origin/") {
        Some(branch) => format!("refs/heads/{branch}"),
        None => "HEAD".to_string(),
    };
    let remote = match git(&["ls-remote", "origin", &head]) {
        Ok(o) => o,
        Err(_) => return mk(source, PkgUpdateState::Error),
    };
    match first_commit_hash(&remote) {
        Some(hash) => {
            let state = if hash.eq_ignore_ascii_case(&local) {
                PkgUpdateState::UpToDate
            } else {
                PkgUpdateState::UpdateAvailable
            };
            mk(source, state)
        }
        None => mk(source, PkgUpdateState::Error),
    }
}

fn read_installed_version(dir: &Path) -> Result<semver::Version, String> {
    let text = std::fs::read_to_string(dir.join("package.json"))
        .map_err(|_| "not installed".to_string())?;
    let v: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("invalid package.json: {e}"))?;
    let raw = v
        .get("version")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "installed package has no version".to_string())?;
    semver::Version::parse(raw).map_err(|_| format!("invalid installed version {raw:?}"))
}

// ---------------------------------------------------------------------------
// 来源串解析（pi-web npm-source.ts / plugin-updates.ts 手写版，不用正则 crate）
// ---------------------------------------------------------------------------

struct NpmSource {
    spec: String,
    version: Option<String>,
}

/// `npm:@scope/pkg@1.2.3` → spec=`@scope/pkg@1.2.3`，version=`1.2.3`。
/// pi-web parseNpmSource 正则 `^(@?[^@]+(?:\/[^@]+)?)(?:@(.+))?$` 的手写版。
fn parse_npm_source(source: &str) -> Option<NpmSource> {
    let spec = source.strip_prefix("npm:")?.trim();
    let name_end = match spec.strip_prefix('@') {
        // scoped：跳过 `@scope/`，版本 @ 从包名段之后找
        Some(rest) => match rest.find('/') {
            Some(i) => i + 2,
            None => spec.len(),
        },
        None => 0,
    };
    let (name, version) = match spec[name_end.min(spec.len())..].find('@') {
        Some(i) => (
            &spec[..name_end + i],
            Some(spec[name_end + i + 1..].to_string()),
        ),
        None => (spec, None),
    };
    (!name.is_empty()).then(|| NpmSource {
        spec: spec.to_string(),
        version,
    })
}

/// git 来源：`git:` 前缀或 https?/ssh/git:// URL。
fn is_git_source(source: &str) -> bool {
    if source.starts_with("git:") {
        return true;
    }
    let lower = source.to_ascii_lowercase();
    ["https://", "http://", "ssh://", "git://"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// 来源串是否钉了 ref/commit（钉死 → 不检测；pi-web hasGitRef 手写版）。
fn has_git_ref(source: &str) -> bool {
    let value = source.strip_prefix("git:").unwrap_or(source).trim();
    if let Some(rest) = value.strip_prefix("git@") {
        // scp 形式 git@host:path：path 段含 '@' 即钉 ref
        if let Some(i) = rest.find(':') {
            return rest[i + 1..].contains('@');
        }
    }
    if let Some(i) = value.find("://") {
        let host_end = i + 3;
        return match value[host_end..].find('/') {
            Some(s) => value[host_end + s + 1..].contains('@'),
            None => false,
        };
    }
    // 短形式 host/path
    match value.find('/') {
        Some(i) => value[i + 1..].contains('@'),
        None => false,
    }
}

// ---------------------------------------------------------------------------
// 子进程与解析辅助
// ---------------------------------------------------------------------------

/// npm 调用（pi-web node-cli 同款）：定位 `<node 目录>/node_modules/npm/bin/
/// npm-cli.js` 用 PF 自带 node 直跑。PATH 上的 `npm` 在 Windows 实为
/// `npm.cmd`，直接 spawn 会被拒（CVE-2024-27980，实测报 `program not
/// found`）；全兜底落空退裸 `npm`（仅 Unix 可用）。
fn npm_invocation(args: &[&str]) -> Result<Command, String> {
    let node = resolve_node_path();
    let npm_cli = node
        .as_ref()
        .and_then(|n| n.parent())
        .map(|d| d.join("node_modules/npm/bin/npm-cli.js"))
        .filter(|p| p.is_file());
    let mut cmd = match (node, npm_cli) {
        (Some(node), Some(script)) => {
            let mut c = Command::new(&node);
            c.arg(script);
            c
        }
        _ => Command::new("npm"),
    };
    cmd.args(args);
    Ok(cmd)
}

/// node 可执行绝对路径：vendored 规则给的已是路径直接用；裸 `"node"`
/// （PATH 兑底）时扫 PATH 找 node.exe——否则定位不了同目录的 npm-cli.js。
fn resolve_node_path() -> Option<PathBuf> {
    let node = pi_link::vendor::node_bin();
    let p = Path::new(&node);
    if p.is_absolute() {
        return Some(p.to_path_buf());
    }
    #[cfg(windows)]
    let candidates = ["node.exe"];
    #[cfg(not(windows))]
    let candidates = ["node"];
    let path_var = std::env::var("PATH").ok()?;
    for dir in std::env::split_paths(&path_var) {
        for name in candidates {
            let cand = dir.join(name);
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// 带超时的子进程执行：输出小（版本串/hash），管道不会写满，轮询 try_wait
/// 到退出或超时强杀。Windows 下不弹控制台窗（CREATE_NO_WINDOW）。
fn run_timeout(mut cmd: Command, timeout: Duration) -> Result<String, String> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("timed out".to_string());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e.to_string()),
        }
    };
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(if err.trim().is_empty() {
            format!("exited with {status}")
        } else {
            err.into_owned()
        })
    }
}

/// `npm view … version --json` 输出 → 最新版：字符串直解；数组按范围取
/// maxSatisfying（无范围取最高，pi-web readLatestVersion 同款）。
fn latest_version(
    out: &str,
    range: Option<&semver::VersionReq>,
) -> Result<semver::Version, String> {
    let parsed: serde_json::Value = serde_json::from_str(out.trim())
        .map_err(|e| format!("unexpected npm view response: {e}"))?;
    if let Some(v) = parsed.as_str() {
        return semver::Version::parse(v).map_err(|_| format!("invalid version {v:?}"));
    }
    if let Some(arr) = parsed.as_array() {
        let versions: Vec<semver::Version> = arr
            .iter()
            .filter_map(|x| x.as_str())
            .filter_map(|s| semver::Version::parse(s).ok())
            .collect();
        let best = match range {
            Some(req) => versions.iter().filter(|v| req.matches(v)).max().cloned(),
            None => versions.into_iter().max(),
        };
        return best.ok_or_else(|| "no matching version".to_string());
    }
    Err("unexpected npm view response".into())
}

/// ls-remote 输出首个 `<40..64 位 hex>\s` 行的 hash（pi-web 同款判定）。
fn first_commit_hash(out: &str) -> Option<&str> {
    out.lines().map(str::trim).find_map(|line| {
        let hash = line.split_whitespace().next()?;
        (hash.len() >= 40 && hash.len() <= 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then_some(hash)
    })
}

fn offline() -> bool {
    std::env::var("PI_OFFLINE")
        .map(|v| {
            v.eq_ignore_ascii_case("1")
                || v.eq_ignore_ascii_case("true")
                || v.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

fn home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

// ---------------------------------------------------------------------------
// Chat 接线
// ---------------------------------------------------------------------------

impl Chat {
    /// 揭幕后延迟加载（startup.rs 钩子）：后台检测全部扩展更新，结果进
    /// `aux.pi_extension_updates`。只读 + 网络，零写入，不阻塞揭幕；op 泵收尾
    ///（装/卸/更后）也会再调一次刷新。
    pub(crate) fn spawn_pi_extension_updates_check(&mut self, cx: &mut gpui::Context<Self>) {
        if self.aux.pi_extension_updates.checking {
            return;
        }
        self.aux.pi_extension_updates.checking = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move { check_all() })
                .await;
            let _ = this.update(cx, |c, cx| {
                c.aux.pi_extension_updates.results = results;
                c.aux.pi_extension_updates.checking = false;
                cx.notify();
            });
        })
        .detach();
    }

    /// 更新单个扩展（详情 panel 按钮）。注意 `--extension` 不能与
    /// `--extensions` 同用（pi 解析期互斥，单给 `--extension` 即扩展目标）。
    pub(crate) fn mc_update_package(&mut self, source: String, cx: &mut gpui::Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        self.mc_clear_error(cx);
        self.cfg.pkg_op = Some(crate::settings::pi_extensions::PkgOp::Update(Some(
            source.clone(),
        )));
        self.mc_cli_op(
            vec!["update".into(), "--extension".into(), source],
            tr("extensions.updated_one").to_string(),
            cx,
        );
    }

    /// 更新全部扩展（新会话页通知 + 扩展页列表 title 按钮）。
    pub(crate) fn mc_update_all_packages(&mut self, cx: &mut gpui::Context<Self>) {
        if self.cfg.pkg_op.is_some() {
            return;
        }
        self.mc_clear_error(cx);
        self.cfg.pkg_op = Some(crate::settings::pi_extensions::PkgOp::Update(None));
        self.mc_cli_op(
            vec!["update".into(), "--extensions".into()],
            tr("extensions.updated_all").to_string(),
            cx,
        );
    }
}
