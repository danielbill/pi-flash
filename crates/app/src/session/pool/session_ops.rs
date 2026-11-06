//! 会话池操作：新建（草稿/清记忆）、文件绑定回灌、标签推导、池内换绑
//! 入口与工作区重挂载。

use super::*;

impl Chat {
    /// 无项目态占位会话：恒在、永不 spawn、非插件功能线。`shell::chat` 建它，
    /// 「关掉功能页回桌面」也回它——不再到处写字面量 `"draft-0"`。
    pub(crate) const PLACEHOLDER_KEY: &'static str = "draft-0";

    /// 新会话草稿（用户显式新建：psp ＋、新建按钮、删会话后归位）。
    /// `clear_last_open` 跟在这里：用户明确要空白会话 ⇒ 该项目不再恢复旧会话。
    pub(crate) fn new_session(&mut self, cx: &mut Context<Self>) {
        self.new_session_with(cx, true);
    }

    /// 启动落草稿（restore=off / 该项目无会话）：**不清 per-project 会话
    /// 记忆**——旧会话还在盘上，下次开 restore 仍应能直接加载（010）。
    pub(crate) fn boot_new_session(&mut self, cx: &mut Context<Self>) {
        self.new_session_with(cx, false);
    }

    fn new_session_with(&mut self, cx: &mut Context<Self>, clear_memory: bool) {
        // 无项目态防御：new_session 只在有项目的上下文被调（composer 发送
        // 已在 send 门处先要目录；切项目/恢复路径 cwd 已就位）
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        // lazy draft (pi-web parity): the pi process spawns on the first
        // prompt, not now; running sessions are untouched
        let key = format!("draft-{}", self.pool.draft_seq);
        self.pool.draft_seq += 1;
        let rt = cx.new(|_| {
            let mut r = session::runtime::SessionRuntime::new(key.clone(), cwd.clone(), None);
            r.default_model = services::workspace::default_model_pref();
            r.status = tr("session.new").to_string();
            r
        });
        self.pool.runtimes.insert(key.clone(), rt.clone());
        pi_link::client::ext_trace(&format!("spawn-draft: key={key}"));
        self.subscribe_runtime(&rt, cx);
        if clear_memory {
            clear_last_open(&cwd.to_string_lossy());
        }
        self.dlg.renaming = None;
        self.dlg.rename_input = None;
        self.switch_to(rt, cx);
    }

