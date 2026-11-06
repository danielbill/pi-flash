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
/// pf-subagents index.ts numberFromEnv 读）。subagents 旗标关着时不传导
/// （扩展不加载，env 无意义）。
pub fn subagent_envs() -> Vec<(String, String)> {
    if !subagents_enabled() {
        return Vec::new();
    }
    vec![
        (
            "PI_SUBAGENTS_CAPACITY".into(),
            subagent_capacity().to_string(),
        ),
        (
            "PI_SUBAGENTS_MAX_DEPTH".into(),
            subagent_max_depth().to_string(),
        ),
    ]
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
