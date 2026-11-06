//! 会话导航面板 (033)：右缘 10px 轮次刻度条（nav_gutter）、悬停摘要卡
//! 列表（nav_card + nav_summary_header）、相位工作行（phase_row：点阵+工作中+计时）。
//! 摘要数据走 runtime 缓存（v63-3 惰性重建）。自 mod.rs 拆出（090 重构）。

use super::*;

/// 会话导航面板 (033)：右缘 10px 竖条，屏高 75%，按「屏高 − composer
/// 高度×50%」居中；生成算法（v63-5，2026-10-10 上限 20→10）：轮次均分
/// 切割整条——1 轮整条弱主题色、N 轮切 N 段（flex_1 等分 + 2px 缝），10
/// 轮后段高不再缩小，>10 走百分比桶映射（轮 k → 段 k×10/N，点击回桶首
/// 轮）。当前定位段 accent
/// 实色——选中跟随真实滚动位置（贴底 = 末段），卡片悬浮框不回写。
/// 悬停向左弹出 400px 轮次摘要卡片列表（用户前 50 字 + agent 首条回复
/// 前 50 字，编号 01-99）+ 30px 总结栏，点击卡片/段定位会话；内容溢出
/// 时右缘显示滚动条。摘要数据走 runtime 缓存（v63-3）：notify_list 置
/// 脏惰性重建，渲染帧零重算。

pub(crate) mod gutter;
pub(crate) use gutter::nav_gutter;
/// 总结栏（v63-4 自 nav_gutter 内切抽出）：30px 固定高，滚动区之外。口径
/// 「按我们自己设计」：N = 用户 + 助手 + 工具调用（ToolCall 块数），非 pi-web
/// totalMessages。样式「icon 数字」四组：消息总数：用户/助手/工具，
/// 全 icon+数字无文案，不走过 i18n。尺寸全取 UI组件规范令牌：图标
/// ICON_MD(16)、图标↔数字 gap SP4(8)、组间 gap SP5(10)、数字
/// ui(11) secondary
pub(crate) fn nav_summary_header(
    summary: &crate::session::runtime::NavData,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    use crate::ui::tokens::{icon as tok_icon, space};
    let dim = t.text_dim;
    let num_style = move |d: gpui::Div| {
        d.text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(dim))
    };
    // [(icon, 数量)]；总数组尾随全角冒号（紧贴数字，后续组隔 gap）
    let stat_groups: [(&'static str, usize, bool); 4] = [
        (
            "message-square-text",
            summary.users + summary.assistants + summary.tool_calls,
            true,
        ),
        ("user-round", summary.users, false),
        ("bot", summary.assistants, false),
        ("wrench", summary.tool_calls, false),
    ];
    let mut header = div()
        .h(px(32.))
        .flex_shrink_0()
        .flex()
        .items_center()
        // 左缘 14 = 面板 pl 6 + 卡片 px 8，与卡片文字对齐
        .px(px(14.))
        .gap(px(space::SP5))
        .border_b_1()
        // 分隔线与面板左边框同档（0x40 太淡看不出）
        .border_color(gpui::rgba(crate::appearance::theme::border_alpha(t, 0x80)))
        // 背景同功能面板（左 dock t.nav）
        .bg(rgb(t.nav));
    for (icon_name, count, colon) in stat_groups {
        let mut group = div()
            .flex()
            .items_center()
            .gap(px(space::SP4))
            .child(crate::ui::icon(icon_name, tok_icon::MD, dim));
        if colon {
            group = group.child(num_style(div()).child(SharedString::from(format!("{count}："))));
        } else {
            group = group.child(num_style(div()).child(SharedString::from(count.to_string())));
        }
        header = header.child(group);
    }
    header.into_any_element()
}

