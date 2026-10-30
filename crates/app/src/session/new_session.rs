//! 012 新会话页 (newSession)：启动时项目|会话列表为空、或用户新建会话
//! 时，聊天区显示这一页（pi-web ChatWindow `isEmptyNew` parity）——空会话
//! 不再只是「消息列为空 + 底部悬浮 composer」，而是整页的
//! 「标题 + 背景 logo + inputpanel + 额外操作栏」。
//!
//! 布局（docs/模块设计/012-新会话页.md，v5 按 zcode 参考截图实测比例）：
//! 1. 背景 logo = 水印方案：墨迹高 51% 聊天区、中线 32%（跨度 ~6%..57%），
//!    text 色低 alpha（深 ~10%/浅 ~6%），下沿被 composer 遮住；
//! 2. 欢迎语「Hi，打算让我做点什么？」中心 37%（压墨迹下半部，zcode 同款
//!    关系），加大字号、斜体、内置 JetBrains Mono；
//! 3. inputpanel + 操作栏簇中心 56%（zcode 实测 ~56%）。
//!
//! 三条绝对定位带实现（百分比高由 flex 链解析，033 同机制）：logo 带
//! 顶对齐 64% 高带内居中 ⇒ 中线 32%；欢迎语带顶对齐 74% ⇒ 中心 37%；
//! 内容簇带底对齐 88% ⇒ 中心 56%。带间重叠无害（无 bg/无 handler 的带
//! 不注册 hitbox，v1 双带已验证）。
//!
//! 触发判据在 `session::main_column`（messages 空且 agent 未跑）；composer
//! 以 hero 模式（`input::input_area(.., hero=true)`）在正常流里排版，宽度
//! 仍由消息列约束（px(15) + 设置项「会话内容宽度」封顶）。

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::i18n::tr;
use crate::session::input;
use crate::ui::{icon, icon_alpha};

/// 背景 logo「墨迹」高 = 32% 视口高（LOGO_SCREEN×INK_RATIO，INK_RATIO 在
/// 乘积里约掉，只用于反推盒高）。logo-marks.svg 为 v5 字标（2026-10 与
/// 徽章母版同步整体放大），墨迹实测占盒高 51.7%（529/1024，取 132/256）。
const LOGO_INK_RATIO: f32 = 132. / 256.;
const LOGO_SCREEN: f32 = 0.32 / LOGO_INK_RATIO;
/// logo 带：顶对齐 50% 高带内居中 ⇒ 墨迹中线 25% 视口高。
const LOGO_BAND: f32 = 0.5;
/// 欢迎语带：顶对齐 74% 高带内居中 ⇒ 欢迎语中心 37%（压墨迹下半部）。
const HEADING_BAND: f32 = 0.74;
/// 内容簇带（inputpanel + 操作栏）：底对齐 88% ⇒ 簇中心 56%。
const CONTENT_BAND: f32 = 0.88;
/// 操作栏高度（012：固定 40px）。
const ACTION_BAR_H: f32 = 40.;

