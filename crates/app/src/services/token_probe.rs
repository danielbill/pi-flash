//! 扩展/技能说明 token 测量（040 延迟加载）。
//!
//! 测量本体在 `assets/token_probe.mjs`：复用 pi 的扩展装载器
//! （`loadExtensions`）与系统提示词组装器（`buildSystemPromptSections`），
//! 逐包算「相对内置 7 件套」的边际 token 数。本模块只负责：
//! 脚本落盘路径、输入输出与缓存（`~/.pi-flash/ext-tokens.json`，按
//! package.json mtime 失效）。
//!
//! 运行挂在揭幕之后（`spawn_splash_gate` 置 `booted` 的同一帧），一次
//! node 进程算完全部包，后台线程执行，不碰启动链路。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::Chat;

/// 单个包的测量结果：扩展工具的 token 数 + 包内 skills 的 token 数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TokenEntry {
    pub ext: i64,
    pub skills: i64,
    /// 测量时包 package.json 的 mtime（秒），用于缓存失效
    pub mtime: u64,
}

/// `token_probe.mjs` 的磁盘绝对路径 —— 与 `full_activate.ts` 同一套三段式
/// 解析（env 覆盖 → dev 资产 → 落 `~/.pi-flash/`）。
pub(crate) fn probe_script_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PI_FLASH_TOKEN_PROBE") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("token_probe.mjs");
    if dev.is_file() {
        return Some(dev);
    }
    const SCRIPT: &str = include_str!("../../assets/token_probe.mjs");
    let dir = pi_link::paths::ensure_dir()?;
    let path = dir.join("token_probe.mjs");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(SCRIPT) {
        std::fs::write(&path, SCRIPT).ok()?;
    }
    Some(path)
}

fn cache_path() -> Option<PathBuf> {
    Some(pi_link::paths::ensure_dir()?.join("ext-tokens.json"))
}

/// 缓存格式版本：探针公式变了（v2 补调用声明 + 指令文本）就 bump，旧缓存
/// 整体作废重测——否则 v1 的 75 会一直被 mtime 判「新鲜」。
const CACHE_VERSION: u64 = 2;

/// 读缓存（文件缺失/损坏/版本不符 = 空表）。
pub(crate) fn read_cache() -> HashMap<String, TokenEntry> {
    let Some(path) = cache_path() else {
        return HashMap::new();
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return HashMap::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return HashMap::new();
    };
    if json.get("version").and_then(|v| v.as_u64()) != Some(CACHE_VERSION) {
        return HashMap::new();
    }
    let Some(map) = json.get("entries").and_then(|v| v.as_object()) else {
        return HashMap::new();
    };
    map.iter()
        .filter_map(|(source, v)| {
            Some((
                source.clone(),
                TokenEntry {
                    ext: v.get("ext")?.as_i64()?,
                    skills: v.get("skills")?.as_i64()?,
                    mtime: v.get("mtime")?.as_u64()?,
                },
            ))
        })
        .collect()
}

pub(crate) fn write_cache(map: &HashMap<String, TokenEntry>) {
    let Some(path) = cache_path() else {
        return;
    };
    let entries = serde_json::Value::Object(
        map.iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    serde_json::json!({"ext": v.ext, "skills": v.skills, "mtime": v.mtime}),
                )
            })
            .collect::<serde_json::Map<String, serde_json::Value>>(),
    );
    let json = serde_json::json!({"version": CACHE_VERSION, "entries": entries});
    if let Ok(s) = serde_json::to_string(&json) {
        let _ = std::fs::write(&path, s);
    }
}

/// 待测包（source + 安装目录 + package.json mtime）。
pub(crate) struct ProbePkg {
    pub source: String,
    pub dir: PathBuf,
    pub mtime: u64,
}

/// 包 package.json 的 mtime（秒）；读不到 = 0（每次都重测）。
pub(crate) fn package_mtime(dir: &Path) -> u64 {
    std::fs::metadata(dir.join("package.json"))
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// 跑一轮探针：阻塞（后台线程调用），返回成功测量的包。
pub(crate) fn probe(pkgs: Vec<ProbePkg>) -> HashMap<String, TokenEntry> {
    if pkgs.is_empty() {
        return HashMap::new();
    }
    let Some(script) = probe_script_path() else {
        return HashMap::new();
    };
    let Some(cli) = pi_link::vendor::cli_path() else {
        return HashMap::new();
    };
    // cli = <pkg>/dist/bundle/cli.js → pi_dist = <pkg>/dist
    let Some(pi_dist) = cli.parent().and_then(|p| p.parent()) else {
        return HashMap::new();
    };
    let input = serde_json::json!({
        "pi_dist": pi_dist.to_string_lossy(),
        "cwd": std::env::current_dir()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_default(),
        "packages": pkgs
            .iter()
            .map(|p| serde_json::json!({
                "source": p.source,
                "dir": p.dir.to_string_lossy(),
            }))
            .collect::<Vec<_>>(),
    });
    let input_path = match pi_link::paths::ensure_dir() {
        Some(dir) => dir.join("token-probe-input.json"),
        None => return HashMap::new(),
    };
    if std::fs::write(&input_path, input.to_string()).is_err() {
        return HashMap::new();
    }
    let out = std::process::Command::new(pi_link::vendor::node_bin())
        .arg(&script)
        .arg(&input_path)
        .output();
    let _ = std::fs::remove_file(&input_path);
    let Ok(out) = out else {
        return HashMap::new();
    };
    let Ok(stdout) = String::from_utf8(out.stdout) else {
        return HashMap::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(stdout.trim()) else {
        return HashMap::new();
    };
    let mut map = HashMap::new();
    if let Some(results) = json.get("results").and_then(|v| v.as_array()) {
        for r in results {
            let (Some(source), Some(ext), Some(skills)) = (
                r.get("source").and_then(|v| v.as_str()),
                r.get("ext_tokens").and_then(|v| v.as_i64()),
                r.get("skills_tokens").and_then(|v| v.as_i64()),
            ) else {
                continue;
            };
            if let Some(p) = pkgs.iter().find(|p| &p.source == source) {
                map.insert(
                    source.to_string(),
                    TokenEntry {
                        ext,
                        skills,
                        mtime: p.mtime,
                    },
                );
            }
        }
    }
    map
}

impl Chat {
    /// 040 延迟加载入口：揭幕（切换启动页）后调用。收集待测包（没测过或
    /// package.json 变了的），后台跑一次 node 探针，回来更新 Chat 并落缓存。
    /// 几十上百个包也只是多一个进程 + 逐包装载，不碰任何启动链路。
    pub(crate) fn spawn_token_probe(&mut self, cx: &mut gpui::Context<Self>) {
        let mut targets: Vec<ProbePkg> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for v in self.mc_pkgs_global.iter().chain(self.mc_pkgs_project.iter()) {
            let src = pi_link::skills::entry_source(v);
            if src.is_empty() || !seen.insert(src.clone()) {
                continue;
            }
            let dir = pi_link::skills::package_install_dir(&src);
            if !dir.is_dir() {
                continue;
            }
            let mtime = package_mtime(&dir);
            if self.ext_tokens.get(&src).is_some_and(|e| e.mtime == mtime) {
                continue;
            }
            targets.push(ProbePkg { source: src, dir, mtime });
        }
        if targets.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { probe(targets) })
                .await;
            let _ = this.update(cx, |c, cx| {
                for (k, v) in result {
                    c.ext_tokens.insert(k, v);
                }
                write_cache(&c.ext_tokens);
                cx.notify();
            });
        })
        .detach();
    }
}
