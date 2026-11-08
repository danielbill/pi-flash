//! 044-0 子代理模块设置（设置-子代理页「全局设置」行）：模块开关 / 最大并发
//! 的读取、落盘与扩展传导。嵌套深度 2026-11-07 用户拍板恒定 1，UI 移除、
//! 存量设置值不再读取。自 app_settings.rs 切出（itlq.9 超规拆分）。
//!
//! **001-S1 起读写走 pf-context**（app-settings.json `subagents` section，
//! typed + 单写者 50ms coalesce + watcher，见 docs/模块设计/001-PFContext/）。
//! 过渡期事实源链（001-1 §6）：
//! - PF 读：v2 `subagents` section（hub 为准）> `agents/config.json`
//!   （2026-11-08 用户定夺「json 参数为准」，手编文件仍受尊重）> v1 平铺键
//!   `subagent_capacity`/`subagent_enabled`（hub 代读）> 缺省 3；
//! - 扩展读：`agents/config.json`（每次委派重读）——设置页改动**双写**它
//!   （[`pi_link::agents::set_capacity`]），未升级扩展可用；S3 起改投影段；
//! - env 值传导退役（`PI_SUBAGENTS_CAPACITY`/`PI_SUBAGENTS_MAX_DEPTH` 已删）：
//!   env 是进程级快照，压过文件且改了要重启——001-0 §1 的 bug 根源；现版
//!   扩展每次委派重读 config.json，只保留路径类 env。
//!
//! 001-S2 起 `save_app_settings` 是 diff 薄壳、subagents 段不经过它——
//! 陈值回写风险已随整份重建写入路径一起消失，无需内存镜像同步。

use super::*;

use pf_context::sections::Subagents;

/// 001-S1：settings 外部/程序化变更 → UI 重绘标记（120ms 泵消费，见
/// startup/pumps.rs）。watcher 重载与 hub.set 都会广播，这里只置标志。
static SETTINGS_RELOAD_PENDING: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

pub(crate) fn mark_settings_reload() {
    SETTINGS_RELOAD_PENDING.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// 120ms 泵取用：有未消化广播则触发一帧全局重绘（外部编辑器改设置 2s 内生效）。
pub(crate) fn take_settings_reload() -> bool {
    SETTINGS_RELOAD_PENDING.swap(false, std::sync::atomic::Ordering::Relaxed)
}

/// 子代理模块开关（设置-子代理页「全局设置」行，2026-11-07 从纯开发旗标
/// 升级为 UI 开关；缺省开）。值在 hub `subagents.enabled`（存量 v1 平铺键
/// `subagent_enabled` 由 hub 的 legacy 读迁移代读）。
pub fn subagent_module_switch() -> bool {
    pf_context::hub().settings.get::<Subagents>().enabled
}

/// 子代理模块运行时生效开关 = 开发旗标（总闸，PI_FLASH_SUBAGENTS 可强关）
/// && 用户面开关。spawn 工具清单 / 子代理面板缓存都看这里；设置页导航可见性
/// 只看旗标——关了开关页面得留在原地，才有地方重新打开。
pub fn subagent_module_active() -> bool {
    subagents_enabled() && subagent_module_switch()
}

/// 子代理最大并发（设置-子代理页「全局设置」行）。
///
/// 过渡期读链（见模块注释）：hub 的 v2 `subagents` section 命中 = hub 为准
/// （validate 已钳 2-10）；否则 `agents/config.json` 为准（2026-11-08 用户
/// 定夺，手编文件、残留 `capacity.default` 都在这里生效）；再退 v1 平铺键
/// （hub 代读）与缺省 3。首次在设置页改动后 hub 即为唯一事实源。
pub fn subagent_capacity() -> u64 {
    let settings = &pf_context::hub().settings;
    if settings.origin::<Subagents>() == pf_context::Origin::Section {
        return settings.get::<Subagents>().capacity;
    }
    pi_link::agents::capacity()
        .map(|v| v.clamp(2, 10))
        .unwrap_or_else(|| settings.get::<Subagents>().capacity)
}

/// 子代理模块设置 → 扩展 env。**只剩路径类两枚**（001-S3 起 env 白名单收口，
/// 001-2 §5：PI_SUBAGENTS_CAPACITY / PI_SUBAGENTS_MAX_DEPTH / PI_FLASH_APP_
/// SETTINGS 全部退役——env 是进程级快照压过文件且改了要重启；现版扩展每次
/// 委派实时重读 `ext-context.json` 投影 → legacy config.json 回退链）。
/// 模块运行时开关关着时不传导（委派停用，env 无意义）。
pub fn subagent_envs() -> Vec<(String, String)> {
    if !subagent_module_active() {
        return Vec::new();
    }
    vec![
        // profile 全局目录（PF 自有，2026-11-06 迁出 ~/.pi/agent/agents——
        // pi 市场社区扩展共用旧目录，schema 撞车）。扩展 defaultProfilePaths
        // 的兜底只在非 PF 宿主挂载时生效。
        (
            "PI_SUBAGENTS_PROFILES_DIR".into(),
            pi_link::agents::agents_dir().to_string_lossy().into_owned(),
        ),
        // PF 数据目录（001-2 §5 路径类白名单）：扩展由此定位 ext-context.json
        // 投影。子进程同时继承父进程 env——PI_FLASH_DIR / PI_FLASH_PI_BIN 在
        // 父进程显式设置时自动传递，这里补的是「未显式设置 = 默认目录」情形。
        (
            "PI_FLASH_DIR".into(),
            pi_link::paths::dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        ),
    ]
}

/// 044-0 模块设置：设子代理最大并发。
///
/// 双写过渡（001-1 §6）：先落 `agents/config.json`（扩展契约，返回**实际
/// 生效值**——文件里更窄的 `capacity.min/max` 会夹住请求值），再进 hub（内存
/// 立即换 → 单写者 50ms 合并 → 原子写 `subagents` section + v1 平铺键）。
/// config.json 写失败仍按请求值进 hub（PF 侧意图保留，日志可见，下次改动
/// 再试）；clamp 唯一落点 = section `validate` 与 config 写入内部，这里不再钳。
pub fn set_subagent_capacity(v: u64) {
    let effective = match pi_link::agents::set_capacity(v) {
        Ok(effective) => effective,
        Err(e) => {
            eprintln!("[subagent] 写 agents/config.json 失败: {e}");
            v
        }
    };
    pf_context::hub()
        .settings
        .set::<Subagents>(|s| s.capacity = effective);
}

/// 用户面模块开关写入（设置-子代理页「全局设置」行）。进 hub（同上管线，
/// 不涉 config.json——开关是 PF 私有概念）。运行中 pi 进程的工具清单要到
/// 下次 spawn 才变。
pub fn set_subagent_module_switch(on: bool) {
    pf_context::hub()
        .settings
        .set::<Subagents>(|s| s.enabled = on);
}
