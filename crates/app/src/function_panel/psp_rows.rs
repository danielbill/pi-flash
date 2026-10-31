//! psp 行模型（015）：扁平化 [`PspRow`] 虚拟列表行集（psp_rows/pinned_rows）
//! 与 Chat 侧 rebuild 簇（rebuild_projects/auto_collapse/sync_current_sessions/
//! pin）。视图渲染在 [`super::mod`]；行视图绘制件在 psp_row_views。
//! 自 mod.rs 拆出（090 重构）。

use super::*;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PspRow {
    Project(usize),
    Session {
        p: usize,
        s: usize,
    },
    /// 置顶组头行（伪项目行）：pin icon + 「置顶」；不收折/不拖拽/无卡，
    /// 仅作分组视觉。组内会话行用 [`PspRow::PinnedSession`]（元素 id 与
    /// 原组行区分，防 gpui id 冲突）。
    PinnedHeader,
    /// 置顶组内的会话行（p/s = 原项目组下标；**移入语义**——置顶会话只在
    /// 置顶组显示，原项目组已按 pin 清单过滤，不重复出现）：
    /// 不参与项目块避让平移（前置头行之前无 Project 行，shift 恒 0）。
    PinnedSession {
        p: usize,
        s: usize,
    },
    /// 伪标签（draft 已发送、pi 未落盘的占位行；key = 池 key draft-N）
    Pending {
        key: String,
    },
    /// 空项目展开时的「无会话」占位
    Empty,
    /// 「显示更多」行（key = 组 ws_key；Flat 模式用全局键）
    More {
        key: String,
    },
}

/// 每页显示的会话标签数（学 zcode：「显示更多」每次续一页）——默认值来自
/// 设置-其他「会话显示数」（3-10）。
pub(crate) fn psp_page() -> usize {
    crate::services::workspace::session_display_count()
}

