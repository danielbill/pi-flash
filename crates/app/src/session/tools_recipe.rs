//! spawn 参数配方（031/034）：`full` 档与「full+」档（内部键 custom）。
//!
//! 为什么不是「full 的 `--tools` + `-e` 插件」：`--tools` 是**注册级**硬
//! allowlist —— 不在名单里的插件工具连 `getAllTools()` 都进不去，任何
//! exposure / 注册时机都挡，`*`、`all` 无通配，`-xt` 只减不加（031 设计
//! A–H 组实测）。唯一通路是**不发 `--tools`**、由扩展在 `session_start`
//! 调 `setActiveTools`（I 组实测）。
//!
//! 两个配方：
//!
//! **full 档（插件零注入）**：`-ne` + 显式加回「个人扩展」+ 内置扩展，
//! 再配 `--tools <full 名单>`。`-ne` 关掉发现/配置/内置扩展后，**包（plugins）
//! 一个都不装** ⇒ 任何插件往系统提示词注入的段落（实测 agent_browser 36,417
//! 字符、占提示词 58~67%）从源头消失；而个人扩展（`~/.pi/agent/extensions/*`，
//! 如 pi-notify）由 [`full_args`] 显式加回，不受影响。skills / prompts / themes
//! 本来就不受 `-ne` 管辖（个人 skills 照常）。
//!
//! **full+ 档（`custom`）**：`-ne` + 会话选中的包 + 内置 + `full_activate.ts`，
//! 不发 `--tools` —— 见 [`full_plugin_args`]。
//!
//! **源消重（2026-10，bead pi-flash-v2g）**：会话清单里的 `npm:x` 在拼装前
//! 先换成 managed 安装路径（`~/.pi/agent/npm/node_modules/<pkg>`，见
//! [`pi_link::extensions::managed_npm_source_path`]）。直接发 `-e npm:x` 会让
//! pi 按 temporary scope 又装一份到 `~/.pi/agent/tmp/extensions`（0700 私有
//! 临时安装）——同一插件 managed 根 / tmp 双份 node_modules + 版本漂移；
//! 而本地路径源 pi 走 `resolveLocalExtensionSource` → `collectPackageResources`
//! 全套挂载（extensions+skills+prompts+themes），零复制零安装。未安装/
//! 装坏的源原样回退 `-e npm:x`（让 pi 自己装）。
//!
//!
//! 为什么 full 不能「装着包只抑制注入」：pi 的扩展加载顺序是
//! `mergePaths(cliEnabledExtensions, enabledExtensions)`（CLI `-e` 在前），
//! 即我们的 `-e` 抑制器**先于**包加载，包的 `before_agent_start`
//! （agent_browser 就是这么注入的）会在我们之后写回；而 `-e` 又无法排到包后面。
//! 所以 full 的干净只能靠「不装载」这条结构性路线（实测：full 档命令集与
//! configured 一字不差 = 包全装，这才是 36KB 注入的来源）。
//!
//! 最终配方（本次 B/C/D/E 复测修订）：
//!
//! ```text
//! pi --mode rpc --session <file>
//!   -ne                                # 精确集：关掉已配置/发现/内置扩展
//!   -e <会话选中的插件…>                # npm:x 优先换成 managed 安装路径（消重）
//!   -e builtin:<settings 里开着的内置…>  # -ne 连内置扩展一起关，逐条加回
//!   -e <full_activate.ts>              # session_start 里 setActiveTools
//! ```
//!
//! 两条与初版设计不同的实测结论：
//! - `-ne` 关的是「已配置 + 发现 + **内置**」扩展：`builtin:codemode`、
//!   `builtin:tool-search` 一起消失（I2 实测），所以要按
//!   [`pi_link::config::enabled_builtin_extensions`] 逐条 `-e` 加回；
//! - `-e builtin:<name>` 会**覆盖** settings 里的 `-builtin:<name>`（C2 实测），
//!   所以内置清单必须跟着 settings 走，不能无条件写死 `builtin:mcp`
//!   （否则用户关掉的 MCP 会被这一档强行打开）。

use std::path::{Path, PathBuf};

/// app 随包分发的激活脚本（编译进二进制；dev 直接读工作区源文件）。
const FULL_ACTIVATE_TS: &str = include_str!("../../assets/full_activate.ts");


