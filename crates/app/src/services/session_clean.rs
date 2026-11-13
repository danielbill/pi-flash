//! 052 会话自动清理：启动延迟加载的后排任务（docs/模块设计/052-会话管理.md）。
//!
//! 「其他」页的【自动清理会话】天数（`auto_clean_days`，默认 3，0 = 关闭，
//! 钳制 [0,100]）生效处：揭幕后扫描 `~/.pi/agent/sessions` 里最后活动
//! （mtime，与 recents/加载窗口同口径）超出天数的会话，**移入回收站**
//! （项目铁律：删除须进回收站防误删——自动删除无人确认，更要可恢复）。
//!
//! 线程纪律：扫描只是 read_dir + stat、删除走 trash crate，全在后台线程；
//! 主线程收尾只做内存状态摘除（清单/缓存/置顶/插件清单/recents），
//! 不碰 IO、绝不阻塞。
//!
//! 保护名单（永不清理）：
//! - 置顶会话（设计明文：pin 的意义就是老会话常驻）
//! - 本进程所有 runtime 持有的会话——活跃那个正被用户看着；后台 runtime
//!   还握着 jsonl 追加句柄，Windows 上回收站搬运也必失败，干脆不挑起

use std::path::PathBuf;

use crate::Chat;
use crate::services::workspace::same_path;

impl Chat {
    /// 052 延迟加载入口：揭幕后调用（spawn_splash_gate 揭幕帧，与
    /// token_probe / 更新检查同批）。days = 0 直接返回。
    pub(crate) fn spawn_session_auto_clean(&mut self, cx: &mut gpui::Context<Self>) {
        let days = crate::services::workspace::auto_clean_days();
        if days == 0 {
            return;
        }
        // 保护名单快照（pin + 全部 runtime 持有的会话文件）
        let mut protect: Vec<PathBuf> = self.psp.session_pins.clone();
        for rt in self.pool.runtimes.values() {
            if let Some(f) = rt.read(cx).file.clone() {
                protect.push(f);
            }
        }
        if let Some(f) = self.psp.active_file.clone() {
            protect.push(f);
        }
        cx.spawn(async move |this, cx| {
            let removed = cx
                .background_executor()
                .spawn(async move {
                    let mut removed = Vec::new();
                    let Some(root) = pi_link::sessions::sessions_root() else {
                        return removed;
                    };
                    let cutoff = std::time::SystemTime::now()
                        - std::time::Duration::from_secs(days * 86_400);
                    for path in pi_link::sessions::stale_session_files(&root, cutoff) {
                        if protect.iter().any(|p| same_path(p, &path)) {
                            continue;
                        }
                        // 单文件失败（句柄被占等）只跳过该会话，不中断整批
                        if trash::delete(&path).is_ok() {
                            removed.push(path);
                        }
                    }
                    removed
                })
                .await;
            if removed.is_empty() {
                return;
            }
            let n = removed.len();
            let _ = this.update(cx, |c, cx| {
                for path in &removed {
                    // 以下与 pool::delete_session 的状态清理同一清单，只是
                    // 不涉及 runtime 拆卸（被保护名单挡住，绝不删活的）：
                    // 会话没了，自定义插件清单也清掉（034）
                    if let Some(store) = pi_link::session_ext::store_path() {
                        let _ = pi_link::session_ext::remove_for(&store, &path.to_string_lossy());
                    }
                    // recents（003-session管理）：文件没了，条目摘除
                    pi_link::recents::remove_recent(path);
                    // PF 名台账同步摘行（session_names/pi-flash-pkhr）
                    pi_link::session_names::remove(path);
                    c.psp.unread.remove(path);
                    c.psp.turn_errors.remove(path);
                    c.psp.turn_warnings.remove(path);
                    // 置顶清册兜底（保护名单已挡，理论上是 no-op）
                    c.psp.session_pins.retain(|p| !same_path(p, path));
                    c.pool.sessions.retain(|s| &s.path != path);
                    c.pool.session_tail_cache.remove(path);
                    // 非当前项目的会话也要从各组缓存摘掉，否则列表一直显示
                    for g in &mut c.psp.projects {
                        g.sessions.retain(|s| &s.path != path);
                    }
                }
                c.set_status(
                    crate::i18n::tf("session.autoclean_done", &[("n", n.to_string())]),
                    cx,
                );
                c.refresh_sessions();
                cx.notify();
            });
        })
        .detach();
    }
}
