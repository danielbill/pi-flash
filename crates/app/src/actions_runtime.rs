//! Runtime subscription + session switching (pool).

//! Split out of main.rs for the file-size budget. Child module of the
//! crate root: Chat's root-private fields stay accessible here.

use crate::*;

impl Chat {
    pub(crate) fn subscribe_runtime(
        &mut self,
        rt: &gpui::Entity<session::runtime::SessionRuntime>,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(rt, |chat, rt, ev: &session::runtime::SessionEvent, cx| {
            use session::runtime::SessionEvent;
            let is_active = rt.read(cx).key == chat.active_key;
            match ev {
                SessionEvent::Changed => {
                    // recents (003-session管理): message-level activity on
                    // this runtime is "recently updated" — promote its file
                    // in the recent-activity list (persist throttled)
                    if let Some(f) = rt.read(cx).file.clone() {
                        pi_link::recents::touch_recent(&f);
                    }
                    if !is_active {
                        // v54 未读绿点：非活跃会话在跑/在流式 → 记未读，
                        // 切换到它时清除
                        let (file, running) = {
                            let r = rt.read(cx);
                            (
                                r.file.clone(),
                                r.agent_running
                                    || r.state.as_ref().is_some_and(|s| s.is_streaming),
                            )
                        };
                        if let Some(f) = file {
                            if running && !chat.unread.contains(&f) {
                                chat.unread.insert(f);
                            }
                        }
                    }
                    cx.notify();
                }
                SessionEvent::ListDirty => {
                    chat.refresh_sessions();
                    cx.notify();
                }
                SessionEvent::ExtUi(req) => {
                    // 微信端与桌面端**并行**拿到同一请求（060 §4.1）：两边都要
                    // 渲染一份文本，但只有一方能应答 —— 桌面 ext_respond 会
                    // clear_pending，pi 侧也只 resolve 一次（§8 档 2 不重复消费）
                    chat.remote.on_ext_ui(req);
                    if is_active {
                        let req = req.clone();
                        chat.on_ext_ui(req, cx);
                    } else {
                        // parked session asks for permission: queue it, the
                        // badge surfaces it; popped when user switches there
                        rt.update(cx, |r, _| r.ext_queue.push(req.clone()));
                        cx.notify();
                    }
                }
                SessionEvent::Models(models) => {
                    // 进程答案写项目槽（pi-web /api/models parity：每个 runtime 只写
                    // 自己 cwd 的条目），同时**并进全局态并回写自有缓存**
                    // （010-启动.md §2/§4.1：包内联 provider 只有进程答得到，
                    // 存下来下次冷启动就有真名字）。
                    let cwd_key = rt.read(cx).cwd.to_string_lossy().to_string();
                    chat.models_by_cwd.insert(cwd_key, models.clone());
                    if crate::startup::merge_models(&mut chat.globals, &models) {
                        let _ = crate::startup::write_cache(
                            &chat.globals.models,
                            &chat.globals.commands,
                        );
                    }
                    chat.mc_state =
                        models_config::compute_state(chat.mc_patterns.as_ref(), &chat.mc_refs());
                    cx.notify();
                }
                SessionEvent::Commands(commands) => {
                    // 扩展命令（包内注册，磁盘没有数据文件）：并进全局清单 + 回写缓存。
                    // `skill:` 前缀不入全局（skill 走磁盘派生，见 startup::merge_commands）。
                    if crate::startup::merge_commands(&mut chat.globals, &commands) {
                        let _ = crate::startup::write_cache(
                            &chat.globals.models,
                            &chat.globals.commands,
                        );
                    }
                    cx.notify();
                }
                SessionEvent::FileBound(path) => {
                    chat.on_file_bound(&rt, path.clone(), cx);
                }
                SessionEvent::RenameReady(prefill) => {
                    if is_active {
                        if let Some(f) = rt.read(cx).file.clone() {
                            chat.start_rename(f, prefill.clone(), cx);
                        }
                    }
                }
            }
        })
        .detach();
    }

    /// Attention switch: flush the editor mirrors into the outgoing
    /// session, load the incoming one's inputPanel state. Zero process
    /// operations, zero IO — this is the pi-web "switch" and it is instant.
    pub(crate) fn switch_to(
        &mut self,
        rt: gpui::Entity<session::runtime::SessionRuntime>,
        cx: &mut Context<Self>,
    ) {
        if let Some(old) = self.runtimes.get(&self.active_key).cloned() {
            if old != rt {
                let (input, images, history) = (
                    std::mem::take(&mut self.input),
                    std::mem::take(&mut self.pending_images),
                    std::mem::take(&mut self.history),
                );
                old.update(cx, |r, _| {
                    r.input = input;
                    r.pending_images = images;
                    r.history = history;
                });
            }
        }
        let (input, images, history, key, file) =
            rt.update(cx, |r, _| {
                (
                    r.input.clone(),
                    r.pending_images.clone(),
                    r.history.clone(),
                    r.key.clone(),
                    r.file.clone(),
                )
            });
        self.input = input;
        self.pending_images = images;
        self.history = history;
        self.history_ix = None;
        self.active_key = key;
        self.active_file = file.clone();
        if let Some(f) = &file {
            set_last_open(&self.cwd.to_string_lossy(), &f.to_string_lossy());
            // v54: 切入会话清除未读
            self.unread.remove(f);
        }
        self.pill_menu = None;
        self.menu_ix = 0;
        // surface queued permission requests of the incoming session
        let queued = rt.update(cx, |r, _| {
            let q = std::mem::take(&mut r.ext_queue);
            r.touch();
            q
        });
        if let Some(first) = queued.first() {
            let first = first.clone();
            cx.notify();
            self.on_ext_ui(first, cx);
            for r in queued.into_iter().skip(1) {
                rt.update(cx, |r2, _| r2.ext_queue.push(r));
            }
        }
        cx.notify();
    }
}