/// 新会话页整页。占满聊天区（`flex_1`），内部两层：背景 logo（绝对、装饰）
/// 与内容簇（绝对、可交互）。
pub(crate) fn page(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    skin: &crate::session::template::SessionPageSkin,
    streaming: bool,
    input_focused: bool,
    viewport_h: f32,
    cx: &mut gpui::Context<Chat>,
) -> AnyElement {
    let t = T();
    // 新会话页也是模版页的一部分：欢迎语与扩展更新卡由**线皮肤**注入，
    // 本文件不自己判「是不是插件功能线」（功能线的欢迎语 = 插件注入语；
    // 扩展更新表只属真正的新会话页）
    let welcome = tr(skin.greeting_key);
    div()
        .id("new-session")
        .relative()
        .flex_1()
        .min_h_0()
        .w_full()
        // 根节点不裁剪（overflow_hidden 已下放到 logo 层）：/ @ 菜单从居中
        // 的 composer 向上弹出会越过页面顶边，根上裁剪会把菜单头部切掉；
        // 内容区在 topbar_r 之后绘制，放开后菜单自然盖在 topbar 上（与会话
        // 界面的悬浮胶囊同行为）
        .child(
            // 墨迹盒 ~0.62×视口高（见 LOGO_SCREEN），窄窗下可横向越界，
            // 裁剪只作用于这层装饰
            div()
                .absolute()
                .inset_0()
                .overflow_hidden()
                .child(logo_backdrop(t, viewport_h * LOGO_SCREEN)),
        )
        .child(
            // 欢迎语带：顶对齐 74% 高带内居中 ⇒ 中心 37%（压墨迹下半部）
            div()
                .absolute()
                .top_0()
                .left_0()
                .right_0()
                .h(relative(HEADING_BAND))
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(heading(t, welcome)),
        )
        .child(
            // 内容簇带：底对齐 88% 高带内居中 ⇒ 簇中心 56%
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .h(relative(CONTENT_BAND))
                .px(px(15.))
                .flex()
                .flex_col()
                .justify_center()
                .items_center()
                .child(
                    // 与消息列同宽（session_list 的 px(15) + 同一内容列宽）
                    div()
                        .w_full()
                        .max_w(px(crate::services::workspace::chat_content_width()))
                        .flex()
                        .flex_col()
                        .items_center()
                        .child(input::input_area(
                            chat,
                            weak,
                            streaming,
                            input_focused,
                            true,
                            cx,
                        ))
                        .child(action_bar(chat, weak, t))
                        // 扩展更新通知 = 零高锚点（不占流空间），卡片
                        // absolute 下移固定 20px（验收定稿），欢迎语/inputpanel
                        // 定位零改动。**只属于真正的新会话页**（皮肤开关）：
                        // 插件功能线不渲染也不触发加载。
                        .children(if skin.plugin_updates {
                            plugin_update_notice(chat, weak, t)
                        } else {
                            None
                        }),
                ),
        )
        .into_any_element()
}

/// 背景 logo：盒子 = 视口高 × 0.32/INK_RATIO ≈ 0.62（墨迹恒 32% 视口高，
/// 见 LOGO_SCREEN），顶对齐 50% 高带内居中 ⇒ 墨迹中线 25%，下沿没人接
/// （composer 簇 56% 居中会盖住 45%+ 以下的部分）。text 色低 alpha（深
/// ~10%、浅 ~6%）。纯装饰层：无 id/无 handler，不参与命中。越界裁剪由
/// 调用方的 inset_0 容器承担（根节点不能裁——会切掉上浮的 / @ 菜单，见
/// page 注释）。
fn logo_backdrop(t: &'static crate::appearance::theme::Theme, size: f32) -> AnyElement {
    let a = if t.dark { 0x1a } else { 0x10 };
    div()
        .absolute()
        .top_0()
        .left_0()
        .right_0()
        .h(relative(LOGO_BAND))
        .flex()
        .flex_col()
        .justify_center()
        .items_center()
        .child(icon_alpha("logo-marks", size, (t.text << 8) | a))
        .into_any_element()
}

/// 欢迎语：「Hi，打算让我做点什么？」——zcode 参考图问候语同款关系（中心
/// 37%，压水印下半部）：加大字号、斜体、内置 JetBrains Mono（中文回退
/// 系统字体，italic 由回退链合成）。文案由调用方给（插件定制页可注入）。
fn heading(t: &'static crate::appearance::theme::Theme, text: &str) -> AnyElement {
    div()
        .text_size(crate::appearance::ui_size(40.))
        .font_family(crate::editor::markdown::MONO_FAMILY)
        .italic()
        .font_weight(gpui::FontWeight::SEMIBOLD)
        .text_color(rgb(t.text))
        .child(SharedString::from(text.to_string()))
        .into_any_element()
}

