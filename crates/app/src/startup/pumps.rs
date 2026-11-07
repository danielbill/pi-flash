//! 启动阶段的后台任务与揭幕闸门（010-启动.md §9；从 startup.rs 外移）。

use gpui::AppContext as _;

use super::{MIN_SPLASH, SPLASH_TIMEOUT, splash_paint_age};
use crate::Chat;

// ---------------------------------------------------------------------------
// §9 启动阶段的后台任务与揭幕闸门（Chat::new 只调下面两个入口）
// ---------------------------------------------------------------------------

/// 启动期后台任务 + 揭幕闸门（010-启动.md §9）。`Chat::new` 只调这一个入口，
/// 启动阶段做了什么全在这一节里可见：
/// 1. 120ms 泵：悬停卡 / 导航 flyout / 状态条过期
/// 2. 3s 外部追加观察：非空闲 runtime 的文件被外部（pi-web / CLI）追加时重读
/// 3. 60s 空闲回收：非活动会话进程超 10 分钟未动就杀掉
/// 4. 30s recents 对账：外部写入者的活跃时间回灌清单
/// 5. 启动页闸门：pi 附着（或恢复已定案/超时兜底）+ 最短展示 500ms → 揭幕
/// 6. 恢复最后会话 + 进入其项目 + 会话清单装载 = spawn_boot_restore
///    （010 重设计：进程附着随恢复路径走，无项目态零进程，不再有首帧附着）
pub(crate) fn spawn_boot_tasks(cx: &mut gpui::Context<Chat>) {
    // 051：pif-secret 入用户级 PATH + auth.json 标记去路径化（后台、毫秒级、
    // 幂等）。必须放所有后台任务**最前**——升级交接的 cleanup 线程可能先删
    // 旧版本载荷目录，标记迁移要抢先改写指向旧目录的条目（0.4.0→0.4.1 auth
    // 全崩事故的治本）。
    std::thread::spawn(|| {
        if pi_link::register_to_sys::ensure_installed() {
            match pi_link::auth_json::migrate_markers_pathless() {
                Ok(0) => {}
                Ok(n) => {
                    eprintln!("[startup] auth.json 标记去路径化：{n} 条 → !pif-secret 裸命令")
                }
                Err(e) => eprintln!("[startup] auth.json 标记迁移跳过（拒写）：{e}"),
            }
        }
        // 081：内置 pi 内核注册为系统全局 `pi`（release 默认开，debug 需
        // PI_FLASH_PI_SHIM=1）——指针自愈指向当前载荷（搬家/手装后也归位）。
        // 幂等成功是常态，成功行仅 debug 打（release 零输出门禁）。
        if let Some(cli) = pi_link::vendor::cli_path() {
            let node = pi_link::vendor::node_bin();
            if pi_link::register_to_sys::ensure_pi_shim(&node, &cli) {
                #[cfg(debug_assertions)]
                eprintln!("[startup] 全局 pi 已注册 → {cli:?}");
            }
        }
        // 044 profile 目录迁移（2026-11-06）：旧 `~/.pi/agent/agents/*.md`
        // copy 进 PF 自有目录（不覆盖已存在，幂等）——pi 市场社区扩展共用
        // 旧目录（命名空间污染），profile schema 归 PF 私有，数据也归位。
        pi_link::agents::migrate_legacy_agents();
    });
    spawn_pump_120ms(cx);
    spawn_external_append_watch(cx);
    spawn_idle_recycle(cx);
    spawn_recents_poll(cx);
    spawn_splash_gate(cx);
    spawn_fs_watch_pump(cx);
    // syntect 语法引擎冷加载 ~百 ms 挪出首个 md 渲染帧（第一次渲染卡顿）：
    // 后台线程预热 OnceLock，命中后首渲零成本
    std::thread::spawn(|| crate::editor::markdown::warm_up());
    // PF 插件清单扫描（beads 360 ①；02 §4：启动只承担清单，store/ctx 懒加载
    // ——微秒级/个，后台落位后状态栏 tab / 右键【插件】即出现）
    crate::pf_plugin::spawn_plugin_scan(cx);
}

