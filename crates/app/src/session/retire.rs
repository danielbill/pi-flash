//! 047-内存削减：空闲退役独立模块——进程 + runtime 实体 + transcript
//! 一起弃（回开走 open_session 的磁盘 tail 秒开路径，与冷启动同一条路）。
//!
//! 定位：这是**内存回收**（何时释放），不是内存管理全域——预算化缓存、
//! 虚拟化、节流等管理纪律仍在各自域（047 文档钉口径），本模块只管退役
//! 决策与执行。
//!
//! 口径（2026-11-24 拍板）：
//! - 参数**模块内定，不进 app_settings**——内存策略不是用户决策面；
//! - **双窗**：通用会话 [`IDLE_RETIRE_SECS`]（600s）；定时任务会话
//!   [`TASK_IDLE_RETIRE_SECS`]（30s）——任务结算后驱动体内存无消费者
//!   且页签已关（046-T：任务页签开着 = 页面上有消费者，用户可能回来
//!   插话，按通用窗待遇；关了才落回 30s 快窗），30s 即弃，零 UX 回退；
//!   回看/续聊走 [`Chat::open_task_session`] 可交互重开（不再走 observer）；
//! - 任务会话**跳过**扩展阻塞件保护窗：任务无输入通道，阻塞件永远无人
//!   兑现（放弃是必然），留着只会把 30s 快窗钉回保护窗时长；
//! - 豁免闸不变：`agent_running`（跑着不动）、`has_live_service`（090-6
//!   插件轮询会话，杀进程 = 远程消息没人收 → icon 假绿）、`active_key`
//!   （前台永不退役）、无进程不退役（observer 观察窗永不 spawn，天然豁免
//!   ——驻留期 = tab 打开期，释放靠 close_observer_tab 纯 drop）。

use std::time::Duration;

use super::runtime::ExtPending;
use crate::Chat;

// ---------------------------------------------------------------------------
// 参数（模块内配置——非用户设置；改动需带 047 文档同步）
// ---------------------------------------------------------------------------

/// 通用空闲退役窗（秒）：非 active 用户会话空闲超过此值即连实体退役。
pub(crate) const IDLE_RETIRE_SECS: u64 = 600;
/// 定时任务会话退役窗（秒，046）：结算后 30s 即弃。
pub(crate) const TASK_IDLE_RETIRE_SECS: u64 = 30;
/// 扩展阻塞件保护窗（秒，047 P0-4）：对齐通用窗——超过一个空闲周期未兑现
/// = 泵死扩展或用户已放弃，不再钉死退役。
pub(crate) const EXT_BLOCKING_TTL_SECS: u64 = 600;
/// 泵 tick（秒）：≤ 任务窗，保证 30s 快窗不被 tick 粒度稀释到翻倍以上。
pub(crate) const TICK_SECS: u64 = 30;

/// 双窗口径：任务派发铸造的会话（`task_owned`）默认走任务窗；页签开着
/// 时（`task_tab_open`，046-T）按通用窗待遇——页面上有消费者。其余走
/// 通用窗。
pub(crate) fn idle_cap_for(task_owned: bool) -> Duration {
    if task_owned {
        Duration::from_secs(TASK_IDLE_RETIRE_SECS)
    } else {
        Duration::from_secs(IDLE_RETIRE_SECS)
    }
}

// ---------------------------------------------------------------------------
// 退役闸
// ---------------------------------------------------------------------------

/// 扩展阻塞件拦退役闸（自 session/runtime/mod.rs 迁入）：无保护窗内的未兑现
/// 阻塞件即放行；fire-and-forget 从不拦（黄点事故口径）。消费方 = [`retire_due`]。
pub(crate) fn ext_queue_allows_retire(q: &[ExtPending]) -> bool {
    let ttl = Duration::from_secs(EXT_BLOCKING_TTL_SECS);
    q.iter()
        .all(|p| !p.req.method.is_blocking() || p.at.elapsed() >= ttl)
}

/// 退役判定（纯函数，单测钉死）：`idle` = `last_activity.elapsed()`。
/// 硬闸（运行中 / 活插件 / 无进程）先挡，再看窗口，最后阻塞件保护窗
/// （任务页签关着的任务会话跳过——见模块头注释）。`task_tab_open` =
/// 该会话有 [`crate::PanelTab::Task`] 页签开着（046-T）：任务窗只在页签
/// 关闭后生效，开着按通用窗待遇（阻塞件保护窗同样恢复）。
pub(crate) fn retire_due(
    task_owned: bool,
    task_tab_open: bool,
    agent_running: bool,
    has_process: bool,
    live_service: bool,
    idle: Duration,
    ext_queue: &[ExtPending],
) -> bool {
    let fast = task_owned && !task_tab_open;
    if agent_running || live_service || !has_process {
        return false;
    }
    if idle <= idle_cap_for(fast) {
        return false;
    }
    fast || ext_queue_allows_retire(ext_queue)
}

// ---------------------------------------------------------------------------
// 泵（自 startup/pumps.rs 迁入：机制跟域走，启动只负责拉起）
// ---------------------------------------------------------------------------

