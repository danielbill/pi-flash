//! PF 插件清单——启动扫描（02 §6）：台账白名单装载、失效包（090-10）收集、
//! 两档布局候选（平铺 dev / market 版本层）、清单头 peek、认领前缀全集。

use std::path::{Path, PathBuf};

use super::{PluginManifest, SUPPORTED_MANIFEST_SCHEMA, parse_manifest};

/// 扫描 `<数据目录>/plugins/*/` 只出清单（后台线程可调——PluginEntry 持
/// 非 Send 的 JsPlugin，条目装配必须在主线程做）；坏清单 eprintln 跳过。
///
/// 扫描结果：可装载清单 + 台账里登记却装不上的「失效包」（090-10 迁移体验）。
pub(crate) struct ScanOutcome {
    pub manifests: Vec<PluginManifest>,
    pub stale: Vec<StalePlugin>,
}

/// 台账启用、磁盘有清单、但本版 PF 拒载的包。用户可见信号的唯一数据源
/// （启动 toast + 市场头部通知行 + 已安装页标签）；此前只进 stderr，release
/// 下用户看到的就是插件凭空消失。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct StalePlugin {
    /// 台账包名（`@scope/name`）——市场 / 已安装页按它对账
    pub name: String,
    /// 清单 version（peek 行扫；缺则取版本目录名）
    pub version: String,
    /// 清单 title.zh（peek；缺则回落短包名）
    pub title_zh: String,
    pub kind: StaleKind,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum StaleKind {
    /// 清单 schema 不在本版支持集 → 「需更新插件」（本轮 = 旧 schema 2 包）
    SchemaUnsupported { got: u32 },
    /// 其他清单错误 → 「包损坏」（原因原文）
    Invalid(String),
}

/// **装载白名单 = 台账**（唯一安装真相）：只有台账里登记且未停用的包才装载，
/// 磁盘上多出来的目录（手拷 / 卸载残留 / 历史野包）一律不装载——否则就会
/// 「市场说未安装、左面板却挂着它」（090-4 v5b 踩过）。
///
/// 薄包装：只要清单。**render 路径也在调它**（已安装页标题回落），所以这里
/// 不能有副作用；收集失效包走 [`scan_manifests_full`]。
pub(crate) fn scan_manifests() -> Vec<PluginManifest> {
    scan_manifests_full().manifests
}

/// 带失效包收集的扫描（启动链 / 单包重载用）。
pub(crate) fn scan_manifests_full() -> ScanOutcome {
    let Some(root) = pi_link::paths::plugins_dir() else {
        return ScanOutcome {
            manifests: Vec::new(),
            stale: Vec::new(),
        };
    };
    scan_root(&root, &pi_link::pf_plugins::load_ledger())
}

