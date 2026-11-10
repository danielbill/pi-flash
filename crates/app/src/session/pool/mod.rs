//! 会话池 Chat 方法域：new/open/delete/switch/subscribe_runtime + 会话改名。
//! （P2 归并：actions_sessions + actions_runtime + actions_rename 池侧；
//! SessionRuntime 本体在 session/runtime.rs。）

use crate::*;

// itlq.5 按域拆分（只搬不改）：runtime_link = pi 子进程挂接与池切换主路；
// session_ops = 新建/绑定/重挂载；open = 打开/删除/页签；本文件 = 改名流
// 与刷新/项目切换/杂项供给。
pub(crate) mod open;
pub(crate) mod runtime_link;
pub(crate) mod session_ops;

impl Chat {
    pub(crate) fn start_rename(&mut self, path: PathBuf, prefill: String, cx: &mut Context<Self>) {
        let weak_ok = cx.entity().downgrade();
        let weak_esc = cx.entity().downgrade();
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .select_all_on_focus()
                .placeholder(tr("action.session_name"))
        });
        input.update(cx, |ti, cx| ti.set_value(prefill, cx));
        input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ok.update(cx, |c, cx| c.apply_rename(v.trim().to_string(), cx));
            }));
            ti.set_on_escape(Box::new(move |cx| {
                let _ = weak_esc.update(cx, |c, cx| {
                    c.dlg.renaming = None;
                    c.dlg.rename_input = None;
                    cx.notify();
                });
            }));
        });
        self.dlg.renaming = Some(path);
        self.dlg.rename_input = Some(input);
        cx.notify();
    }

    /// Rename commit path that never reads the input entity (called from
    /// the input's own submit callback where the entity is borrowed).
    pub(crate) fn apply_rename(&mut self, name: String, cx: &mut Context<Self>) {
        self.rt().read(cx).rpc(&Command::SetSessionName { name });
        // sidebar label comes from the session file's `session_info` entry;
        // reload it when pi confirms the write (set_session_name response —
        // an immediate re-read here would race the file flush)
        self.refresh_state(cx);
        self.dlg.renaming = None;
        self.dlg.rename_input = None;
        cx.notify();
    }
}
impl Chat {
    pub(crate) fn refresh_state(&self, cx: &mut gpui::App) {
        self.rt().update(cx, |r, _| r.refresh_state());
    }

    pub(crate) fn refresh_sessions(&mut self) {
        // 无项目态：没有「当前项目会话清单」可言（psp 窗口分组照常）
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let cwd = cwd.to_string_lossy().to_string();
        // 影子会话（插件认领线）不进任何列表：过滤清单由插件自己注入
        let hidden = pi_link::hidden_sessions::load();
        let full = list_sessions_for_cwd(&cwd, 500)
            .into_iter()
            .filter(|s| same_ws(&s.cwd, &cwd))
            .filter(|s| !hidden.hides(s.name.as_deref(), &s.path))
            .collect::<Vec<_>>();
        // v60: 加载语义 = 时间窗口（设置.加载时间窗口）——当前项目只保留
        // 窗口内活跃的会话（mtime 近似清单 last_active）+ 当前激活会话
        // 兜底（保证改名/打开的旧会话始终可见）；显示量由 psp 分页控制
        let cutoff = std::time::SystemTime::now()
            - std::time::Duration::from_secs(load_window_days() * 86_400);
        let mut sessions: Vec<SessionInfo> = full
            .iter()
            .filter(|s| s.modified >= cutoff)
            .cloned()
            .collect();
        if let Some(active) = &self.psp.active_file {
            if !sessions.iter().any(|s| &s.path == active) {
                if let Some(info) = full.iter().find(|s| &s.path == active) {
                    sessions.push(info.clone());
                }
            }
        }
        // 置顶兑底（020 会话置顶）：当前项目的置顶会话哪怕跌出加载时间
        // 窗口也要在列表里——pin 的意义就是老会话常驻可见，对齐上面的
        // active 兑底；其他项目的置顶会话沿用各组启动时装入的缓存
        for pin in &self.psp.session_pins {
            if !sessions.iter().any(|s| same_path(&s.path, pin))
                && let Some(info) = full.iter().find(|s| same_path(&s.path, pin))
            {
                sessions.push(info.clone());
            }
        }
        self.pool.sessions = sessions;
        // sync the current project's group, then re-pin project order
        let all: Vec<SessionInfo> = self
            .psp
            .projects
            .iter()
            .flat_map(|g| {
                if same_ws(&g.path.to_string_lossy(), &cwd) {
                    self.pool.sessions.clone()
                } else {
                    g.sessions.clone()
                }
            })
            .collect();
        self.rebuild_projects(all);
        // 侧栏清单刚重扫完，顺手把 topbar 固定段的标题快照对齐真值：
        // 草稿落盘（FileBound）/ pi 端改名回执 / 首条消息预览全走
        // ListDirty → 这条链；switch_to 只在切换会话时刷快照，留在本
        // 会话里一路聊到拿到真名也永不触发（2026-11-07 用户报障）
        self.sync_primary_tab_label();
    }