/// inputpanel 下方的额外操作栏（012 第 4 条）：与 inputpanel 同宽、间隔
/// 5px、高 40px、无边框。左 = 目录图标 + 当前项目名（无项目时显示
/// 「选择项目」；004：点击进入打开项目菜单，菜单里的【打开文件夹】才是
/// 目录选择器），右 = pi-flash 版本号小字（012：不显示 pi 版本号，搜索
/// 入口只在功能面板）。
fn action_bar(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> AnyElement {
    let project: SharedString = chat
        .project()
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| p.to_string_lossy().to_string())
        })
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| tr("action.select_project").to_string())
        .into();
    let w_open = weak.clone();
    let mut bar = div()
        .id("new-session-bar")
        .mt(px(5.))
        .w_full()
        .h(px(ACTION_BAR_H))
        .px(px(10.))
        .flex()
        .items_center()
        .gap(px(8.));
    // 打开项目
    bar = bar.child(
        div()
            .id("ns-open-project")
            .h(px(28.))
            .px(px(9.))
            .flex()
            .items_center()
            .gap(px(7.))
            .rounded(px(8.))
            .cursor_pointer()
            .text_size(crate::appearance::ui_size(12.))
            .text_color(rgb(t.text_muted))
            .hover(|s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = w_open.update(cx, |c, cx| c.open_project_picker(true, cx));
            })
            .child(icon("folder", 16., t.text_muted))
            .child(project),
    );
    // pi-flash 版本号（placeholder 同款淡色，用户 2026-10-07 定稿）。
    // 081：状态化——下载中转圈（faint）、就绪 = 可点击「下载完成，点击
    // 重启更新」（青蓝 NOTICE，点击 spawn swap helper 立即切换，081 §7）、
    // 关自动更新且有新版 = 可点击「↑」开 release 页（青蓝）。
    // Idle = 淡字版本号可点击，手动开当前版本更新日志页（2026-10-14）。
    {
        use crate::services::updater::UpdateState;
        let version_el: gpui::AnyElement = match &chat.aux.update {
            UpdateState::Downloading { version } => {
                // 百分比来自下载/解压线程的原子量；spinner 动画每帧触发本臂
                // 重跑，文案随之刷新。下载段带 NN%；下载完切「解压安装中…」
                // （解压段此前无提示，单线程解压分钟级只能干等——0.1.8 实录）
                use crate::services::updater as u;
                let extracting = u::update_phase() == u::PHASE_EXTRACT;
                let mut text = if extracting {
                    crate::i18n::tr("about.extracting_v").to_string()
                } else {
                    crate::i18n::tf("about.downloading_v", &[("v", version.clone())])
                };
                if !extracting {
                    if let Some(pct) = u::download_percent() {
                        text = format!("{text} {pct}%");
                    }
                }
                div()
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .child(crate::ui::spinner(11., t.text_faint))
                    .child(SharedString::from(text))
                    .into_any_element()
            }
            UpdateState::Ready { .. } => {
                let w_upd = weak.clone();
                div()
                    .id("ns-update-ready")
                    .cursor_pointer()
                    .text_color(rgb(crate::appearance::theme::NOTICE))
                    .hover(|s| s.text_color(rgb(t.text)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = w_upd.update(cx, |c, cx| c.restart_to_apply_update(cx));
                    })
                    .child(SharedString::from(tr("update.ready_click")))
                    .into_any_element()
            }
            UpdateState::Available { version } => {
                let ver = version.clone();
                div()
                    .id("ns-update-available")
                    .flex()
                    .items_center()
                    .gap(px(3.))
                    .cursor_pointer()
                    .child(SharedString::from(format!("v{ver}")))
                    .child(crate::ui::icon(
                        "arrow-up",
                        11.,
                        crate::appearance::theme::NOTICE,
                    ))
                    .hover(|s| s.text_color(rgb(t.text)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                        crate::services::updater::open_release_page(&ver);
                    })
                    .into_any_element()
            }
            UpdateState::Idle => {
                let ver = env!("CARGO_PKG_VERSION");
                let w_ver = weak.clone();
                div()
                    .id("ns-version")
                    .cursor_pointer()
                    .child(SharedString::from(format!("v{ver}")))
                    .hover(|s| s.text_color(rgb(t.text)))
                    .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                        cx.stop_propagation();
                        let _ = w_ver.update(cx, |c, cx| c.show_changelog(ver.to_string(), cx));
                    })
                    .into_any_element()
            }
        };
        let version_color = match &chat.aux.update {
            // 下载中保持淡色（设计文档：下载信息仍为 faint 字体颜色）；
            // 就绪/有新版 = 青蓝（NOTICE，通知点三色之「完成通知」）
            UpdateState::Ready { .. } | UpdateState::Available { .. } => {
                crate::appearance::theme::NOTICE
            }
            _ => t.text_faint,
        };
        bar = bar.child(
            div()
                .ml_auto()
                .flex()
                .items_center()
                .gap(px(5.))
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(version_color))
                .child(version_el),
        );
    }
    bar.into_any_element()
}

