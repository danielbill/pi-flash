//! Pi 扩展更新检测（082-扩展更新.md）：Rust 移植 pi-web `lib/plugin-updates.ts`。
//!
//! 为什么不用 pi 的 SDK `checkForAvailableUpdates()`：那是进程内 API，PF 是
//! RPC 消费方吃不到（RPC 协议无此方法，`pi update` CLI 也没有 --check 旗标）。
//! 检测本质 = 「读包清单 + 探最新版」，照 pi-web 逻辑纯 Rust 实现：
//! - npm：实装版本（`<install>/package.json`）vs **registry `dist-tags.latest`**
//!   （来源带 `^` 范围时取 maxSatisfying）；
//! - git：本地 HEAD vs `git ls-remote origin <upstream>`。
//! 检测零写入、不碰 node_modules。
//!
//! 为什么不用 `npm view` 子进程（v1 已删，见 082 文档「检测 v2」）：npm 最新版
//! 的取法只需要一个 HTTP GET，而 spawn `npm` 要 node 目录里带着
//! `node_modules/npm`——release 布局（`dist/pi-flash/`）只带 `node.exe`，于是
//! 整条链在发布版静默失败（每包 error → n=0 → 新会话页按安静原则不渲染），
//! 表现为「调试有提示、发布版没有」。改 `ureq` 直连 registry：零额外产物、
//! 无 Windows spawn 陷阱。注册源可用 `PI_FLASH_NPM_REGISTRY` 覆盖（私有镜像）；
//! `.npmrc` 的 `registry=`/鉴权不解析，私有源包落 error（需 UI 显性展示）。
//!
//! 执行侧不在这：更新 = `cli.js update --extensions [--extension <src>]`，
//! 走 settings/pi_extensions.rs 的 `mc_cli_op` 管道（与安装/卸载同路，op 泵收尾）。
//!
//! v1 边界：只查全局 settings.json `packages`（与扩展页 global only 对齐）；
//! npm 钉精确版本 / git 钉 ref / 本地路径 → unsupported；`PI_OFFLINE=1` 全
//! 部 error；读不到实装版本按 error 容错。

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::Chat;
use crate::i18n::tr;

mod npm;

pub(crate) use npm::*;

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
    /// 仅 error 态填：失败原因。**不进 UI**（用户不需要看检测失败），
    /// 只供开发构建的 trace 打印定位（release 静默）。
    pub(crate) detail: Option<String>,
}

fn mk(source: &str, state: PkgUpdateState) -> PkgUpdateResult {
    PkgUpdateResult {
        source: source.to_string(),
        display_name: pi_link::skills::display_source(source).to_string(),
        state,
        detail: None,
    }
}

/// error 态带原因（只用于开发构建 trace）。
fn mk_err(source: &str, detail: impl ToString) -> PkgUpdateResult {
    PkgUpdateResult {
        detail: Some(detail.to_string()),
        ..mk(source, PkgUpdateState::Error)
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
    let results: Vec<PkgUpdateResult> = std::thread::scope(|scope| {
        let handles: Vec<_> = sources
            .iter()
            .map(|s| scope.spawn(|| check_one(&agent_dir, s)))
            .collect();
        handles.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    // 失败不进 UI（082 定稿：升级检测失败对用户不重要），但开发构建留 trace，
    // 免得再出现「release 静默失效、事后只能读源码」那种排查成本。
    if cfg!(debug_assertions) {
        for r in results.iter().filter(|r| r.state == PkgUpdateState::Error) {
            eprintln!(
                "[ext-updates] error {}: {}",
                r.source,
                r.detail.as_deref().unwrap_or("unknown")
            );
        }
    }
    results
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
        Err(e) => return mk_err(source, e),
    };
    let doc = match registry_doc(&npm.name, REGISTRY_TIMEOUT) {
        Ok(d) => d,
        Err(e) => return mk_err(source, e),
    };
    match latest_from_registry(&doc, npm.version.as_deref()) {
        Ok(latest) => {
            let state = if latest > current {
                PkgUpdateState::UpdateAvailable
            } else {
                PkgUpdateState::UpToDate
            };
            mk(source, state)
        }
        Err(e) => mk_err(source, e),
    }
}

fn check_git(source: &str) -> PkgUpdateResult {
    let dir = pi_link::skills::package_install_dir(source);
    if !dir.is_dir() {
        return mk(source, PkgUpdateState::Error);
    }
    let git = |args: &[&str]| {
        // 基座走 git.rs::git_command（CREATE_NO_WINDOW + GIT_TERMINAL_PROMPT=0）
        let mut cmd = crate::services::git::git_command();
        cmd.args(args).current_dir(&dir);
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

// ---------------------------------------------------------------------------
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
// 子进程辅助（git 探测）
// ---------------------------------------------------------------------------

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

#[cfg(test)]
mod tests;
