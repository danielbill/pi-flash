//! 文件树 Chat 侧状态与命令：reveal/右键菜单（impl Chat #1）+ archive/restore
//! 树状态、rebuild_tree、fs watch（impl Chat #2）。视图渲染在
//! [`super::file_tree`]。自 file_tree.rs 拆出（090 重构）。

use super::*;

impl Chat {
    /// 行内改名前置（Zed project_panel 语义）：沿途展开祖先目录 + 把改名
    /// 行滚进视口。树右键菜单入口行本就可见（幂等）；编辑区右键菜单入口
    /// 可能该行还没展开/滚出视口，不做这些就没有输入框可编辑。
    pub(crate) fn reveal_tree_row(&mut self, path: &Path) {
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let mut anc = path.parent().map(|p| p.to_path_buf());
        let mut opened = false;
        while let Some(p) = anc {
            if same_path(&p, &cwd) {
                break;
            }
            opened |= self.ws.expanded_dirs.insert(p.clone());
            anc = p.parent().map(|q| q.to_path_buf());
        }
        if opened {
            self.rebuild_tree();
        }
        // 「只看变更」/Pin 收藏夹视图可能不含目标（变更列表无未变更文件、
        // 收藏视图无未收藏行）：回退树模式
        if (self.ws.changes_only || self.ws.pin_view)
            && !self.ws.tree_rows.iter().any(|r| same_path(&r.path, path))
        {
            self.ws.changes_only = false;
            self.ws.pin_view = false;
            self.rebuild_tree();
            self.persist_ui();
        }
        // 滚动对齐 Zed project_panel 的 autoscroll（reveal 链的真实落点）：
        // scroll_to_item_with_offset 非 strict——目标已可见不动视口，出界才
        // 最小滚动；offset = 深度（sticky 祖先链行数，视口自顶收缩，目标居中
        // 于钉住链之下）。deferred 到下一帧 paint 按真实视口高夹紧执行——
        // gpui 偏移向下滚为负，此前手写正 y 的 set_offset 被夹回 0，长树
        // 永不跳转。
        let Some(ix) = self
            .ws
            .tree_rows
            .iter()
            .position(|r| same_path(&r.path, path))
        else {
            return;
        };
        let depth = self.ws.tree_rows[ix].depth;
        let handle = crate::ui::vlist::scroll_handle(FILE_TREE_LIST_ID);
        handle.scroll_to_item_with_offset(ix, gpui::ScrollStrategy::Center, depth);
    }