/// `full_activate.ts` 的磁盘绝对路径 —— `-e` 只吃真实文件路径，不吃内存。
///
/// 解析顺序（与 `pi_link::vendor::cli_path` 同机制）：
/// 1. `PI_FLASH_FULL_ACTIVATE` 显式覆盖（探针/调试）；
/// 2. 开发工作区 `<crates/app>/assets/full_activate.ts`（编译期路径，改完
///    即生效、无需重建）；
/// 3. 把内置内容落到 `~/.pi-flash/full_activate.ts`（内容变了才写）——
///    release 包不带 `assets/` 目录，靠这一步保证 `-e` 有路径可用。
pub(crate) fn activate_script_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PI_FLASH_FULL_ACTIVATE") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets")
        .join("full_activate.ts");
    if dev.is_file() {
        return Some(dev);
    }
    let dir = pi_link::paths::ensure_dir()?;
    let path = dir.join("full_activate.ts");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(FULL_ACTIVATE_TS) {
        std::fs::write(&path, FULL_ACTIVATE_TS).ok()?;
    }
    Some(path)
}

/// full+plugins 档要追加的 CLI 参数（spawn 时接在 `--session <file>` 之后）。
pub(crate) fn full_plugin_args(ext_sources: &[String], cwd: &Path) -> Vec<String> {
    let builtins = pi_link::config::enabled_builtin_extensions(cwd);
    let script = activate_script_path();
    if script.is_none() {
        eprintln!(
            "full+plugins: full_activate.ts 解析失败（无 dev 资产且 ~/.pi-flash 不可写）——\
             本会话只发 -ne + -e 插件，active 会退回 defaultTools + 插件工具"
        );
    }
    let resolved = resolve_ext_sources(ext_sources, cwd);
    args_from(&resolved, &builtins, script.as_deref())
}

/// `-e` 源消重：`npm:x` 换成 managed 安装路径（pi 对本地目录全套挂载、
/// 不再触发 tmp 临时安装）；未安装的源原样保留（pi 临时安装兼做兑底）。
pub(crate) fn resolve_ext_sources(ext_sources: &[String], cwd: &Path) -> Vec<String> {
    resolve_ext_sources_at(ext_sources, &pi_link::config::agent_dir(), cwd)
}

/// 纯内核（显式注入 agent dir，便于断言）。
pub(crate) fn resolve_ext_sources_at(
    ext_sources: &[String],
    agent_dir: &Path,
    cwd: &Path,
) -> Vec<String> {
    ext_sources
        .iter()
        .map(|src| {
            pi_link::extensions::managed_npm_source_path(src, agent_dir, cwd)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|| src.clone())
        })
        .collect()
}

/// full 档要追加的 CLI 参数：`-ne` + 个人扩展 + settings 里开着的内置扩展。
///
/// 不发 `--tools`（`full` 档的 `--tools` 由 runtime 那边拼，见 `spawn`）——
/// 这里只管「精确集」，让包一个都不进。
pub(crate) fn full_args(cwd: &Path) -> Vec<String> {
    let builtins = pi_link::config::enabled_builtin_extensions(cwd);
    let personal = pi_link::extensions::personal_extensions();
    full_args_from(&personal, &builtins)
}

/// 纯拼装（显式注入两份清单，便于断言）。
pub(crate) fn full_args_from(personal: &[String], builtins: &[String]) -> Vec<String> {
    let mut out = vec!["-ne".to_string()];
    let mut seen: Vec<&str> = Vec::new();
    for p in personal {
        let p = p.trim();
        if p.is_empty() || seen.contains(&p) {
            continue;
        }
        seen.push(p);
        out.push("-e".into());
        out.push(p.to_string());
    }
    for b in builtins {
        out.push("-e".into());
        out.push(b.clone());
    }
    out
}

