//! 扩展/技能说明 token 测量（040 延迟加载，v3）。
//!
//! 两段式，全在后台线程：
//! 1. **静态探针**（`assets/token_probe.mjs`）：复用 pi 的扩展装载器
//!    （`loadExtensions`）+ 系统提示词组装器，逐包测装载期可见的三块——
//!    工具 promptSnippet/promptGuidelines 差值、装载期工具声明、
//!    before_agent_start/指令组通道的指令文本——并报告「懒注册信号」
//!    （挂了 `session_start`）与实际装载入口。
//! 2. **单会话精确测量**（`assets/context_dump.mjs`，仅懒注册包存在时）：
//!    把全部懒注册包挂进一个 `pi --mode rpc` 进程，pi bind 即派发
//!    `session_start`（无需 prompt/模型调用），懒注册工具（computer-use
//!    的 58 颗、pi-fff 的 replace 等）这时才挂齐；dump 脚本 debounce 后把
//!    `getAllTools()` 全量原子写到文件，Rust 按 `sourceInfo.source` 归属包、
//!    CJK 感知估算声明 token。
//!
//! 缓存 `~/.pi-flash/ext-tokens.json`，带 CACHE_VERSION（公式变更整体
//! 作废重测）+ 逐包 package.json mtime 失效。运行挂在揭幕之后
//! （`spawn_splash_gate` 置 `booted` 的同一帧），不碰启动链路。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::Chat;

/// 单个包的测量结果：展示值 = ext（提示词差值 + 指令文本 + 声明的总和）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TokenEntry {
    pub ext: i64,
    pub skills: i64,
    /// 测量时包 package.json 的 mtime（秒），用于缓存失效
    pub mtime: u64,
}

/// `token_probe.mjs` / `context_dump.mjs` 共用的三段式资产解析
/// （env 覆盖 → dev 资产 → 落 `~/.pi-flash/`）。
fn asset_path(env_key: &str, file_name: &str, embedded: &str) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(env_key) {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join(file_name);
    if dev.is_file() {
        return Some(dev);
    }
    let dir = pi_link::paths::ensure_dir()?;
    let path = dir.join(file_name);
    if std::fs::read_to_string(&path).ok().as_deref() != Some(embedded) {
        std::fs::write(&path, embedded).ok()?;
    }
    Some(path)
}

fn probe_script_path() -> Option<PathBuf> {
    asset_path(
        "PI_FLASH_TOKEN_PROBE",
        "token_probe.mjs",
        include_str!("../../assets/token_probe.mjs"),
    )
}

fn dump_script_path() -> Option<PathBuf> {
    asset_path(
        "PI_FLASH_CONTEXT_DUMP",
        "context_dump.mjs",
        include_str!("../../assets/context_dump.mjs"),
    )
}

fn cache_path() -> Option<PathBuf> {
    Some(pi_link::paths::ensure_dir()?.join("ext-tokens.json"))
}

/// 缓存格式版本：探针公式变了（v3 引入单会话 dump 归属）就 bump，旧缓存
/// 整体作废重测——否则旧值会一直被 mtime 判「新鲜」。
const CACHE_VERSION: u64 = 3;

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

