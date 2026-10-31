//! PF 插件市场【已安装】页签（090-4 v4）：插件管理视图，版式对齐
//! settings-扩展页（左列表 + 右详情），差异：无安装栏，
//! 详情不出「说明 / 提示词大小」。
//!
//! 数据源 = PF 插件台账 `plugins.json`（市场安装的唯一落点），**不是**
//! pi 扩展 settings.json——所以 settings 页的行内启停开关 / 总开关 /
//! 更新全部不出（PF 插件没有持久启停概念，加载在启动扫描定死）。
//! 展示名 / 发布人 / 描述补全查市场索引（`pf_market::index_snapshot()`），索引
//! 拉不到时回落包名。
//!
//! 头段沿用 [`super::pf_market`] 的段骨架（居中外壳 + 页签行 + 计数 +
//! 错误行）；主体 = 定宽段盒子内 左列表（LIST_W，同 settings 400px）+
//! 右详情（状态 / 来源 / 路径 + 卸载垃圾桶，确认浮层 040 同款机制）。

use gpui::prelude::*;
use gpui::{AnyElement, FontWeight, MouseButton, SharedString, px, rgb};

use pi_link::pf_plugins;

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::{tf, tr};
use crate::market::pf_market::{MONO, centered, index_snapshot, slice_shell};
use crate::market::pi_native_market::design::Metrics;
use crate::ui::kit::{
    LIST_W, config_switch, detail_shell, field, sidebar_item, sidebar_list, sidebar_shell,
};

/// 一条已装插件（台账 × 索引 display 补全后的展示行）。
struct InstalledItem {
    name: String,
    display: String,
    version: String,
    source: String,
    disabled: bool,
}

impl Chat {
 /// 启停单插件（090-4 v5）：台账 disabled 位落盘 + 运行态立即生效。
 /// 停用 = 台账落盘 + `plugin_evict`（丢 JS 运行时与组件树缓存、左 dock
 /// 正显示它就退默认）——状态栏 tab / 文件树插件菜单 / 左面板全部隐身；
 /// 启用 = 有条目则复位，无条目（启动时被扫描跳过）只补装这一个。
 /// 失败进头段错误行。
    pub(crate) fn pf_toggle_plugin(
        &mut self,
        name: String,
        enable: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if let Err(e) = pf_plugins::set_disabled(&name, !enable) {
            self.aux.market.pf.error = Some(e);
            cx.notify();
            return;
        }
        if let Some(l) = self
            .aux
            .market
            .pf
            .ledger
            .iter_mut()
            .find(|l| l.name == name)
        {
            l.disabled = !enable;
        }
 // 台账包名 → 运行态条目：只认顶层包目录名（版本层目录名是版本号，
 // 拿它比永远比不中——上一版停用点了没反应就是因为这个）
        let hit = self.plugin_ix_by_pkg(&name);
        match (enable, hit) {
            (true, Some(ix)) => {
                if let Some(entry) = self.plugins.plugins.get_mut(ix) {
                    entry.disabled = false;
                }
 // 重新装载：ext 自动重挂当前会话（off 时被摘除）——绑定态在镜像
 // 里原样保留，ext 重挂后门控命中、渠道自恢复（免重扫）
                self.plugin_attach_pi_exts(ix, &self.pool.active_key.clone(), cx);
                self.ensure_active_process(cx);
            }
            (true, None) => {
 // 启动时被停用过滤、运行态无条目：后台重扫**只补装这一个**
 // （内建 `plugin_replace_by_pkg`，不整体替换 plugins_set）
                self.reload_plugin_by_pkg(name.clone(), cx);
 // 启用即预热编译产物：下次启动验签走缓存命中（不现场编）
                crate::pf_plugin::spawn_precompile_installed();
            }
 // 停用：运行态就地撤下（含左 dock 退默认），否则面板还照旧渲染
            (false, Some(ix)) => {
                self.plugin_evict(ix);
 // 摘除活跃运行时里的 ext（空闲即重启；忙 = 下次 spawn 生效）
                self.plugin_detach_active_ext(ix, cx);
            }
            (false, None) => {} // 未装载，无运行态可撤
        }
        cx.notify();
    }

