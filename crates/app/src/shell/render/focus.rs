//! 焦点仲裁：render 每帧的焦点保持/回收 if-else 链（弹窗/设置/搜索/市场/
//! 终端/文件树行内编辑各白名单 + 末级「回家」分支）。

use crate::*;

impl Chat {
    /// keep terminal focus alive across frames (render focuses chat input
    /// otherwise, which would steal it back every redraw)
    pub(super) fn focus_arbitrate(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Chat>,
    ) {
        let dialog_input = match &self.dlg.dialog {
            Some(Dialog::ProviderPicker { input, .. })
            | Some(Dialog::ProjectPicker { input, .. }) => Some(input.clone()),
            _ => None,
        }
        .or_else(|| {
            // 042：模型菜单的过滤输入同享焦点保持（每帧不被 composer 抢回）；
            // 过滤框未渲染（<10 条）时不抢——菜单退化为纯鼠标操作
            self.dlg
                .model_picker
                .as_ref()
                .filter(|_| self.picker_filter_shown(cx))
                .map(|p| p.input.clone())
        });
        // 详情卡的原地改名输入框也要持有焦点——否则每帧的焦点回收
        // 会把它抢回主输入框，键盘输入进不去
        let card_rename_focus = self
            .dlg
            .hover_card
            .as_ref()
            .and_then(|c| c.rename_input.clone());
        let rename_focus = self.dlg.rename_input.clone().or(card_rename_focus);
        if let Some(input) = rename_focus.or(dialog_input) {
            let handle = input.read(cx).focus_handle_in(cx);
            if !handle.is_focused(window) {
                window.focus(&handle);
            }
        } else if self.aux.ask.as_ref().is_some_and(|a| a.open) {
            // 问卷卡焦点保持：卡片持 dialog_focus（Tab/Enter/数字键），
            // 用户点进自定义答案输入框后不被抢回；两者都不在时回收给卡片
            let in_input = self
                .aux
                .ask_input
                .read(cx)
                .focus_handle_in(cx)
                .is_focused(window);
            if !in_input && !self.dlg.dialog_focus.is_focused(window) {
                window.focus(&self.dlg.dialog_focus);
            }
        } else if self.aux.ext_dialog.is_some() {
            let ext_text = matches!(
                self.aux.ext_dialog.as_ref().map(|r| &r.method),
                Some(
                    pi_link::protocol::ExtUiMethod::Input { .. }
                        | pi_link::protocol::ExtUiMethod::Editor { .. }
                )
            );
            // ext_input 必须用内件真句柄：focus_handle() 是建件前的 fallback，
            // 不在元素树里，推上焦点位后焦点路径变空（见 text_input.rs has_inner
            // 注释）；且每帧仲裁会把用户点进内件的焦点抢回——RPC 扩展弹窗
            // （ask_user_question 的 input/多选填数题）因此打不进字。
            // 内件未建（弹窗首帧）时本轮不抢，等 has_inner 后下一帧再聚焦。
            let handle = if ext_text {
                self.aux
                    .ext_input
                    .read(cx)
                    .has_inner()
                    .then(|| self.aux.ext_input.read(cx).focus_handle_in(cx))
            } else {
                Some(self.dlg.dialog_focus.clone())
            };
            if let Some(handle) = handle {
                if !handle.is_focused(window) {
                    window.focus(&handle);
                }
            }
        } else if self.dlg.dialog.is_some()
            // overlay::confirm 两槽（文件树删除/删除失败）不占 Dialog 枚举，
            // 但同样要一打开就持有焦点——Enter 确认 / ESC 取消才能立即生效
            //（zed window.prompt 同款，键盘可操作）
            || self.dlg.tree_confirm_del.is_some()
            || self.dlg.tree_del_error.is_some()
            // 文件树标题项目下拉：↑/↓/Enter 键盘导航的前提是 dialog_focus
            // 持焦（overlay::confirm 同款键盘路径），打开即接管焦点
            || self.dlg.tree_proj_menu.is_some()
        {
            if !self.dlg.dialog_focus.is_focused(window) {
                window.focus(&self.dlg.dialog_focus);
            }
        } else if let Some(panel) = self.dlg.settings.as_ref() {
            // 焦点在面板任意输入框内就不抢（白名单在 v60/v70 两度漏新输入框，
            // 现由 SettingsPanel::focus_within 结构化自检）
            if !panel.read(cx).focus_within(window, cx) && !self.dlg.dialog_focus.is_focused(window)
            {
                window.focus(&self.dlg.dialog_focus);
            }
        } else if matches!(
            self.layout.content_view,
            crate::ContentView::Search | crate::ContentView::SessionSearch
        ) && self.search_input_focused(window, cx)
        {
            // 083：搜索条输入框持焦不回收（同 040-1 市场搜索框白名单）——
            // 否则每帧仲裁把焦点推回中性锚点，搜索框打不进字
        } else if self.file_editor_focus(window, cx).is_some() {
            // 023：文件编辑器持有焦点时不回收——否则点进去下一帧就被抢回
            // composer，打字全失效（编辑器此前不在白名单）
        } else if self.layout.content_view == crate::ContentView::PiNativeMarket
            && self
                .aux
                .market
                .native
                .input
                .as_ref()
                .is_some_and(|i| i.read(cx).focus_handle_in(cx).is_focused(window))
        {
            // 040-2：原生市场搜索框持焦不回收（同下面 040-1）
        } else if self.layout.content_view == crate::ContentView::PiExtMarket
            && self
                .aux
                .market
                .pi_ext
                .input
                .as_ref()
                .is_some_and(|i| i.read(cx).focus_handle_in(cx).is_focused(window))
        {
            // 040-1：市场搜索框持焦不回收（同文件编辑器白名单）——render
            // 每帧仲裁会把焦点抢回 composer，市场页搜索框点进去打不进字
        } else if !(self.layout.content_view == crate::ContentView::Term
            && self
                .shown_terminal_focus()
                .is_some_and(|h| h.is_focused(window)))
        {
            // 终端持焦白名单只在终端视图内生效：切回 chat/文件后终端元素
            // 已卸载，但 gpui 的 focus handle 不会随卸载自动 blur，残留持焦
            // 会把每帧的焦点回收永久挡在 composer 之外（打字无处可去）
            //
            // 文件树行内编辑（Zed project_panel `filename_editor`）：编辑
            // 存续期它就是焦点所有者——Zed 里这条由面板列表 `track_focus`
            // 承担（编辑行是面板的一部分，焦点不会跑到别处），pi-flash 的
            // 面板不可聚焦，故在此把它排在 composer 之前。每帧只在「没被
            // 别人夺走」时补焦：抢在级联末级之前让出 composer 回收，编辑行
            // 不再被逐帧抢回——那正是「改名焦点捕捉不到」的根因（首发聚焦
            // 请求见 open_file_new / open_file_rename 的 focus_soon；用户点
            // 别处造成的真夺焦（对话框/终端/编辑器/设置）走前面各分支，本
            // 分支看不到，blur 收场照常触发）。020-1：文件树并入整合面板
            // 常驻显示，只要面板不是 git 视图就按在树前处理。
            match self
                .dlg
                .tree_edit
                .as_ref()
                .filter(|_| self.layout.dock_panel != DockPanel::Git)
            {
                Some(edit) if edit.input.read(cx).has_inner() => {
                    let handle = edit.input.read(cx).focus_handle_in(cx);
                    if !handle.is_focused(window) {
                        window.focus(&handle);
                    }
                }
                // 内件未就绪（打开当帧）：本帧让位，输入框渲染时由 focus_soon
                // 一次性首发（Zed focus_filename_editor 请求同位）
                Some(_) => {}
                None => {
                    // 082 三期焦点归属（切 tab 不再自动聚焦内容区）：只有
                    // 两个「回家」分支保留逐帧补焦——Chat = composer（home
                    // 输入位），md 预览 = 预览容器（被动只读，方向键滚动
                    // 无输入副作用）；Term 与 File 源码态不抢：内容区点击
                    // 才接管键盘，中性态 = blur（键事件按根派发，全局键与
                    // Ctrl+←/→ 照常可用）
                    if self.layout.content_view == crate::ContentView::Chat {
                        window.focus(&self.focus_handle(cx));
                    } else if self.md_preview_active()
                        && !self.ws.md_preview_focus.is_focused(window)
                    {
                        window.focus(&self.ws.md_preview_focus);
                    } else {
                        // 中性归属 = content-main 锚点（track_focus 在树上，
                        // 路径含 "ContentTabs"）：切标签/中性态后 Ctrl+←/→
                        // 仍可连续切。window.focus() 同值早退，逐帧空转
                        //（曾用 blur()——它无同值早退且空焦点派发路径只
                        // 剩 node0，on_action 够不着 = 连续切换卡死，验收
                        // 已证伪，勿回退）。锚点宿主在 content.rs（勿挂
                        // content-col：topbar_r 在其子树，会吞 NC 窗口钮）
                        window.focus(&self.layout.focus);
                    }
                }
            }
        }
    }
}
