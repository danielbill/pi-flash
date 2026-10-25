//! psp 项目级动作：选目录开项目（fresh 草稿 vs 恢复语义）、组内新会话、
//! 终端定向打开、删除项目及全部会话、psp 卡片行内改名（含未加载会话的
//! 临时进程发改名即收）。
//! （P2 归并：actions_panels 的 psp 侧。）

use crate::*;

impl Chat {
    /// psp 详情卡标题点击 → 原地改名（三行高输入区，2026-10-13 用户定夺）：
    /// 构建 rename_input + focus_soon（内件就绪即聚焦，select_all_on_focus
    /// 全选且光标锚起点）。标题点击处理器与 ui.debug_hover {session, rename}
    /// 自动化入口共用。
    pub(crate) fn hover_card_begin_rename(&mut self, cx: &mut Context<Self>) {
        let Some(card) = self.dlg.hover_card.as_mut() else {
            return;
        };
        let path = card.path.clone();
        // 标题口径与 hover_card 渲染一致：会话名优先，空则预览/占位
        let title = self
            .psp
            .projects
            .iter()
            .find_map(|g| g.sessions.iter().find(|s| same_path(&s.path, &path)))
            .map(|info| {
                info.name
                    .clone()
                    .filter(|n| !n.trim().is_empty())
                    .unwrap_or_else(|| {
                        if info.preview.is_empty() {
                            "(empty)".to_string()
                        } else {
                            info.preview.clone()
                        }
                    })
            })
            .unwrap_or_default();
        let weak_ok = cx.entity().downgrade();
        let weak_esc = weak_ok.clone();
        // 三行高输入区：会话名可长，单行放不下；auto_grow(3,3) 内件自算高
        let input = cx.new(|cx| crate::ui::TextInput::new(cx).lines(3).select_all_on_focus());
        input.update(cx, |ti, cx| ti.set_value(title, cx));
        input.update(cx, |ti, _cx| {
            let weak_ok = weak_ok.clone();
            let weak_esc = weak_esc.clone();
            let path_ok = path.clone();
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ok.update(cx, |c, cx| {
                    c.rename_card_commit(&path_ok, v.trim().to_string(), cx)
                });
            }));
            let path_esc = path.clone();
            ti.set_on_escape(Box::new(move |cx| {
                let _ = weak_esc.update(cx, |c, cx| c.rename_card_cancel(&path_esc, cx));
            }));
        });
        // 首发聚焦走 focus_soon（内件下一帧建好即聚焦）：fallback 句柄不在
        // 元素树里，直接 window.focus 是空路径——composer 白白 blur，输入框
        // 收不到焦点，全选要再点一下才发生（同文件树行内改名）
        input.update(cx, |ti, tcx| ti.focus_soon(tcx));
        if let Some(card) = self.dlg.hover_card.as_mut() {
            card.renaming = true;
            card.rename_input = Some(input);
        }
        cx.notify();
    }

    /// 打开项目菜单的【打开文件夹】：目录选择器 → 切换工作区。`fresh`
    /// （新会话页来源）= 选完落**全新草稿**（new_session_in，不恢复该目录
    /// 的 last_open）；psp 来源保持恢复上次会话的既定行为。
    pub(crate) fn pick_project_folder(&mut self, fresh: bool, cx: &mut Context<Self>) {
        let opts = gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        };
        let rx = cx.prompt_for_paths(opts);
        cx.spawn(async move |this, cx| {
            let picked = rx.await.ok().and_then(|r| r.ok()).flatten();
            let Some(mut paths) = picked else {
                return;
            };
            if let Some(dir) = paths.pop() {
                let _ = this.update(cx, |c, cx| {
                    if c.wx_setup_ctx() {
                        // 微信设置会话页：选项目写独立工作目录 wx_cwd（禁止联动）
                        c.wx_pick_project(dir, cx);
                    } else if fresh {
                        c.new_session_in(dir, cx);
                    } else {
                        c.switch_project(dir, cx);
                    }
                });
            }
        })
        .detach();
    }

    /// psp 项目行 ＋：切换到该项目并新建会话（组展开、选中）。
    pub(crate) fn new_session_in(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| same_path(cur, &path))
        {
            self.switch_project(path, cx);
        }
        self.new_session(cx);
        // send 门回填（010 重设计）：无项目态发来的文字，选定目录后落新
        // 草稿并把话说出去
        self.flush_pending_send(cx);
    }

    /// psp 项目菜单「在终端中打开」：以该项目为 cwd 开终端 + 切内容区 tab。
    pub(crate) fn open_terminal_in(
        &mut self,
        cwd: PathBuf,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.open_terminal(Some(cwd), window, cx);
        self.set_content_view(ContentView::Term);
        cx.notify();
    }

    /// psp「删除所有会话移除项目」：删除该项目全部会话文件（活跃会话先中止
    /// 并重置为新草稿），从 psp 移除该组。
    pub(crate) fn delete_project(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let sessions: Vec<PathBuf> = self
            .psp
            .projects
            .iter()
            .find(|g| same_path(&g.path, &path))
            .map(|g| g.sessions.iter().map(|s| s.path.clone()).collect())
            .unwrap_or_default();
        for p in &sessions {
            if self
                .pool
                .runtimes
                .get(&self.pool.active_key)
                .map(|rt| rt.read(cx).file.as_deref() == Some(p.as_path()))
                .unwrap_or(false)
            {
                self.new_session(cx);
            }
            if let Some(rt) = self.pool.runtimes.remove(&p.to_string_lossy().to_string()) {
                rt.update(cx, |r, _| {
                    r.rpc(&Command::Abort);
                    r.shutdown_process();
                });
            }
            let _ = std::fs::remove_file(p);
        }
        self.psp.projects.retain(|g| !same_path(&g.path, &path));
        // PF 项目注册表同步清册：删了的项目不再凭注册出现在两面板
        pi_link::projects::remove_project(&path);
        if self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| same_path(cur, &path))
        {
            // 当前项目被删：清当前组数据，保留草稿会话
            self.pool.sessions.clear();
            self.sync_current_sessions();
        }
        self.psp
            .unread
            .retain(|p| sessions.iter().all(|s| !same_path(s, p)));
        self.psp
            .turn_errors
            .retain(|p| sessions.iter().all(|s| !same_path(s, p)));
        self.psp
            .turn_warnings
            .retain(|p| sessions.iter().all(|s| !same_path(s, p)));
        // 项目整个没了，其下会话的置顶记录同步清册（020 会话置顶）
        self.psp
            .session_pins
            .retain(|p| !sessions.iter().any(|s| same_path(s, p)));
        self.set_status(
            crate::i18n::tf("psp.project_deleted", &[("n", sessions.len().to_string())]),
            cx,
        );
        cx.notify();
    }
}

