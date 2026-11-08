//! 计划模式域（031-1）：pi 进程内的闸门扩展 + spawn 装配。
//!
//! 闸门必须住在 pi 进程里（RPC 模式无宿主审批回调，工具在 pi 内执行），
//! 所以载体是随包分发的 pi 扩展 [`PLAN_MODE_TS`]，机制同
//! `session::tools_recipe` 的 full_activate.ts：dev 直读工作区源文件、
//! release 落盘 `~/.pi-flash`。扩展与 PF 的通道契约（`/plan` 三命令、
//! `setStatus("pf-plan")` 广播、present_plan 挂起审批）见 plan_mode.ts
//! 文件头与 docs/模块设计/031-输入面板/031-1-计划模式.md §1。
//!
//! 本模块 P0 只管「把扩展挂上所有档位」；状态投影（P1）、工具菜单项（P2）、
//! 审批弹窗（P3）后续以 `mode/` 子模块落在这里。

pub(crate) mod dialog;
mod state;

pub(crate) use dialog::PlanUi;

use std::path::{Path, PathBuf};

/// 扩展→PF 状态广播的 statusKey（plan_mode.ts `setStatus` 契约键）。
pub(crate) const STATUS_KEY: &str = "pf-plan";

/// setStatus("pf-plan") 广播载荷（plan_mode.ts 契约：
/// `{"v":1,"enabled":bool,"pending":string|null}`，pending = 挂起计划正文）。
pub(crate) struct PlanBroadcast {
    pub enabled: bool,
    pub pending: Option<String>,
}

/// 手工解析（app 侧不引 serde derive，JSON 字段按契约直读）；非 JSON 或缺
/// enabled 字段返回 None（照旧走通用状态带路径，防御性）。
pub(crate) fn parse_plan_broadcast(text: &str) -> Option<PlanBroadcast> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(PlanBroadcast {
        enabled: v["enabled"].as_bool()?,
        pending: v["pending"].as_str().map(str::to_string),
    })
}

/// 随包分发的计划模式扩展（编译进二进制；dev 直接读工作区源文件）。
const PLAN_MODE_TS: &str = include_str!("plan_mode.ts");

/// present_plan 工具名：注册级 `--tools` allowlist 必须点名它
/// （tools_recipe A–H 组实测：不在名单的扩展工具连 getAllTools 都进不去）。
pub(crate) const PRESENT_PLAN_TOOL: &str = "present_plan";

/// plan_mode.ts 的磁盘绝对路径 —— `-e` 只吃真实文件路径，不吃内存。
///
/// 解析顺序（与 `session::tools_recipe::activate_script_path` 同机制）：
/// 1. `PI_FLASH_PLAN_MODE_TS` 显式覆盖（探针/调试）；
/// 2. 开发工作区源文件（编译期路径，改完即生效、无需重建）；
/// 3. 把内置内容落到 `~/.pi-flash/plan_mode.ts`（内容变了才写）——
///    release 包不带源码目录，靠这一步保证 `-e` 有路径可用。
pub(crate) fn script_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("PI_FLASH_PLAN_MODE_TS") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("mode")
        .join("plan_mode.ts");
    if dev.is_file() {
        return Some(dev);
    }
    let dir = pi_link::paths::ensure_dir()?;
    let path = dir.join("plan_mode.ts");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(PLAN_MODE_TS) {
        std::fs::write(&path, PLAN_MODE_TS).ok()?;
    }
    Some(path)
}

/// 全档位追加的挂载参数：`-e <plan_mode.ts>`（031-1 P0）。
pub(crate) fn mount_args() -> Vec<String> {
    match script_path() {
        Some(p) => vec!["-e".into(), p.to_string_lossy().into_owned()],
        None => {
            eprintln!(
                "plan-mode: plan_mode.ts 解析失败（无 dev 资产且 ~/.pi-flash 不可写）\
                 ——计划模式在本会话不可用"
            );
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn srcs(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn mount_args_pair_e_with_dev_script() {
        // dev 工作区必然带源文件（include_str! 同路径），除非显式覆盖 env
        // 指向别处——那种环境跳过精确断言，只验形状。
        if std::env::var("PI_FLASH_PLAN_MODE_TS").is_ok() {
            let args = mount_args();
            assert_eq!(args.len(), 2);
            assert_eq!(args[0], "-e");
            return;
        }
        let dev = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("mode")
            .join("plan_mode.ts");
        let args = mount_args();
        assert_eq!(args, srcs(&["-e", &dev.to_string_lossy()]));
    }

    #[test]
    fn embedded_script_declares_contract_surface() {
        // 内嵌件自检：三命令 + 工具 + 状态键都在源里（防止 include 指错文件）
        for needle in [
            "registerCommand(\"plan\"",
            "registerCommand(\"plan-approve\"",
            "registerCommand(\"plan-reject\"",
            "registerTool(",
            "pf-plan",
            "present_plan",
            "session_start",
        ] {
            assert!(PLAN_MODE_TS.contains(needle), "missing {needle}");
        }
    }
}