/// 轮次摘要卡片（v63-6 从 nav_gutter 循环抽出供 list() 按需调用）：
/// 编号 + 用户前 50 字 + A 行 agent 摘要，A 对齐编号、正文对齐用户正文。
/// `on` = 悬浮选中框；hover 只在 enter 置位、leave 不清——gpui 鼠标事件
/// bubble 阶段按绘制逆序派发，向下移动时上一张卡的 leave 在下一张卡的
/// enter 之后触发会误清（v63 根因），统一由面板 on_hover(false) 清理。
pub(crate) fn nav_card(
    ti: usize,
    s: &crate::session::runtime::TurnSummary,
    on: bool,
    // 该轮子代理（044-0 验收⑥）：Some 非空 = 有第三行 + hover 左弹扩展菜单
    sub_rows: Option<Vec<pi_link::subagent_catalog::CatalogRow>>,
    weak: &gpui::WeakEntity<Chat>,
    rt_entity: &gpui::Entity<crate::session::runtime::SessionRuntime>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    let no = format!("{:02}", ti + 1);
    let chars = s.user_prefix.clone();
    let user_ix = s.user_ix;
    let rt_scroll = rt_entity.clone();
    let weak_hover = weak.clone();
    // 轮卡 bounds 测量槽位（扩展菜单锚；canvas 每帧覆盖写，hover 事件读
    // ——PillBtns::tracker 同款，但生命周期在单帧闭包内）
    let bounds_slot: crate::session::subagent_panel::BoundsSlot =
        std::rc::Rc::new(std::cell::RefCell::new(None));
    let has_sub = sub_rows.is_some();
    let bounds_hover = bounds_slot.clone();
    let mut turn_el = div()
        .id(SharedString::from(format!("nv-turn-{}", s.user_ix)))
        .mt(px(2.))
        .px(px(8.))
        .py(px(7.))
        // 选择框四面完整：常驻 1px 边框（透明↔accent 切换，无布局跳动），
        // bg 同步切换
        .rounded(px(8.))
        .border_1()
        .border_color(gpui::rgba(0x00000000))
        .when(on, |d| {
            d.bg(gpui::rgba((t.accent as u32) << 8 | 0x0f))
                .border_color(rgb(t.accent))
        })
        .on_hover(move |h: &bool, _window, cx| {
            let _ = weak_hover.update(cx, |this, cx| {
                // 第三行轮 hover = 左弹扩展菜单（锚 = 卡 bounds，测量槽位
                // 由 canvas 每帧写入）；无第三行的卡 hover 清锚（卡间移动
                // 不残留上一轮的菜单）
                let anchor = if *h && has_sub {
                    bounds_hover.borrow().map(|b| (ti, b))
                } else {
                    None
                };
                if let Some(p) = this.dlg.subagent_panel.as_mut() {
                    if p.menu_anchor != anchor {
                        p.menu_anchor = anchor;
                    }
                }
                if *h && this.layout.nav_hover_turn != Some(ti) {
                    this.layout.nav_hover_turn = Some(ti);
                    cx.notify();
                } else if !*h {
                    // 离开卡不清锚（菜单紧邻 flyout 左缘，鼠标移入菜单的
                    // 路径上会短暂离开卡）——锚生命周期绑定 flyout hover
                    cx.notify();
                }
            });
        })
        // bounds 测量端（absolute inset_0 canvas，不拦事件）
        .child({
            let capture = bounds_slot.clone();
            gpui::canvas(
                move |b, _, _| *capture.borrow_mut() = Some(b),
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
        })
        .child(
            // 用户行：点击定位到该用户消息
            div()
                .id(SharedString::from(format!("nv-user-{user_ix}")))
                .flex()
                .gap(px(9.))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    rt_scroll.read(cx).pager.nav_goto(ti, user_ix);
                    // 程序化定位不触发任何 notify，点击后当帧重渲染，
                    // 选中段才跟得上定位
                    window.refresh();
                })
                .child(
                    div()
                        .w(px(18.))
                        .text_right()
                        .font_family(crate::editor::markdown::MONO_FAMILY)
                        .text_size(crate::appearance::ui_size(10.))
                        .line_height(relative(1.7))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(no)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        // 会话导航面板：用户发言 = 面板字号 -1
                        //（字体大小设置.md §1）；不加粗（2026-10-10 定夺）
                        .text_size(crate::appearance::ui_size(11.))
                        .line_height(relative(1.55))
                        .text_color(rgb(t.text))
                        // 用户发言最多三行：按行数钳制而非 max_h 写死像素。
                        // 旧实现 max_h(58px) 是按默认 11px 字号估的常数，而字
                        // 号随面板设置可调（ui_size），字号调大后行高变大，第
                        // 三行被拦腰剪掉（用户报障截图）；line_clamp 高度跟随
                        // 真实行高，任意字号自适配（会话悬浮卡标题同款）。
                        // 硬换行已在 plain_text_prefix 拍平——line_clamp 只限
                        // 软换行，硬 \n 会绕过钳制
                        .line_clamp(3)
                        .child(SharedString::from(chars)),
                ),
        );
    // 摘要行：A 与轮次编号对齐、正文与用户发言对齐（033）——与用户行
    // 同构（18px 右对齐标号列 + gap 9）
    if let Some((aix, preview)) = s.agent.as_ref() {
        let (aix, preview) = (*aix, preview.clone());
        let rt_scroll_a = rt_entity.clone();
        turn_el = turn_el.child(
            div()
                .id(SharedString::from(format!("nv-agent-{aix}")))
                .flex()
                .gap(px(9.))
                .mt(px(5.))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    rt_scroll_a.read(cx).pager.nav_goto(ti, aix);
                    window.refresh();
                })
                .child(
                    div()
                        .w(px(18.))
                        .text_right()
                        .text_size(crate::appearance::ui_size(10.))
                        .line_height(relative(1.7))
                        .text_color(rgb(t.text_dim))
                        .child(SharedString::from(tr("session.shortcut_a"))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        // 会话导航面板：agent 发言 = 面板字号 -2
                        .text_size(crate::appearance::ui_size(10.))
                        .line_height(relative(1.55))
                        .text_color(rgb(t.text_dim))
                        .overflow_hidden()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .child(SharedString::from(preview)),
                ),
        );
    }
    // 第三行（044-0 验收⑥①）：[bot] ●●●● ——点数 = 该轮子代理数，样式取
    // 会话运行结果青红点
    if let Some(rows) = &sub_rows {
        turn_el = turn_el.child(crate::session::subagent_panel::turn_agents_row(rows));
    }
    turn_el.into_any_element()
}

