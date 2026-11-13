//! psp 项目级动作：选目录开项目（fresh 草稿 vs 恢复语义）、组内新会话、
//! 终端定向打开、删除项目及全部会话、psp 卡片行内改名（PF 台账落账 +
//! 在场进程 pi 同步，session_names）。
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

    /// 会话引用 chip token：输入以已知会话全路径（/ 归一形态）开头时返回
    /// (path, title) 供 composer 隐藏原始路径、展示 图标+会话名（ZCode
    /// parity）。title 口径与会话详情卡一致：会话名优先，空则预览/占位。
    /// 前缀门控：非路径形态（盘符/UNC 开头）不扫清单——普通打字零开销。
    pub(crate) fn session_token_for_input(&self) -> Option<(String, String)> {
        let input = self.composer.input.as_str();
        let b = input.as_bytes();
        let plausible = b.len() >= 2
            && ((b[0].is_ascii_alphanumeric() && b[1] == b':') || input.starts_with("//"));
        if !plausible {
            return None;
        }
        for g in &self.psp.projects {
            for s in &g.sessions {
                let p = s.path.to_string_lossy().replace('\\', "/");
                let hit = input.len() >= p.len()
                    && input.starts_with(&p)
                    && (input.len() == p.len() || input.as_bytes()[p.len()] == b' ');
                if hit {
                    let title = crate::session::menu::session_title(s).to_string();
                    return Some((p, title));
                }
            }
        }
        None
    }

    /// 会话卡「@到会话输入框」：会话可跨项目，不能走文件 @（tree_at_insert
    /// 按当前 cwd 剥前缀、且绑定 at 索引补全语义）；@ 前缀特指项目内短
    /// 路径，这里插**会话文件全路径纯文本**（不带 @，/ 分隔，后续空格收
    /// 尾）。收卡 + 内容区切回会话，让插入可见。
    pub(crate) fn hover_card_at_insert(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.dlg.hover_card = None;
        let full = path.to_string_lossy().replace('\\', "/");
        let mut base = std::mem::take(&mut self.composer.input);
        if !base.is_empty() && !base.ends_with(' ') && !base.ends_with('\n') {
            base.push(' ');
        }
        base.push_str(&full);
        let cursor = base.len() + 1;
        base.push(' ');
        self.set_input_with_cursor(base, cursor, cx);
        self.set_content_view(ContentView::Chat);
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
                    if fresh {
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
            // 会话文件删了：tab 同步摘牌（与 delete_session 同口径）
            let gone = p.to_string_lossy().to_string();
            self.pool.tab_labels.remove(&gone);
            if self.pool.primary_key == gone {
                self.pool.primary_key.clear();
            }
            while let Some(ix) = self
                .ws
                .panel_tabs
                .iter()
                .position(|t| matches!(t, crate::PanelTab::Observer { key } if *key == gone))
            {
                self.close_panel_tab(ix, cx);
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
    /// psp 详情卡改名提交（Enter）：PF 台账即时落账（显示层真值，
    /// session_names），pi 侧只对**在场进程**尽力同步一条 set_session_name
    /// ——冷会话不再为改名 spawn 临时进程（pi-flash-n3k8 方案 B
    /// pi-flash-pkhr，2026-11-13 定案；旧「发完即杀」竞态链整链删除）。
    /// 名字乐观写入内存，列表随后随 ListDirty 重扫走台账 overlay 同值。
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
        pi_link::session_names::set(path, &name);
        let key = path.to_string_lossy().to_string();
        if let Some(rt) = self.pool.runtimes.get(&key).cloned() {
            // 进程在场才同步；rpc 内部判无进程返回 false，台账已兜住显示
            rt.update(cx, |r, _| {
                r.rpc(&Command::SetSessionName { name: name.clone() });
            });
        }
        // 乐观更新内存中的标题：其他项目组的缓存行不随刷新重建，靠这步
        // 即时可见（当前项目组随后重扫，overlay 同值不打架）
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