/// 扫描本体（显式 root + 台账，单测直接喂）。
pub(crate) fn scan_root(root: &Path, ledger: &[pi_link::pf_plugins::LedgerEntry]) -> ScanOutcome {
    // 目录别名（`@scope__name` 与平铺 `name` 都认）→ (台账包名, 台账版本)；只含未停用的
    let enabled: std::collections::HashMap<String, (String, String)> = ledger
        .iter()
        .filter(|l| !l.disabled)
        .flat_map(|l| {
            pi_link::pf_plugins::dir_aliases(&l.name)
                .into_iter()
                .map(move |a| (a, (l.name.clone(), l.version.clone())))
        })
        .collect();
    let mut out = ScanOutcome {
        manifests: Vec::new(),
        stale: Vec::new(),
    };
    let mut candidates = scan_candidates(root);
    candidates.sort_by(|a, b| a.1.cmp(&b.1));
    // 装载单位 = 插件 id，不是磁盘目录（2026-12 真机）：市场包安装后，历史
    // link:dev 平铺残留（prune 认清单不删、一直留在磁盘）经目录别名冒名命中
    // 同一台账条目 → 同插件双装载 → fn_bar 双 icon。同 id 多候选时：版本层
    // 目录名精确等于台账版本者优先，平铺（dev 兼容布局）回落。
    let mut picked: Vec<(bool, PluginManifest)> = Vec::new(); // (exact, m)，按 id 去重
    for (pkg, dir) in candidates {
        // 准入/停用判定必须看**顶层包目录名**：版本层目录名是版本号，拿它比
        // 包名永远比不中（停用过滤失效 → 重启后照旧装载，090-4 v5 踩过）
        let Some((ledger_name, ledger_version)) = enabled.get(&pkg) else {
            continue; // 未安装（台账无此包）或已停用，不装载不绘制
        };
        // 版本层布局**只认台账版本那一个目录**（台账 = 唯一安装真相）：更新后
        // 旧版本目录若还没清掉（被占用 / 上版 PF 没清），两版清单都合法时会
        // 双装载——双 tab、双右键项，且 register_plugin_dir 按字符串序 last-wins
        // （0.1.10 < 0.1.9）。平铺布局 dir == <pkg>，无版本可比，照旧（回落位）。
        let flat = dir == root.join(&pkg);
        let exact =
            !flat && dir.file_name().and_then(|n| n.to_str()) == Some(ledger_version.as_str());
        if !flat && !exact {
            continue; // 非台账版本的残留目录：不装载、不算失效（prune 会清）
        }
        match parse_manifest(&dir) {
            Ok(mut m) => {
                // 内层 wasm 插件无 contributes 也合法（验签在 wasm 内，090-5 L3）
                if m.wasm.is_none()
                    && m.bytecode.is_none()
                    && m.status_bar.is_none()
                    && m.file_menu.is_empty()
                    && m.panels.is_empty()
                    && m.fn_bar.is_none()
                    && m.popups.is_empty()
                {
                    continue; // 纯库包
                }
                m.pkg = pkg;
                match picked.iter().position(|(_, e)| e.id == m.id) {
                    Some(ix) => {
                        // 同 id 已有候选：精确匹配把非精确挤掉，反之让位
                        if exact && !picked[ix].0 {
                            picked[ix] = (exact, m);
                        }
                    }
                    None => picked.push((exact, m)),
                }
            }
            Err(e) => {
                // 拒载原因进 stale 走 toast/市场提示；stderr 只在 debug 构建打
                if cfg!(debug_assertions) {
                    eprintln!("[pf-plugin] 跳过 {}：{e}", dir.display());
                }
                out.stale.push(stale_of(ledger_name, &pkg, &dir, e));
            }
        }
    }
    for (_, m) in picked {
        let dir = m.dir.clone();
        super::assets::register_plugin_dir(&m.id, &dir);
        out.manifests.push(m);
    }
    out
}

/// 拒载清单 → 失效包条目：schema 不支持单列（可操作 = 去更新），其余归损坏。
fn stale_of(ledger_name: &str, pkg: &str, dir: &Path, err: String) -> StalePlugin {
    let head = peek_manifest_head(dir);
    let kind = match head.schema {
        Some(got) if got != SUPPORTED_MANIFEST_SCHEMA && got != 3 && got != 4 => {
            StaleKind::SchemaUnsupported { got }
        }
        _ => StaleKind::Invalid(err),
    };
    let version = head.version.unwrap_or_else(|| {
        dir.file_name()
            .and_then(|n| n.to_str())
            .filter(|n| n.starts_with(|c: char| c.is_ascii_digit()))
            .unwrap_or("")
            .to_string()
    });
    let title_zh = head
        .title_zh
        .unwrap_or_else(|| pkg.rsplit("__").next().unwrap_or(pkg).to_string());
    StalePlugin {
        name: ledger_name.to_string(),
        version,
        title_zh,
        kind,
    }
}

