use super::migrate::now_iso;
use super::*;

fn plugin_dir(name: &str, version: &str) -> Result<PathBuf, String> {
    let root = crate::paths::plugins_dir().ok_or("无法定位插件安装目录")?;
    Ok(root.join(dir_name(name)).join(version))
}

// ---------------------------------------------------------------------------
// npm pack（与宿主 plugin_updates::npm_invocation 同款调用链）
// ---------------------------------------------------------------------------

/// 组 npm 命令：内置 node 直跑内置 npm-cli.js；找不到时退 PATH 上的 node；
/// 最后才裸 npm（仅 Unix 可用——Windows 裸 npm 是 .cmd，会被 spawn 拒绝）。
fn npm_command(args: &[&str]) -> Result<Command, String> {
    let node = crate::vendor::node_bin();
    let node_path = Path::new(&node);
    let mut cmd = if node_path.is_absolute() {
        let cli = node_path
            .parent()
            .map(|d| d.join("node_modules/npm/bin/npm-cli.js"))
            .filter(|p| p.is_file());
        match cli {
            Some(cli) => {
                let mut c = Command::new(node_path);
                c.arg(cli);
                c
            }
            None => {
                #[cfg(windows)]
                {
                    let _ = args;
                    return Err("未找到内置 npm（node 目录缺 npm-cli.js）".into());
                }
                #[cfg(not(windows))]
                {
                    let mut c = Command::new("npm");
                    c.args(args);
                    return Ok(c);
                }
            }
        }
    } else {
        // 裸 "node"：扫 PATH 找真实路径，才能定位同目录的 npm-cli.js
        let found = find_in_path(if cfg!(windows) { "node.exe" } else { "node" });
        match found {
            Some(p) => {
                let cli = p
                    .parent()
                    .map(|d| d.join("node_modules/npm/bin/npm-cli.js"))
                    .filter(|f| f.is_file());
                match cli {
                    Some(cli) => {
                        let mut c = Command::new(&p);
                        c.arg(cli);
                        c
                    }
                    None => {
                        #[cfg(windows)]
                        {
                            let _ = args;
                            return Err("未找到内置 npm（PATH node 目录缺 npm-cli.js）".into());
                        }
                        #[cfg(not(windows))]
                        {
                            let mut c = Command::new("npm");
                            c.args(args);
                            return Ok(c);
                        }
                    }
                }
            }
            None => {
                #[cfg(windows)]
                {
                    return Err("未找到 node".into());
                }
                #[cfg(not(windows))]
                {
                    let mut c = Command::new("npm");
                    c.args(args);
                    return Ok(c);
                }
            }
        }
    };
    cmd.args(args);
    no_window(&mut cmd);
    Ok(cmd)
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var("PATH").ok()?;
    std::env::split_paths(&path)
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

/// GUI 进程 spawn 控制台子进程（npm/tar）不弹黑窗（0x08000000 CREATE_NO_WINDOW）。
#[cfg(windows)]
fn no_window(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000);
}
#[cfg(not(windows))]
fn no_window(_cmd: &mut Command) {}

/// `npm pack <name>@<version>` 到 `dest`，返回 tarball 路径。
fn pack(name: &str, version: &str, dest: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("创建临时目录失败: {e}"))?;
    let spec = format!("{name}@{version}");
    let out = npm_command(&["pack", "--json", "--pack-destination"])?
        .arg(dest)
        .arg(&spec)
        .current_dir(std::env::temp_dir())
        .output()
        .map_err(|e| format!("npm 启动失败: {e}"))?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "npm pack {spec} 失败: {}",
            err.lines().last().unwrap_or("未知错误")
        ));
    }
    let json: serde_json::Value =
        serde_json::from_slice(&out.stdout).map_err(|e| format!("npm pack 输出解析失败: {e}"))?;
    let filename = json[0]["filename"]
        .as_str()
        .ok_or("npm pack 输出缺少 filename")?
        .to_string();
    Ok(dest.join(filename))
}

// ---------------------------------------------------------------------------
// 校验 + 解包 + 安装/卸载
// ---------------------------------------------------------------------------

fn sha512_sri(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读 tarball 失败: {e}"))?;
    let digest = Sha512::digest(&bytes);
    Ok(format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode(digest)
    ))
}