    /// 新建虚拟编辑行在 vlist 输出位里的落点（下标, 缩进深度）：Zed new_entry
    /// 虚拟行插在目标目录行下方、子项之前；目标 = 项目根时插列表顶（根下新
    /// 建目录按排序也落最顶）。目录行不在表里（理论不可达：open_file_new 已
    /// 沿途展开）兜底排表尾。渲染落点（tree_list_view edit_slot）与打开编辑
    /// 行时的自动滚动共用本公式，两处永不漂移。
    pub(crate) fn tree_new_edit_slot(&self, dir: &Path) -> (usize, usize) {
        if self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| same_path(dir, cur))
        {
            return (0, 0);
        }
        self.ws
            .tree_rows
            .iter()
            .position(|r| r.is_dir && same_path(&r.path, dir))
            .map_or((self.ws.tree_rows.len(), 1), |ix| {
                (ix + 1, self.ws.tree_rows[ix].depth + 1)
            })
    }

    /// 文件标签激活时的树同步（PF 与 Zed 的分歧点：Zed 多 worktree 面板挂
    /// 多根，PF 单根树跟随激活标签）：文件在当前项目内只定位选中；属于已知
    /// 其它项目（psp.projects，含当前 cwd 的 0 会话组）则走 **switch_project
    /// 完整切项目**——树要能定位文件在项目里的位置（快捷面板/搜索/消息路径
    /// 开文件都经这里）。会话标签的去向由 switch_project 定夺（2026-12-06
    /// 定案）：新项目有上次会话 → 恢复；没有 → **进行中的原会话原地保留**，
    /// 只有当前本身是空白草稿才落新草稿。内容视图保持不动：用户正看着这个
    /// 文件，不能被 switch_to 的回 chat 前台拽走。两处都不沾（附件类游离
    /// 文件）则树不动。定位复用 reveal_tree_row。
    pub(crate) fn sync_tree_to_file(&mut self, path: &Path, cx: &mut gpui::Context<Self>) {
        let under_current = self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| services::workspace::is_under(path, cur));
        if !under_current {
            let root = self
                .psp
                .projects
                .iter()
                .filter(|g| services::workspace::is_under(path, &g.path))
                .max_by_key(|g| g.path.components().count())
                .map(|g| g.path.clone());
            let Some(root) = root else {
                return;
            };
            let view = self.layout.content_view;
            self.switch_project(root, cx);
            if self.layout.content_view != view {
                self.set_content_view(view);
            }
        }
        self.ws.tree_selected = Some(path.to_path_buf());
        self.reveal_tree_row(path);
    }

    // -----------------------------------------------------------------------
    // 标题项目下拉（树根跨项目快切）
    // -----------------------------------------------------------------------

    /// 标题左键 / 自动化 files.project_menu：开/收项目下拉。打开时键盘
    /// 高亮默认落当前项目（Enter 空转、↓ 即下一个）。
    pub(crate) fn toggle_tree_proj_menu(&mut self, cx: &mut gpui::Context<Self>) {
        if self.dlg.tree_proj_menu.take().is_some() {
            cx.notify();
            return;
        }
        let cur = self.ws.cwd.clone();
        let sel = self
            .psp
            .projects
            .iter()
            .position(|g| {
                cur.as_deref()
                    .is_some_and(|c| same_ws(&g.path.to_string_lossy(), &c.to_string_lossy()))
            })
            .unwrap_or(0);
        self.dlg.tree_proj_menu = Some(crate::function_panel::TreeProjMenu { sel });
        cx.notify();
    }

    /// ↑/↓ 移动键盘高亮（钳位不回绕，model picker 同款）。
    pub(crate) fn tree_proj_menu_move(&mut self, delta: i32, cx: &mut gpui::Context<Self>) {
        let Some(m) = self.dlg.tree_proj_menu.as_mut() else {
            return;
        };
        let n = self.psp.projects.len();
        if n == 0 {
            return;
        }
        m.sel = ((m.sel as i32) + delta).clamp(0, n as i32 - 1) as usize;
        cx.notify();
    }

    /// Enter：应用当前高亮行。
    pub(crate) fn tree_proj_menu_apply(&mut self, cx: &mut gpui::Context<Self>) {
        let sel = match self.dlg.tree_proj_menu.as_ref() {
            Some(m) => m.sel,
            None => return,
        };
        self.tree_proj_menu_pick(sel, cx);
    }

    /// 点击行 / Enter 共用：切树的项目并收下拉。切换 = **switch_project 完整
    /// 切项目**（单一语义 2026-10-18：当前项目 ≡ 活跃会话的项目，该项目恢复
    /// 上次会话或落新草稿，注意力跟过去；树根/branch/git/fs watcher/侧栏
    /// 当前项目随之重挂）。「只改 cwd 不动活跃会话」的旧语义已废除——不然
    /// composer 还连着旧项目的草稿，发消息会落错项目（2026-10-18 实测事故）。
    /// 内容视图保持不动：用户可能停在终端/文件上，不被拽走。
    pub(crate) fn tree_proj_menu_pick(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        self.dlg.tree_proj_menu = None;
        let Some(g) = self.psp.projects.get(ix) else {
            return;
        };
        let path = g.path.clone();
        let cur_same = self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| same_ws(&path.to_string_lossy(), &cur.to_string_lossy()));
        if !cur_same {
            let view = self.layout.content_view;
            self.switch_project(path, cx);
            if self.layout.content_view != view {
                self.set_content_view(view);
            }
            // 旧项目的选中条目不再成立（F2/高亮都按行存在性复核，残留无害，
            // 这里清掉是为了下拉切走再切回来不闪旧高亮）
            self.ws.tree_selected = None;
        }
        cx.notify();
    }
}

// ---------------------------------------------------------------------------
// 文件树与 fs 监听方法（P3 自 main.rs impl Chat 拆入）
// ---------------------------------------------------------------------------

impl Chat {
    /// 归档当前项目的文件树状态（展开集合 + Pin 视图）→ 内存表 + 落盘
    /// （`__tree`）。调用点：切项目（remount 开头）、保存 UI 状态（关窗/退出）、
    /// Pin 视图开关。展开/折叠本身不即时落盘（高频动作，靠这些时机兜住）。
    pub(crate) fn archive_tree_state(&mut self) {
        let Some((key, cwd)) = self.ws.tree_owner.clone() else {
            return;
        };
        let mem = crate::TreeMemory {
            expanded: self.ws.expanded_dirs.clone(),
            pin: self.ws.pin_view,
        };
        crate::services::workspace::save_tree_state(
            &cwd.to_string_lossy(),
            &crate::services::workspace::TreeState {
                expanded: crate::services::workspace::tree_rel_paths(&cwd, &mem.expanded),
                pin: mem.pin,
            },
        );
        self.ws.tree_memory.insert(key, mem);
    }

