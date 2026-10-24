//! pi-flash 自有文件路径 —— **不污染 pi**（010-启动.md §4）。
//!
//! pi-flash 自己写的文件全部落 `~/.pi-flash/`；`~/.pi/agent/` 只留给 pi 自己
//! （`sessions/`、`settings.json`、`models.json`、`models-store.json`、`skills/`、
//! `npm/`、`auth.json` …）。
//!
//! - 目录解析：`PI_FLASH_DIR` 优先（显式覆盖一切）；未设时按**构建档位**取
//!   默认——release（npm 载荷/绿色包）= `%USERPROFILE%\.pi-flash`（正式 home），
//!   debug/test（cargo run / cargo test）= workspace 的 `pi-flash-dev`（兜底
//!   `~/.pi-flash-dev`）。正式/开发两条 home 由构建档位焊死，裸跑测试摸不到
//!   正式目录（2026-10-18 测试写穿真实 workspace.json 冲掉 pins，即缺这道墙）。
//! - 旧文件迁移：老的 `~/.pi/agent/pi-flash-*.json` 在新路径缺失时原样 `rename`
//!   过来（不复制、不改内容）；新路径已存在则不动旧文件、不做清理。
//!   迁移必须在任何读者（sessions 扫描器 / recents 单例 / workspace 记忆缓存）
//!   首次读盘**之前**跑 —— 由 `startup` 在启动第一步调用。

use std::path::{Path, PathBuf};

/// pi-flash 配置目录（可能不存在）。
///
/// pro/dev 双 home：`PI_FLASH_DIR` 显式指定永远优先；未设时由**构建档位**
/// 决定默认——debug/test 构建永不落正式目录，隔离不依赖命令行记得带前缀。
pub fn dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("PI_FLASH_DIR") {
        if !d.trim().is_empty() {
            return Some(PathBuf::from(d));
        }
    }
    // 属性级 cfg 分支：release 构建不编译 dev_dir，反之亦然
    #[cfg(debug_assertions)]
    return dev_dir();
    #[cfg(not(debug_assertions))]
    return pro_dir();
}

/// 正式 home：仅 release 构建走到这（npm 载荷 / 绿色包）。debug 档不调用，压告警。
#[cfg_attr(debug_assertions, allow(dead_code))]
fn pro_dir() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(Path::new(&home).join(".pi-flash"))
}

/// 开发 home：debug/test 构建默认值。路径编译期烙进 `CARGO_MANIFEST_DIR`
/// （`crates/<crate>` 上两级 = workspace 根）；workspace 已不在（debug exe
/// 被拷走等）则退 `~/.pi-flash-dev`——仍非正式目录。
#[cfg(debug_assertions)]
fn dev_dir() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2)?;
    let ws_dev = root.join("pi-flash-dev");
    if ws_dev.parent().is_some_and(|p| p.is_dir()) {
        return Some(ws_dev);
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(Path::new(&home).join(".pi-flash-dev"))
}

/// pi-flash 配置目录，写前确保存在。
pub fn ensure_dir() -> Option<PathBuf> {
    let d = dir()?;
    let _ = std::fs::create_dir_all(&d);
    Some(d)
}

/// 自有文件全路径（目录不存在也返回路径，写方负责 create_dir_all）。
fn file(name: &str) -> Option<PathBuf> {
    Some(dir()?.join(name))
}

// ---------------------------------------------------------------------------
// 应用自有目录识别（never-workspace 口径）：安装根下的任何路径都不是用户
// 项目，禁止进工作区记忆与项目注册表。
// ---------------------------------------------------------------------------

/// 应用安装根（纯函数，exe 路径显式注入）：
/// - npm 分发布局 `<pkg>/payload[-<版本>]/pi-flash.exe` → 根 = `<pkg>`（bin/、
///   payload/、payload-* 全在它下面——更新交接跨载荷目录，只认 exe 目录
///   本身拦不住「cwd 继承自旧 payload 目录」）；父目录名须含 pi-flash 才
///   升一级，防用户项目恰好叫 `payload-*` 时整棵被误判；
/// - 其余（绿色解压包 / target\debug 等）→ 根 = exe 目录本身。
pub fn app_install_root(exe: &Path) -> PathBuf {
    let dir = exe.parent().unwrap_or(exe);
    let name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let looks_like_payload = name == "payload" || name.starts_with("payload-");
    if !looks_like_payload {
        return dir.to_path_buf();
    }
    let parent_is_pkg = dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.to_ascii_lowercase().contains("pi-flash"));
    if parent_is_pkg {
        dir.parent().unwrap_or(dir).to_path_buf()
    } else {
        dir.to_path_buf()
    }
}

