//! statusbar (v54): 40px，只在面板段（panel-col 内）。020-1 起面板 tab
//! 只剩「项目」一个（46px 宽 briefcase icon 常亮连体卡，顶无边、底圆角、
//! accent 色，与 dock 背景连成一体；文件树已并入整合面板，tab 位留给后续
//! 新面板）。右端 = 设置钮（016 下放）。收起态整个隐藏（panel-col 随
//! panes_hidden 不渲染）。

use gpui::{MouseButton, div, prelude::*, px, rgb};

use gpui::SharedString;

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::services::workspace::{dock_max_from_min, save_ui_state, ui_state};
use crate::ui::tokens::align;
use crate::ui::tooltip::WithTip;

const HEIGHT: f32 = 40.;

pub(crate) fn control_bar(chat: &mut Chat, cx: &mut gpui::Context<Chat>) -> impl gpui::IntoElement {
    let t = T();

    // PF 插件 tab（beads 360 ②）：清单 contributes.status_bar 声明 → 46px
    // 连体卡（「项目」tab 同形），点击 open_plugin_panel（懒加载链 02 §4）。
    // icon = 插件 assets/ 相对路径（磁盘经 Assets 缓存）或宿主内置名；
    // active 跟随 content_view（accent 图标），禁用插件不出 tab。
    let mut tabs = div()
        .flex()
        .items_center()
        .gap(px(2.))
        // 首 tab 卡片左缘落在图标对齐线（§7 / tokens::align，与上方
        // psp 列表图标同线；2026-10-13 定夺）
        .ml(px(align::ICON_LINE))
        // 020-1：面板 tab 只剩「项目」一个（briefcase 图标；文件树
        // tab 隐藏——文件树已并入上方整合面板）。tab 位保留，后续
        // 新面板在此续可点 tab。微信 tab 已撤（2026-10-23：入口收
        // 三横菜单【微信通信】，内容区全屏页，不再占 statusbar 图标位）。
        .child(project_tab(chat, cx));
    for (pix, p) in chat.plugins.plugins.iter().enumerate() {
        let Some(sb) = p.manifest.status_bar.as_ref() else {
            continue;
        };
        if p.disabled {
            continue;
        }
        let Some(icon_path) = crate::pf_plugin::status_bar_icon_path(p) else {
            continue;
        };
        // active = 左列正显示该插件面板（互斥切换，同「项目」tab 口径）
        let active = chat.layout.dock_panel == crate::DockPanel::Plugin(pix);
        let color = if active { t.accent } else { t.text_muted };
        let title = sb.title_zh.clone();
        tabs = tabs.child(
            // 样式对齐 topbar（beads 360 反馈①）：未激活融入 chrome 背景
            //（无卡底无边框、图标 muted），激活才出连体卡 + accent 图标
            div()
                .id(SharedString::from(format!("plugin-tab-{}", p.manifest.id)))
                // ui-raw: ok 与「项目」tab 同形的 46px tab（w46/mt−4），
                // 尺寸待 tab 体系 token 化时一并收编（§10.8）
                .w(px(46.))
                // tab 高 = bar 高 −4（2026-10-13 定夺，上下两 bar 同规则）
                .h(px(HEIGHT - 4.))
                // 连体卡顶贴 dock（同「项目」tab）
                .mt(px(-4.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .when(active, |d| {
                    d.bg(rgb(t.nav))
                        .border_1()
                        .border_t_0()
                        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
                        .rounded_bl(px(9.))
                        .rounded_br(px(9.))
                })
                // hover 只给未激活态（topbar tab 同款口径）：激活连体卡已有
                // nav 卡底，换 bg_hover 会在 light 主题下猛然变暗（nav 近白
                // → bg_hover 深），看起来像 bug
                .when(!active, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
                .with_tip_text(title)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                        this.open_plugin_dock(pix, cx);
                    }),
                )
                .child(
                    gpui::svg()
                        .path(icon_path)
                        .text_color(rgb(color))
                        // 大图标档 LG18（020-1 图标两档定夺）
                        .size(px(18.)),
                ),
        );
    }

    let mut bar = div()
        .id("status-bar")
        .h(px(HEIGHT))
        .flex_shrink_0()
        .flex()
        .items_center()
        .bg(rgb(t.chrome))
        // 上沿不画线（2026-10-10 定夺）：与内容区之间不放 border_t
        // 右缘不画线：slp|内容分隔线 = content-col 的 border_l 一条全高线，
        // statusbar 再画 border_r 会与其相邻成 2px 双线（设计稿 ::before 在
        // 浏览器亚像素下糊成一条，gpui 锐利像素下显形）
        .child(tabs);
    bar = bar.child(div().flex_1());

    // 016：设置钮下放状态栏右槽（原手机图标位；手机遥控迁 topbar 左段）。
    // 图标右缘对齐上方列表右线（容器 LIST_PX + 行 pr LIST_PX = 2×LIST_PX，
    // 2026-10-13 定夺「向上对齐」）——右对齐 + pr 直取线位，勿改回居中
    // （居中是 15px，会比右线多伸出 5px）
    bar = bar.child(
        div()
            .id("statusbar-settings")
            .w(px(46.))
            .h(px(HEIGHT))
            .flex()
            .items_center()
            .justify_end()
            .pr(px(align::LIST_PX * 2.))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                    this.open_settings(0, cx);
                }),
            )
            // 大图标档 LG18（020-1 图标两档定夺：设置钮与 topbar 收放钮同档）
            .child(crate::ui::icon_hover(
                "sliders-horizontal",
                18.,
                t.text_muted,
            )),
    );
    bar
}

