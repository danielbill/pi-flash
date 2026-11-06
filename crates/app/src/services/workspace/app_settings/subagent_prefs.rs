//! 044-0 子代理模块设置（设置-子代理页「模块设置」）：最大并发 / 嵌套深度
//! 的读取、钳制、落盘与扩展 env 传导。自 app_settings.rs 切出（itlq.9 超规
//! 拆分，纯结构搬移）。

use super::*;

/// 044-0 模块设置默认值：最大并发 3（glm 侧限 3）、嵌套深度 1（禁止套娃）。
pub const SUBAGENT_CAPACITY_DEFAULT: u64 = 3;
pub const SUBAGENT_MAX_DEPTH_DEFAULT: u64 = 1;

/// 子代理最大并发（设置-子代理页「模块设置」；钳 2-5，缺省 3）。
pub fn subagent_capacity() -> u64 {
    app_settings()
        .subagent_capacity
        .unwrap_or(SUBAGENT_CAPACITY_DEFAULT)
        .clamp(2, 5)
}

/// 子代理嵌套深度（钳 1-3，缺省 1）。
pub fn subagent_max_depth() -> u64 {
    app_settings()
        .subagent_max_depth
        .unwrap_or(SUBAGENT_MAX_DEPTH_DEFAULT)
        .clamp(1, 3)
}

/// 子代理模块设置 → 扩展 env（PI_SUBAGENTS_CAPACITY / PI_SUBAGENTS_MAX_DEPTH，
/// pf-subagents index.ts numberFromEnv 读；PI_FLASH_APP_SETTINGS = 设置文件
/// 绝对路径，扩展**每次委派实时重读**——env 是进程级快照，只传死值会让
/// 设置变更永远等不到下一次 spawn pi，真机 2026-11-06：改 3 后 4 并发照跑）。
/// subagents 旗标关着时不传导（扩展不加载，env 无意义）。
pub fn subagent_envs() -> Vec<(String, String)> {
    if !subagents_enabled() {
        return Vec::new();
    }
    let mut envs = vec![
        (
            "PI_SUBAGENTS_CAPACITY".into(),
            subagent_capacity().to_string(),
        ),
        (
            "PI_SUBAGENTS_MAX_DEPTH".into(),
            subagent_max_depth().to_string(),
        ),
        // profile 全局目录（PF 自有，2026-11-06 迁出 ~/.pi/agent/agents——
        // pi 市场社区扩展共用旧目录，schema 撞车）。扩展 defaultProfilePaths
        // 的兜底只在非 PF 宿主挂载时生效。
        (
            "PI_SUBAGENTS_PROFILES_DIR".into(),
            pi_link::agents::agents_dir().to_string_lossy().into_owned(),
        ),
    ];
    if let Some(p) = pi_link::paths::app_settings_file() {
        envs.push((
            "PI_FLASH_APP_SETTINGS".into(),
            p.to_string_lossy().into_owned(),
        ));
    }
    envs
}

/// 044-0 模块设置：设子代理最大并发（钳 2-5；spawn 时 env 传导）。
pub fn set_subagent_capacity(v: u64) {
    with_app_settings_cache(|cache| {
        let mut s = (*cache).clone().unwrap_or_default();
        s.subagent_capacity = Some(v.clamp(2, 5));
        *cache = Some(s);
    });
    save_app_settings(&app_settings());
}

/// 044-0 模块设置：设子代理嵌套深度（钳 1-3；spawn 时 env 传导）。
pub fn set_subagent_max_depth(v: u64) {
    with_app_settings_cache(|cache| {
        let mut s = (*cache).clone().unwrap_or_default();
        s.subagent_max_depth = Some(v.clamp(1, 3));
        *cache = Some(s);
    });
    save_app_settings(&app_settings());
}