 /// 点垃圾桶：弹确认浮层（040 统一确认组件；渲染在 shell/render 顶层）。
    pub(crate) fn pf_ask_remove(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pf.removing.is_some() {
            return;
        }
        self.aux.market.pf.confirm_remove = Some(name);
        cx.notify();
    }

 /// 确认浮层的「确认 / 取消」：ok=true 才真删。
    pub(crate) fn pf_remove_dialog_close(&mut self, ok: bool, cx: &mut gpui::Context<Self>) {
        let Some(name) = self.aux.market.pf.confirm_remove.take() else {
            return;
        };
        if ok {
            self.pf_uninstall(name, cx);
        } else {
            cx.notify();
        }
    }

 /// 卸载：后台线程删目录 + 摘台账（幂等），完成重读台账刷新列表；
 /// **成功则同账号把运行态条目一并摘掉**（否则状态栏 tab / 左 dock 面板 /
 /// 文件树菜单 / JS 运行时全活着，删了还能用直到重启）。
 /// 失败原因进头段错误行（pf_install 同款通道）。
    fn pf_uninstall(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pf.removing.is_some() {
            return;
        }
        self.aux.market.pf.removing = Some(name.clone());
        self.aux.market.pf.error = None;
 // 先就地撤运行态（drop wasm 实例 → 释放 .cache/*.cwasm 的 mmap），
 // 否则后台删目录必被 Windows 拒绝（os error 32，文件被占用）。
        if let Some(ix) = self.plugin_ix_by_pkg(&name) {
            self.plugin_evict(ix);
            self.plugins.plugins.remove(ix);
            if matches!(self.layout.dock_panel, crate::DockPanel::Plugin(_)) {
                self.layout.dock_panel = crate::DockPanel::Default;
            }
        }
        let pkg_name = name.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { pf_plugins::uninstall(&name) })
                .await;
            let _ = this.update(cx, |c, cx| {
                let ok = result.is_ok();
                c.aux.market.pf.removing = None;
                c.aux.market.pf.error = result.err();
                c.aux.market.pf.ledger = pf_plugins::load_ledger();
                if c.aux
                    .market
                    .pf
                    .section
                    .as_deref()
                    .is_some_and(|s| !c.aux.market.pf.ledger.iter().any(|l| l.name == s))
                {
                    c.aux.market.pf.section = None;
                }
                if ok && let Some(ix) = c.plugin_ix_by_pkg(&pkg_name) {
                    let id = c.plugins.plugins[ix].manifest.id.clone();
                    c.plugin_evict(ix);
                    c.plugins.plugins.remove(ix);
                    crate::pf_plugin::plugin_forget(&id);
 // 下标位移：插件面板存的是位置，任何一条被摘都退默认
                    if matches!(c.layout.dock_panel, crate::DockPanel::Plugin(_)) {
                        c.layout.dock_panel = crate::DockPanel::Default;
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }
}

/// 卸载确认浮层（040 同款机制，叠在一切之上；shell/render 顶层挂）。
pub(crate) fn remove_confirm_overlay(
    chat: &Chat,
    cx: &mut gpui::Context<Chat>,
) -> Option<gpui::AnyElement> {
    let name = chat.aux.market.pf.confirm_remove.as_ref()?;
    let display = index_snapshot()
        .entry(name)
        .map(|e| e.display.zh.clone())
        .unwrap_or_else(|| name.clone());
    let message = tf("market.pf.uninstall_confirm", &[("name", display)]);
    let weak = cx.entity().downgrade();
    Some(crate::ui::overlay::confirm(
        &chat.dlg.dialog_focus,
        message,
        {
            let weak = weak.clone();
            move |_, cx| {
                let _ = weak.update(cx, |c, cx| c.pf_remove_dialog_close(false, cx));
            }
        },
        move |_, cx| {
            let _ = weak.update(cx, |c, cx| c.pf_remove_dialog_close(true, cx));
        },
    ))
}

/// 【已安装】body：定宽段盒子内两栅（列表 / 详情），吃满骨架剩余高度。
/// 头段与底留白由 `pf_market::page_frame` 统一提供（两个页签共用一套骨架）。
pub(crate) fn body(chat: &Chat, weak: &gpui::WeakEntity<Chat>, m: &Metrics) -> AnyElement {
    let t = crate::appearance::theme::theme();
    let st = &chat.aux.market.pf;

    let items: Vec<InstalledItem> = {
        let ix = index_snapshot();
 // 未上架的包（dev 侧载/内层私有包）索引里查不到 display：回落已装
 // 清单的 title_zh，裸 slug 只作最后兑底（用户看名字，不看包名）
        let manifests = crate::pf_plugin::scan_manifests();
        st.ledger
            .iter()
            .map(|l| {
                let display = ix
                    .entry(&l.name)
                    .map(|e| e.display.zh.clone())
                    .or_else(|| {
                        manifests
                            .iter()
                            .find(|m| m.pkg == l.name)
                            .map(|m| m.title_zh.clone())
                    })
                    .unwrap_or_else(|| l.name.clone());
                InstalledItem {
                    name: l.name.clone(),
                    display,
                    version: l.version.clone(),
                    source: l.source.clone(),
                    disabled: l.disabled,
                }
            })
            .collect()
    };

    let selected_name = st
        .section
        .clone()
        .filter(|s| items.iter().any(|i| &i.name == s))
        .or_else(|| items.first().map(|i| i.name.clone()));
    let selected = items
        .iter()
        .find(|i| Some(&i.name) == selected_name.as_ref());

    let removing = st.removing.clone();
    let weak_c = weak.clone();

 // ── 左列表（LIST_W，对齐 settings-扩展；无开关——PF 插件无持久启停）──
 // 列表头不再画「已安装」——页签行就在正上方，同一个词不重复
 // 上内距对齐右详情（detail_shell 的 p(15)）：左列表首行不再贴着段盒顶边
    let mut list = sidebar_list().px(px(7.)).pt(px(15.));
    for (n, item) in items.iter().enumerate() {
        let active = selected_name.as_deref() == Some(item.name.as_str());
        let weak_item = weak_c.clone();
        let name = item.name.clone();
        let weak_sw = weak_c.clone();
        let sw_name = item.name.clone();
        let enable = item.disabled;
        list = list.child(
            sidebar_item(format!("pf-installed-{n}-{}", item.name), active)
                .bg(rgb(t.bg))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_item.update(cx, |c, cx| {
                        c.aux.market.pf.section = Some(name.clone());
                        cx.notify();
                    });
                })
                .child(
                    gpui::div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(item.display.clone())),
                )
                .child(
                    gpui::div()
                        .font_family(MONO)
                        .text_size(crate::appearance::ui_size(10.))
                        .text_color(rgb(t.text_faint))
                        .child(SharedString::from(item.version.clone())),
                )
 // 启停开关（090-4 v5）：停用 = PF 不装载不绘制（台账 disabled
 // 位 + 运行态 entry.disabled，状态栏/文件树/分发全部隐身）
                .child(config_switch(
                    SharedString::from(format!("pf-installed-sw-{n}")),
                    &weak_sw,
                    !item.disabled,
                    removing.is_some(),
                    move |c, cx| c.pf_toggle_plugin(sw_name.clone(), enable, cx),
                )),
        );
    }
    let sidebar = sidebar_shell("pf-installed-sidebar")
        .w(px(LIST_W))
        .bg(rgb(t.bg))
        .child(list);