/// 相位行（v64 改版）：3×3 点阵图标 + 「工作中」+ 秒级计时（ZCode 同款），
/// 取代原「正在运行 {tool}...」纯文本脉冲。计时文本放进 1s repeat 动画
/// 闭包逐帧刷新——免额外定时器；起点取 runtime `stream_started`
/// （AgentStart 记录、settle/abort 清除），跨 streaming 隐藏期持续累计。
/// 计时进位到天：天/时/分/秒自高位起只显非零段。
pub(crate) fn phase_row(
    started: std::time::Instant,
    t: &crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .ml(px(5.))
        .gap(px(8.))
        .text_size(crate::appearance::ui_size(13.))
        .text_color(rgb(t.text_muted))
        .child(crate::ui::working_dots(rgb(t.text_muted)))
        .child(tr("status.working"))
        .with_animation(
            "phase-row-timer",
            Animation::new(std::time::Duration::from_millis(1000)).repeat(),
            move |el, _| {
                let secs = started.elapsed().as_secs();
                let days = secs / 86_400;
                let hours = (secs % 86_400) / 3_600;
                let mins = (secs % 3_600) / 60;
                let mut seg: Vec<String> = Vec::with_capacity(4);
                if days > 0 {
                    seg.push(crate::i18n::tf("time.day", &[("n", days.to_string())]));
                }
                if days > 0 || hours > 0 {
                    seg.push(crate::i18n::tf("time.hour", &[("n", hours.to_string())]));
                }
                if days > 0 || hours > 0 || mins > 0 {
                    seg.push(crate::i18n::tf("time.minute", &[("n", mins.to_string())]));
                }
                seg.push(crate::i18n::tf(
                    "time.seconds",
                    &[("n", (secs % 60).to_string())],
                ));
                el.child(SharedString::from(seg.join(" ")))
            },
        )
        .into_any_element()
}
