//! 021 文件树删除动作：统一确认浮层 → trash crate 送回收站（Windows
//! 回收站 / macOS 废纸篓，可恢复）。**trash 是同步阻塞 IO**——真删下沉
//! background_spawn，完成后回主线程收尾（tab 丢弃/清枝/git 随刷）。
//! 自 file_actions.rs 按动作族切出（itlq.12 超规拆分，纯结构搬移）。

use crate::*;

impl Chat {
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
}