/// 当前进程的应用安装根（定位不到 exe 时 None = 无从判定，调用方放行）。
pub fn current_install_root() -> Option<PathBuf> {
    std::env::current_exe().ok().map(|e| app_install_root(&e))
}

/// 路径键规范化（与 `projects::path_key` 同口径：`/`→`\\`、去尾分隔符、
/// 大小写折叠）。paths 在 projects 下游，不能反向引用，就地实现。
fn path_key_norm(p: &Path) -> String {
    let mut k = p.to_string_lossy().replace('/', "\\");
    while k.ends_with('\\') {
        k.pop();
    }
    k.to_ascii_lowercase()
}

/// `p` 是否等于 `install_root` 或在其之下（纯函数，测试显式注入根）。
pub fn is_app_install_path_at(p: &Path, install_root: &Path) -> bool {
    let r = path_key_norm(install_root);
    let k = path_key_norm(p);
    k == r || k.starts_with(&format!("{r}\\"))
}

/// `p` 是否落在当前进程的应用安装根内（含根本身）。载荷目录 / npm 包根是
/// 分发布局的一部分，永远不该成为用户工作区或项目：0.1.x 的更新交接
/// spawn 不设 cwd，新进程继承旧载荷目录曾被当项目注册并写脏 `__last`
/// （「payload 项目删了每次启动又回来」的根因），写入方统一引用本判定。
pub fn is_app_install_path(p: &Path) -> bool {
    current_install_root().is_some_and(|root| is_app_install_path_at(p, &root))
}

/// `~/.pi-flash/pf-auth.json` — PF 独立密钥账本（051-apikey管理.md）。
pub fn pf_auth_file() -> Option<PathBuf> {
    file("pf-auth.json")
}

/// `~/.pi-flash/projects.json` — PF 项目注册表（打开项目即注册，004）。
pub fn projects_file() -> Option<PathBuf> {
    file("projects.json")
}

/// `~/.pi-flash/providers.json` — PF 自定义 provider 账本（051 M1.1：PF
/// 永不写 pi 的 models.json，自定义内容全部记在这里，spawn 经扩展注入）。
pub fn pf_providers_file() -> Option<PathBuf> {
    file("providers.json")
}

/// `~/.pi-flash/registry-cache.json` — 全量 provider 注册表缓存（registry.rs，
/// vendor VERSION 为有效性键）。
pub fn registry_cache_file() -> Option<PathBuf> {
    file("registry-cache.json")
}

/// `~/.pi-flash/pi-ext-market-cache.json` — Pi 扩展市场首页快照（040-1：
/// 单文件、≤50 条、<100KB；启动延迟加载只预取首页写它）。
pub fn pi_ext_market_cache_file() -> Option<PathBuf> {
    file("pi-ext-market-cache.json")
}

/// `~/.pi-flash/plugins.json` — PF 插件台账（090-4：安装/卸载记账，唯一落盘物）。
pub fn plugins_ledger_file() -> Option<PathBuf> {
    file("plugins.json")
}

/// `~/.pi-flash/plugins/` — PF 插件安装根（`<name>/<version>/`，写前建目录）。
pub fn plugins_dir() -> Option<PathBuf> {
    let d = ensure_dir()?.join("plugins");
    let _ = std::fs::create_dir_all(&d);
    Some(d)
}

/// `~/.pi-flash/cache/pf-plugin/` — 内层 wasm 插件的**编译产物缓存**（`.cwasm`）。
/// 刻意不落 `plugins/` 下：插件扫描只认 `plugins/<dir>/pf-plugin.toml`，缓存
/// 不掺进发现路径；删掉整个目录也只是回到「重编一次」。
pub fn plugin_cache_dir() -> Option<PathBuf> {
    let d = ensure_dir()?.join("cache").join("pf-plugin");
    let _ = std::fs::create_dir_all(&d);
    Some(d)
}

/// `~/.pi-flash/registry-dump.mjs` — 注册表 dump 一次性脚本（每次 dump 重写）。
pub fn registry_dump_script_file() -> Option<PathBuf> {
    file("registry-dump.mjs")
}

/// `~/.pi-flash/pf-providers.mjs` — spawn 时 `-e` 注入的官方扩展（进程内
/// `pi.registerProvider`），由 pf_providers::ensure_extension_template 生成。
pub fn pf_providers_ext_file() -> Option<PathBuf> {
    file("pf-providers.mjs")
}

