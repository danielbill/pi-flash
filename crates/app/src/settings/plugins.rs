//! Extensions tab（040-Pi的扩展管理）：「扩展」= pi 的 package（settings.json
//! `packages` 条目）。顶部安装区（整条 `pi install …` 命令可直接粘贴，全局
//! only）+ 左侧已装列表（350px，行内开关 + 组头总开关，内容左右各留 15px）+
//! 右侧五行详情（说明取包内 package.json 的 description）。安装/卸载走
//! vendored pi CLI 后台线程，完成后 op 泵清 busy 并刷新列表（main.rs）。

use super::*;

impl Chat {
    /// 启停单个扩展：资源数组置空 = 停用，恢复字符串条目 = 启用
    /// （pi-web disable 语义），写全局 settings.json。
    pub(crate) fn mc_toggle_package(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let Some(entry) = self.mc_pkgs_global.get(ix) else {
            return;
        };
        let source = pi_link::skills::entry_source(entry);
        let next: Vec<serde_json::Value> = self
            .mc_pkgs_global
            .iter()
            .enumerate()
            .map(|(i, e)| {
                if i != ix {
                    return e.clone();
                }
                if pi_link::skills::entry_disabled(e) {
                    serde_json::Value::String(source.clone())
                } else {
                    serde_json::json!({
                        "source": source,
                        "extensions": [], "skills": [], "prompts": [], "themes": []
                    })
                }
            })
            .collect();
        let path = pi_link::config::settings_path();
        if let Err(e) = pi_link::config::write_packages(&path, next.clone()) {
            self.mc_set_error(&crate::i18n::tf("写入 settings.json 失败: {e}", &[("e", e)]), cx);
            return;
        }
        // globals 是启动时的内存副本：不同步的话 reload_settings_panel 拷回旧列表
        self.globals.packages = next;
        self.reload_settings_panel();
        cx.notify();
    }

    /// 总开关：统一开启/关闭全部扩展（040 定稿，不做资源过滤豁免）。
    pub(crate) fn mc_toggle_packages_bulk(&mut self, enable: bool, cx: &mut Context<Self>) {
        self.mc_clear_error(cx);
        let mut changed = 0usize;
        let next: Vec<serde_json::Value> = self
            .mc_pkgs_global
            .iter()
            .map(|e| {
                if pi_link::skills::entry_disabled(e) == !enable {
                    return e.clone(); // 已在目标状态
                }
                changed += 1;
                if enable {
                    serde_json::Value::String(pi_link::skills::entry_source(e))
                } else {
                    serde_json::json!({
                        "source": pi_link::skills::entry_source(e),
                        "extensions": [], "skills": [], "prompts": [], "themes": []
                    })
                }
            })
            .collect();
        if changed == 0 {
            return;
        }
        if let Err(e) = pi_link::config::write_packages(&pi_link::config::settings_path(), next.clone())
        {
            self.mc_set_error(&crate::i18n::tf("写入 settings.json 失败: {e}", &[("e", e)]), cx);
            return;
        }
        self.globals.packages = next;
        self.reload_settings_panel();
        cx.notify();
    }

    /// 安装（040：全局 only）。来源支持整条 `pi install …` 粘贴
    /// （normalize_source 剥前缀）；后台跑 vendored pi CLI，转圈在安装
    /// 按钮，完成后 op 泵清零并刷新列表。op 进行中忽略新请求。
    pub(crate) fn mc_install_package(&mut self, source: String, cx: &mut Context<Self>) {
        if self.pkg_op.is_some() {
            return;
        }
        self.mc_clear_error(cx);
        let source = pi_link::skills::normalize_source(&source);
        if source.is_empty() {
            self.mc_set_error(&tr("请输入扩展来源，或粘贴 pi install 安装命令"), cx);
            return;
        }
        self.pkg_op = Some(crate::PkgOp::Install);
        self.mc_cli_op(
            vec!["install".to_string(), source.clone()],
            crate::i18n::tf("已安装 {source}", &[("source", source.clone())]),
            cx,
        );
    }

