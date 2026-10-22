//! 021 文件树操作栏 + 右键菜单的文件动作：切换显示变更 / 搜索文件弹窗 /
//! 新建文件·目录 / 改名 / 删除至回收站（trash crate）/ 复制路径 /
//! @ 插入会话输入框（资源管理器定位在 services::shell_open）。
//! （P2 自根 actions_files.rs 归并入 function_panel 域。）

use super::{TreeEdit, TreeEditKind, TreeValidation};
use crate::*;

impl Chat {
    // -------------------------------------------------------------------
    // 操作栏
    // -------------------------------------------------------------------

    /// 「切换显示变更」：开 = 只列 git 变更文件（flatten changes_only，
    /// 全量下钻无视展开集合）。持久化走 UiState.tree_changes_only。
    /// 与 Pin 收藏夹视图互斥（同一树体区域，开这边关那边）。
    pub(crate) fn toggle_changes_only(&mut self, cx: &mut Context<Self>) {
        self.ws.changes_only = !self.ws.changes_only;
        if self.ws.changes_only && self.ws.pin_view {
            self.ws.pin_view = false;
        }
        self.rebuild_tree();
        self.persist_ui();
        cx.notify();
    }

    /// 「搜索文件」：**打开 083 项目搜索面板**（文件 + 内容同搜）。
    ///
    /// 2026-10-21 用户定案：原来的 700×500 弹窗（`Dialog::FileSearch`，只按
    /// 文件名过滤清单）退役，入口统一到 Zed 式搜索面板（Ctrl+L / Ctrl+Shift+F
    /// / 本按钮三条路同一个方法），面板打开即聚焦查询框。
    pub(crate) fn open_file_search(&mut self, cx: &mut Context<Self>) {
        self.open_project_search(cx);
    }

    // -------------------------------------------------------------------
    // Pin 收藏夹（021 三改：与「只看变更」同级的独立视图 = 文件树过滤器，
    // per-project 持久化收藏清单）
    // -------------------------------------------------------------------

    /// 菜单「Pin/取消固定」：收藏清单 toggle（无需确认）。pin 视图开着时
    /// 根行集随之变化，rebuild 跟上。相对路径（自动化 op 直传）归一到
    /// cwd 下——收藏清单一律存绝对路径（flatten/gitignore 匹配/改名跟随
    /// 都按绝对路径工作）。
    pub(crate) fn tree_pin_toggle(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let path = if path.is_relative() {
            cwd.join(path)
        } else {
            path
        };
        match self.ws.pinned.iter().position(|p| same_path(p, &path)) {
            Some(ix) => {
                self.ws.pinned.remove(ix);
            }
            None => self.ws.pinned.push(path),
        }
        self.save_pins();
        // Pin 视图开着时根行集随之变化；取消最后一个收藏 → 自动退回文件树
        if self.ws.pin_view && self.ws.pinned.is_empty() {
            self.ws.pin_view = false;
        }
        self.rebuild_tree();
        cx.notify();
    }

    /// pin 面内存态落盘（pin/unpin/改名跟随/删除清理共用）。
    fn save_pins(&mut self) {
        let pins: Vec<String> = self
            .ws
            .pinned
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        // 无项目态：收藏无归属项目，不落盘（pinned 本应为空）
        if let Some(cwd) = self.ws.cwd.as_deref() {
            crate::services::workspace::save_pins_for(&cwd.to_string_lossy(), &pins);
        }
    }

    /// 操作栏 Pin 钮：开/收 Pin 收藏夹视图（树体区域整体切换，同「切换
    /// 显示变更」）。开时：变更视图让位（互斥）、清失效条目、清完为空则
    /// 不开（无提示）。pin 视图会话态不持久化——启动默认文件树。
    pub(crate) fn toggle_pin_view(&mut self, cx: &mut Context<Self>) {
        self.ws.pin_view = !self.ws.pin_view;
        if self.ws.pin_view {
            if self.ws.changes_only {
                self.ws.changes_only = false;
                self.persist_ui();
            }
            let before = self.ws.pinned.len();
            self.ws.pinned.retain(|p| p.exists());
            if self.ws.pinned.len() != before {
                self.save_pins();
            }
            if self.ws.pinned.is_empty() {
                self.ws.pin_view = false;
            }
        }
        // Pin 视图开关悬项目态：立即归档（重启后按项目恢复该视图）
        self.archive_tree_state();
        self.rebuild_tree();
        cx.notify();
    }