/// 「项目」tab（020-1）：常亮连体卡（nav 底 + 顶无边 + 底圆角 + accent
/// 图标），与上方 dock 连成一体。整合面板常驻后无可切换对象，暂为纯
/// 指示位；后续新面板在此续可点 tab（切换走 layout.dock_panel）。
/// 「项目」tab（020-1）：连体卡（nav 底 + 顶无边 + 底圆角），点击回整合
/// 面板（DockPanel::Default）。active = 左列正是整合面板（accent 图标）。
fn project_tab(chat: &Chat, cx: &mut gpui::Context<Chat>) -> impl gpui::IntoElement {
    let t = T();
    let active = chat.layout.dock_panel == crate::DockPanel::Default;
    div()
        .id("project-tab")
        // ui-raw: ok 020-1 连体卡原形制（w46/mt−4）升级为可点 tab；
        // 与插件 tab 同形同源，尺寸待 tab 体系 token 化时一并收编（§10.8）
        .w(px(46.))
        // tab 高 = bar 高 −4（2026-10-13 定夺，上下两 bar 同规则）
        .h(px(HEIGHT - 4.))
        .flex()
        .items_center()
        .justify_center()
        // 样式对齐 topbar（beads 360 反馈①）：未激活融入 chrome 背景
        .when(active, |d| {
            d.bg(rgb(t.nav))
                .border_1()
                .border_t_0()
                .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x8c)))
                .rounded_bl(px(9.))
                .rounded_br(px(9.))
        })
        // 连体卡顶贴 dock（mt = −(bar−tab) = −4 时卡片顶缘 y=0）
        .mt(px(-4.))
        .cursor_pointer()
        // hover 只给未激活态：理由同插件 tab（激活连体卡不吃 hover 底色，
        // light 主题下 nav→bg_hover 变暗像 bug）
        .when(!active, |d| d.hover(|s| s.bg(rgb(t.bg_hover))))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                if this.layout.dock_panel != crate::DockPanel::Default {
                    this.layout.dock_panel = crate::DockPanel::Default;
                    cx.notify();
                }
            }),
        )
        // 大图标档 LG18（020-1 图标两档定夺：与设置钮同档）
        .child(crate::ui::icon(
            "icon-project",
            18.,
            if active { t.accent } else { t.text_muted },
        ))
}

/// Persist the current shell layout (panel/width/modes/collapse).
impl Chat {
    pub(crate) fn persist_ui(&mut self) {
        let mut st = ui_state();
        st.panel = self.layout.dock_panel.as_str().to_string();
        st.slp_w = self.layout.slp_w;
        st.panes_hidden = self.layout.panes_hidden;
        st.dock_split = Some(self.layout.dock_split);
        st.list_mode = match self.psp.list_mode {
            crate::ListMode::Flat => "flat".to_string(),
            crate::ListMode::Grouped => "grouped".to_string(),
        };
        st.sort_mode = match self.psp.sort_mode {
            crate::SortMode::Manual => "manual".to_string(),
            crate::SortMode::Time => "time".to_string(),
        };
        st.collapsed = self.psp.collapsed_keys.iter().cloned().collect();
        st.tree_changes_only = self.ws.changes_only;
        // 020 项目用户排序：拖过才有序列，没拖过存空表（读回 = None）
        st.proj_order = self.psp.proj_order.clone().unwrap_or_default();
        // 会话置顶（pin）：有序表落盘，跨重启保留
        st.session_pins = self
            .psp
            .session_pins
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        // 020-1 dock 最大化态（映射见 services::workspace::dock_max_from_min）
        st.dock_max = dock_max_from_min(self.layout.sess_min, self.layout.files_min).to_string();
        save_ui_state(&st);
        // 文件树状态（展开集合 + Pin 视图）跟着 UI 持久化时机落盘：关窗/退出
        self.archive_tree_state();
    }

    pub(crate) fn toggle_panes(&mut self, cx: &mut gpui::Context<Self>) {
        self.layout.panes_hidden = !self.layout.panes_hidden;
        self.persist_ui();
        cx.notify();
    }
}