pub(crate) fn psp_rows(chat: &Chat) -> Vec<PspRow> {
    let mut rows: Vec<PspRow> = Vec::new();
    // 伪标签归属：cwd 匹配项目组的插在组内会话行之前（它就是该项目最新的
    // 活动）；没有任何组可归（该项目还没有已落盘会话）的插在 Title 后。
    // 折叠的组里伪标签照常显示——它正是用户刚发出的那条消息的标签。
    let orphan_pending: Vec<String> = chat
        .psp
        .pending_tabs
        .iter()
        .filter(|e| {
            !chat
                .psp
                .projects
                .iter()
                .any(|g| same_ws(&g.path.to_string_lossy(), &e.cwd))
        })
        .map(|e| e.key.clone())
        .collect();
    match chat.psp.list_mode {
        ListMode::Grouped => {
            // 置顶组（会话 pin）：列表最顶；内容为空不建组（空置顶组永不
            // 显示）。**zcode 同款「移入」语义**：置顶会话只在置顶组出现，
            // 原项目组按 pin 清单过滤（见下方 visible），不重复显示；
            // 组内按 pin 序（后置顶的总在最上面）。
            let pinned = pinned_rows(chat);
            if !pinned.is_empty() {
                rows.push(PspRow::PinnedHeader);
                rows.extend(pinned);
            }
            for (pi, g) in chat.psp.projects.iter().enumerate() {
                rows.push(PspRow::Project(pi));
                let key = same_ws_key(&g.path.to_string_lossy());
                for e in chat
                    .psp
                    .pending_tabs
                    .iter()
                    .filter(|e| same_ws(&g.path.to_string_lossy(), &e.cwd))
                {
                    rows.push(PspRow::Pending { key: e.key.clone() });
                }
                // 拖拽中的项目收起为标签（020）：渲染期折叠——只显示组标题
                // 行，落位重排后状态清零自动还原，不碰 collapsed_keys
                let drag_collapsed = chat.psp.proj_drag.as_ref().is_some_and(|d| d.key == key);
                if drag_collapsed || chat.psp.collapsed_keys.contains(&key) {
                    continue;
                }
                // 置顶会话已被「移入」置顶组，原项目组不再重复显示（zcode
                // parity）：按 pin 清单过滤；分页计数用过滤后的可见条数，
                // 否则会留下空组还挂「显示更多」的死行
                let visible: Vec<usize> = (0..g.sessions.len())
                    .filter(|si| !chat.session_pinned(&g.sessions[*si].path))
                    .collect();
                if visible.is_empty() {
                    rows.push(PspRow::Empty);
                } else {
                    let shown = chat
                        .psp
                        .psp_shown
                        .get(&key)
                        .copied()
                        .unwrap_or_else(psp_page)
                        .min(visible.len());
                    for si in visible.iter().take(shown) {
                        rows.push(PspRow::Session { p: pi, s: *si });
                    }
                    if visible.len() > shown {
                        rows.push(PspRow::More { key });
                    }
                }
            }
        }
        ListMode::Flat => {
            // Flat：伪标签按插入序排列表最前（最新活动优先），其后置顶会话
            // 排到会话区第一位（pin 序），其余维持现排序
            for e in &chat.psp.pending_tabs {
                rows.push(PspRow::Pending { key: e.key.clone() });
            }
            let mut all: Vec<(usize, usize)> = Vec::new();
            for (pi, g) in chat.psp.projects.iter().enumerate() {
                for si in 0..g.sessions.len() {
                    all.push((pi, si));
                }
            }
            if chat.psp.sort_mode == SortMode::Time {
                all.sort_by(|a, b| {
                    let ma = chat.psp.projects[a.0].sessions[a.1].modified;
                    let mb = chat.psp.projects[b.0].sessions[b.1].modified;
                    mb.cmp(&ma)
                });
            }
            // 置顶分区：pin 序（session_pins 位置升序）排到最前
            let pin_rank = |p: usize, s: usize| -> usize {
                let path = &chat.psp.projects[p].sessions[s].path;
                chat.psp
                    .session_pins
                    .iter()
                    .position(|q| same_path(q, path))
                    .unwrap_or(usize::MAX)
            };
            all.sort_by_key(|&(p, s)| pin_rank(p, s));
            let key = "__flat__".to_string();
            let shown = chat
                .psp
                .psp_shown
                .get(&key)
                .copied()
                .unwrap_or_else(psp_page);
            let total = all.len();
            all.truncate(shown);
            all.into_iter()
                .for_each(|(p, s)| rows.push(PspRow::Session { p, s }));
            if total > shown {
                rows.push(PspRow::More { key });
            }
        }
    }
    // 无组可归的伪标签（该项目组还没被 rebuild_projects 建出来）：插在置顶
    // 组（若有）之后、项目区之前——用户刚发的消息必须是第一眼能看到的东西，
    // 但不能盖住置顶组的头行
    let orphan_at = rows
        .iter()
        .take_while(|r| matches!(r, PspRow::PinnedHeader | PspRow::PinnedSession { .. }))
        .count();
    for (i, key) in orphan_pending.into_iter().enumerate() {
        rows.insert(orphan_at + i, PspRow::Pending { key });
    }
    rows
}

/// 置顶组的会话行（Grouped 视图）：按 session_pins 序（后置顶的总在最上面）
/// 在 projects 里找到每个置顶会话的当前下标，产出副本行引用。清单外
/// （已删 / 未装载）的置顶项静默跳过；全部跳过则空向量 —— 上层不建组。
pub(crate) fn pinned_rows(chat: &Chat) -> Vec<PspRow> {
    let mut rows = Vec::new();
    for pin in &chat.psp.session_pins {
        for (pi, g) in chat.psp.projects.iter().enumerate() {
            if let Some(si) = g.sessions.iter().position(|s| same_path(&s.path, pin)) {
                rows.push(PspRow::PinnedSession { p: pi, s: si });
                break;
            }
        }
    }
    rows
}