    // -------------------------------------------------------------------
    // 右键菜单：新建 / 改名 / 删除 / 复制路径 / @ 插入
    // -------------------------------------------------------------------

    /// 「新建文件/新建目录」= Zed project_panel `new_entry` 移植：不弹窗，
    /// 目标目录行下方插入一行行内编辑器（无 placeholder），Enter 创建 /
    /// Esc 取消 / 点别处按 Zed 失焦语义收场。名字可含子目录，尾随 `/`
    /// 升级为目录（Zed filename_indicates_dir）。
    pub(crate) fn open_file_new(&mut self, dir: PathBuf, is_dir: bool, cx: &mut Context<Self>) {
        self.dlg.tree_menu = None;
        self.dlg.editor_menu = None;
        // 「只看变更」/Pin 收藏夹视图放不下新建行（视图里没有目标目录行）：
        // 回退树模式
        if self.ws.changes_only || self.ws.pin_view {
            self.ws.changes_only = false;
            self.ws.pin_view = false;
            self.rebuild_tree();
            self.persist_ui();
        }
        // 目标目录可见且展开（Zed new_entry：directory_id 沿途展开）
        self.expand_tree_dir(&dir);
        let weak = cx.entity().downgrade();
        let input = cx.new(|cx| TextInput::new(cx).compact().flat());
        wire_tree_edit(&weak, &input, cx);
        // 首发聚焦请求（Zed `add_entry` → `update_visible_entries(... ,
        // focus_filename_editor: true, ...)` 同位）：一次性，输入框渲染时
        // 内件建好即兑现为焦点；此后由焦点级联按「编辑存续期 = 焦点所有者」
        // 兜底，不再重复请求。
        input.update(cx, |ti, tcx| ti.focus_soon(tcx));
        // 编辑行滚进视口（Zed new_entry：虚拟行即选中行 → update_visible_entries
        // autoscroll 同位）：根下新建行排列表最顶，视窗停在下方时键入全程
        // 不可见。deferred 滚动（reveal 同构：按 sticky 链收缩视口后居中）
        // 下一帧按真实视口高夹紧执行，编辑行出界才最小滚动
        let (slot, depth) = self.tree_new_edit_slot(&dir);
        crate::ui::vlist::scroll_handle(super::file_tree::FILE_TREE_LIST_ID)
            .scroll_to_item_with_offset(slot, gpui::ScrollStrategy::Center, depth);
        self.dlg.tree_edit = Some(TreeEdit {
            kind: TreeEditKind::New { dir },
            is_dir,
            input,
            validation: TreeValidation::None,
        });
        cx.notify();
    }