/// fs 监听泵（Zed worktree 扫描器的轻量对应物）：watcher 事件经后台
/// 去抖线程合批（FS_WATCH_LATENCY ≈ 100ms 静默期 + 批内一次抽干），
/// gpui 侧 150ms 轮询信号 → 重扫展开目录 + git 状态。线程与 executor
/// 各干各的活，互不阻塞。
fn spawn_fs_watch_pump(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        let rx = match this.update(cx, |c, _| c.ws.fs_watch_rx.take()) {
            Ok(Some(rx)) => rx,
            _ => return,
        };
        let _ = this.update(cx, |c, _| c.attach_fs_watch());

        // 去抖线程：首批事件后进入静默期循环，静默 100ms 才放行一次信号
        let (ui_tx, ui_rx) = std::sync::mpsc::channel::<()>();
        std::thread::spawn(move || {
            while rx.recv().is_ok() {
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    let mut more = false;
                    while rx.try_recv().is_ok() {
                        more = true;
                    }
                    if !more {
                        break;
                    }
                }
                if ui_tx.send(()).is_err() {
                    break; // UI 侧已亡
                }
            }
        });

        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(150))
                .await;
            if ui_rx.try_recv().is_err() {
                continue;
            }
            let ok = this
                .update(cx, |chat, cx| {
                    chat.refresh_git(cx);
                    // 023：同一信号顺带做打开文件的外部改动检测
                    //（无未保存修改自动重载；有修改标冲突等用户裁决）
                    chat.check_external_file_changes(cx);
                    cx.notify();
                })
                .is_ok();
            if !ok {
                break;
            }
        }
    })
    .detach();
}

/// 首帧后的进程附着已退役（010 重设计）：附着随恢复路径走——open_session
/// 自带 spawn + GetMessages/anchors/refresh_state，无项目态零进程。