    /// 恢复一个项目的文件树状态：内存表（本次会话切回过）→ 磁盘 `__tree`
    /// → 默认「只展开根」（对齐 Zed：打开项目默认展开 cwd）。失效条目在
    /// `tree_abs_paths` 里 prune（目录已删 / 项目外），恒含根。
    pub(crate) fn restore_tree_state(&mut self, cwd: &Path) {
        use crate::services::workspace as ws_store;
        let key = ws_store::same_ws_key(&cwd.to_string_lossy());
        let (expanded, pin) = match self.ws.tree_memory.get(&key) {
            Some(mem) => (mem.expanded.clone(), mem.pin),
            None => match ws_store::tree_state_for(&cwd.to_string_lossy()) {
                Some(st) => (ws_store::tree_abs_paths(cwd, &st.expanded), st.pin),
                // 无记录 = 默认态：只展开根（flatten 从这里 walk 出第一层）
                None => (std::collections::HashSet::from([cwd.to_path_buf()]), false),
            },
        };
        self.ws.expanded_dirs = expanded;
        self.ws.expanded_dirs.insert(cwd.to_path_buf());
        self.ws.tree_owner = Some((key, cwd.to_path_buf()));
        // Pin 视图按项目恢复；收藏清单为空时无意义（行为与 021 一致：退回树）
        self.ws.pin_view = pin && !self.ws.pinned.is_empty();
        if self.ws.pin_view {
            self.ws.changes_only = false;
        }
    }

    /// 文件树展平缓存重建（services::file_tree::flatten：每展开目录读一次
    /// 盘 + gitignore 过滤 + Zed 语义排序 + git 徽标回填）。展开/折叠、
    /// git 刷新、fs 事件、切项目后都走这里。三种视图各吃各的展平：
    /// pin_view → flatten_pins（收藏森林）/ changes_only → flatten_changes
    /// （git status 直出）/ 树 → flatten。
    pub(crate) fn rebuild_tree(&mut self) {
        // 无项目态：树恒空（占位 runtime 不挂任何工作区）
        let Some(cwd) = self.ws.cwd.clone() else {
            self.ws.tree_rows = std::sync::Arc::new(Vec::new());
            return;
        };
        self.ws.tree_rows = std::sync::Arc::new(if self.ws.pin_view {
            services::file_tree::flatten_pins(
                &cwd,
                &self.ws.pinned,
                &self.ws.expanded_dirs,
                &self.ws.git_files,
            )
        } else if self.ws.changes_only {
            // 「只看变更」= 扁平表直接吃 git status 结果（不扫盘）
            services::file_tree::flatten_changes(&cwd, &self.ws.git_files)
        } else {
            services::file_tree::flatten(&cwd, &self.ws.expanded_dirs, &self.ws.git_files)
        });
    }

    /// 挂上 fs watcher（递归 watch cwd；旧句柄 drop 即解除）。切项目时
    /// 重挂。事件由 startup::spawn_fs_watch_pump 合批后驱动 refresh_git。
    pub(crate) fn attach_fs_watch(&mut self) {
        // 无项目态：无工作区监听
        let Some(cwd) = self.ws.cwd.clone() else {
            self.ws.fs_watch = None;
            return;
        };
        self.ws.fs_watch = services::watcher::watch(&cwd, self.ws.fs_watch_tx.clone()).ok();
    }

    /// 023 外部改动检测的监听面维护（对齐 Zed 的 worktree 监听面）：
    /// - cwd 内文件：cwd 的**递归** watch 已覆盖；
    /// - cwd 外文件（Obsidian 库 / pick_open_files / 远程 FILE_OPEN）：此前
    ///   完全没监听——磁盘怎么改都没有信号，编辑器不跟随。现按**所在目录**
    ///   挂非递归 watch（Zed single-file worktree 同款），事件与 cwd 事件
    ///   走同一条通道合批，`check_external_file_changes` 按 disk_sig 复查。
    ///
    /// 打开/关闭文件 tab、切项目后调用；不再被任何打开文件需要的目录卸掉
    /// （句柄 drop = 停止监听），避免长期持有无关目录的监听。
    pub(crate) fn retain_file_watches(&mut self) {
        let mut wanted: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
        for t in &self.ws.panel_tabs {
            if let PanelTab::File(p) = t
                && let Some(dir) = p.parent()
                && !self
                    .ws
                    .cwd
                    .as_deref()
                    .is_some_and(|cur| services::workspace::is_under(dir, cur))
            {
                wanted.insert(dir.to_path_buf());
            }
        }
        self.ws.file_watches.retain(|dir, _| wanted.contains(dir));
        for dir in wanted {
            if self.ws.file_watches.contains_key(&dir) {
                continue;
            }
            if let Ok(w) = services::watcher::watch_dir(&dir, self.ws.fs_watch_tx.clone()) {
                self.ws.file_watches.insert(dir, w);
            }
        }
    }

    // -----------------------------------------------------------------------
    // built-in terminal (pi-web TerminalPanel parity)
    // -----------------------------------------------------------------------

    pub(crate) fn load_project_files(&mut self) {
        // 无项目态：项目文件索引恒空
        let Some(cwd) = self.ws.cwd.clone() else {
            self.ws.project_files = Vec::new();
            return;
        };
        self.ws.project_files = walk_files(&cwd, 3, 400);
    }
}