    /// 「改名」= Zed project_panel `rename_impl` 移植：行内编辑器预填原
    /// 名，文件选 stem（扩展名不选）、目录全选；Enter 提交 / Esc 取消 /
    /// 点别处收场。`reveal` = 编辑区右键菜单入口：先切到文件树面板并滚
    /// 动到该行（Zed「改名聚焦 project panel」同语义）。
    pub(crate) fn open_file_rename(
        &mut self,
        path: PathBuf,
        is_dir: bool,
        reveal: bool,
        cx: &mut Context<Self>,
    ) {
        self.dlg.tree_menu = None;
        self.dlg.editor_menu = None;
        if reveal {
            self.layout.dock_panel = DockPanel::Files;
            self.reveal_tree_row(&path);
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        // Zed rename_impl 选区：目录无扩展名选全名，文件选 stem
        let sel_end = if is_dir {
            name.len()
        } else {
            std::path::Path::new(&name)
                .file_stem()
                .map(|s| s.to_string_lossy().len())
                .unwrap_or(name.len())
        };
        let weak = cx.entity().downgrade();
        let input = cx.new(|cx| TextInput::new(cx).compact().flat());
        input.update(cx, |ti, cxi| ti.set_value(name, cxi));
        input.update(cx, |ti, cxi| ti.select_range(0..sel_end, cxi));
        wire_tree_edit(&weak, &input, cx);
        // 首发聚焦请求同 open_file_new（Zed `rename_impl` → 同一条
        // focus_filename_editor 请求）
        input.update(cx, |ti, tcx| ti.focus_soon(tcx));
        self.dlg.tree_edit = Some(TreeEdit {
            kind: TreeEditKind::Rename { path },
            is_dir,
            input,
            validation: TreeValidation::None,
        });
        cx.notify();
    }

    /// 082 文件树 F2 改名：条目选中（点击/右键留存）时 F2 = 右键菜单
    /// 「改名」同一路径。`reveal` = 祖先展开 + 滚动到行（选中后滚出视口
    /// 也能改）。文件树不可见（面板隐藏/最小化/git 视图）或有模态弹层时
    /// 不接管。返回是否接管了这次按键（接管方 stop_propagation）。
    pub(crate) fn tree_f2_rename(&mut self, cx: &mut Context<Self>) -> bool {
        if self.modal_layer_open()
            || self.layout.panes_hidden
            || self.layout.files_min
            || self.layout.dock_panel == DockPanel::Git
        {
            return false;
        }
        let Some(sel) = self.ws.tree_selected.clone() else {
            return false;
        };
        // 行已不在树里（外部删除/切项目残留）→ 静默忽略
        let Some(row) = self.ws.tree_rows.iter().find(|r| same_path(&r.path, &sel)) else {
            return false;
        };
        self.open_file_rename(row.path.clone(), row.is_dir, true, cx);
        true
    }

    /// 模态与弹层集中守卫：任一打开时，根节点按键兜底的域语义（082 文件
    /// 树 F2 改名不接管；Ctrl+←/→ 切视图退回组件词移动）——此时焦点级联
    /// 都不归对应域，接管会把动作藏到弹层背后。纯输入框焦点不用这条守卫：
    /// 那些键会被 Input 组件绑定先吞（词移动/复制），根本到不了根兜底。
    pub(crate) fn modal_layer_open(&self) -> bool {
        self.dlg.dialog.is_some()
            || self.dlg.settings.is_some()
            || self.dlg.pill_menu.is_some()
            || self.dlg.pi_extension_picker.is_some()
            || self.dlg.mcp_picker.is_some()
            || self.dlg.model_picker.is_some()
            || self.dlg.hover_card.is_some()
            || self.dlg.renaming.is_some()
            || self.dlg.confirm_delete.is_some()
            || self.dlg.tree_menu.is_some()
            || self.dlg.tree_edit.is_some()
            || self.dlg.editor_menu.is_some()
            || self.dlg.term_menu.is_some()
            || self.dlg.input_menu.is_some()
            || self.dlg.msg_menu.is_some()
            || self.dlg.tab_menu.is_some()
            || self.dlg.tree_confirm_del.is_some()
            || self.dlg.tree_del_error.is_some()
            || self.dlg.tree_proj_menu.is_some()
            || self.aux.ask.as_ref().is_some_and(|a| a.open)
            || self.aux.ext_dialog.is_some()
    }

    /// 行内编辑提交（Zed `confirm_edit` 移植）。`via` 区分两条收场路：
    /// - Enter：空名/重名/非法名 → 留在编辑态（校验消息在行上显示）；
    /// - 点别处/失焦：一律收场——空名 = 丢弃，重名 = 静默丢弃（Zed
    ///   confirm_edit 返回 None → discard_edit_state 同款）。
    /// 提交动作：新建支持子目录段与尾随 `/` 升级目录；Windows 剥尾随点。
    /// `value` = 键入值，随回调传入（禁止回读输入框实体，见 wire_tree_edit）。
    pub(crate) fn tree_edit_confirm(
        &mut self,
        via: TreeEditVia,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(mut edit) = self.dlg.tree_edit.clone() else {
            return;
        };
        let mut filename = value.to_string();
        // Zed confirm_edit：Windows 下尾随点被文件系统忽略，剥掉防错位
        #[cfg(windows)]
        while let Some(trimmed) = filename.strip_suffix('.') {
            filename = trimmed.to_string();
        }
        if filename.trim().is_empty() {
            if via == TreeEditVia::Dismiss {
                self.tree_edit_discard(cx);
            }
            return;
        }
        let indicates_dir = filename.ends_with('/') || filename.ends_with('\\');
        let filename = filename.trim_start_matches(['/', '\\']).to_string();
        // Zed RelPath 语义：禁空段 / "." / ".."（绝对路径由越界护栏兜底）
        let segs: Vec<&str> = filename.split(['/', '\\']).collect();
        let name_ok = !segs.iter().any(|s| s.is_empty() || *s == "." || *s == "..");
        if !name_ok {
            if via == TreeEditVia::Dismiss {
                self.tree_edit_discard(cx);
            } else {
                edit.validation =
                    TreeValidation::Error(tr("tree.name_illegal_segment").to_string());
                cx.notify();
            }
            return;
        }
        match &edit.kind {
            TreeEditKind::New { dir } => {
                // 尾随 / 升级目录（仅新建，Zed edit_state.is_dir 同款）
                if indicates_dir {
                    edit.is_dir = true;
                }
                let target = dir.join(&filename);
                if target.exists() {
                    if via == TreeEditVia::Dismiss {
                        self.tree_edit_discard(cx);
                    } else {
                        if let Some(slot) = self.dlg.tree_edit.as_mut() {
                            slot.validation = TreeValidation::Error(existing_msg(&filename));
                        }
                        cx.notify();
                    }
                    return;
                }
                self.dlg.tree_edit = None;
                self.file_new_commit(dir, edit.is_dir, filename, cx);
            }
            TreeEditKind::Rename { path } => {
                let Some(parent) = path.parent() else { return };
                let target = parent.join(&filename);
                if same_path(&target, path) {
                    // 字面同名：Enter = 留在编辑（Zed refocus 分支），
                    // 点别处 = 取消
                    if via == TreeEditVia::Dismiss {
                        self.tree_edit_discard(cx);
                    } else {
                        cx.notify();
                    }
                    return;
                }
                if target.exists() {
                    if via == TreeEditVia::Dismiss {
                        self.tree_edit_discard(cx);
                    } else {
                        if let Some(slot) = self.dlg.tree_edit.as_mut() {
                            slot.validation = TreeValidation::Error(existing_msg(&filename));
                        }
                        cx.notify();
                    }
                    return;
                }
                self.dlg.tree_edit = None;
                self.file_rename_commit(path, edit.is_dir, filename, cx);
            }
        }
    }

    /// 键入即校验（Zed populate_validation_error 移植）：首尾空白 = 警告，
    /// 目标已存在 = 错误（同名自身除外），其余清空。`value` 随回调传入。
    pub(crate) fn tree_edit_validate(&mut self, value: &str, cx: &mut Context<Self>) {
        let Some(edit) = self.dlg.tree_edit.as_mut() else {
            return;
        };
        let filename = value.to_string();
        edit.validation = if filename.trim().is_empty() {
            TreeValidation::None
        } else if filename.trim() != filename {
            TreeValidation::Warning(tr("tree.name_has_whitespace").to_string())
        } else {
            let f = filename.trim_start_matches(['/', '\\']);
            let target = match &edit.kind {
                TreeEditKind::New { dir } => dir.join(f),
                TreeEditKind::Rename { path } => match path.parent() {
                    Some(parent) => parent.join(f),
                    None => PathBuf::from(f),
                },
            };
            let is_same_entry = match &edit.kind {
                TreeEditKind::Rename { path } => same_path(&target, path),
                TreeEditKind::New { .. } => false,
            };
            if !is_same_entry && target.exists() {
                TreeValidation::Error(existing_msg(f))
            } else {
                TreeValidation::None
            }
        };
        cx.notify();
    }

    /// 取消行内编辑（Esc；Zed cancel → discard_edit_state）。
    pub(crate) fn tree_edit_discard(&mut self, cx: &mut Context<Self>) {
        if self.dlg.tree_edit.take().is_some() {
            cx.notify();
        }
    }

    /// 面板「点别处提交」（文件树 mouse-down 冒泡入口）：现读键入值再走
    /// Dismiss。这里的实体读发生在面板的 mouse-down 派发栈里——输入框
    /// 自身的点击在 edit_input_slot 已 stop_propagation，绝无派发中的
    /// 输入框租借（与快照读同类）。
    pub(crate) fn tree_edit_confirm_from_click(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.dlg.tree_edit.clone() else {
            return;
        };
        let value = edit.input.read(cx).value().to_string();
        self.tree_edit_confirm(TreeEditVia::Dismiss, &value, cx);
    }

    /// 沿途展开 `dir` 的祖先链与自身并重建展平表（新建行要落在可见的
    /// 目录行下方，Zed new_entry 展开 directory_id 同款）。项目根无需展开。
    fn expand_tree_dir(&mut self, dir: &Path) {
        let cwd = self.ws.cwd.clone();
        let mut anc = Some(dir.to_path_buf());
        let mut opened = false;
        while let Some(p) = anc {
            if cwd.as_deref().is_some_and(|cur| same_path(&p, cur)) {
                break;
            }
            opened |= self.ws.expanded_dirs.insert(p.clone());
            anc = p.parent().map(|q| q.to_path_buf());
        }
        if opened {
            self.rebuild_tree();
        }
    }

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

    /// 「删除」第一步：挂统一确认浮层（overlay::confirm，居中）。
    pub(crate) fn confirm_tree_delete(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.dlg.tree_menu = None;
        self.dlg.editor_menu = None;
        self.dlg.tree_confirm_del = Some(path);
        cx.notify();
    }

    /// 删除确认的「确认」键（UI 按钮与 file.delete_confirm 自动化 op 同一
    /// 方法）：取槽内路径、清槽、再 commit——取/清都在这一个方法里做，
    /// 杜绝「回调先清槽、commit 再 take 取到空」的时序（文件树删除从未
    /// 生效过的根因）。
    pub(crate) fn tree_delete_confirm_commit(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.dlg.tree_confirm_del.clone() else {
            return;
        };
        self.dlg.tree_confirm_del = None;
        self.tree_delete_commit(&path, cx);
    }

    /// 确认后真删：trash crate 送回收站（Windows 回收站 / macOS 废纸篓，
    /// 可恢复）。**trash 是同步阻塞 IO**——Windows 走 IFileOperation 枚举
    /// 整棵子树 + 逐文件搬进 $Recycle.Bin + Defender 逐文件扫描，大目录
    /// （node_modules/target 级）曾把主线程整段卡死。现主线程只投递
    /// （挂 tree_deleting 在飞状态 + 状态栏提示），真删下沉
    /// background_spawn（refresh_git 同款 spawn 范式），完成后回主线程
    /// 走 [`Self::tree_delete_applied`] 收尾（tab 丢弃/清枝/git 随刷）。
    pub(crate) fn tree_delete_commit(&mut self, path: &Path, cx: &mut Context<Self>) {
        // 兜底护栏：项目根不可删（操作栏右键菜单已豁免该入口，这里防
        // 其他调用路径误投递）
        if self
            .ws
            .cwd
            .as_deref()
            .is_some_and(|cur| same_path(path, cur))
        {
            self.set_status(tr("tree.delete_root_denied").to_string(), cx);
            cx.notify();
            return;
        }
        // 同路径去重（双击/自动化重复投递；不同路径允许并行在飞，
        // 各自收尾 retain 语义幂等互不干扰）
        if self.dlg.tree_deleting.iter().any(|p| same_path(p, path)) {
            return;
        }
        self.dlg.tree_deleting.push(path.to_path_buf());
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());
        self.set_status(crate::i18n::tf("tree.deleting", &[("name", name)]), cx);
        cx.notify();
        let path_bg = path.to_path_buf();
        let path_done = path_bg.clone();
        cx.spawn(async move |this, cx| {
            // 双保险：trash 内部若再 panic（如 COM 模式冲突），在 gpui 线程
            // 池的 FFI 回调里无法 unwind、会整进程 abort——就地 catch 降级
            // 成 Err，走既有错误弹层而不是崩溃。
            let res = cx
                .background_spawn(async move {
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        trash::delete(&path_bg)
                    }))
                    .unwrap_or_else(|_| {
                        Err(trash::Error::Unknown {
                            description: "trash::delete panicked".into(),
                        })
                    })
                })
                .await;
            let _ = this.update(cx, |chat, cx| {
                chat.dlg.tree_deleting.retain(|p| !same_path(p, &path_done));
                match res {
                    Ok(_) => chat.tree_delete_applied(&path_done, cx),
                    Err(e) => {
                        // 持久弹层：状态栏 toast 一闪容易漏看，用户回报也
                        // 拿不到具体错误
                        chat.dlg.tree_del_error = Some(e.to_string());
                        chat.set_status(
                            crate::i18n::tf("extensions.remove_failed", &[("e", e.to_string())]),
                            cx,
                        );
                        cx.notify();
                    }
                }
            });
        })
        .detach();
    }

    /// 删除成功的主线程收尾（原 tree_delete_commit Ok 分支原样迁出，
    /// 语义不变）。成功静默（同创建：成功不提示，失败才弹）。
    fn tree_delete_applied(&mut self, path: &Path, cx: &mut Context<Self>) {
        let stale: Vec<PathBuf> = self
            .ws
            .panel_tabs
            .iter()
            .filter_map(|t| match t {
                PanelTab::File(p) if same_path(p, path) || p.starts_with(path) => Some(p.clone()),
                _ => None,
            })
            .collect();
        for p in stale {
            self.discard_file_tab(&p, cx);
        }
        // 兜底：file_cache 可能残留无 tab 的 FileTab（历史关闭路径
        // 只摘 tab 没清缓存），按路径一并清，防编辑器还渲染已删文件
        self.ws
            .file_cache
            .retain(|k, _| !(same_path(k, path) || k.starts_with(path)));
        self.ws
            .expanded_dirs
            .retain(|d| !(same_path(d, path) || d.starts_with(path)));
        // 021 Pin 收藏清理：被删条目（目录连子树）出收藏夹
        let pins_before = self.ws.pinned.len();
        self.ws
            .pinned
            .retain(|p| !(same_path(p, path) || p.starts_with(path)));
        if self.ws.pinned.len() != pins_before {
            self.save_pins();
        }
        self.refresh_git(cx);
        cx.notify();
    }

    /// 「复制路径」：绝对路径进剪贴板（gpui 原生 ClipboardItem）。
    pub(crate) fn tree_copy_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.dlg.tree_menu = None;
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
            path.to_string_lossy().to_string(),
        ));
        self.set_status(tr("session.copied").to_string(), cx);
        cx.notify();
    }

    /// 「@到会话输入框」：相对 cwd 路径经 build_at_insert_text（含空格
    /// 自动引号、目录不闭合供钻取）追加到输入框末尾，光标落在插入之后；
    /// 内容区切回会话让插入可见（在终端/文件视图时）。
    pub(crate) fn tree_at_insert(&mut self, path: PathBuf, is_dir: bool, cx: &mut Context<Self>) {
        self.dlg.tree_menu = None;
        let rel = path
            .strip_prefix(self.ws.cwd.as_deref().unwrap_or(Path::new("")))
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let (text, off) = crate::services::at_file::build_at_insert_text(&rel, is_dir);
        let mut base = std::mem::take(&mut self.composer.input);
        if !base.is_empty() && !base.ends_with(' ') && !base.ends_with('\n') {
            base.push(' ');
        }
        let cursor = base.len() + off;
        base.push_str(&text);
        self.set_input_with_cursor(base, cursor, cx);
        self.set_content_view(ContentView::Chat);
    }
}

