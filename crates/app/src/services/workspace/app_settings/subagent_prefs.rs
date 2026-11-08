//! 044-0 子代理模块设置（设置-子代理页「全局设置」行）：模块开关 / 最大
//! 并发的读取、钳制、落盘与扩展 env 传导。嵌套深度 2026-11-07 用户拍板
//! 恒定 1，UI 移除、存量设置值不再读取。自 app_settings.rs 切出（itlq.9
//! 超规拆分，纯结构搬移）。

use super::*;

/// 044-0 模块设置默认值：最大并发 3、嵌套深度 1（禁止套娃）。
/// 并发缺省值同时用于「`agents/config.json` 缺失时补一份」——扩展自己的
/// FALLBACK 是 8，不补齐会与设置页默认 3 静默分叉。
pub const SUBAGENT_CAPACITY_DEFAULT: u64 = 3;
pub const SUBAGENT_MAX_DEPTH_DEFAULT: u64 = 1;

/// 子代理模块开关（设置-子代理页「全局设置」行，2026-11-07 从纯开发旗标
/// 升级为 UI 开关；缺省开）。落盘 app_settings `subagent_enabled`。
pub fn subagent_module_switch() -> bool {
    app_settings().subagent_enabled.unwrap_or(true)
}

/// 子代理模块运行时生效开关 = 开发旗标（总闸，PI_FLASH_SUBAGENTS 可强关）
/// && 用户面开关。spawn 工具清单 / 子代理面板缓存 / env 传导都看这里；
/// 设置页导航可见性只看旗标——关了开关页面得留在原地，才有地方重新打开。
pub fn subagent_module_active() -> bool {
    subagents_enabled() && subagent_module_switch()
}

/// 子代理最大并发（设置-子代理页「全局设置」行；钳 2-10，缺省 3）。
///
/// **事实源 = `<PF数据目录>/agents/config.json`**（2026-11-08 用户定夺
/// 「以 json 参数为准」）：扩展每次委派重读那份文件，设置页每次改动实时
/// 写它（[`set_subagent_capacity`]）。文件缺失/损坏才回退 app-settings 的
/// 存量镜像值（老版本只写那里），再退缺省 3。
pub fn subagent_capacity() -> u64 {
    pi_link::agents::capacity()
        .map(|v| v.clamp(2, 10))
        .unwrap_or_else(|| {
            app_settings()
                .subagent_capacity
                .unwrap_or(SUBAGENT_CAPACITY_DEFAULT)
                .clamp(2, 10)
        })
}

/// 子代理嵌套深度：恒定 1（禁止套娃，2026-11-07 用户拍板，UI 已移除）。
/// 存量 app_settings `subagent_max_depth` 不再读取；env 仍显式传 1，
/// 扩展侧语义不变。
pub fn subagent_max_depth() -> u64 {
    SUBAGENT_MAX_DEPTH_DEFAULT
}

/// 子代理模块设置 → 扩展 env（PI_SUBAGENTS_CAPACITY / PI_SUBAGENTS_MAX_DEPTH，
/// pf-subagents index.ts numberFromEnv 读；PI_FLASH_APP_SETTINGS = 设置文件
/// 绝对路径，扩展**每次委派实时重读**——env 是进程级快照，只传死值会让
/// 设置变更永远等不到下一次 spawn pi，真机 2026-11-06：改 3 后 4 并发照跑）。
/// 模块运行时开关关着时不传导（委派停用，env 无意义）。
/// 子代理模块设置 → 扩展 env。
///
/// 现版扩展（`extensions/pf-subagents`）只认 `<profiles dir>/config.json`
/// （每次委派重读）与 `PI_SUBAGENTS_PROFILES_DIR`；`PI_SUBAGENTS_CAPACITY`
/// / `PI_SUBAGENTS_MAX_DEPTH` / `PI_FLASH_APP_SETTINGS` 保留给旧版扩展
/// （发布包里的旧构建仍读 env），新构建忽略。
pub fn subagent_envs() -> Vec<(String, String)> {
    if !subagent_module_active() {
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

/// 044-0 模块设置：设子代理最大并发（钳 2-10）。
///
/// 先写 `agents/config.json`（唯一事实源，扩展下次委派即按新值放行），
/// 再落 app-settings 作存量镜像/回退。写入返回的实际值可能被文件里更窄的
/// `capacity.min/max` 夹住，镜像按实际值存，避免两处再次分叉。
pub fn set_subagent_capacity(v: u64) {
    let v = v.clamp(2, 10);
    let effective = match pi_link::agents::set_capacity(v) {
        Ok(effective) => effective,
        Err(e) => {
            eprintln!("[subagent] 写 agents/config.json 失败: {e}");
            v
        }
    };
    with_app_settings_cache(|cache| {
        let mut s = (*cache).clone().unwrap_or_default();
        s.subagent_capacity = Some(effective);
        *cache = Some(s);
    });
    save_app_settings(&app_settings());
}

/// 用户面模块开关写入（设置-子代理页「全局设置」行；立即落盘）。运行中
/// pi 进程的工具清单要到下次 spawn 才变；env 传导当轮委派即停。
pub fn set_subagent_module_switch(on: bool) {
    with_app_settings_cache(|cache| {
        let mut s = (*cache).clone().unwrap_or_default();
        s.subagent_enabled = Some(on);
        *cache = Some(s);
    });
    save_app_settings(&app_settings());
}
