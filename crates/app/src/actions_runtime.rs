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
                    if is_active {
                        chat.available_models = rt.read(cx).available_models.clone();
                    } else {
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
                    // psp 状态槽: 维护运行集合（任何 runtime 的流式状态）
                    let mut running: Vec<(PathBuf, bool)> = Vec::new();
                    for (k, rt) in &chat.runtimes {
                        let r = rt.read(cx);
                        if let Some(f) = &r.file {
                            let on = r.agent_running
                                || r.state.as_ref().is_some_and(|s| s.is_streaming);
                            running.push((f.clone(), on));
                        }
                        let _ = k;
                    }
                    for (f, on) in running {
                        if on {
                            chat.running_files.insert(f);
                        } else {
                            chat.running_files.remove(&f);
                        }
                    }
                    cx.notify();
                }
                SessionEvent::ListDirty => {
                    chat.refresh_sessions();
                    cx.notify();
                }
                SessionEvent::ExtUi(req) => {
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
