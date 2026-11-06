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
//!
//! itlq.9 超规拆分后本文件只剩常量/状态机/Chat 接线：release 抓取与
//! last_seen = `fetch`，下载引擎与进度 = `download`，落位管线 = `stage`，
//! 载荷布局与交接 = `payload`，稳定入口自愈 = `stable_entry`，版本比较 =
//! `version`；测试外移 `tests`。

mod download;
mod fetch;
mod payload;
mod stable_entry;
mod stage;
mod version;

pub(crate) use download::*;
pub(crate) use fetch::*;
pub(crate) use payload::*;
pub(crate) use stable_entry::*;
pub(crate) use stage::*;
pub(crate) use version::*;

#[cfg(test)]
mod tests;

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
        // 90-9 §6：重启换版 = 一次退出，插件先说再见（unload 落 plugin.log；
        // 本路径拿不到 window，窗口状态由新版启动页承接，persist_ui 照旧）
        self.plugin_unload_all_for_quit();
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