impl Chat {
    /// psp 详情卡改名提交（Enter）：对目标会话的 runtime 发
    /// SetSessionName；未加载的会话临时 spawn 进程发完即收。名字先乐观
    /// 写入内存，扫描器随后从会话文件回读确认。
    pub(crate) fn rename_card_commit(
        &mut self,
        path: &PathBuf,
        name: String,
        cx: &mut Context<Self>,
    ) {
        self.dlg.hover_card = None;
        if name.is_empty() {
            cx.notify();
            return;
        }
        let name_for_status = name.clone();
        let key = path.to_string_lossy().to_string();
        match self.pool.runtimes.get(&key).cloned() {
            Some(rt) => {
                rt.update(cx, |r, _| {
                    r.rpc(&Command::SetSessionName { name: name.clone() });
                });
            }
            None => {
                // 未加载的会话：tail 渲染 + 临时进程发改名，不切换活跃会话
                let tail = match self.pool.session_tail_cache.get(path).cloned() {
                    Some(msgs) => msgs,
                    None => msgs_from_tail(read_tail_messages(path, 256 * 1024, 100)),
                };
                let cwd = self
                    .psp
                    .projects
                    .iter()
                    .find(|g| g.sessions.iter().any(|s| same_path(&s.path, path)))
                    .map(|g| g.path.clone())
                    .or_else(|| self.ws.cwd.clone());
                let Some(cwd) = cwd else {
                    return;
                };
                let rt = cx.new(|_| {
                    let mut r =
                        session::runtime::SessionRuntime::new(key.clone(), cwd, Some(path.clone()));
                    r.default_model = services::workspace::default_model_pref();
                    // 秒开深度优化（pi-flash-2o92）：磁盘计数改后台——主线程
                    // 全文件读是 O(会话大小) 的冻结源，回灌走 load_disk_msg_count
                    r.messages = tail;
                    r.pager.reload(r.messages.len());
                    r
                });
                self.pool.runtimes.insert(key, rt.clone());
                self.subscribe_runtime(&rt, cx);
                rt.update(cx, |r, cx| r.load_disk_msg_count(cx));
                let rt_spawn = rt.clone();
                cx.spawn(async move |_this, cx| {
                    let _ = rt_spawn.update(cx, |r, cx| {
                        if r.agent.session.is_none() {
                            if let Some(rx) = r.spawn() {
                                let epoch = r.agent.epoch;
                                session::runtime::SessionRuntime::attach_pump(
                                    &rt_spawn, rx, epoch, cx,
                                );
                            }
                        }
                        r.rpc(&Command::SetSessionName { name: name.clone() });
                    });
                    // 改名发完即收进程（消息保留，下次打开秒开）
                    let _ = rt_spawn.update(cx, |r, _| r.shutdown_process());
                })
                .detach();
            }
        }
        // 乐观更新内存中的标题
        for g in &mut self.psp.projects {
            for s in &mut g.sessions {
                if same_path(&s.path, path) {
                    s.name = Some(name_for_status.clone());
                }
            }
        }
        // 不发「已改名」状态提示（2026-10-13 用户定夺）：改名框原地生效，
        // 列表标题即时可见，提示冗余
        cx.notify();
    }

    /// psp 详情卡改名取消（Esc）。
    pub(crate) fn rename_card_cancel(&mut self, _path: &PathBuf, cx: &mut Context<Self>) {
        if let Some(card) = self.dlg.hover_card.as_mut() {
            card.renaming = false;
            card.rename_input = None;
        }
        cx.notify();
    }

    /// 改名中点卡外 = 取消（Esc 同语义：不提交）并收卡。卡是 hover 态浮层，
    /// 鼠标已不在卡上，留着等于一张钉死的空壳，直接收掉。
    pub(crate) fn hover_card_rename_dismiss(&mut self, cx: &mut Context<Self>) {
        self.dlg.hover_card = None;
        cx.notify();
    }
}