impl Chat {
    /// Group the cross-project session scan into project groups (020 拖拽
    /// 版排序：用户排序 > 首建默认序（当前项目钉顶 + 活动倒序，仅一次）>
    /// 运行期保序；组内会话恒按活动倒序——详见下方实现注释）。
    pub(crate) fn rebuild_projects(&mut self, all: Vec<pi_link::sessions::SessionInfo>) {
        // PF 项目注册表（打开项目即注册）：打开时刻参与 newest() 排序兑底。
        // 注册项目**零会话不建组**（用户 2026-10-18 定案，推翻 10-16 空组
        // 并集口径）——psp 只显示装载窗口内有会话的项目 + 当前项目空组；
        // 全部注册项仍在「打开项目」弹窗可寻
        let records = pi_link::projects::project_records();
        let opened: std::collections::HashMap<String, i64> = records
            .iter()
            .map(|r| (same_ws_key(&r.path.to_string_lossy()), r.last_opened_ms))
            .collect();
        let opened_ms = |p: &std::path::Path| {
            opened
                .get(&same_ws_key(&p.to_string_lossy()))
                .copied()
                .unwrap_or(0)
        };
        let mut groups: Vec<crate::ProjectGroup> = Vec::new();
        // 影子会话（插件认领线）不建行：过滤清单由插件注入
        let hidden = pi_link::hidden_sessions::load();
        // 同一会话文件可能被索引两次（路径大小写变体：`C:/…` 与 `c:/…`，自动化
        // 打开/外部工具写索引都会留下双条）→ 列表出现重复行。按归一化路径去重。
        let mut seen_sessions: std::collections::HashSet<String> = std::collections::HashSet::new();
        for s in all {
            if !seen_sessions.insert(same_ws_key(&s.path.to_string_lossy())) {
                continue;
            }
            if hidden.hides(s.name.as_deref(), &s.path) {
                continue;
            }
            let cwd = PathBuf::from(s.cwd.clone());
            // 应用自有目录（载荷目录/npm 包根）不建组：污染期落在 payload
            // 的历史脏会话不得把安装目录钉成常驻项目组（注册表已由 boot
            // 清洗，这里堵会话扫描并集这一路）
            if pi_link::paths::is_app_install_path(&cwd) {
                continue;
            }
            if let Some(g) = groups
                .iter_mut()
                .find(|g| same_ws(&g.path.to_string_lossy(), &cwd.to_string_lossy()))
            {
                g.sessions.push(s);
            } else {
                groups.push(crate::ProjectGroup {
                    name: crate::services::workspace::project_name(&cwd),
                    opened_ms: opened_ms(&cwd),
                    path: cwd,
                    sessions: vec![s],
                });
            }
        }
        // 当前项目空组仅在有项目时建（无项目态没有「当前项目」行）
        if let Some(cwd) = self.ws.cwd.clone()
            && !groups
                .iter()
                .any(|g| same_ws(&g.path.to_string_lossy(), &cwd.to_string_lossy()))
        {
            groups.push(crate::ProjectGroup {
                name: crate::services::workspace::project_name(&cwd),
                path: cwd.clone(),
                sessions: Vec::new(),
                opened_ms: opened_ms(&cwd),
            });
        }
        // 组内会话恒按活动倒序（020）：项目排序走哪条路都不影响组内
        for g in &mut groups {
            g.sessions.sort_by(|a, b| b.modified.cmp(&a.modified));
        }
        // —— 项目排序（020 拖拽版）——
        // 1) 用户排序（拖拽产物）一旦存在永远生效：按保存序；未收录的
        //    新项目按活动倒序垫底；
        // 2) 无用户排序且是首建（projects 还为空）：默认序只排这一次——
        //    当前项目钉顶 + 活动倒序；此后运行期重建不再自动置顶；
        // 3) 其余运行期重建：保序，新出现的组按活动倒序追加尾部。
        if let Some(order) = &self.psp.proj_order {
            groups.sort_by(|a, b| {
                let ka = order
                    .iter()
                    .position(|k| *k == same_ws_key(&a.path.to_string_lossy()));
                let kb = order
                    .iter()
                    .position(|k| *k == same_ws_key(&b.path.to_string_lossy()));
                match (ka, kb) {
                    (Some(x), Some(y)) => x.cmp(&y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => newest(b).cmp(&newest(a)),
                }
            });
        } else if self.psp.projects.is_empty() {
            // current pinned first, the rest by newest session mtime desc
            let cwd = self.ws.cwd.clone();
            groups.sort_by(|a, b| {
                let ka = cwd
                    .as_deref()
                    .is_some_and(|cur| same_ws(&a.path.to_string_lossy(), &cur.to_string_lossy()));
                let kb = cwd
                    .as_deref()
                    .is_some_and(|cur| same_ws(&b.path.to_string_lossy(), &cur.to_string_lossy()));
                match (ka, kb) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => newest(b).cmp(&newest(a)),
                }
            });
        } else {
            let mut ordered: Vec<crate::ProjectGroup> = Vec::with_capacity(groups.len());
            let mut pool = groups;
            for old in &self.psp.projects {
                let k_old = same_ws_key(&old.path.to_string_lossy());
                if let Some(pos) = pool
                    .iter()
                    .position(|g| same_ws_key(&g.path.to_string_lossy()) == k_old)
                {
                    ordered.push(pool.remove(pos));
                }
            }
            pool.sort_by(|a, b| newest(b).cmp(&newest(a)));
            ordered.extend(pool);
            groups = ordered;
        }
        // v60: 加载单位是会话（设置.默认加载会话数）——组由这批会话的
        // cwd 自然形成，不再按项目数截断
        self.psp.projects = groups;
        self.sync_current_sessions();
    }