/// 空闲退役泵：`TICK_SECS` 一跳，先收集后摘除（退役即 drop 实体，不在遍历
/// 中动 map）。transcript（消息含工具结果全文、system_msgs 含整份系统提示词）
/// 随实体释放，是长跑内存的大头；清单/psp 标记/tab 标题快照不动。
pub(crate) fn spawn_idle_retire(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(Duration::from_secs(TICK_SECS))
                .await;
            let ok = this
                .update(cx, |chat, cx| {
                    let mut retire: Vec<String> = Vec::new();
                    for (key, rt) in &chat.pool.runtimes {
                        if *key == chat.pool.active_key {
                            continue; // 活跃会话永不退役
                        }
                        let due = {
                            // 执行域豁免（90-6）：该会话挂着绑定中的插件实例
                            // 时，pi 进程 = 轮询器本体；回收它 = 远程消息没人
                            // 收、ui.json 的绑定指针却还在 → icon 假绿
                            //（2026-10-30 真机「发消息收不到」主因之一）。
                            // 046-T 页签开合先行查（不可变借用，顺序在
                            // has_live_service 的 &mut 之前）。
                            let tab_open = chat.ws.panel_tabs.iter().any(
                                |t| matches!(t, crate::PanelTab::Task { key: k } if *k == *key),
                            );
                            let live_service = chat.has_live_service(key, cx);
                            let r = rt.read(cx);
                            retire_due(
                                r.task_owned,
                                tab_open,
                                r.agent_running,
                                r.agent.session.is_some(),
                                live_service,
                                r.last_activity.elapsed(),
                                &r.ext_queue,
                            )
                        };
                        if due {
                            retire.push(key.clone());
                        }
                    }
                    for key in retire {
                        if let Some(rt) = chat.pool.runtimes.remove(&key) {
                            rt.update(cx, |r2, _| r2.shutdown_process());
                            // rt 在此 drop：transcript 随实体释放；
                            // session_tail_cache 不动，供秒开回切
                        }
                    }
                })
                .is_ok();
            if !ok {
                break;
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(blocking: bool, age: Duration) -> Vec<ExtPending> {
        let method = if blocking {
            pi_link::protocol::ExtUiMethod::Select {
                title: "t".into(),
                options: vec!["a".into()],
            }
        } else {
            pi_link::protocol::ExtUiMethod::Notify {
                message: "m".into(),
                notify_type: None,
            }
        };
        vec![ExtPending {
            at: std::time::Instant::now() - age,
            req: pi_link::protocol::ExtensionUiRequest {
                id: "r1".into(),
                method,
            },
        }]
    }

    #[test]
    fn dual_idle_windows() {
        assert_eq!(idle_cap_for(false), Duration::from_secs(600));
        assert_eq!(idle_cap_for(true), Duration::from_secs(30));
    }

    #[test]
    fn hard_gates_block_regardless_of_idle() {
        let long = Duration::from_secs(3600);
        assert!(!retire_due(false, false, true, true, false, long, &[]));
        assert!(!retire_due(false, false, false, true, true, long, &[]));
        // 无进程不退役：observer 观察窗永不 spawn，天然豁免——
        // 驻留期 = tab 打开期，释放靠 close_observer_tab。
        assert!(!retire_due(false, false, false, false, false, long, &[]));
    }

    #[test]
    fn general_window_respects_blocking_ttl() {
        let fresh = Duration::from_secs(599);
        let stale = Duration::from_secs(601);
        // 通用窗内不退
        assert!(!retire_due(false, false, false, true, false, fresh, &[]));
        // 通用窗外 + 新鲜阻塞件拦（P0-4）
        assert!(!retire_due(
            false,
            false,
            false,
            true,
            false,
            stale,
            &q(true, Duration::ZERO)
        ));
        // 通用窗外 + 无阻塞件放行
        assert!(retire_due(false, false, false, true, false, stale, &[]));
    }

    #[test]
    fn task_window_discards_fast_and_skips_blocking_ttl() {
        // 任务窗内不退（哪怕刚好过通用窗的一半）
        assert!(!retire_due(
            true,
            false,
            false,
            true,
            false,
            Duration::from_secs(29),
            &[]
        ));
        // 任务窗外即退——新鲜阻塞件不拦（页签关着 = 无消费者，放弃是必然）
        assert!(retire_due(
            true,
            false,
            false,
            true,
            false,
            Duration::from_secs(31),
            &q(true, Duration::ZERO)
        ));
    }

    #[test]
    fn task_tab_open_upgrades_to_general_window() {
        // 046-T：页签开着 = 有消费者，按通用窗待遇（阻塞件保护窗同步恢复）
        let fresh = Duration::from_secs(599);
        let stale = Duration::from_secs(601);
        assert!(!retire_due(true, true, false, true, false, fresh, &[]));
        // 通用窗外 + 新鲜阻塞件拦（页签开着恢复保护窗）
        assert!(!retire_due(
            true,
            true,
            false,
            true,
            false,
            stale,
            &q(true, Duration::ZERO)
        ));
        // 通用窗外 + 无阻塞件放行
        assert!(retire_due(true, true, false, true, false, stale, &[]));
    }
}