/// 纯参数拼装（去重、跳过空格；显式注入内置清单与脚本路径，便于断言）。
pub(crate) fn args_from(
    ext_sources: &[String],
    builtins: &[String],
    script: Option<&Path>,
) -> Vec<String> {
    let mut out = vec!["-ne".to_string()];
    let mut seen: Vec<&str> = Vec::new();
    for src in ext_sources {
        let src = src.trim();
        if src.is_empty() || seen.contains(&src) {
            continue;
        }
        seen.push(src);
        out.push("-e".into());
        out.push(src.to_string());
    }
    for b in builtins {
        out.push("-e".into());
        out.push(b.clone());
    }
    if let Some(p) = script {
        out.push("-e".into());
        out.push(p.to_string_lossy().into_owned());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srcs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn zero_selection_is_legal_and_keeps_ne_and_script() {
        let builtins = srcs(&["builtin:codemode"]);
        let args = args_from(&[], &builtins, Some(Path::new("/x/full_activate.ts")));
        assert_eq!(
            args,
            srcs(&["-ne", "-e", "builtin:codemode", "-e", "/x/full_activate.ts"])
        );
        // 不发 --tools：它是注册级 allowlist，会把插件工具挡在注册之外
        assert!(!args.iter().any(|a| a == "--tools" || a == "--no-tools"));
    }

    #[test]
    fn selection_order_is_preserved_and_deduped() {
        let args = args_from(
            &srcs(&["npm:a", "npm:b", "npm:a", "  ", "npm:c"]),
            &[],
            None,
        );
        assert_eq!(args, srcs(&["-ne", "-e", "npm:a", "-e", "npm:b", "-e", "npm:c"]));
    }

    #[test]
    fn full_args_are_ne_plus_personal_plus_builtins_no_tools() {
        let args = full_args_from(
            &srcs(&["C:/u/.pi/agent/extensions/pi-notify.ts"]),
            &srcs(&["builtin:codemode"]),
        );
        assert_eq!(
            args,
            srcs(&["-ne", "-e", "C:/u/.pi/agent/extensions/pi-notify.ts", "-e", "builtin:codemode"])
        );
        assert!(!args.iter().any(|a| a == "--tools" || a == "--no-tools"), "full 的 --tools 在 runtime 拼");
        assert!(!args.iter().any(|a| a.starts_with("npm:")), "full 不装任何包");
    }

    #[test]
    fn full_args_dedupe_and_skip_blank_personal() {
        let args = full_args_from(&srcs(&["a.ts", "a.ts", "  "]), &[]);
        assert_eq!(args, srcs(&["-ne", "-e", "a.ts"]));
    }

    #[test]
    fn builtins_come_after_selection_and_before_script() {
        let args = args_from(
            &srcs(&["npm:a"]),
            &srcs(&["builtin:mcp", "builtin:tool-search"]),
            Some(Path::new("/s.ts")),
        );
        assert_eq!(
            args,
            srcs(&[
                "-ne",
                "-e",
                "npm:a",
                "-e",
                "builtin:mcp",
                "-e",
                "builtin:tool-search",
                "-e",
                "/s.ts"
            ])
        );
    }

    #[test]
    fn npm_sources_rewrite_to_managed_paths_with_fallback() {
        let base = std::env::temp_dir().join(format!("pi-flash-recipe-{}", std::process::id()));
        let agent = base.join("agent");
        let cwd = base.join("proj");
        std::fs::create_dir_all(agent.join("npm").join("node_modules").join("pi-freeflow"))
            .unwrap();
        std::fs::create_dir_all(cwd.join(".pi").join("npm").join("node_modules").join("pi-goal"))
            .unwrap();

        let out = resolve_ext_sources_at(
            &srcs(&[
                "npm:pi-freeflow",        // user 根命中 → 路径
                "npm:pi-goal",            // 项目根命中 → 路径
                "npm:pi-missing",         // 没装 → 原样回退
                "git:github.com/x/y@v1",  // 非 npm: 原样
                "C:/u/.pi/agent/extensions/pi-notify.ts", // 本地路径原样
            ]),
            &agent,
            &cwd,
        );
        assert_eq!(out[0], agent.join("npm").join("node_modules").join("pi-freeflow").to_string_lossy());
        assert_eq!(out[1], cwd.join(".pi").join("npm").join("node_modules").join("pi-goal").to_string_lossy());
        assert_eq!(out[2], "npm:pi-missing");
        assert_eq!(out[3], "git:github.com/x/y@v1");
        assert_eq!(out[4], "C:/u/.pi/agent/extensions/pi-notify.ts");
        let _ = std::fs::remove_dir_all(&base);
    }

}