    /// 启动自动折叠（其他页「自动折叠时间」档位 1/3/7 天，内部按小时算）：
    /// 项目内**所有**会话的最后活动时间都早于阈值 → 该项目组默认折叠。
    /// 只在启动装载项目集后调一次（`startup::spawn_session_list_load`）——
    /// 运行期 `refresh_sessions` 等重建不得再折叠，否则用户手动展开的组
    /// 会被下一轮重建打回去。空组（无会话）不折叠；当前项目永不折叠
    /// （钉顶的活动项目折叠等于看不见自己刚开的会话）。
    pub(crate) fn auto_collapse_stale_projects(&mut self) {
        let hours = crate::services::workspace::auto_collapse_hours();
        let cutoff = std::time::SystemTime::now() - std::time::Duration::from_secs(hours * 3600);
        let cwd = self.ws.cwd.clone();
        for g in &self.psp.projects {
            if g.sessions.is_empty() {
                continue;
            }
            if cwd
                .as_deref()
                .is_some_and(|cur| same_ws(&g.path.to_string_lossy(), &cur.to_string_lossy()))
            {
                continue;
            }
            if newest(g) < cutoff {
                self.psp
                    .collapsed_keys
                    .insert(same_ws_key(&g.path.to_string_lossy()));
            }
        }
    }

    /// Keep `sessions` (current project) in sync with the matching group.
    pub(crate) fn sync_current_sessions(&mut self) {
        // 无项目态：没有「当前项目」清单
        let Some(cwd) = self.ws.cwd.clone() else {
            self.pool.sessions = Vec::new();
            return;
        };
        let cwd = cwd.to_string_lossy().to_string();
        self.pool.sessions = self
            .psp
            .projects
            .iter()
            .find(|g| same_ws(&g.path.to_string_lossy(), &cwd))
            .map(|g| g.sessions.clone())
            .unwrap_or_default();
    }

    /// 会话是否已置顶（020 会话置顶；路径判等 same_path）。
    pub(crate) fn session_pinned(&self, path: &std::path::Path) -> bool {
        self.psp.session_pins.iter().any(|p| same_path(p, path))
    }

    /// 会话置顶开关（pin/unpin 唯一写路）：pin = 插到最前（后置顶的总在
    /// 最上面）；unpin = 移除（置顶组随之空掉 → 不再显示）。落盘时机即
    /// 点击时刻（不等关窗，防异常退出丢失）。
    pub(crate) fn session_pin_toggle(&mut self, path: PathBuf, cx: &mut gpui::Context<Self>) {
        match self
            .psp
            .session_pins
            .iter()
            .position(|p| same_path(p, &path))
        {
            Some(ix) => {
                self.psp.session_pins.remove(ix);
            }
            None => self.psp.session_pins.insert(0, path),
        }
        self.persist_ui();
        cx.notify();
    }
}

pub(crate) fn newest(g: &crate::ProjectGroup) -> std::time::SystemTime {
    let from_sessions = g
        .sessions
        .iter()
        .map(|s| s.modified)
        .max()
        .unwrap_or(std::time::UNIX_EPOCH);
    // 注册表打开时刻兑底：零会话的注册项目按打开时刻参与活动排序
    let from_open =
        std::time::UNIX_EPOCH + std::time::Duration::from_millis(g.opened_ms.max(0) as u64);
    from_sessions.max(from_open)
}