    pub(crate) fn switch_project(&mut self, cwd: PathBuf, cx: &mut Context<Self>) {
        // 「进入项目」唯一显式入口：cwd 只在这里 / 恢复路径 / open_session
        // 跨项目三处被写入，全部源自用户动作（010 重设计：应用永不搢造）
        self.ws.cwd = Some(cwd.clone());
        // 打开项目即注册（PF 项目注册表）：供「打开项目」弹窗枚举与 newest()
        // 排序兑底；psp 分组侧零会话项目不建组（用户 2026-10-18 定案）
        pi_link::projects::register_project(&cwd);
        self.dlg.dialog = None;
        // 全套跟着新 cwd 重挂（Pin 收藏夹/树/git/fs watcher/项目文件索引/
        // cwd 外单文件监听）——与 open_session 的池命中路径共用同一条
        // remount_workspace。此前本函数把这一套自己抄了一遍，漏掉了
        // **Pin 收藏夹**（按项目存的白名单）：切项目后 ws.pinned 还是上一个
        // 项目的绝对路径，Pin 视图下 rebuild_tree → flatten_pins →
        // is_abs_path_ignored → strip_prefix().unwrap() 当场 panic，
        // 且 panic 发生在鼠标事件回调（非 unwind 边界）里 → 进程直接消失
        // （用户 2026-10-17 报崩：从文件树切项目后点 psp 项目行 ＋ 新建会话）。
        self.remount_workspace(cx);
        // 恢复目标过 restorable_session 共享闸（与启动恢复同源）：影子/
        // 认领/子会话（历史污染指针）一律不开——落新会话页
        let resumed = crate::startup::restorable_session(&cwd.to_string_lossy())
            .map(|path| self.open_session(path, false, cx))
            .is_some();
        if resumed {
            cx.notify();
            return;
        }
        // 2026-12-06 用户定夺：项目切了 ≠ 要新开会话。新项目没有可恢复的
        // 上次会话时，当前会话若**进行中**（已绑定会话文件、非空白草稿）则
        // 原地保留：树/侧栏已随新 cwd 重挂，注意力与 composer 仍连原会话，
        // 用户看完新项目文档点会话标签即回原对话（切回原项目/点会话行也
        // 能靠 last-open 记忆归位）。只有当前本身就是空白草稿才落新草稿
        // ——空白换空白无感知，且发送目标跟着落到可见的新项目，不会重演
        // 2026-10-18「会话落错项目且列表隐形」（那次正是空白草稿跨项目
        // 发消息，新会话落进启动时旧项目的缓存组）。
        let in_progress = self
            .pool
            .runtimes
            .get(&self.pool.active_key)
            .is_some_and(|rt| rt.read(cx).file.is_some());
        if in_progress {
            cx.notify();
            return;
        }
        self.new_session(cx);
        cx.notify();
    }

    pub(crate) fn rt(&self) -> gpui::Entity<session::runtime::SessionRuntime> {
        self.pool
            .runtimes
            .get(&self.pool.active_key)
            .expect("active runtime exists")
            .clone()
    }

    /// 「其他」页提示音开关 → 全池广播。轮末提示音归 runtime 职责，每个常驻
    /// runtime 各持一份副本，所以新开关必须扇出到**所有** runtime 而不只是活跃
    /// 那个——后台会话同样会跑 agent、同样需要提示。
    pub(crate) fn broadcast_sound_on(&self, on: bool, cx: &mut Context<Self>) {
        for (_, rt) in &self.pool.runtimes {
            rt.update(cx, |r, _| r.sound_on = on);
        }
    }

    /// 当前项目的上下文（010-启动.md §5）：命中启动装载的集合就直接返回，
    /// 未命中（新打开的项目）用同一个装载函数现算一次并纳入集合。
    pub(crate) fn project_ctx_now(&mut self) -> startup::ProjectCtx {
        // 无项目态：无项目上下文（全局设置照常，项目层为空默认）
        let Some(cwd) = self.ws.cwd.clone() else {
            return startup::ProjectCtx::default();
        };
        let key = cwd.to_string_lossy().to_string();
        if let Some(ctx) = self.pool.project_ctx.get(&key) {
            return ctx.clone();
        }
        let ctx = startup::load_project(&cwd);
        self.pool.project_ctx.insert(key, ctx.clone());
        ctx
    }

