//! file/files 组：文件 tab 开存关、编辑器直调、右键菜单、视图模式、删除流、
//! 文件树（折叠/行内编辑/项目下拉/Pin）。

use gpui::{Context, Window};
use serde_json::{Value, json};

use super::{OpResult, bad, not_found, ok, params_path};
use crate::Chat;
use pi_link::automation::method;

pub(super) fn dispatch(
    chat: &mut Chat,
    window: &mut Window,
    cx: &mut Context<Chat>,
    m: &str,
    params: &Value,
) -> OpResult {
    match m {
        method::FILE_OPEN => {
            let path = params_path(params, "path")?;
            if !path.is_file() {
                return Err(not_found(format!("文件不存在: {}", path.display())));
            }
            chat.open_file_tab(path, cx);
            ok()
        }
        // 023 fileView：保存 / 关闭。close 对脏缓冲会转成确认弹窗——事务
        // 范式里用 ui.snapshot 的 dialogs surface + file.close 收尾。
        method::FILE_SAVE => {
            let path = params_path(params, "path")?;
            chat.save_file(&path, cx);
            ok()
        }
        method::FILE_CLOSE => {
            let path = params_path(params, "path")?;
            if !chat
                .ws
                .panel_tabs
                .iter()
                .any(|t| matches!(t, crate::PanelTab::File(p) if crate::services::workspace::same_path(p, &path)))
            {
                return Err(not_found(format!("文件 tab 未打开: {}", path.display())));
            }
            chat.close_file_tab(&path, cx);
            ok()
        }
        method::FILE_SET_TEXT => {
            let path = params_path(params, "path")?;
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"text\": \"内容\"}"))?
                .to_string();
            let ed = chat
                .ws
                .file_cache
                .get(&path)
                .and_then(|f| f.editor.clone())
                .ok_or_else(|| not_found("文件没有编辑器（未渲染）"))?;
            ed.update(cx, |st, scx| st.set_value(text, window, scx));
            // set_value 发 Change → dirty 按「编辑器值 != 磁盘真值」比较落位
            cx.notify();
            ok()
        }
        // 冲突横幅「重新加载」：磁盘为准（丢弃本地未保存修改）——与 banner
        // 按钮同一方法，让冲突裁决路径可被自动化断言
        method::FILE_RELOAD => {
            let path = params_path(params, "path")?;
            if !chat.ws.file_cache.contains_key(&path) {
                return Err(not_found(format!("文件 tab 未打开: {}", path.display())));
            }
            chat.file_reload_from_disk(&path, cx);
            ok()
        }
        // 编辑区右键菜单（自动化验证入口：状态与真右键同槽，配合 shot
        // 断言菜单渲染/收起；x/y 缺省给视口中上部）
        method::FILE_CONTEXT_MENU => {
            let path = params_path(params, "path")?;
            if !chat.ws.file_cache.contains_key(&path) {
                return Err(not_found(format!("文件 tab 未打开: {}", path.display())));
            }
            let x = params.get("x").and_then(Value::as_f64).unwrap_or(600.) as f32;
            let y = params.get("y").and_then(Value::as_f64).unwrap_or(160.) as f32;
            chat.dlg.editor_menu = Some((path, x, y));
            cx.notify();
            ok()
        }
        // 编辑区右键菜单收起（file.context_menu 的对向 op，与点菜单项外
        // 区域同效——清槽）。幂等：无菜单开着也 ok，回 had_menu 供断言
        method::FILE_CONTEXT_MENU_CLOSE => {
            let had_menu = chat.dlg.editor_menu.is_some();
            chat.dlg.editor_menu = None;
            cx.notify();
            Ok(json!({"ok": true, "had_menu": had_menu}))
        }
        method::FILE_VIEW_MODE => {
            let path = params_path(params, "path")?;
            let mode = params
                .get("mode")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"mode\": \"source|preview\"}"))?;
            let source = match mode {
                "source" => true,
                "preview" => false,
                other => return Err(bad(format!("未知 mode: {other}"))),
            };
            match chat.ws.file_cache.get_mut(&path) {
                Some(ft) => {
                    // 双态类型才有视图模式可切（仅 md）：其余静默成功（op
                    // 语义同前——不报错，只是没有可切换的态）
                    if crate::editor::kind::kind_of(&path).supports_view_mode() {
                        ft.md_source = source;
                    }
                }
                None => return Err(not_found(format!("文件 tab 未打开: {}", path.display()))),
            }
            cx.notify();
            ok()
        }
        // 文件树删除流（自动化验证入口）：与右键菜单「删除」同槽同方法——
        // 弹确认浮层（真删在 file.delete_confirm 应答后）
        method::FILE_DELETE => {
            let path = params_path(params, "path")?;
            if !path.exists() {
                return Err(not_found(format!("文件不存在: {}", path.display())));
            }
            chat.confirm_tree_delete(path, cx);
            ok()
        }
        // 删除确认浮层的「确认」（与确认按钮同一方法）：清槽 + 投递后台
        // trash 真删（大目录回收站搬运可达分钟级，不再阻塞主线程；完成
        // 后由 watcher/git 刷新回灌树）
        method::FILE_DELETE_CONFIRM => {
            if chat.dlg.tree_confirm_del.is_none() {
                return Err(bad("没有待确认的删除（tree_confirm_del 为空）"));
            }
            chat.tree_delete_confirm_commit(cx);
            ok()
        }
        method::FILES_TOGGLE_DIR => {
            let path = params_path(params, "path")?;
            if !chat.ws.expanded_dirs.remove(&path) {
                chat.ws.expanded_dirs.insert(path);
            }
            chat.rebuild_tree();
            cx.notify();
            ok()
        }
        // 021 行内改名：打开编辑行（可选 name 预填新名）。提交/取消走
        // input.keys（enter/escape）或 input.focus（失焦收场），断言读
        // files 面快照的 tree_edit 字段
        method::FILES_RENAME => {
            let path = params_path(params, "path")?;
            if !path.exists() {
                return Err(not_found(format!("条目不存在: {}", path.display())));
            }
            let is_dir = path.is_dir();
            chat.open_file_rename(path, is_dir, false, cx);
            tree_edit_prefill(chat, params, cx);
            ok()
        }
        // 021 行内新建：在 dir 下开编辑行（is_dir = 建目录；尾随 `/` 名字
        // 同样会升级为目录）。文件/目录菜单的行内语义直通。
        method::FILES_NEW_ENTRY => {
            let dir = params_path(params, "dir")?;
            if !dir.is_dir() {
                return Err(not_found(format!("目录不存在: {}", dir.display())));
            }
            let is_dir = params
                .get("is_dir")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            chat.open_file_new(dir, is_dir, cx);
            tree_edit_prefill(chat, params, cx);
            ok()
        }
        method::FILES_PROJECT_MENU => {
            // 文件树标题项目下拉的无坐标入口（model.picker_open 同款）：
            // toggle 开/收；↑/↓/Enter 走 input.keys，状态读 files 面 proj_menu
            chat.toggle_tree_proj_menu(cx);
            ok()
        }
        method::FILES_PIN_VIEW => {
            // 021 Pin 收藏夹视图的无坐标入口：toggle 树体区域视图开关；
            // 开合读 files 面 pin_view，视图行读 rows
            chat.toggle_pin_view(cx);
            ok()
        }
        method::FILES_PIN => {
            // 与右键菜单「Pin/取消固定」同一方法（toggle）；清单读 files 面 pins
            let path = params_path(params, "path")?;
            if !path.exists() {
                return Err(not_found(format!("条目不存在: {}", path.display())));
            }
            chat.tree_pin_toggle(path, cx);
            ok()
        }

        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}

/// 021 行内编辑的预填（files.rename / files.new_entry 的可选 `name`）：
/// 打开编辑行后直接把新名灌进输入框，省一次 input.keys 逐字键入。
fn tree_edit_prefill(chat: &mut Chat, params: &Value, cx: &mut gpui::Context<Chat>) {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return;
    };
    if let Some(edit) = chat.dlg.tree_edit.as_ref() {
        let name = name.to_string();
        edit.input.update(cx, |ti, cxi| ti.set_value(name, cxi));
    }
}