/// 行内编辑的收场来路：Enter 显式提交 vs 点别处/失焦（Zed confirm_edit
/// 的 `refocus` 参数同义——true = Enter，false = Dismiss）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TreeEditVia {
    Enter,
    Dismiss,
}

/// 重名校验消息（Zed "File or directory '{}' already exists at location."
/// 同义，走三语）。
fn existing_msg(name: &str) -> String {
    crate::i18n::tf("tree.exists_named", &[("name", name.to_string())])
}

/// 行内编辑回调统一接线：Enter 提交 / Esc 取消 / 失焦与点别处提交、键入
/// 即校验（Zed filename_editor 的 EditorEvent 订阅同款）。值一律随回调
/// 参数传递——回调可能在输入框实体自身的事件派发栈内执行（set_value 的
/// Change 同步回流），回读实体 = entity_map 双重租借 panic（门面文档明
/// 令禁止）。提交会 drop 派发事件的输入框，defer 出帧。
fn wire_tree_edit(
    weak: &gpui::WeakEntity<Chat>,
    input: &gpui::Entity<TextInput>,
    cx: &mut Context<Chat>,
) {
    input.update(cx, |ti, _| {
        let weak_submit = weak.clone();
        ti.set_on_submit(Box::new(move |v, cx| {
            let weak = weak_submit.clone();
            let v = v.to_string();
            cx.defer(move |cx| {
                let _ = weak.update(cx, |c, cx| c.tree_edit_confirm(TreeEditVia::Enter, &v, cx));
            });
        }));
        let weak_esc = weak.clone();
        ti.set_on_escape(Box::new(move |cx| {
            let weak = weak_esc.clone();
            cx.defer(move |cx| {
                let _ = weak.update(cx, |c, cx| c.tree_edit_discard(cx));
            });
        }));
        let weak_blur = weak.clone();
        ti.set_on_blur(Box::new(move |v, cx| {
            let weak = weak_blur.clone();
            let v = v.to_string();
            cx.defer(move |cx| {
                let _ = weak.update(cx, |c, cx| {
                    c.tree_edit_confirm(TreeEditVia::Dismiss, &v, cx)
                });
            });
        }));
        let weak_change = weak.clone();
        ti.set_on_change(Box::new(move |v, cx| {
            let weak = weak_change.clone();
            let v = v.to_string();
            let _ = weak.update(cx, |c, cx| c.tree_edit_validate(&v, cx));
        }));
    });
}