    /// 技能表现读（盘上真值）并回写启动缓存。
    ///
    /// 设置页的启停开关**写盘后必须走这里**：`project_ctx` 是进程启动时的
    /// 快照，直接拿它回读只能看到开关前的旧 `disable-model-invocation`，
    /// 于是 UI 永远不翻转、开关回调又按旧状态再算一次同样的目标值——看起来
    /// 就是「两个开关都是摆设」（本页写盘后回读的坑与 043 MCP 同源）。
    pub(crate) fn skills_fresh(&mut self) -> Vec<pi_link::skills::SkillEntry> {
        let Some(cwd) = self.ws.cwd.clone() else {
            return Vec::new();
        };
        let skills = startup::load_skills(&cwd);
        let commands = pi_link::catalog::skill_commands(&skills);
        if let Some(ctx) = self
            .pool
            .project_ctx
            .get_mut(&cwd.to_string_lossy().to_string())
        {
            ctx.skills = skills.clone();
            ctx.skill_commands = commands;
        }
        skills
    }

    /// 侧栏清单里的标题素材（pi 会话名优先，回落首条消息预览；都空 =
    /// None）。session_title_of 与 tab 快照同步共用同一出处，两条路
    /// 永不各说各话。
    pub(crate) fn psp_title_of(&self, file: &std::path::Path) -> Option<String> {
        self.psp
            .projects
            .iter()
            .flat_map(|g| &g.sessions)
            .find(|s| same_path(&s.path, file))
            // 当前项目清单兑底（侧栏行同一出处）：psp 分组建自装载窗口
            // （recent_load_paths），窗口外/未重建的会话在 pool.sessions
            // 仍有行——分组缺席不该让标题取空落兑底词（2026-11-07
            // 「新会话」复发根因③）
            .or_else(|| self.pool.sessions.iter().find(|s| same_path(&s.path, file)))
            .and_then(|s| {
                s.name
                    .clone()
                    .filter(|n| !n.trim().is_empty())
                    .or_else(|| Some(s.preview.clone()).filter(|p| !p.trim().is_empty()))
            })
    }

    /// 指定池 key 的会话标题（topbar 第一格在查看插件功能页时显示桌面会话用；
    /// ≤30 字截断）。优先 pi 会话名，回落首条用户消息，再回落「新会话」。
    pub(crate) fn session_title_of(&self, key: &str, cx: &gpui::App) -> String {
        self.pool
            .runtimes
            .get(key)
            .and_then(|rt| rt.read(cx).file.clone())
            .and_then(|f| self.psp_title_of(&f))
            .map(|raw| tab_title(&raw))
            .unwrap_or_else(|| tr("session.new").to_string())
    }

    /// topbar 固定段标题快照对齐（refresh_sessions 末尾调 + 启动链
    /// spawn_boot_restore 分组建好后补调）：快照命中优先
    /// 于活值渲染（titlebar::session_tabs），旧值不刷「新会话」就钉死在
    /// tab 上。真值取不到（draft-N 不是文件路径 / 空闲退役后 runtime 出池）
    /// **不写**——兜底词绝不能覆盖已有快照，退役后快照是 tab 标题唯一
    /// 出处（PoolUi::tab_labels 注）。
    pub(crate) fn sync_primary_tab_label(&mut self) {
        let key = self.pool.primary_key.clone();
        // 池 key 即会话文件路径（FileBound 迁移后）；is_file 守卫一步
        // 分流草稿键，不出兜底词
        let f = std::path::Path::new(&key);
        if !f.is_file() {
            return;
        }
        if let Some(raw) = self.psp_title_of(f) {
            self.pool.tab_labels.insert(key, tab_title(&raw));
        }
    }
}
// ---------------------------------------------------------------------------
// 会话池方法：刷新/切换/活跃态/上下文/标题（P3 自 main.rs impl Chat 拆入）
// ---------------------------------------------------------------------------

/// 标题入 tab 的统一整形：首行 + ≤30 字截断（超长补 …）。session_title_of
/// 与 sync_primary_tab_label / switch_to 快照写入共用，快照与活值同一个形。
pub(crate) fn tab_title(raw: &str) -> String {
    let first_line = raw.lines().next().unwrap_or("").trim().to_string();
    let mut out: String = first_line.chars().take(30).collect();
    if first_line.chars().count() > 30 {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tab_title_tests {
    use super::tab_title;

    #[test]
    fn keeps_short_single_line() {
        assert_eq!(tab_title("派发3个子代理做测试"), "派发3个子代理做测试");
    }

    #[test]
    fn takes_first_line_and_trims() {
        assert_eq!(tab_title("  首行标题\n第二行"), "首行标题");
    }

    #[test]
    fn truncates_over_30_chars() {
        let long = "字".repeat(40);
        let out = tab_title(&long);
        assert_eq!(out.chars().count(), 31); // 30 字 + …
        assert!(out.ends_with('…'));
    }

    #[test]
    fn empty_yields_empty() {
        assert_eq!(tab_title(""), "");
    }
}