    /// 卸载（040：移除按钮换成垃圾箱 icon，转圈在该垃圾桶上）。
    pub(crate) fn mc_remove_package(&mut self, source: String, cx: &mut Context<Self>) {
        if self.pkg_op.is_some() {
            return;
        }
        self.mc_clear_error(cx);
        self.pkg_op = Some(crate::PkgOp::Remove(source.clone()));
        self.mc_cli_op(
            vec!["remove".to_string(), source.clone()],
            crate::i18n::tf("已移除 {source}", &[("source", source.clone())]),
            cx,
        );
    }

    /// 点垃圾桶：弹确认浮层（040，统一居中确认组件）。
    pub(crate) fn mc_ask_remove_package(&mut self, source: String, cx: &mut Context<Self>) {
        if self.pkg_op.is_some() {
            return;
        }
        self.pkg_confirm_remove = Some(source);
        cx.notify();
    }

    /// 确认浮层的「确认 / 取消」：ok=true 才真删。
    pub(crate) fn mc_remove_dialog_close(&mut self, ok: bool, cx: &mut Context<Self>) {
        let Some(source) = self.pkg_confirm_remove.take() else {
            return;
        };
        if ok {
            self.mc_remove_package(source, cx);
        } else {
            cx.notify();
        }
    }
}

/// 扩展页：顶部安装区 + 下方 左列表（300px）/ 右详情。
pub(crate) fn mc_plugins_view(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    install_input: &gpui::Entity<TextInput>,
    error: &Option<String>,
) -> gpui::AnyElement {
    let t = T();
    let entries: Vec<(usize, &serde_json::Value)> =
        chat.mc_pkgs_global.iter().enumerate().collect();
    div()
        .flex()
        .flex_col()
        .w_full()
        .h_full()
        .min_h_0()
        .child(pl_install_bar(
            weak,
            install_input,
            error,
            matches!(chat.pkg_op, Some(crate::PkgOp::Install)),
            t,
        ))
        .child(two_pane(
            pl_sidebar(chat, weak, section, &entries, t),
            pl_detail(chat, weak, section, &entries, t),
        ))
        .into_any_element()
}

fn two_pane(sidebar: gpui::AnyElement, detail: gpui::AnyElement) -> gpui::AnyElement {
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .child(sidebar)
        .child(detail)
        .into_any_element()
}

/// 安装区（040）：输入框（400px，整条命令可粘贴，左缘对齐下方列表面板）
/// + 安装按钮；提示行在输入框下方。安装中按钮转圈禁用，错误行紧随其下。
fn pl_install_bar(
    weak: &gpui::WeakEntity<Chat>,
    install_input: &gpui::Entity<TextInput>,
    error: &Option<String>,
    busy: bool,
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    div()
        .flex_shrink_0()
        .flex()
        .flex_col()
        .gap(px(6.))
        // pl 15 = 列表内容左缘（list px7 + 行内 8），安装区与列表文字左对齐
        .pl(px(15.))
        .pr(px(20.))
        .pt(px(14.))
        .pb(px(10.))
        .border_b_1()
        .border_color(rgb(t.border))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .child(div().w(px(400.)).flex_shrink_0().child(install_input.clone()))
                .child(if busy {
                    div()
                        .id("pkg-install-go")
                        .h(px(32.))
                        .px(px(14.))
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .rounded(px(5.))
                        .bg(rgb(t.accent))
                        .text_size(crate::appearance::ui_size(12.))
                        .text_color(rgb(t.accent_contrast))
                        .opacity(0.8)
                        .child(crate::ui::spinner(12., t.accent_contrast))
                        .child(tr("安装中"))
                        .into_any_element()
                } else {
                    config_button(
                        "pkg-install-go",
                        weak,
                        &tr("安装"),
                        Btn::Primary,
                        false,
                        false,
                        |c, cx| {
                            let src = c
                                .settings
                                .clone()
                                .map(|st| st.read(cx).install_input.clone())
                                .map(|input| input.read(cx).value().to_string())
                                .unwrap_or_default();
                            c.mc_install_package(src, cx);
                        },
                    )
                }),
        )
        // 提示行：pi.dev/packages 做成下划线链接（accent 色），点击开浏览器
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child("从 ")
                .child(
                    div()
                        .id("pkg-pi-dev-link")
                        .text_color(rgb(t.accent))
                        .underline()
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.75))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| {
                            cx.stop_propagation();
                            cx.open_url("https://pi.dev/packages");
                        })
                        .child("https://pi.dev/packages"),
                )
                .child(" 复制安装命令，做全局安装"),
        )
        .children(error.as_ref().map(|e| error_note(e)))
        .into_any_element()
}