/// 120ms 泵：导航 flyout / 状态条过期（光标闪烁由输入组件自管，
/// 不再需要 2Hz 切换；会话详情卡 2026-10-15 起改右键召唤，无定时逻辑）。
fn spawn_pump_120ms(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        let mut svc_tick: u32 = 0;
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(120))
                .await;
            // 执行域 outbox 泵（90-6）：本循环 120ms 一拍，泵按 ~360ms 抽一次
            svc_tick += 1;
            if svc_tick % 3 == 0 {
                let _ = this.update(cx, |c, cx| crate::pf_plugin::svc::svc_pump(c, cx));
            }
            // 插件弹窗 1s 重取（超时白遮/新码/开关态）：同样挂这条长跑循环
            let _ = this.update(cx, |c, cx| c.plugin_popup_tick(cx));
            let ok = this
                .update(cx, |c, cx| {
                    let mut dirty = false;
                    // 导航 flyout 250ms 离开宽限（pi-web PREVIEW_HIDE_DELAY parity）
                    if c.layout.nav_open {
                        if let Some(at) = c.layout.nav_hide_at {
                            if at.elapsed() > std::time::Duration::from_millis(250) {
                                c.layout.nav_open = false;
                                c.layout.nav_flyout_hovered = false;
                                c.layout.nav_hover_turn = None;
                                c.layout.nav_hide_at = None;
                                dirty = true;
                            }
                        }
                    }
                    // 子代理扩展菜单同款 250ms 宽限：flyout/菜单 hover 离开挂
                    // deadline，进菜单（menu on_hover）/回卡（nav_card hover）
                    // 取消——到期且鼠标不在菜单上才清锚收菜单
                    if let Some(p) = c.dlg.subagent_panel.as_mut() {
                        if let Some(at) = p.menu_hide_at {
                            if at.elapsed() > std::time::Duration::from_millis(250)
                                && !p.menu_hovered
                            {
                                p.menu_anchor = None;
                                p.menu_hide_at = None;
                                dirty = true;
                            }
                        }
                    }
                    if let Some((_, at)) = &c.dlg.status_toast {
                        if at.elapsed() > std::time::Duration::from_millis(2500) {
                            c.dlg.status_toast = None;
                            dirty = true;
                        }
                    }
                    if dirty {
                        cx.notify();
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

/// 3s 外部追加观察（pi-web session-revision parity）：非空闲 runtime 的文件被
/// 外部写入（pi-web / CLI 写同一会话）时重读。运行中的 agent 自己持有文件，
/// 由 `check_external_append` 内部判断。
fn spawn_external_append_watch(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(3))
                .await;
            let ok = this
                .update(cx, |chat, cx| {
                    for rt in chat.pool.runtimes.values() {
                        rt.update(cx, |r, cx| r.check_external_append(cx));
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

/// 60s 空闲回收（pi-web idle-timeout parity）：非活动会话空闲超 10 分钟就
/// 杀掉 pi 进程，并连 runtime 实体一起退役——transcript（messages 含工具
/// 结果全文、system_msgs 含整份系统提示词）随实体释放，是长跑内存的
/// 大头；清单/psp 标记不动，回开走 open_session 的 tail 秒开路径（磁盘
/// 尾巴 + 后台重挂），与冷启动打开旧会话同一条路。活跃会话永不退役。
fn spawn_idle_recycle(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(60))
                .await;
            let ok = this
                .update(cx, |chat, cx| {
                    let idle_cap = std::time::Duration::from_secs(600);
                    // 先收集后摘除：退役即 drop 实体，不在遍历中动 map
                    let mut retire: Vec<String> = Vec::new();
                    for (key, rt) in &chat.pool.runtimes {
                        if *key == chat.pool.active_key {
                            continue;
                        }
                        // 执行域豁免（90-6）：该会话挂着绑定中的插件实例时，
                        // pi 进程 = 轮询器本体；回收它 = 远程消息没人收、
                        // ui.json 的绑定指针却还在 → icon 假绿（2026-10-30
                        // 真机「发消息收不到」主因之一）。
                        if chat.has_live_service(key, cx) {
                            continue;
                        }
                        let idle = {
                            let r = rt.read(cx);
                            !r.agent_running
                                && r.last_activity.elapsed() > idle_cap
                                && r.agent.session.is_some()
                                // 阻塞件未兑现不得退役（切回要浮卡兑现）；
                                // fire-and-forget 队尾残留不拦退役——旧口径
                                // 整队非空即拦，服务会话 park 期间猛发
                                // setStatus 后进程永不退休（2026-11-05 黄点同源）
                                && r.ext_queue.iter().all(|q| !q.method.is_blocking())
                        };
                        if idle {
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

/// 30s recents 对账（003-session管理）：外部写入者（pi-web / CLI）改了会话文件
/// 就把活跃时间回灌清单并落盘。活着的 runtime 的文件排除在外——它们的活跃时间
/// 由事件路径维护，轮询会在 pi 追加时反复重扫。
fn spawn_recents_poll(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(30))
                .await;
            let exclude = match this.update(cx, |chat, cx| {
                chat.pool
                    .runtimes
                    .values()
                    .filter_map(|rt| rt.read(cx).file.clone())
                    .collect::<Vec<_>>()
            }) {
                Ok(v) => v,
                Err(_) => break,
            };
            let changed = cx
                .background_spawn(async move { pi_link::recents::poll_recent_sessions(&exclude) })
                .await;
            if changed {
                let _ = this.update(cx, |_chat, cx| cx.notify());
            }
        }
    })
    .detach();
}

/// 启动页闸门（010-启动.md §6 第 7 步）：pi 附着（或 `SPLASH_TIMEOUT` 兜底）
/// 且最短展示 `MIN_SPLASH` 之后揭幕；揭幕帧由 render 的 `pending_zoom` 补最大化。
fn spawn_splash_gate(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        let t0 = std::time::Instant::now();
        loop {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(60))
                .await;
            let (attached, boot_resolved) = this
                .update(cx, |c, cx| {
                    let attached = c
                        .pool
                        .runtimes
                        .get(&c.pool.active_key)
                        .is_some_and(|r| r.read(cx).agent.session.is_some());
                    (attached, c.pool.boot_resolved)
                })
                .unwrap_or((true, true)); // 实体已亡（窗口关闭）：停止闸门
            let elapsed = t0.elapsed();
            // MIN_SPLASH 从启动页首帧起算（忙机首帧可能晚于 pi 附着）
            let splash_shown = splash_paint_age().is_some_and(|age| age >= MIN_SPLASH);
            // 无项目态没有可附着的进程：boot_resolved 直接放行，不等超时
            if ((attached || boot_resolved) && splash_shown) || elapsed >= SPLASH_TIMEOUT {
                break;
            }
        }
        let _ = this.update(cx, |c, cx| {
            c.pool.booted = true;
            c.pool.pending_zoom = true;
            // 040 延迟加载：揭幕（切换启动页）后才开始测扩展/skill 说明
            // token——非关键数据，不对启动造成任何负担
            c.spawn_token_probe(cx);
            // 010-启动.md §4.1（2026-11-01 恢复）：启动后台 sweep 已配置 key
            // 的内置商模型目录 → 并进全局态 + 回写 catalog-cache（重启仍在）。
            // 网络探测走后台线程，失败静默，不阻塞揭幕后的任何交互
            c.spawn_builtin_catalog_sweep(cx);
            // 081 延迟加载：新版本首启开「更新日志」tab + 后台检查更新
            // （timeout 30s、失败重试 2 次间隔 10min，全程后台线程不阻塞）
            c.spawn_startup_update_hooks(cx);
            // 082 延迟加载：Pi 扩展更新检测（只读 + 网络，零写入；结果进
            // aux.plugin_updates，新会话页通知与设置·扩展页消费）
            c.spawn_pi_extension_updates_check(cx);
            // 040-1 延迟加载：Pi 扩展市场预热——只预取首页，且**只在已有
            // 首页快照时**才发请求（从没用过市场页 = 零请求），见
            // market/prefetch.rs
            // 052 延迟加载：会话自动清理（后排任务——扫描+回收站搬运全在
            // 后台线程，主线程只做状态摘除；0 天 = 关闭）
            c.spawn_session_auto_clean(cx);
            if crate::PERF.load(std::sync::atomic::Ordering::Relaxed) {
                eprintln!("[perf] splash reveal: {:?}", t0.elapsed());
            }
            cx.notify();
        });
    })
    .detach();
}