    /// Draft persisted its first prompt: pi bound this process to a fresh
    /// session file (pi-web promoteNewSession parity). Migrate the pool key
    /// draft-N → path so the sidebar entry and the pool share one identity,
    /// otherwise reopening the session would spawn a duplicate runtime.
    pub(crate) fn on_file_bound(
        &mut self,
        rt: &gpui::Entity<session::runtime::SessionRuntime>,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let path_key = path.to_string_lossy().to_string();
        let old_key = rt.read(cx).key.clone();
        if old_key == path_key {
            return;
        }
        // 伪标签到此退役：磁盘扫描现在找得到真文件，真标签接管
        self.psp.pending_tabs.retain(|e| e.key != old_key);
        rt.update(cx, |r, _| r.key = path_key.clone());
        // 会话级插件清单跟着身份走（草稿 key → 会话文件路径）：确认时按草稿
        // key 落盘，这里把它搬到真实会话键上，重启后按会话恢复（034）
        if let Some(store) = pi_link::session_ext::store_path() {
            let sources = pi_link::session_ext::read_for(&store, &old_key);
            if !sources.is_empty() {
                let _ = pi_link::session_ext::write_for(&store, &path_key, &sources);
            }
            let _ = pi_link::session_ext::remove_for(&store, &old_key);
        }
        // 执行域锚点注册表随身份搬家（token 不变 → 插件侧绑定指针跨迁移稳定，90-6 §2）
        if let Some(base) = pi_link::paths::dir() {
            let _ = pi_link::service_ext::move_key(&base, &old_key, &path_key);
        }
        // #问题卡不弹：bind 迁移碰撞 = 已证明发生的运行时替换源——若目标键
        // 已被占，旧实体会从这里被顶出 map（其 pi 仍活着），后续事件/请求
        // 全部落到被顶实体或双实体并存，必须留痕
        if self.pool.runtimes.contains_key(&path_key) {
            pi_link::client::ext_trace(&format!(
                "bind COLLISION: {path_key} already held — entity replaced"
            ));
        }
        if let Some(entity) = self.pool.runtimes.remove(&old_key) {
            self.pool.runtimes.insert(path_key.clone(), entity);
        }
        if self.pool.active_key == old_key {
            self.pool.active_key = path_key.clone();
        }
        // tab 身份跟随迁移（draft-N → 会话文件键）：主会话指针、标题快照、
        // 自由区观察窗 tab 的 key（草稿标题「新会话」在下次 switch_to 刷真名）
        if self.pool.primary_key == old_key {
            self.pool.primary_key = path_key.clone();
        }
        for t in &mut self.ws.panel_tabs {
            if let crate::PanelTab::Observer { key } = t {
                if *key == old_key {
                    *key = path_key.clone();
                }
            }
        }
        if let Some(label) = self.pool.tab_labels.remove(&old_key) {
            self.pool.tab_labels.insert(path_key.clone(), label);
        }
        pi_link::client::ext_trace(&format!(
            "bind: {old_key} -> {path_key} active_follow={}",
            self.pool.active_key == path_key
        ));
        // 插件认领线跟随迁移（同语义，泛化形态）：草稿键 → 会话文件键，
        // 过滤清单按真身补一条（path 精确，重启后仍藏得住）；交接文件同步
        if let Some(c) = self
            .plugins
            .claims
            .iter_mut()
            .find(|c| c.key == old_key)
            .map(|c| (c.key.clone(), c.prefix.clone(), c.plugin_id.clone()))
        {
            let (_old, prefix, plugin_id) = c;
            self.plugins
                .claims
                .iter_mut()
                .find(|c| c.plugin_id == plugin_id)
                .map(|c| c.key = path_key.clone());
            let name = std::path::Path::new(&path_key)
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned());
            pi_link::hidden_sessions::set_owned(
                &plugin_id,
                name.as_deref(),
                Some(&path),
                Some(&prefix),
            );
            // 交接文件同步（ext 读的是 key，草稿键已退役）
            if let Some(e) = self
                .plugins
                .plugins
                .iter()
                .find(|e| e.manifest.id == plugin_id)
            {
                let f = e.manifest.data_dir().join("claim.json");
                if f.exists() {
                    let _ = std::fs::write(
                        &f,
                        serde_json::json!({"key": path_key, "name": name.as_deref().unwrap_or(""), "cwd": "", "prefix": prefix, "plugin_id": plugin_id})
                            .to_string(),
                    );
                }
            }
        }
        self.psp.active_file = Some(path.clone());
        // recents (003-session管理): a fresh session file just landed
        pi_link::recents::touch_recent(&path);
        // 插件认领线不记 last_open（002：影子会话永不进桌面恢复线）
        if !self.plugin_claimed(&path_key) {
            set_last_open(&rt.read(cx).cwd.to_string_lossy(), &path_key);
        }
        // 新会话落地的项目组若处于折叠态则自动展开：伪标签（draft）在折叠
        // 组里照常显示，FileBound 一换真身就被折叠藏掉——用户眼里刚发的
        // 会话凭空消失。fork / 远程端新会话同经此处（2026-10-15 用户报障）。
        self.psp
            .collapsed_keys
            .remove(&same_ws_key(&rt.read(cx).cwd.to_string_lossy()));
        self.refresh_sessions();
        cx.notify();
    }

    /// 池内直切（会话行点击/伪标签行/自动化 session.switch）：目标会话属
    /// 别的项目时先重挂工作区再切注意力——switch_to 本身是纯注意力切换，
    /// 跨项目场景绕过 open_session 的入口都必须走这里（用户实测
    /// 2026-10-13：池命中跨项目只改 cwd 不重建，树/变更面板停在上一项目）。
    /// 会话显示名：优先会话列表里的现成名字/预览（psp 索引），退化为文件名
    /// 时间片（2026-11-04T07-47-54-411Z_… → 07-47-54）；draft 键原样。
    pub(super) fn session_label_of(chat: &Chat, key: &str) -> String {
        for g in &chat.psp.projects {
            for si in &g.sessions {
                if crate::services::workspace::same_path(&si.path, std::path::Path::new(key)) {
                    if let Some(n) = si.name.clone().filter(|n| !n.trim().is_empty()) {
                        return n;
                    }
                    if !si.preview.is_empty() {
                        return si.preview.chars().take(18).collect();
                    }
                }
            }
        }
        let stem = std::path::Path::new(key)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(key);
        if stem.len() >= 19 && stem.as_bytes()[4] == b'-' {
            // ISO 时间戳文件名：截时刻 07-47-54
            stem[11..19].to_string()
        } else {
            stem.to_string()
        }
    }

    pub(crate) fn switch_pool_rt(
        &mut self,
        rt: gpui::Entity<session::runtime::SessionRuntime>,
        cx: &mut Context<Self>,
    ) {
        let cwd = rt.read(cx).cwd.clone();
        if !self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| same_path(cur, &cwd))
        {
            self.ws.cwd = Some(cwd);
            self.remount_workspace(cx);
        }
        self.switch_to(rt, cx);
    }

    /// 跨项目切换的工作区重挂（cwd 已由调用方写入 self.ws.cwd）：树根/
    /// git/文件树/fs watcher/cwd 外单文件监听全套跟着新 cwd 重建。
    /// open_session 的池命中与新运行时两条路径共用。
    pub(crate) fn remount_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        // ① 归档旧项目的文件树状态（展开集合 + Pin 视图）：对齐 Zed 的
        // per-worktree 内存表，切回来不丢；同时落盘（`__tree`）
        self.archive_tree_state();
        // ①.5 「进入项目」即 touch 项目注册表：切项目三条路（switch_project /
        //     点会话行 open_session / 树选文件 sync_tree_to_file）都要算活动，
        //     否则最近活动项目会停在别处（用户实测：点观宏知微的会话后重启
        //     回到 pi-flash——那条路漏了 touch）
        pi_link::projects::register_project(&cwd);
        self.ws.branch = read_branch(&cwd);
        // ② Pin 收藏清单先装载（下面恢复树状态时要判断 Pin 视图是否可用）
        self.ws.pinned = crate::services::workspace::pins_for(&cwd.to_string_lossy())
            .into_iter()
            .map(PathBuf::from)
            .collect();
        // ③ 文件树展开集合：内存 → 磁盘 → 默认「只展开根」（对齐 Zed：打开
        //    项目默认展开 cwd）；Pin 视图同样按项目恢复（关闭前开着 = 重启
        //    还开），但收藏为空时无意义（照旧退回文件树）
        self.restore_tree_state(&cwd);
        self.refresh_sessions();
        self.load_project_files();
        // 9rk0：refresh_git 的 rebuild_tree 随数据后台换入（~150ms），这里先
        // 用空 git 数据同步展平新 cwd 的树，否则换项目瞬间文件树残留旧项目
        self.rebuild_tree();
        // 换工作区：git 状态/树缓存/fs watcher 全部跟着 cwd 重挂
        //（此前这里漏 refresh_git，git 面板会显示上一个项目的状态）
        self.refresh_git(cx);
        self.attach_fs_watch();
        // cwd 变了：cwd 外单文件监听面跟着重算（对齐 switch_project）
        self.retain_file_watches();
    }
}