/// 系统 tar 解包（Win10+ 自带 bsdtar；Unix 恒有）。剥掉顶层 `package/`。
/// 市场扫描（market::pf_scan）解清单包复用同一条。
pub fn extract(tarball: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| format!("创建安装目录失败: {e}"))?;
    let mut out = Command::new("tar");
    out.args([
        "-xzf",
        &tarball.to_string_lossy(),
        "-C",
        &dest.to_string_lossy(),
        "--strip-components=1",
    ]);
    no_window(&mut out);
    let out = out
        .output()
        .map_err(|e| format!("tar 启动失败（系统缺 tar?）: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "解包失败: {}",
            String::from_utf8_lossy(&out.stderr)
                .lines()
                .last()
                .unwrap_or("")
        ));
    }
    Ok(())
}

/// 安装尾声钩子：把重活（wasm → cwasm 预编译）压在**安装那一次**做完。
///
/// pi-link 不依赖 wasmtime（那是 app 侧 `pf-plugin-host` 的事），故走 fn 指针
/// 钩子：app 启动时注入。注入后 [`install`] 变成「解压 + 预编译都完成才算装完」，
/// 失败回滚整个安装目录（不会留半成品）。未注入 = 不做事（pi-link 单测/CLI 场景）。
pub type InstallPrepareFn = fn(&Path) -> Result<(), String>;

pub(super) static INSTALL_PREPARE: std::sync::RwLock<Option<InstallPrepareFn>> =
    std::sync::RwLock::new(None);

/// app 启动时注入安装尾声钩子（见 [`InstallPrepareFn`]）。
pub fn install_prepare_hook(f: InstallPrepareFn) {
    if let Ok(mut c) = INSTALL_PREPARE.write() {
        *c = Some(f);
    }
}

pub(super) fn run_install_prepare(dest: &Path) -> Result<(), String> {
    let f = INSTALL_PREPARE.read().ok().and_then(|c| *c);
    match f {
        Some(f) => f(dest),
        None => Ok(()),
    }
}

/// 单插件编译产物缓存目录：`<插件安装目录>/.cache/`。
///
/// 绑死在安装目录上：卸载即随之删除、整个插件目录拷走时缓存跟着走（换机免重编）。
pub fn plugin_cache_for(plugin_dir: &Path) -> PathBuf {
    plugin_dir.join(".cache")
}

/// 安装/更新（同一入口：同版本重装 = 覆盖）。返回给 UI 的完成消息。
pub fn install(entry: &IndexEntry) -> Result<String, String> {
    let dest = plugin_dir(&entry.name, &entry.version)?;
    let tmp = crate::paths::plugins_dir()
        .ok_or("无法定位插件安装目录")?
        .join(".tmp-pack");
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| format!("创建临时目录失败: {e}"))?;

    let result = (|| {
        // 下载档：npm 档按 registry 约定解析（npm pack）；url 档索引自描述路径直连拉取
        // （私有渠道/内层 wasm 包，090-3 §3.1 source 双档）。后续 integrity 对账同一条。
        let tarball = match entry.source.as_str() {
            "url" => {
                if entry.url.is_empty() {
                    return Err("url 档条目缺 url 字段".into());
                }
                let out = tmp.join("pkg.tgz");
                let bytes = crate::market::http_get_bytes(&entry.url, 32 * 1024 * 1024)?;
                std::fs::write(&out, &bytes).map_err(|e| format!("写入临时文件失败: {e}"))?;
                out
            }
            _ => pack(&entry.name, &entry.version, &tmp)?,
        };
        let actual = sha512_sri(&tarball)?;
        if actual != entry.integrity {
            std::fs::remove_file(&tarball).ok();
            return Err(format!(
                "完整性校验失败：索引 {} ≠ 实际 {actual}",
                entry.integrity
            ));
        }
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|e| format!("清理旧版本失败: {e}"))?;
        }
        extract(&tarball, &dest)?;
        std::fs::remove_file(&tarball).ok();
        // 安装即终态：预编译（wasm → cwasm）在用户等待的这几秒里一次做完，
        // 此后启动/开面板永远走 mmap 命中，运行时零编译。失败 = 回滚，不留半成品。
        if let Err(e) = run_install_prepare(&dest) {
            let _ = std::fs::remove_dir_all(&dest);
            return Err(format!("安装失败：{e}"));
        }
        ledger_upsert(LedgerEntry {
            name: entry.name.clone(),
            version: entry.version.clone(),
            integrity: entry.integrity.clone(),
            installed_at: now_iso(),
            source: "market".into(),
            disabled: false,
        })?;
        // 台账已指向新版本：旧版本目录即残留，当场清（数据在 <pkg>/data 不受
        // 影响；被 mmap 占住删不掉的留给下次启动 prune_stale_versions）
        if let Some(pkg_dir) = dest.parent() {
            prune_old_versions(pkg_dir, &entry.version);
        }
        Ok(())
    })();

    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    Ok(format!("{} {} 安装完成", entry.display.zh, entry.version))
}