/// 静态探针的单包结果（装载期可见面）。
struct StaticResult {
    prompt: i64,
    instruct: i64,
    decl: i64,
    skills: i64,
    lazy: bool,
    entries: Vec<PathBuf>,
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

fn spawn_node(script: &Path, args: &[String]) -> std::io::Result<std::process::Output> {
    let mut cmd = std::process::Command::new(pi_link::vendor::node_bin());
    cmd.arg(script).args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd.output()
}

/// 静态探针：阻塞（后台线程调用），返回成功装载的包。
fn probe(pkgs: &[ProbePkg]) -> HashMap<String, StaticResult> {
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
    let out = spawn_node(&script, &[input_path.to_string_lossy().into_owned()]);
    let _ = std::fs::remove_file(&input_path);
    let map = out.ok().and_then(|o| parse_probe_output(Some(&o.stdout), pkgs));
    map.unwrap_or_default()
}

fn parse_probe_output(
    stdout: Option<&[u8]>,
    pkgs: &[ProbePkg],
) -> Option<HashMap<String, StaticResult>> {
    let stdout = String::from_utf8(stdout?.to_vec()).ok()?;
    let json = serde_json::from_str::<serde_json::Value>(stdout.trim()).ok()?;
    let results = json.get("results")?.as_array()?;
    let mut map = HashMap::new();
    for r in results {
        let Some(source) = r.get("source").and_then(|v| v.as_str()) else {
            continue;
        };
        if !pkgs.iter().any(|p| p.source == source) {
            continue;
        }
        // error 条目（整包装载失败）不入表 → UI 显示「—」
        let (Some(prompt), Some(instruct), Some(decl), Some(skills)) = (
            r.get("prompt_tokens").and_then(|v| v.as_i64()),
            r.get("instruct_tokens").and_then(|v| v.as_i64()),
            r.get("decl_tokens").and_then(|v| v.as_i64()),
            r.get("skills_tokens").and_then(|v| v.as_i64()),
        ) else {
            continue;
        };
        map.insert(
            source.to_string(),
            StaticResult {
                prompt,
                instruct,
                decl,
                skills,
                lazy: r.get("lazy").and_then(|v| v.as_bool()).unwrap_or(false),
                entries: r
                    .get("entries")
                    .and_then(|v| v.as_array())
                    .map(|a| {
                        a.iter()
                            .filter_map(|e| e.as_str())
                            .map(PathBuf::from)
                            .collect()
                    })
                    .unwrap_or_default(),
            },
        );
    }
    Some(map)
}

/// 单会话精确声明：全部懒注册包挂进一个 rpc 进程（bind 即派发
/// session_start，懒注册工具这时挂齐），轮询 dump 文件后杀进程。
/// 返回 包源 → 声明 token 数。阻塞（后台线程调用）。
fn dump_decls(lazy: &[(String, Vec<PathBuf>, PathBuf)]) -> HashMap<String, i64> {
    let (Some(cli), Some(dump)) = (pi_link::vendor::cli_path(), dump_script_path()) else {
        return HashMap::new();
    };
    let Some(workdir) = pi_link::paths::ensure_dir() else {
        return HashMap::new();
    };
    let dump_file = workdir.join("context-dump.json");
    let _ = std::fs::remove_file(&dump_file);
    let mut args: Vec<String> = vec!["--mode".into(), "rpc".into(), "-ne".into()];
    for (_, entries, _) in lazy {
        for e in entries {
            args.push("-e".into());
            args.push(e.to_string_lossy().into_owned());
        }
    }
    args.push("-e".into());
    args.push(dump.to_string_lossy().into_owned());
    let child = std::process::Command::new(pi_link::vendor::node_bin())
        .arg(&cli)
        .args(&args)
        .current_dir(&workdir)
        .env("CONTEXT_DUMP_FILE", &dump_file)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return HashMap::new();
    };
    // 轮询 dump（debounce 5s + 懒注册连接时间），超时 30s
    let mut raw: Option<Vec<u8>> = None;
    for _ in 0..60 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if let Ok(text) = std::fs::read_to_string(&dump_file) {
            raw = Some(text.into_bytes());
            break;
        }
    }
    kill_tree(&mut child);
    let Some(raw) = raw else {
        return HashMap::new();
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(
        String::from_utf8_lossy(&raw).trim(),
    ) else {
        return HashMap::new();
    };
    let Some(tools) = json.get("tools").and_then(|v| v.as_array()) else {
        return HashMap::new();
    };
    // 按 sourceInfo.source 归属：精确匹配包源；否则按入口路径前缀兜底
    let mut map: HashMap<String, i64> = HashMap::new();
    for t in tools {
        let (Some(name), Some(desc), Some(schema)) = (
            t.get("name").and_then(|v| v.as_str()),
            t.get("description").and_then(|v| v.as_str()),
            t.get("schema_text").and_then(|v| v.as_str()),
        ) else {
            continue;
        };
        // hidden 工具永不声明也不可调用，不计
        if t.get("exposure").and_then(|v| v.as_str()) == Some("hidden") {
            continue;
        }
        let text = format!("{name}\n{desc}\n{schema}");
        let n = pi_link::estimate::estimate_tokens(&text) as i64;
        let source = t.get("source").and_then(|v| v.as_str()).unwrap_or("");
        let path = t.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let owner = lazy
            .iter()
            .find(|(src, _, _)| src == source)
            .map(|(src, _, _)| src.clone())
            .or_else(|| {
                lazy.iter()
                    .find(|(_, _, dir)| path.starts_with(&dir.to_string_lossy().to_string()))
                    .map(|(src, _, _)| src.clone())
            });
        let Some(owner) = owner else {
            continue; // builtin / local / 无法归属 → 不是扩展的
        };
        *map.entry(owner).or_default() += n;
    }
    map
}

/// 杀掉 dump 进程树（rpc 进程可能带子进程，如 computer-use 的驱动）。
fn kill_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &child.id().to_string()])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(not(windows))]
    {
        let _ = child.kill();
    }
    let _ = child.wait();
}

/// 两段合并：静态探针 → 有懒注册信号再跑 dump 会话 → 每包取三者之和。
/// dump 失败/缺包回退静态装载期声明，UI 永远有数。阻塞（后台线程调用）。
pub(crate) fn probe_all(pkgs: Vec<ProbePkg>) -> HashMap<String, TokenEntry> {
    let statics = probe(&pkgs);
    let lazy: Vec<(String, Vec<PathBuf>, PathBuf)> = statics
        .iter()
        .filter(|(_, s)| s.lazy && !s.entries.is_empty())
        .filter_map(|(src, s)| {
            pkgs.iter()
                .find(|p| &p.source == src)
                .map(|p| (src.clone(), s.entries.clone(), p.dir.clone()))
        })
        .collect();
    let dump = if lazy.is_empty() {
        HashMap::new()
    } else {
        dump_decls(&lazy)
    };
    statics
        .into_iter()
        .map(|(src, s)| {
            let mtime = pkgs
                .iter()
                .find(|p| p.source == src)
                .map(|p| p.mtime)
                .unwrap_or(0);
            let decl = dump.get(&src).copied().unwrap_or(s.decl);
            (
                src,
                TokenEntry {
                    ext: s.prompt + s.instruct + decl,
                    skills: s.skills,
                    mtime,
                },
            )
        })
        .collect()
}

impl Chat {
    /// 040 延迟加载入口：揭幕（切换启动页）后调用。收集待测包（没测过或
    /// package.json 变了的），后台跑静态探针；有懒注册信号再追加一个
    /// dump 会话（无模型调用）。回来更新 Chat 并落缓存。
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
                .spawn(async move { probe_all(targets) })
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
