//! Inline rename + model-select dialog.

//! Split out of main.rs for the file-size budget. Child module of the
//! crate root: Chat's root-private fields stay accessible here.

use crate::*;

impl Chat {
    pub(crate) fn start_rename(&mut self, path: PathBuf, prefill: String, cx: &mut Context<Self>) {
        let weak_ok = cx.entity().downgrade();
        let weak_esc = cx.entity().downgrade();
        let input = cx.new(|cx| {
            TextInput::new(cx)
                .select_all_on_focus()
                .placeholder(tr("session name"))
        });
        input.update(cx, |ti, cx| ti.set_value(prefill, cx));
        input.update(cx, |ti, _| {
            ti.set_on_submit(Box::new(move |v, cx| {
                let _ = weak_ok.update(cx, |c, cx| c.apply_rename(v.trim().to_string(), cx));
            }));
            ti.set_on_escape(Box::new(move |cx| {
                let _ = weak_esc.update(cx, |c, cx| {
                    c.renaming = None;
                    c.rename_input = None;
                    cx.notify();
                });
            }));
        });
        self.renaming = Some(path);
        self.rename_input = Some(input);
        cx.notify();
    }


    /// Model-select dialog with a live filter input.
    pub(crate) fn model_select_dialog(cx: &mut Context<Self>) -> Dialog {
        let weak_change = cx.entity().downgrade();
        let weak_esc = cx.entity().downgrade();
        let weak_submit = cx.entity().downgrade();
        let input = cx.new(|cx| TextInput::new(cx).placeholder(tr("过滤模型...")));
        input.update(cx, |ti, _| {
            ti.set_on_change(Box::new(move |_, cx| {
                let _ = weak_change.update(cx, |c, cx| {
                    // list contents changed with the filter text — restart
                    // keyboard selection from the top
                    if let Some(Dialog::ModelSelect { sel, .. }) = &mut c.dialog {
                        *sel = 0;
                    }
                    cx.notify();
                });
            }));
            ti.set_on_submit(Box::new(move |_, cx| {
                // Enter dispatch happens while the filter input (+ its inner
                // InputState) is leased by the event chain; apply_model_sel
                // drops both (dialog = None) — doing that synchronously here
                // crashed (double-lease, 0xc0000409, same as the composer
                // incident). Defer until the dispatch cycle has settled.
                let weak = weak_submit.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| c.apply_model_sel(cx));
                });
            }));
            ti.set_on_escape(Box::new(move |cx| {
                // same dispatch-lease hazard as submit: closing the dialog
                // drops the dispatching input — defer the dismissal
                let weak = weak_esc.clone();
                cx.defer(move |cx| {
                    let _ = weak.update(cx, |c, cx| {
                        c.dialog = None;
                        cx.notify();
                    });
                });
            }));
        });
        Dialog::ModelSelect { input, sel: 0 }
    }

    /// Models visible in the picker: the active project's catalog
    /// (`catalog_for` = 该 cwd 的进程答案，无则全局磁盘 ∪ 缓存清单）, narrowed
    /// by the enabledModels whitelist and the live
    /// filter text. Shared by rendering and keyboard navigation so ↑/↓/Enter
    /// always match what is on screen. Works for process-less drafts — the
    /// catalog belongs to Chat, not to any runtime.
    pub(crate) fn filtered_models(&self, cx: &App) -> Vec<pi_link::protocol::ModelInfo> {
        let flt = match &self.dialog {
            Some(Dialog::ModelSelect { input, .. }) => input.read(cx).value().to_lowercase(),
            _ => String::new(),
        };
        let picker_enabled = !self.mc_state.all_enabled;
        // 目录取 Chat 的共享入口（`catalog_for`）：该 cwd 的进程答过 → 用它，否则
        // 回落启动装载的全局清单（磁盘 models.json/models-store.json ∪ 自有缓存）。
        // **不能直读 `models_by_cwd`**：切项目（`switch_project`）建的是**无进程
        // 草稿**（`new_session` 惰性，不像启动那个 runtime 会被 `spawn_initial_attach`
        // 拉进程），而 `ensure_models_requested` 对无进程草稿只借「同 cwd 的活
        // runtime」——新目录一个都没有 → `models_by_cwd` 无此 cwd 条目 → 直读即空
        // 列表：胶囊上写着默认模型名、弹窗里却「no models match」。
        let cwd = self.rt().read(cx).cwd.clone();
        self.catalog_for(&cwd)
            .iter()
            .filter(|m| {
                if picker_enabled {
                    let r = format!("{}/{}", m.provider, m.id);
                    if !self.mc_state.enabled.iter().any(|e| e == &r) {
                        return false;
                    }
                }
                flt.is_empty()
                    || m.id.to_lowercase().contains(&flt)
                    || m.name.to_lowercase().contains(&flt)
                    || m.provider.to_lowercase().contains(&flt)
            })
            .cloned()
            .collect()
    }

    /// ↑/↓ on the picker: move the keyboard selection (clamped).
    pub(crate) fn move_model_sel(&mut self, delta: i32, cx: &mut Context<Self>) {
        let n = self.filtered_models(cx).len().min(MODEL_PICKER_ROWS);
        if n == 0 {
            return;
        }
        if let Some(Dialog::ModelSelect { sel, .. }) = &mut self.dialog {
            *sel = ((*sel as i32) + delta).clamp(0, n as i32 - 1) as usize;
            cx.notify();
        }
    }

    /// Enter on the picker: switch to the highlighted model and close.
    pub(crate) fn apply_model_sel(&mut self, cx: &mut Context<Self>) {
        let sel = match &self.dialog {
            Some(Dialog::ModelSelect { sel, .. }) => *sel,
            _ => return,
        };
        let pick = self.filtered_models(cx).get(sel).map(|m| (m.provider.clone(), m.id.clone()));
        #[cfg(debug_assertions)]
        eprintln!("[model-picker] apply sel={sel} pick={pick:?} session={}", self.rt().read(cx).agent.session.is_some());
        if let Some((provider, id)) = pick {
            self.rt().update(cx, |r, cx| r.select_model(provider, id, cx));
        }
        self.dialog = None;
        cx.notify();
    }

    /// Rename commit path that never reads the input entity (called from
    /// the input's own submit callback where the entity is borrowed).
    pub(crate) fn apply_rename(&mut self, name: String, cx: &mut Context<Self>) {
        if let Some(session) = &self.rt().read(cx).agent.session {
            let _ = session.send(&Command::SetSessionName { name });
        }
        // sidebar label comes from the session file's `session_info` entry;
        // reload it when pi confirms the write (set_session_name response —
        // an immediate re-read here would race the file flush)
        self.refresh_state(cx);
        self.renaming = None;
        self.rename_input = None;
        cx.notify();
    }
}
