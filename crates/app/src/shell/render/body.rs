//! 布局装配：v54 body 两栏（panel-col = topbar-l + dock + statusbar |
//! content-col = topbar_r + content_main）+ 根外壳（全局 action/键位兜底）。

use crate::*;

impl Chat {
    pub(super) fn body_shell(
        &mut self,
        entity: gpui::Entity<Chat>,
        weak: gpui::WeakEntity<Chat>,
        t: &'static crate::appearance::theme::Theme,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Chat>,
    ) -> gpui::Stateful<gpui::Div> {
        let entity_for_body = entity.clone();
        let weak_for_body = weak.clone();
        let panes_hidden = self.layout.panes_hidden;
        let slp_dragging = self.layout.slp_drag.is_some();
        let slp_hovered = self.layout.slp_hover;
        let mut body = div()
            .id("app-body")
            .flex_1()
            .min_h_0()
            .relative()
            .flex()
            // slp 宽度拖拽（resizer 按下时在此跟踪）
            .on_mouse_move(move |ev: &gpui::MouseMoveEvent, _, cx| {
                let _ = weak_for_body.update(cx, |c, cx| {
                    if let Some((start_x, start_w)) = c.layout.slp_drag {
                        c.layout.slp_w =
                            (start_w + f32::from(ev.position.x) - start_x).clamp(300., 500.);
                        cx.notify();
                    }
                    // 020-1 子面板分隔线拖拽：按 dock 实测 bounds 把窗口 y
                    // 换算成占比，钳到两控制栏之间（上推/下拉到底 = 对面只剩
                    // 控制栏）；拖拽即退出两侧最小态
                    if c.layout.split_drag {
                        if let Some(b) = c.layout.dock_bounds.borrow().as_ref() {
                            let dock_h = f32::from(b.size.height);
                            if dock_h > 100. {
                                let bar = crate::ui::tokens::panel_ctrl::BAR_H;
                                let div_h = crate::ui::tokens::divider::HIT;
                                let r = (f32::from(ev.position.y) - f32::from(b.origin.y)) / dock_h;
                                c.layout.sess_min = false;
                                c.layout.files_min = false;
                                c.layout.dock_split =
                                    r.clamp(bar / dock_h, (dock_h - div_h - bar) / dock_h);
                                cx.notify();
                            }
                        }
                    }
                });
            })
            .on_mouse_up(MouseButton::Left, {
                let weak = weak.clone();
                move |_, _, cx| {
                    let _ = weak.update(cx, |c, cx| {
                        let mut changed = false;
                        if c.layout.slp_drag.take().is_some() {
                            changed = true;
                        }
                        if c.layout.split_drag {
                            c.layout.split_drag = false;
                            changed = true;
                        }
                        if changed {
                            c.persist_ui();
                            cx.notify();
                        }
                    });
                }
            })
            // tab 拖拽全局兑底 drop：gpui 只在 drop 命中注册目标时才清
            // active_drag，落到无目标处（会话 tab/topbar 空白/其他区域）
            // 会残留——ghost 跟随鼠标不消失，且新拖拽被 has_active_drag
            // 挡住永不能发起。根容器 no-op listener 兼作清道夫：bubble 相
            // 最深目标优先，tab/滚动区的真 drop 先处理并 stop_propagation，
            // 轮不到这里。
            .on_drop::<crate::titlebar::TabDrag>(move |_, _, _| {});
        if !panes_hidden {
            // 分隔线响应区（跨线）：统一感应厚度 divider::HIT=7，面板侧只
            // 占 3px（避让列表滚动条命中区），其余落在内容侧；可视线由
            // ui::divider 统一画（§5.12）
            body = body.child(
                div()
                    .id("slp-resizer")
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(self.layout.slp_w - 3.))
                    .w(px(crate::ui::tokens::divider::HIT))
                    .cursor_col_resize()
                    .on_hover(cx.listener(|this, h: &bool, _w, cx| {
                        if this.layout.slp_hover != *h {
                            this.layout.slp_hover = *h;
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, ev: &gpui::MouseDownEvent, _w, cx| {
                            if ev.click_count == 2 {
                                this.layout.slp_w = 300.;
                                this.persist_ui();
                                cx.notify();
                            } else {
                                this.layout.slp_drag =
                                    Some((f32::from(ev.position.x), this.layout.slp_w));
                            }
                        }),
                    ),
            );
            body = body.child(
                div()
                    .id("panel-col")
                    .w(px(self.layout.slp_w))
                    .flex_shrink_0()
                    .flex()
                    .flex_col()
                    .bg(rgb(t.chrome))
                    .child(titlebar::topbar_l(self, cx))
                    .child(function_panel::dock(
                        self,
                        entity_for_body.clone(),
                        &weak,
                        window,
                        cx,
                    ))
                    .child(status_bar::control_bar(self, cx)),
            );
        }
        body = body.child(
            div()
                .id("content-col")
                // 082 四期锚点宿主 = content-main（content.rs），**不是本容器**：
                // track_focus 的 div 会注册「命中盒内 MouseDown → focus +
                // prevent_default」监听（gpui div.rs paint），而 topbar_r（窗口
                // 控制钮 + 拖拽区）是本容器的子——Windows 平台把 NC 按下伪装成
                // 普通 MouseDown 派进 app（events.rs handle_nc_mouse_down_msg），
                // 锚点监听一 prevent_default 就算 handled，平台不再记录
                // nc_button_pressed → 最小化/最大化/关闭/拖拽全哑。Zed 同款
                // 不踩坑：其 titlebar 不在 pane 子树内
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .relative()
                .bg(rgb(t.bg))
                .child(titlebar::topbar_r(self, window, cx))
                .child(content::content_main(
                    self,
                    entity_for_body,
                    &weak,
                    window,
                    cx,
                ))
                // §5.12 统一分隔线：常驻 1px / hover·拖拽 4px accent，线形由
                // ui::divider 统一算（与 dock 上下分区同一条代码）；overlay
                // 不占布局 = hover 零推挤。
                .when(!panes_hidden, |d| {
                    d.child(crate::ui::divider::line(
                        crate::ui::divider::LineSpec::vertical(),
                        slp_hovered || slp_dragging,
                        t,
                    ))
                }),
        );
        body
    }
}

/// 根外壳：全局 action 兜底（Composer 四键 / psp 三动作 / 最大化两键 /
/// Ctrl+←/→ / 划选 Ctrl+C / F2 改名）+ body 挂载。
pub(super) fn root_shell(
    body: impl gpui::IntoElement,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut gpui::Context<Chat>,
) -> gpui::Div {
    let root = div()
        .size_full()
        .relative()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        .text_color(rgb(t.text))
        .font_family(crate::appearance::panel_font().family.clone())
        // ComposerPaste 兜底：焦点在对话框等非 composer 输入框时，ctrl-v
        // 截获后走到这里——原样重派发组件 Paste，普通文本粘贴不受影响
        //（composer 胶囊内有更近的 on_action，bubble 最内层先停）
        .on_action(cx.listener(|_, _: &ComposerPaste, window, cx| {
            window.dispatch_action(Box::new(gpui_component::input::Paste), cx);
        }))
        // 023：同款兜底 ×3——composer 绑定在 "Input" 上下文全局劫持了
        // up/down/tab（同深度后注册者优先），焦点在文件编辑器时这四个
        // 动作无人处理=按键全死；bubble 走到根=非 composer，重派发组件
        // 原动作（composer 内层有更近 handler，不会到这里）
        .on_action(cx.listener(|_, _: &ComposerUp, window, cx| {
            window.dispatch_action(Box::new(gpui_component::input::MoveUp), cx);
        }))
        .on_action(cx.listener(|_, _: &ComposerDown, window, cx| {
            window.dispatch_action(Box::new(gpui_component::input::MoveDown), cx);
        }))
        .on_action(cx.listener(|_, _: &ComposerTab, window, cx| {
            window.dispatch_action(Box::new(gpui_component::input::IndentInline), cx);
        }))
        // psp 全局三动作（Ctrl+P / Ctrl+N / Ctrl+K，绑定见 main.rs bind_keys）：
        // 直开项目/新建会话/搜索会话，与 psp 头部三钮同一方法
        .on_action(cx.listener(|this, _: &OpenProjectPicker, _window, cx| {
            let _ = this.open_project_picker(false, cx);
        }))
        .on_action(cx.listener(|this, _: &NewSession, _window, cx| {
            let _ = this.new_session(cx);
        }))
        .on_action(cx.listener(|this, _: &OpenSessionSearch, _window, cx| {
            this.open_session_search_panel(cx);
        }))
        // 全局文件搜索（Ctrl+L，绑定见 main.rs）：与文件树工具栏
        // 「搜索文件」钮同一方法（toggle 语义，再按一次 = 关）
        .on_action(cx.listener(|this, _: &OpenFileSearch, _window, cx| {
            this.open_file_search(cx);
        }))
        // 082 全局最大化两键（Alt+1 / Alt+2，绑定见 main.rs bind_keys）：
        // 与 psp/文件树头部最大化钮同一语义（点谁谁填满、对面收起到控制栏）
        .on_action(cx.listener(|this, _: &MaximizeSessionList, _window, cx| {
            this.layout.files_min = true;
            this.layout.sess_min = false;
            // 最大化态立即落盘（重启还原）
            this.persist_ui();
            cx.notify();
        }))
        .on_action(cx.listener(|this, _: &MaximizeFileTree, _window, cx| {
            this.layout.sess_min = true;
            this.layout.files_min = false;
            this.persist_ui();
            cx.notify();
        }))
        // 082 四期视图切换 Ctrl+←/→ 分流（绑定见 main.rs：锚定
        // "ContentTabs" 上下文 + Input 同深度后注册双绑定，内容区任意
        // 焦点命中、终端/输入框原键位一律截断）：仅模态/弹层与编辑器
        // 搜索面板两处退回组件词移动（模态不在 content-col 子树，绑定
        // 本就不命中，guard 防未来结构变化），其余切 tab；边界 = no-op
        // 死键（不再退回词移动）
        .on_action(cx.listener(|this, _: &ViewLeft, window, cx| {
            if this.modal_layer_open() || this.editor_search_panel_focused(window, cx) {
                window.dispatch_action(Box::new(gpui_component::input::MoveToPreviousWord), cx);
            } else {
                this.cycle_content_tab(-1, window, cx);
            }
        }))
        .on_action(cx.listener(|this, _: &ViewRight, window, cx| {
            if this.modal_layer_open() || this.editor_search_panel_focused(window, cx) {
                window.dispatch_action(Box::new(gpui_component::input::MoveToNextWord), cx);
            } else {
                this.cycle_content_tab(1, window, cx);
            }
        }))
        // 消息划选 Ctrl+C 分流（绑定在 main.rs："Input" 深度后注册抢占
        // 组件 ctrl-c→input::Copy——gpui 键盘派发绑定先于 on_key_down
        // 监听器，纯监听器够不到 composer 焦点场景）：有消息选区复制
        // 消息；否则原样转发组件 Copy，输入框选区照常复制
        //（ComposerPaste 兜底同款模式）
        .on_action(cx.listener(|_, _: &CopyMessageSelection, window, cx| {
            if let Some(text) = gpui_component::text::take_last_selected_text(cx) {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            } else {
                window.dispatch_action(Box::new(gpui_component::input::Copy), cx);
            }
        }))
        // 消息划选兜底（焦点不在任何 "Input" 上下文时——如 chat 根——
        // 无 ctrl-c 绑定命中、监听器可达）：存在消息选区即复制并拦截。
        // 点选区外（含点 composer/终端/编辑器）时 TextView 自身的
        // outside-down 清除逻辑已先清选区，此处取不到自然放行。终端
        // 焦点下 terminal_key 是更内层的监听器且两条 Ctrl+C 路径都吞键
        // （bubble 内层先跑），不会到这里
        .on_key_down(|ev: &gpui::KeyDownEvent, _window, cx| {
            let k = &ev.keystroke;
            if k.key == "c"
                && k.modifiers.control
                && !k.modifiers.alt
                && !k.modifiers.shift
                && let Some(text) = gpui_component::text::take_last_selected_text(cx)
            {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                cx.stop_propagation();
            }
        })
        // 082 文件树 F2 改名：条目选中（点击留存）时 F2 = 右键菜单
        // 「改名」同一路径。挂根节点兜底（面板不可聚焦，选中态靠点击
        // 留存）；监听器只在无绑定命中时可达——终端焦点下 F2 由
        // terminal_key 先吞（发 PTY），不会到这里；模态/弹层/行内编辑
        // 守卫在 tree_f2_rename 内
        .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
            let k = &ev.keystroke;
            if k.key == "f2" && !k.modifiers.modified() && this.tree_f2_rename(cx) {
                cx.stop_propagation();
            }
        }))
        // 082 三期：Ctrl+←/→ 已升级为无上下文绑定（任意焦点命中，
        // 绑定期先于监听器），此处的根 on_key_down 兜底不再可达，删除
        .child(body);
    root
}