/// `~/.pi-flash/providers-migrated` — models.json 一次性复制迁移完成标记。
pub fn pf_providers_migrated_file() -> Option<PathBuf> {
    file("providers-migrated")
}

/// `~/.pi-flash/workspace.json` — 工作区记忆（每项目 last_open、布局、UI 状态）。
pub fn workspace_file() -> Option<PathBuf> {
    file("workspace.json")
}

/// `~/.pi-flash/app-settings.json` — 主题/语言/提示音/字体/加载窗口/恢复开关。
pub fn app_settings_file() -> Option<PathBuf> {
    file("app-settings.json")
}

/// `<PF数据目录>/dev-flags.json` — 开发旗标（功能总开关，默认关；改动重启
/// 生效）。debug 构建目录 = `./pi-flash-dev/`（gitignore，开发机各自建），
/// release = `~/.pi-flash/`（用户机器无此文件 = 功能恒关，发布包即关态）。
pub fn dev_flags_file() -> Option<PathBuf> {
    file("dev-flags.json")
}

/// `~/.pi-flash/session-index.json` — 会话摘要指纹索引（tail-seek 缓存）。
pub fn session_index_file() -> Option<PathBuf> {
    file("session-index.json")
}

/// `~/.pi-flash/session-recents.json` — 最近活跃会话清单（加载窗口过滤用）。
pub fn session_recents_file() -> Option<PathBuf> {
    file("session-recents.json")
}

/// `~/.pi-flash/provider-catalog.json` — 「获取模型」落盘的 provider 目录
/// （probe 成功响应的 best-effort 解析；disk_models 优先读取，覆盖
/// models-store 的陈旧缓存）。与 010 的 catalog-cache.json（进程答案缓存）
/// 是两个文件。
pub fn provider_catalog_file() -> Option<PathBuf> {
    file("provider-catalog.json")
}

/// `~/.pi-flash/session-ext.json` — 会话级插件清单（自定义档：每个对话保存一份）。
pub fn session_ext_file() -> Option<PathBuf> {
    file("session-ext.json")
}

/// `~/.pi-flash/catalog-cache.json` — 模型目录 + 扩展命令缓存（010-启动.md §4.1）。
pub fn catalog_cache_file() -> Option<PathBuf> {
    file("catalog-cache.json")
}

/// `~/.pi-flash/update-state.json` — 自动更新状态（081）：last_seen_version
/// （新版本首启判定，开更新日志 tab 的依据）。
pub fn update_state_file() -> Option<PathBuf> {
    file("update-state.json")
}

/// `~/.pi-flash/changelog-cache/<version>.md` — 各版本更新日志缓存（081：
/// 检查更新时随 release body 落盘；更新日志页读这里，缺失再按 tag 现拉）。
pub fn changelog_cache_file(version: &str) -> Option<PathBuf> {
    Some(
        dir()?
            .join("changelog-cache")
            .join(format!("{}.md", version.trim_start_matches('v'))),
    )
}

/// 迁移表：新文件名 ← 旧文件名（`~/.pi/agent/pi-flash-<旧>`）。
const MIGRATION: &[(&str, &str)] = &[
    ("workspace.json", "workspace.json"),
    ("app-settings.json", "app-settings.json"),
    ("session-index.json", "session-index.json"),
    ("session-recents.json", "session-recents.json"),
];
/// 旧目录：`~/.pi/agent`（迁移源目录）。
fn legacy_dir() -> Option<PathBuf> {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .ok()?;
    Some(Path::new(&home).join(".pi").join("agent"))
}

/// pi 自己的全局目录 `~/.pi/agent`（全局 AGENTS.md / settings.json 所在，
/// 不落 pi-flash 文件）。系统提示词 breakdown 用它判「全局 vs 项目 AGENTS.md」。
pub fn pi_agent_dir() -> Option<PathBuf> {
    legacy_dir()
}

/// 迁移实现（目录显式注入，便于测试且不碰进程环境）：逐个把旧文件原样搬过来。
fn migrate_into(new_dir: &Path, legacy_dir: &Path) -> usize {
    let mut moved = 0;
    for (new_name, legacy_name) in MIGRATION {
        let new_path = new_dir.join(new_name);
        if new_path.exists() {
            continue;
        }
        let old_path = legacy_dir.join(format!("pi-flash-{legacy_name}"));
        if !old_path.is_file() {
            continue;
        }
        if std::fs::rename(&old_path, &new_path).is_ok() {
            moved += 1;
        }
    }
    moved
}

