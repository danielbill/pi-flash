//! 021 文件树 fs 提交动作：新建提交（逐段校验 + 沿途展开）、改名提交
//! （打开面重映射）、拖拽移动提交（Zed move_worktree_entry 语义）。
//! 自 file_actions.rs 按动作族切出（itlq.12 超规拆分，纯结构搬移）。

use crate::*;

impl Chat {
    /// 新建提交：逐段校验（禁 `..` / 空段 / 绝对路径），成功后沿途目录
    /// 展开 + 刷新 git（未跟踪文件立即可见）。重名拦截在 tree_edit_confirm
    /// （Zed 校验先行），这里只兜竞态。
    pub(crate) fn file_new_commit(
        &mut self,
        dir: &Path,
        is_dir: bool,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let segs: Vec<&str> = name.split(['/', '\\']).collect();
        if name.starts_with('/')
            || name.contains(':')
            || segs.iter().any(|s| s.is_empty() || *s == "..")
        {
            self.set_status(tr("conflict.path_escapes_root").to_string(), cx);
            return;
        }
        let target = dir.join(&name);
        let in_root = self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| target.starts_with(cur));
        if !in_root {
            self.set_status(tr("conflict.path_escapes_root").to_string(), cx);
            return;
        }
        let r = if is_dir {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())
        } else {
            std::fs::create_dir_all(target.parent().unwrap_or(dir))
                .and_then(|_| std::fs::write(&target, b""))
                .map_err(|e| e.to_string())
        };
        match r {
            Ok(_) => {
                // 沿途目录展开，让新条目立即可见（树与变更过滤两态都认）
                let cwd = self.ws.cwd.clone();
                let mut anc = target.parent().map(|p| p.to_path_buf());
                while let Some(p) = anc {
                    if cwd.as_deref().is_some_and(|cur| same_path(&p, cur)) {
                        break;
                    }
                    self.ws.expanded_dirs.insert(p.clone());
                    anc = p.parent().map(|q| q.to_path_buf());
                }
                // 树表即时重建（不等 fs watcher 合批 ~100-250ms）：新建行
                // 此刻就在表里，下面的 reveal / 打开链才有行可定位
                self.rebuild_tree();
                self.refresh_git(cx);
                if is_dir {
                    // 目录：选中 + 滚进视口（Zed confirm_edit：selection 挪到
                    // 新条目 → expand_to_selection → autoscroll；目录不打开）。
                    // 根下新建目录按排序落列表最顶，视窗停在下方时不滚动
                    // 跟随就看不到刚建的目录
                    self.ws.tree_selected = Some(target.clone());
                    self.reveal_tree_row(&target);
                } else {
                    // Zed auto_open：新建的文件随即打开；激活链 open_file_tab
                    // → activate_panel_tab → sync_tree_to_file 顺带选中 +
                    // reveal（行表已重建，定位不再扑空）
                    self.open_file_tab(target, cx);
                }
                // 创建成功静默（Zed 同：成功无提示）
                cx.notify();
            }
            Err(e) => {
                self.set_status(format!("{}: {e}", tr("conflict.create_failed")), cx);
            }
        }
    }

    /// 改名提交：目标 = 父目录 + 新名（允许子目录段，Zed RelPath 同款；
    /// 缺失的中间目录随建）。打开的文件 tab / 缓冲 / 待聚焦 / 展开集合
    /// （目录）按前缀重映射。重名/同名拦截在 tree_edit_confirm，这里只
    /// 兜竞态。
    pub(crate) fn file_rename_commit(
        &mut self,
        path: &Path,
        is_dir: bool,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let Some(parent) = path.parent() else { return };
        let target = parent.join(&name);
        if target == path {
            // 字面同名：视为取消（大小写改名走 fs::rename 的原生支持）
            cx.notify();
            return;
        }
        if target.exists() {
            self.set_status(
                format!("{}：{}", tr("file_tree.already_exists"), target.display()),
                cx,
            );
            return;
        }
        // 子目录段改名：缺失的中间目录随建（Zed rename_entry 同语义）
        if target.parent().is_some_and(|p| !p.exists())
            && let Err(e) = std::fs::create_dir_all(target.parent().unwrap())
        {
            self.set_status(
                crate::i18n::tf("file_tree.rename_failed", &[("e", e.to_string())]),
                cx,
            );
            return;
        }
        if let Err(e) = std::fs::rename(path, &target) {
            self.set_status(
                crate::i18n::tf("file_tree.rename_failed", &[("e", e.to_string())]),
                cx,
            );
            return;
        }
        // 打开面重映射：tab 与缓冲持旧路径的话跟上（目录改名 = 整棵前缀）
        self.remap_moved_entry(path, &target, is_dir);
        self.refresh_git(cx);
        // 改名成功静默（同新建/删除：成功不提示，失败才提示）
        cx.notify();
    }

    /// 移动/改名后的打开面重映射（`file_rename_commit` 与拖拽移动共用）：
    /// 文件 tab、缓冲缓存、待聚焦编辑器按前缀跟随；目录时连带展开集合
    /// 整棵前缀（023 编辑器实体的缓冲跟着 tab 路径走，无需另改）。
    pub(crate) fn remap_moved_entry(&mut self, from: &Path, to: &Path, is_dir: bool) {
        let remap = |p: &PathBuf| -> PathBuf {
            let hit = if is_dir {
                same_path(p, from) || p.starts_with(from)
            } else {
                same_path(p, from)
            };
            if hit {
                let rel = p.strip_prefix(from).unwrap_or(p);
                to.join(rel)
            } else {
                p.clone()
            }
        };
        for t in &mut self.ws.panel_tabs {
            if let PanelTab::File(p) = t {
                *p = remap(p);
            }
        }
        self.ws.file_cache = self
            .ws
            .file_cache
            .drain()
            .map(|(k, v)| (remap(&k), v))
            .collect();
        if let Some(f) = self.ws.pending_focus_file.as_mut() {
            *f = remap(f);
        }
        if is_dir {
            self.ws.expanded_dirs = self.ws.expanded_dirs.iter().map(remap).collect();
        }
        // 082 选中条目跟随（改名/移动后选中态不悬空，F2 仍指向新路径）
        if let Some(sel) = self.ws.tree_selected.as_mut() {
            *sel = remap(sel);
        }
        // 021 Pin 收藏跟随（文件 = 单条，目录 = 整棵前缀）
        let pins_before = self.ws.pinned.clone();
        self.ws.pinned = self.ws.pinned.iter().map(remap).collect();
        if self.ws.pinned != pins_before {
            self.save_pins();
        }
    }

    /// 拖拽移动提交（Zed drag_onto → move_worktree_entry 语义）：src 移入
    /// dst_dir。落点合法性（原地/进自身子树 = no-op）由调用方
    /// `drag_move_target` 先行过滤，这里只兜竞态与 fs 错误。Windows 跨盘
    /// rename 直接报错（Zed 同：无复制降级）。
    pub(crate) fn tree_move_commit(
        &mut self,
        src: &Path,
        src_is_dir: bool,
        dst_dir: &Path,
        cx: &mut Context<Self>,
    ) {
        // 护栏：源与目标都必须在项目根内，根本身不可移动（无项目态无树可动）
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let in_root =
            |p: &Path| same_path(p, &cwd) || crate::services::workspace::is_under(p, &cwd);
        if same_path(src, &cwd) || !in_root(src) || !in_root(dst_dir) {
            return;
        }
        let Some(name) = src.file_name() else { return };
        let target = dst_dir.join(name);
        // 原地 no-op（Zed `(new_path != source_path).then(...)` 同款）
        if same_path(&target, src) {
            return;
        }
        if target.exists() {
            self.set_status(
                format!("{}：{}", tr("file_tree.already_exists"), target.display()),
                cx,
            );
            return;
        }
        if let Err(e) = std::fs::rename(src, &target) {
            self.set_status(
                crate::i18n::tf("file_tree.move_failed", &[("e", e.to_string())]),
                cx,
            );
            return;
        }
        self.remap_moved_entry(src, &target, src_is_dir);
        self.retain_file_watches();
        // 目标链沿途展开，让移动结果立即可见（同新建提交）
        let cwd = self.ws.cwd.clone();
        let mut anc = target.parent().map(|p| p.to_path_buf());
        while let Some(p) = anc {
            if cwd.as_deref().is_some_and(|cur| same_path(&p, cur)) {
                break;
            }
            self.ws.expanded_dirs.insert(p.clone());
            anc = p.parent().map(|q| q.to_path_buf());
        }
        self.refresh_git(cx);
        // 移动成功静默（同改名：成功不提示，失败才提示）
        cx.notify();
    }
}