/// 卸载：摘台账 + 删目录。目录已不在视为成功（幂等）。
///
/// **台账先删**：台账=安装唯一真相，删不干净目录也不能让「市场显示已安装」。
/// 目录删除分两步：先删非缓存内容（清单没了 = 扫描就看不见了），最后试删
/// `.cache/`（cwasm 被运行中的 wasm 实例 mmap 住时 Windows 会拒绝，os error 32
/// ——这时**不算失败**，留待下次启动的孤儿清扫）。
pub fn uninstall(name: &str) -> Result<String, String> {
    // ① 台账先摘（误删目录不影响"已卸载"这个结论）
    ledger_remove(name)?;
    // ② 目录内容尽量清
    let root = crate::paths::plugins_dir().ok_or("无法定位插件安装目录")?;
    let base = root.join(dir_name(name));
    let mut warning = None;
    if base.exists() {
        if let Err(e) = remove_tree_keeping_cache(&base) {
            warning = Some(format!("删除插件目录失败（已标记卸载）: {e}"));
        }
        // ③ 缓存目录最后试：失败 = 有实例正 mmap 住，留给启动清扫
        let _ = std::fs::remove_dir_all(base.join(".cache"));
        let _ = std::fs::remove_dir_all(&base);
    }
    match warning {
        Some(w) => Ok(format!("{name} 已卸载（{w}）")),
        None => Ok(format!("{name} 已卸载")),
    }
}

/// 递归删除目录内容，但**跳过 `.cache/`**（cwasm 可能被 mmap 住）。
pub(super) fn remove_tree_keeping_cache(dir: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.file_name().is_some_and(|n| n == ".cache") {
            continue;
        }
        if path.is_dir() {
            remove_tree_keeping_cache(&path)?;
            let _ = std::fs::remove_dir(&path);
        } else {
            std::fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// 启动清扫：删掉「没有清单也没有台账归属」的残留目录（卸载时被占用留下的
/// `.cache` 等）。只动**明显无主**的目录，跑在后台扫描线程里，失败静默。
pub fn sweep_orphans() {
    let Some(root) = crate::paths::plugins_dir() else {
        return;
    };
    let enabled: std::collections::HashSet<String> = load_ledger()
        .into_iter()
        .flat_map(|l| dir_aliases(&l.name))
        .collect();
    let Ok(rd) = std::fs::read_dir(&root) else {
        return;
    };
    for e in rd.flatten() {
        let pkg_dir = e.path();
        if !pkg_dir.is_dir() {
            continue;
        }
        let pkg = pkg_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if enabled.contains(&pkg) {
            continue; // 台账里还有它：正常安装目录，不动
        }
        // 无台账归属 + 没有任何清单 = 残留。清单两档布局都要认（02 §6）：
        // market 版本层 = 清单在版本子目录；**平铺 dev = 清单在包目录顶层**
        //（漏认顶层曾把 link:dev 部署、尚未登记台账的 dev 插件当残留删掉）
        let has_manifest = pkg_dir.join("pf-plugin.toml").is_file()
            || std::fs::read_dir(&pkg_dir)
                .map(|rd| {
                    rd.flatten()
                        .any(|v| v.path().is_dir() && v.path().join("pf-plugin.toml").is_file())
                })
                .unwrap_or(false);
        if !has_manifest {
            let _ = std::fs::remove_dir_all(&pkg_dir);
        }
    }
}