/// 一次性迁移（启动第一步）：新文件缺失且旧文件存在 → 原样 `rename`。
/// 返回搬过来的文件数。任何一步失败都只是少搬一个文件，不阻塞启动。
pub fn migrate_legacy_files() -> usize {
    let Some(dir) = ensure_dir() else { return 0 };
    let Some(old_dir) = legacy_dir() else {
        return 0;
    };
    migrate_into(&dir, &old_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 迁移的四个动作：搬家成功 / 目标已在则跳过 / 旧文件缺失则跳过 / rename 而非复制。
    /// 目录显式注入，不碰进程环境（并行测试安全）。
    #[test]
    fn migration_moves_legacy_files_without_touching_env() {
        let base = std::env::temp_dir().join(format!(
            "pi-link-paths-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .subsec_nanos()
        ));
        let new_dir = base.join(".pi-flash");
        let old_dir = base.join(".pi").join("agent");
        std::fs::create_dir_all(&old_dir).unwrap();
        std::fs::write(old_dir.join("pi-flash-workspace.json"), "{\"old\":1}").unwrap();
        // 目标已存在的那个：旧文件必须保留、新内容不能被覆盖
        std::fs::write(old_dir.join("pi-flash-session-recents.json"), "{\"old\":1}").unwrap();
        std::fs::create_dir_all(&new_dir).unwrap();
        std::fs::write(new_dir.join("session-recents.json"), "{\"new\":1}").unwrap();

        let moved = migrate_into(&new_dir, &old_dir);
        assert_eq!(moved, 1, "只有 workspace.json 该搬（recents 目标已在）");
        assert_eq!(
            std::fs::read_to_string(new_dir.join("workspace.json")).unwrap(),
            "{\"old\":1}"
        );
        assert!(
            !old_dir.join("pi-flash-workspace.json").exists(),
            "旧文件是 rename 搬家，不是复制"
        );
        assert!(
            old_dir.join("pi-flash-session-recents.json").exists(),
            "目标已在则跳过"
        );
        assert_eq!(
            std::fs::read_to_string(new_dir.join("session-recents.json")).unwrap(),
            "{\"new\":1}"
        );
        // 幂等：再跑一次没有任何可搬的
        assert_eq!(migrate_into(&new_dir, &old_dir), 0);
        let _ = std::fs::remove_dir_all(&base);
    }

    /// 安装根识别：npm 布局的跨载荷目录覆盖 + 用户项目误伤面检查。
    #[test]
    fn install_root_covers_sibling_payload_dirs() {
        // npm 布局：<pkg>/payload-0.1.12/ → 根 = <pkg>；旧载荷目录
        // <pkg>/payload（交接继承 cwd 的场景）与包内其他文件一律命中
        let pkg = Path::new("D:/softwares/node-v24.21.0-win-x64/node_modules/pi-flash");
        let root = app_install_root(&pkg.join("payload-0.1.12").join("pi-flash.exe"));
        assert_eq!(root, pkg);
        assert!(is_app_install_path_at(&pkg.join("payload"), &root));
        assert!(is_app_install_path_at(
            &pkg.join("payload").join("vendor").join("pi"),
            &root
        ));
        assert!(is_app_install_path_at(
            &pkg.join("bin").join("pi-flash.js"),
            &root
        ));
        assert!(!is_app_install_path_at(
            Path::new("D:/ai_workspace/pi-flash"),
            &root
        ));
        assert!(!is_app_install_path_at(Path::new("D:/softwares"), &root));
        // 同一套字符串规范化：大小写 / 正反斜杠不影响判定
        assert!(is_app_install_path_at(
            Path::new("d:/SOFTWARES/node-v24.21.0-win-x64/node_modules/PI-FLASH/PAYLOAD"),
            &root
        ));
        // 用户项目恰好叫 payload-*（父目录名不含 pi-flash）：只排除 exe
        // 所在目录本身，不升一级
        let root2 = app_install_root(Path::new("D:/work/payload-demo/pi-flash.exe"));
        assert_eq!(root2, Path::new("D:/work/payload-demo"));
        assert!(is_app_install_path_at(
            Path::new("D:/work/payload-demo/vendor"),
            &root2
        ));
        assert!(!is_app_install_path_at(Path::new("D:/work"), &root2));
        // 绿色解压包：目录名不含 payload 前缀 → exe 目录即根
        assert_eq!(
            app_install_root(Path::new("D:/tools/pi-flash-v0.1.12/pi-flash.exe")),
            Path::new("D:/tools/pi-flash-v0.1.12")
        );
    }
}