/// 左列表（350px，040，无底色）：标题行【配置全局默认扩展】+ {enabled}/{total}
/// + 总开关（与行内开关同规格 32×18，标题字号同列表行）；行 = 展示名（去
/// npm:）+ 行内启停开关。列表内容左右各留 15px（list px7 + 行内 px8 等距，
/// 与右侧详情 p15 一致），上方安装栏 pl15 与之对齐。
fn pl_sidebar(
    _chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    entries: &[(usize, &serde_json::Value)],
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    let mut list = sidebar_list().px(px(7.));
    let enabled = entries
        .iter()
        .filter(|(_, v)| !pi_link::skills::entry_disabled(v))
        .count();
    let total = entries.len();
    list = list.child(
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .px(px(8.))
            .pt(px(8.))
            .pb(px(3.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(crate::appearance::ui_size(12.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text_dim))
                    .child(tr("配置全局默认扩展")),
            )
            .child(
                div()
                    .font_family(crate::markdown::MONO_FAMILY)
                    .text_size(crate::appearance::ui_size(10.))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(format!("{enabled}/{total}"))),
            )
            .child(config_switch(
                "pl-bulk-g",
                weak,
                total > 0 && enabled == total,
                false,
                move |c, cx| c.mc_toggle_packages_bulk(enabled != total, cx),
            )),
    );
    for entry in entries {
        let (ix, v) = *entry;
        let src = pi_link::skills::entry_source(v);
        let disabled = pi_link::skills::entry_disabled(v);
        let active = src == section;
        let weak_item = weak.clone();
        let item_src = src.clone();
        list = list.child(
            widgets::sidebar_item(format!("pkg-{ix}-{}", src), active)
                // 行背景压平（040：列表无底色，选中态只靠加粗+深字色，hover 保留）
                .bg(rgb(t.bg))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |c, cx| {
                        if let Some(st) = c.settings.clone() {
                            st.update(cx, |s, cx| {
                                s.section = item_src.clone();
                                s.error = None;
                                cx.notify();
                            });
                        }
                    });
                })
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(pi_link::skills::display_source(&src).to_string())),
                )
                .child(config_switch(
                    SharedString::from(format!("pkg-sw-{ix}")),
                    weak,
                    !disabled,
                    false,
                    move |c, cx| c.mc_toggle_package(ix, cx),
                )),
        );
    }
    sidebar_shell("mc-sidebar")
        .w(px(350.))
        .bg(rgb(t.bg))
        .child(list)
        .into_any_element()
}