    let detail = detail_pane(selected, removing.as_deref(), weak, t);

 // 段盒吃满全高：centered 外壳必须是 flex（否则 flex_1 的 shell 在块级父里
 // 拿不到剩余高度 → 段盒只到内容高度，和【所有插件】页不一样高）
    let body = centered(
        slice_shell(m, false, true, t).flex_1().min_h_0().child(
            gpui::div()
                .flex_1()
                .min_h_0()
                .flex()
                .child(sidebar)
                .child(detail),
        ),
        m,
    )
    .flex_1()
    .min_h_0()
    .min_w_0()
    .flex();

    body.into_any_element()
}

/// 右详情（040 五行裁掉说明/提示词大小 → 状态 / 来源 / 路径）：
/// 名称行 = 展示名（mono semibold）+ [全局] 标 + 垃圾桶。
fn detail_pane(
    item: Option<&InstalledItem>,
    removing: Option<&str>,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
) -> AnyElement {
 // 没有选中/没有安装：详情栏留空（不加任何提示文案）
    let Some(item) = item else {
        return detail_shell("pf-installed-detail").into_any_element();
    };

 // 垃圾桶（settings-扩展同款 danger 盒；卸载本插件 → spinner，任一卸载中禁用）
    let removing_this = removing == Some(item.name.as_str());
    let mut trash = gpui::div()
        .id("pf-installed-remove")
        .size(px(28.))
 // 视觉微调：标题是 mono 12px（字形重心偏下），垃圾桶要跟标题垂直居中对齐，
 // 整体下挪 6px（relative 不占布局，不动行高）
        .relative()
        .top(px(6.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(5.))
        .border_1()
        .border_color(gpui::rgba(crate::appearance::theme::danger_alpha(
            crate::appearance::theme::theme(),
            0x59,
        )))
        .bg(gpui::rgba(crate::appearance::theme::danger_alpha(
            crate::appearance::theme::theme(),
            0x0f,
        )));
    if removing_this {
        trash = trash.child(crate::ui::spinner(
            12.,
            crate::appearance::theme::theme().danger,
        ));
    } else {
        trash = trash.child(crate::ui::icon("icon-trash-solid", 14., t.danger));
    }
    if removing_this || removing.is_some() {
        trash = trash.opacity(0.5);
    } else {
        let weak_trash = weak.clone();
        let name = item.name.clone();
        trash = trash
            .cursor_pointer()
            .hover(|s| {
                s.bg(gpui::rgba(crate::appearance::theme::danger_wash(
                    crate::appearance::theme::theme(),
                )))
            })
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_trash.update(cx, |c, cx| c.pf_ask_remove(name.clone(), cx));
            });
    }

 // 索引补全：发布人（来源行）+ 一句话描述（说明行，缺索引/无描述 = —）
    let ix = index_snapshot();
    let entry = ix.entry(&item.name);
    let source_display = match &entry {
        Some(e) if !e.publisher.is_empty() => e.publisher.clone(),
        Some(_) => "Pi-Flash".to_string(),
        None => item.source.clone(),
    };
    let description = entry
        .and_then(|e| e.desc.as_ref())
        .map(|d| {
            if crate::i18n::lang_ix() == 2 && !d.en.is_empty() {
                d.en.clone()
            } else {
                d.zh.clone()
            }
        })
        .unwrap_or_else(|| "—".to_string());
 // 安装路径（版本化目录，与 install 落点一致）；索引/版本缺失退插件根目录
    let path = match pi_link::paths::plugins_dir() {
        Some(root) => {
            let base = root.join(pf_plugins::dir_name(&item.name));
            if item.version.is_empty() {
                base
            } else {
                base.join(&item.version)
            }
        }
        None => std::path::PathBuf::new(),
    };

    detail_shell("pf-installed-detail")
        .p(px(15.))
        .child(
            gpui::div()
                .flex()
                .items_center()
                .gap(px(8.))
                .min_h(px(28.))
 // 名前标后（与技能页 / settings-扩展 同款）
                .child(
                    gpui::div()
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(12.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(t.text))
                        .child(SharedString::from(item.display.clone())),
                )
                .child(gpui::div().flex_1())
                .child(trash),
        )
        .child(field(
            &tr("about.description"),
            gpui::div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(description)),
        ))
        .child(field(
            &tr("pi_extensions.state"),
            gpui::div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(if item.disabled {
                    tr("status.disabled").to_string()
                } else {
                    tr("status.enabled").to_string()
                })),
        ))
        .child(field(
            &tr("extensions.source"),
            gpui::div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(source_display)),
        ))
        .child(field(
            &tr("extensions.path"),
            gpui::div()
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(breakable_path(&path.to_string_lossy()))),
        ))
        .into_any_element()
}

/// 长路径折行（settings-扩展同款）：分隔符后插零宽空格。
fn breakable_path(path: &str) -> String {
    path.replace('\\', "\\\u{200b}").replace('/', "/\u{200b}")
}
