//! Inline rename + provider-picker dialog (042).

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


    /// 042 ProviderPicker 弹窗构造：搜索输入每敲一字通知重渲染（清单按值
    /// 过滤在渲染期读取快照）。ESC 交给浮层公共基座，这里只兜输入框内路径。
    pub(crate) fn provider_picker_dialog(cx: &mut Context<Self>) -> Dialog {
        let weak_esc = cx.entity().downgrade();
        let input = cx.new(|cx| TextInput::new(cx).placeholder(tr("搜索 Provider...")));
        input.update(cx, |ti, _| {
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
        Dialog::ProviderPicker { input }
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