/// 右侧详情（040 五行）：[全局]+名称+卸载 / 说明 / 状态 / 来源 / 路径。
/// 启停统一在左列表的行内开关（040：详情页不放开关）。
fn pl_detail(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    section: &str,
    entries: &[(usize, &serde_json::Value)],
    t: &crate::theme::Theme,
) -> gpui::AnyElement {
    let Some((_, v)) = entries
        .iter()
        .find(|(_, v)| pi_link::skills::entry_source(v) == section)
        .or_else(|| entries.first())
        .map(|(i, v)| (*i, *v))
    else {
        return detail_shell("mc-detail")
            .p(px(15.))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(12.))
                    .text_color(rgb(t.text_dim))
                    .child(tr("没有已安装的扩展")),
            )
            .into_any_element();
    };
    let src = pi_link::skills::entry_source(v);
    let disabled = pi_link::skills::entry_disabled(v);
    let description = pi_link::skills::package_description(&src);
    let weak_trash = weak.clone();
    let trash_src = src.clone();
    // 转圈落点：正在卸载本包 → 垃圾桶转圈；有任一 op → 垃圾桶禁用
    let removing_this = matches!(chat.pkg_op.as_ref(), Some(crate::PkgOp::Remove(s)) if *s == src);
    let mut trash = div()
        .id("pkg-remove")
        .size(px(28.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .border_1()
        .border_color(gpui::rgba(crate::theme::danger_alpha(
            crate::theme::theme(),
            0x59,
        )))
        .bg(gpui::rgba(crate::theme::danger_alpha(
            crate::theme::theme(),
            0x0f,
        )));
    if removing_this {
        trash = trash.child(crate::ui::spinner(12., crate::theme::theme().danger));
    } else {
        trash = trash.child(crate::ui::icon(
            "icon-trash-solid",
            14.,
            crate::theme::theme().danger,
        ));
    }
    if removing_this || chat.pkg_op.is_some() {
        trash = trash.opacity(0.5);
    } else {
        trash = trash
            .cursor_pointer()
            .hover(|s| s.bg(gpui::rgba(crate::theme::danger_wash(crate::theme::theme()))))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_trash.update(cx, |c, cx| {
                    c.mc_ask_remove_package(trash_src.clone(), cx)
                });
            });
    }
    // 本页详情内边距与列表 15px 统一（detail_shell 默认 p20）
    detail_shell("mc-detail")
        .p(px(15.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(28.))
                // 范围标在后：title 在前（与技能页「路径 + 开关」同为名前标后）
                .child(
                    div()
                        .font_family(crate::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(12.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(SharedString::from(pi_link::skills::display_source(&src).to_string())),
                )
                .child(scope_tag(&tr("全局"), false))
                .child(div().flex_1())
                .child(trash),
        )
    // 详情排版与技能页同款：field = 标签在上、内容在下（040 定稿）
    .child(field(
        &tr("说明"),
        div()
            .text_size(crate::appearance::ui_size(12.))
            .text_color(rgb(t.text_muted))
            .child(SharedString::from(
                description.unwrap_or_else(|| "—".to_string()),
            )),
    ))
    .child(field(
        &tr("状态"),
        div().child(if disabled {
            SharedString::from(tr("已停用").to_string())
        } else {
            SharedString::from(tr("已启用").to_string())
        }),
    ))
    .child(field(
        &tr("说明大小"),
        // 尚未测得（延迟加载未跑完/未安装）显示「—」
        mono_text(
            match chat.ext_tokens.get(&src) {
                Some(e) => format!(
                    "{} tokens",
                    crate::services::format::fmt_thousand(e.ext.max(0) as u64)
                ),
                None => "—".to_string(),
            },
            true,
        ),
    ))
    .child(field(&tr("来源"), mono_text(src.clone(), false)))
    .child(field(
        &tr("路径"),
        div()
            .font_family(crate::markdown::MONO_FAMILY)
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text_dim))
            .child(SharedString::from(breakable_path(&pi_link::skills::package_install_dir(&src).to_string_lossy()))),
    ))
        .into_any_element()
}

/// 长路径折行（040）：分隔符后插零宽空格，让无空格的路径也能断行。
fn breakable_path(path: &str) -> String {
    path.replace('\\', "\\\u{200b}").replace('/', "/\u{200b}")
}

/// 卸载确认浮层（040）：统一 `overlay::confirm` 组件，自动居中。
pub(crate) fn remove_confirm_overlay(
    chat: &Chat,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let src = chat.pkg_confirm_remove.as_ref()?;
    let name = pi_link::skills::display_source(src).to_string();
    let weak = cx.entity().downgrade();
    let message = crate::i18n::tf("卸载扩展 {name}？", &[("name", name)]);
    Some(crate::ui::overlay::confirm(
        &chat.dialog_focus,
        message,
        {
            let weak = weak.clone();
            move |_, cx| {
                let _ = weak.update(cx, |c, cx| c.mc_remove_dialog_close(false, cx));
            }
        },
        move |_, cx| {
            let _ = weak.update(cx, |c, cx| c.mc_remove_dialog_close(true, cx));
        },
    ))
}