/// 清单头三字段的容错行扫（`schema` / `version` / `title.zh`），只看第一个
/// `[` 段之前的顶层行。给失效包取元数据用——此时 `parse_manifest` 已经拒了，
/// 不能再依赖它；也**不**做校验，拿到什么算什么。
#[derive(Default)]
struct ManifestHead {
    schema: Option<u32>,
    version: Option<String>,
    title_zh: Option<String>,
}

fn peek_manifest_head(dir: &Path) -> ManifestHead {
    let mut head = ManifestHead::default();
    let Ok(src) = std::fs::read_to_string(dir.join("pf-plugin.toml")) else {
        return head;
    };
    for line in src.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            break;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "schema" => head.schema = v.split('#').next().unwrap_or("").trim().parse().ok(),
            "version" => head.version = quoted(v),
            "title" => {
                // title = { zh = "快捷面板", en = "Quick Panel" }
                head.title_zh = v
                    .split_once("zh")
                    .and_then(|(_, rest)| rest.split_once('='))
                    .and_then(|(_, rest)| quoted(rest.trim()));
            }
            _ => {}
        }
    }
    head
}

/// 取行首的 `"..."` 字面量内容。
fn quoted(v: &str) -> Option<String> {
    let rest = v.strip_prefix('"')?;
    rest.split_once('"').map(|(s, _)| s.to_string())
}

/// 按包名直读清单标题（已装列表用）：停用插件被 scan 过滤、没有清单条目，
/// 显示名不能因此跌回裸包名——盘上清单还在，title 直读即可。
pub(crate) fn manifest_title_for(pkg: &str) -> Option<String> {
    for alias in pi_link::pf_plugins::dir_aliases(pkg) {
        let dir = pi_link::paths::plugins_dir()?.join(&alias);
        if dir.join("pf-plugin.toml").is_file() {
            if let Ok(m) = parse_manifest(&dir) {
                return Some(m.title_zh);
            }
        }
    }
    None
}

/// 已装插件清单声明的认领前缀全集（契约 v2.4 `[[contributes.sessions]]`）。
/// 启动恢复的**影子会话铁闸**数据源：直接读磁盘清单（后台线程可调），
/// 不看运行期台账——停用/未登记的包也算：它的认领线残留会话同样不该
/// 被桌面恢复接走。PF 不内置任何插件身份，前缀全部来自插件自声明。
pub(crate) fn declared_claim_prefixes() -> Vec<String> {
    let Some(root) = pi_link::paths::plugins_dir() else {
        return Vec::new();
    };
    scan_candidates(&root)
        .iter()
        .filter_map(|(_, dir)| parse_manifest(dir).ok())
        .flat_map(|m| m.sessions.into_iter().map(|s| s.prefix))
        .collect()
}

/// 扫插件根目录出 `(顶层包目录名, 清单所在目录)` 候选。
///
/// 布局两档：`plugins/<pkg>/pf-plugin.toml`（dev 手工平铺）与
/// `plugins/<pkg>/<version>/pf-plugin.toml`（market 安装链落盘，带版本层）。
/// 顶层没有清单就下钻一层收版本目录。**顶层包目录名才是可对账的身份**，
/// 下钻出来的版本目录只是装载位置。
pub(super) fn scan_candidates(root: &Path) -> Vec<(String, PathBuf)> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    rd.flatten()
        .flat_map(|e| {
            let d = e.path();
            if !d.is_dir() {
                return Vec::new();
            }
            let Some(pkg) = d.file_name().map(|f| f.to_string_lossy().into_owned()) else {
                return Vec::new();
            };
            if d.join("pf-plugin.toml").is_file() {
                return vec![(pkg, d)];
            }
            std::fs::read_dir(&d)
                .map(|rd| {
                    rd.flatten()
                        .map(|v| v.path())
                        .filter(|v| v.is_dir() && v.join("pf-plugin.toml").is_file())
                        .map(|v| (pkg.clone(), v))
                        .collect()
                })
                .unwrap_or_default()
        })
        .collect()
}