/// 082 Pi 扩展更新通知（零高锚点 + absolute 卡片）：锚点紧跟 action_bar
/// 流内不占高度，卡片固定下移 20px（验收定稿：与项目选择操作栏固定 20px
/// 间距，欢迎语/inputpanel 定位零改动）；宽 = 上方内容列 80%，卡片式（圆
/// 角边框同 composer 呼应），标题行下分隔线，列表上限 300px 超长滚动；
/// 标题行右侧【更新全部(n)】= UI 规范按钮（与设置·扩展页统一
/// config_button）。安静原则：检查中不显示、n=0 不渲染；点击列表行暂无
/// 动作（v1 仅展示，入口在设置·扩展页）。
fn plugin_update_notice(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static crate::appearance::theme::Theme,
) -> Option<AnyElement> {
    let n = chat.aux.pi_extension_updates.available();
    if n == 0 {
        return None;
    }
    let updating_all = matches!(
        chat.cfg.pkg_op.as_ref(),
        Some(crate::settings::pi_extensions::PkgOp::Update(None))
    );
    let rows: Vec<&crate::services::pi_extension_updates::PkgUpdateResult> = chat
        .aux
        .pi_extension_updates
        .results
        .iter()
        .filter(|r| {
            r.state == crate::services::pi_extension_updates::PkgUpdateState::UpdateAvailable
        })
        .collect();
    // 更新全部按钮：与设置·扩展页同一 config_button（ui::button 规范件）
    let label = if updating_all {
        tr("extensions.updating_all").to_string()
    } else {
        format!("{}({n})", tr("extensions.update_all"))
    };
    let btn = crate::ui::kit::config_button(
        "ns-pkg-update-all",
        weak,
        &label,
        crate::ui::kit::Btn::Secondary,
        true,
        chat.cfg.pkg_op.is_some(),
        |c, cx| c.mc_update_all_packages(cx),
    );
    let mut list = div()
        .id("ns-pkg-list")
        .w_full()
        .max_h(px(300.))
        .overflow_y_scroll()
        .flex()
        .flex_col();
    for (ix, r) in rows.iter().enumerate() {
        list = list.child(
            div()
                .id(SharedString::from(format!("ns-pkg-row-{ix}")))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(6.))
                .py(px(3.))
                .rounded(px(4.))
                .text_size(crate::appearance::ui_size(12.))
                .hover(|h| h.bg(rgb(t.bg_hover)))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_color(rgb(t.text_muted))
                        .child(SharedString::from(r.display_name.clone())),
                )
                .child(crate::ui::icon("arrow-up", 10., t.accent)),
        );
    }
    // 零高锚点：紧跟 action_bar（流内不占高度，欢迎语/inputpanel 定位零
    // 改动），卡片 absolute 下移固定 20px（验收定稿：与项目选择操作栏固定
    // 20px 间距），水平居中、内容列 80% 宽；卡片式圆角边框同 composer 呼
    // 应，标题行下分隔线，列表上限 300px 超长滚动。安静原则：检查中不显
    // 示、n=0 不渲染；点击列表行暂无动作（v1 仅展示，入口在设置·扩展页）。
    Some(
        div()
            .h(px(0.))
            // w_full 必须显式写：父列是 items_center，子项按内容收缩，
            // 缺宽度锚点会塌成 0 宽，absolute 卡片 left_0/right_0 跟着变
            // 0 宽 → 标题一字一行竖排、按钮溢出（2026-02 变形实录）
            .w_full()
            .relative()
            .child(
                div()
                    .absolute()
                    .top(px(20.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .w_full()
                            .max_w(px(crate::services::workspace::chat_content_width() * 0.8))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(rgb(t.border))
                            .bg(rgb(t.bg))
                            .p(px(10.))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(6.))
                                    .pb(px(6.))
                                    // 标题行下分隔线（验收：原有的分隔线要加）
                                    .border_b_1()
                                    .border_color(rgb(t.border))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .text_size(crate::appearance::ui_size(11.))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(rgb(t.text_dim))
                                            .child(tr("new_session.pi_extension_updates")),
                                    )
                                    .child(btn),
                            )
                            .child(list),
                    ),
            )
            .into_any_element(),
    )
}
