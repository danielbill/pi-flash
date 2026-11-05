//! 021 文件树行内编辑流（Zed project_panel new_entry/rename_impl/confirm_edit
//! 移植）：新建/改名的行内编辑器打开、提交校验（重名/非法段/Windows 尾随
//! 点）、取消与点别处收场，回调统一接线。自 file_actions.rs 按动作族切出
//!（itlq.12 超规拆分，纯结构搬移）。

use super::{TreeEdit, TreeEditKind, TreeValidation};
use crate::*;

impl Chat {
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
            // dock_panel 不再切 Files（与 Default 渲染无差别，020-1 遗留已清）
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
