//! 021 文件树操作栏 + 右键菜单的文件动作：切换显示变更 / 搜索文件弹窗 /
//! Pin 收藏夹 / 复制路径 / @ 插入会话输入框 / 模态守卫（资源管理器定位在
//! services::shell_open）。（P2 自根 actions_files.rs 归并入 function_panel
//! 域。itlq.12 超规拆分后行内编辑流=file_actions_edit、fs 提交=
//! file_actions_fs、删除回收站=file_actions_delete。）

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
    /// 文件名过滤清单）退役，入口统一到 Zed 式搜索面板（Ctrl+L
    /// / 本按钮两条路同一个方法），面板打开即聚焦查询框。
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
    pub(crate) fn save_pins(&mut self) {
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
    // 模态守卫 / 杂项
    // -------------------------------------------------------------------

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
