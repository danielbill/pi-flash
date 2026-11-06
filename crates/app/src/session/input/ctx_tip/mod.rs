//! composer 上下文用量：环悬停状态机、详情卡与用量面板（v58 响应式）。

use gpui::{AnimationExt, Context, MouseButton, SharedString, div, prelude::*, px, relative, rgb};

use crate::Chat;
use crate::i18n::tr;
use crate::services::format::fmt_thousand;

/// 悬停标志更新 + 淡出状态机（乱序免疫）：只按 (was_open, open) 转移，
/// 环→面板交接时两个 hover 事件无论先后都收敛到正确状态。双面全空才
/// 开始淡出（200ms 定时器到期清除，守卫时刻戳防旧定时器误杀新一轮）。

pub(crate) mod capacity;
use capacity::{ctx_capacity_block, ctx_tokens_cost_parts};
/// 悬停标志更新 + 淡出状态机（乱序免疫）：只按 (was_open, open) 转移，
/// 环→面板交接时两个 hover 事件无论先后都收敛到正确状态。双面全空才
/// 开始淡出（200ms 定时器到期清除，守卫时刻戳防旧定时器误杀新一轮）。
pub(super) fn ctx_tip_hover(chat: &mut Chat, ring: bool, entered: bool, cx: &mut Context<Chat>) {
    let was_open = chat.dlg.ctx_tip_ring_hover || chat.dlg.ctx_tip_panel_hover;
    if ring {
        chat.dlg.ctx_tip_ring_hover = entered;
    } else {
        chat.dlg.ctx_tip_panel_hover = entered;
    }
    let open = chat.dlg.ctx_tip_ring_hover || chat.dlg.ctx_tip_panel_hover;
    match (was_open, open) {
        (false, true) => {
            chat.dlg.ctx_tip_closing = None;
            cx.notify();
        }
        (true, false) => {
            let t = std::time::Instant::now();
            chat.dlg.ctx_tip_closing = Some(t);
            cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let _ = this.update(cx, |c, cx| {
                    if c.dlg.ctx_tip_closing == Some(t) {
                        c.dlg.ctx_tip_closing = None;
                        cx.notify();
                    }
                });
            })
            .detach();
            cx.notify();
        }
        _ => {}
    }
}

/// 悬浮详情浮层（挂在 ctx-ring 容器内，absolute 越界浮在聊天区上）：
/// Open 常显；Closing 包一层 160ms 透明度淡出，定时器随后卸载。wrapper
/// 下方 12px 隐形尾迹压住环顶 2px，环→面板悬停交接无死区。
pub(super) fn ctx_tip_element(
    chat: &mut Chat,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> Option<gpui::AnyElement> {
    let visible = chat.dlg.ctx_tip_ring_hover
        || chat.dlg.ctx_tip_panel_hover
        || chat.dlg.ctx_tip_closing.is_some();
    if !visible {
        return None;
    }
    let (stats, sys, compacting, can_compact) = {
        let r = chat.rt().read(cx);
        (
            r.stats.clone(),
            r.sys.clone(),
            r.compacting,
            r.can_compact(),
        )
    };
    let closing = chat.dlg.ctx_tip_closing.is_some();
    // 入口行点亮态（2026-10-16 自会话功能栏迁入）：对应弹窗开着亮 accent
    let sys_open = chat.session_info_open(crate::TopPanel::System);
    let tools_open = chat.session_info_open(crate::TopPanel::Tools);
    let wrap = div()
        .id("ctx-tip")
        .absolute()
        .bottom(px(24.)) // 环命中区 26px，压顶 2px 防接缝死区
        // 相对圆环居中：向左/右对称扩张 + flex 居中。手算 left 偏移要跟着
        // 环的 padding(4)+负 margin(-4) 一起走，对称扩张则天然以环为轴，
        // 环宽变了也不用改常数。±260 > 460/2 保证面板不被压缩。
        .left(px(-260.))
        .right(px(-260.))
        .pb(px(12.))
        .flex()
        .flex_row()
        .justify_center()
        // hover 挂整层：面板与环之间的空档也属于浮层，鼠标横穿不闪断
        .on_hover(cx.listener(|this, h: &bool, _w, cx| ctx_tip_hover(this, false, *h, cx)))
        // mousedown 拦截只挂面板本体（点面板不穿透到消息列表）；空档区域
        // 无 mousedown handler，点击照常落到下层消息
        .child(
            div()
                // 面板 460 宽（2026-10-20 从 420 +40：英文下底部三钮并排
                // 超长溢出，加宽后仍每行 2 列网格）
                .w(px(460.))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(ctx_usage_panel(
                    stats,
                    sys.as_ref(),
                    compacting,
                    can_compact,
                    sys_open,
                    tools_open,
                    t,
                    cx,
                )),
        );
    let el = if closing {
        wrap.with_animation(
            "ctx-tip-out",
            gpui::Animation::new(std::time::Duration::from_millis(160)),
            |el, delta| el.opacity(1. - delta),
        )
        .into_any_element()
    } else {
        wrap.into_any_element()
    };
    // vendored gpui 的 Style::paint 把边框画在子元素之后——面板挂在胶囊
    // 内部会被胶囊聚焦边框压线（实测横穿面板）；deferred 推迟到整帧末尾
    // 绘制（布局仍在原位，锚定/hover 关系不变，弹层同机制）
    Some(gpui::deferred(el).into_any_element())
}

/// 用量详情面板（ZCode「上下文容量」弹层风格）组装：标题行（已用/窗口 ·
/// 百分比）+ 进度条 + 分类占比行 + Token 分项网格 + 费用 + 底部动作排
/// （系统提示词 / 会话工具集 / 压缩三钮等宽，2026-10-16 入口自会话功能栏
/// 迁入、二稿从中部行改到底排统一描边钮形制——用户定夺）。
/// 分类占比为**估算**：系统侧 7 大类走 `pi_link::transcript::breakdown`
/// （与系统提示词面板分析块同源），「消息」= 官方上下文总量 − 系统侧估算
/// （反推）——各行加总恒等于 pi 报告的官方总量，估算误差全部收进「消息」行。
fn ctx_usage_panel(
    stats: Option<pi_link::protocol::SessionStats>,
    sys: Option<&pi_link::transcript::TranscriptSystem>,
    compacting: bool,
    can_compact: bool,
    sys_open: bool,
    tools_open: bool,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    let body: Vec<gpui::AnyElement> = match stats {
        None => vec![
            div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .child(tr("stats.no_usage"))
                .into_any_element(),
        ],
        Some(s) => vec![ctx_capacity_block(&s, sys, t).into_any_element()]
            .into_iter()
            .chain(ctx_tokens_cost_parts(&s, t))
            .collect(),
    };

    div()
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg))
        .shadow_lg()
        // 左右 15（用户定夺：上下两片区域统一），垂直仍走 SP6 12
        .px(px(15.))
        .py(px(12.))
        .flex()
        .flex_col()
        .gap(px(12.))
        .children(body)
        .child(ctx_bottom_actions(
            sys_open,
            tools_open,
            compacting,
            can_compact,
            t,
            cx,
        ))
}

/// 底部动作排（2026-10-16 二稿，用户定夺）：系统提示词 / 会话工具集 /
/// 压缩三钮等宽一排，统一压缩钮的描边形制——一稿插在中部的纯文字入口行
/// 打断了数据区，收编到底排后视觉语言单一。无 stats 的空态也保留（新会话
/// 仍要能开这两块）。
fn ctx_bottom_actions(
    sys_open: bool,
    tools_open: bool,
    compacting: bool,
    can_compact: bool,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::Div {
    div()
        .flex()
        .gap(px(8.))
        .child(ctx_entry_button(
            "ctx-sys-prompt",
            "file-sliders",
            tr("session.system_prompt"),
            sys_open,
            crate::TopPanel::System,
            t,
            cx,
        ))
        .child(ctx_entry_button(
            "ctx-session-tools",
            "wrench",
            tr("session.session_tools"),
            tools_open,
            crate::TopPanel::Tools,
            t,
            cx,
        ))
        .child(ctx_compact_button(compacting, can_compact, t, cx))
}

/// 描边入口钮（同压缩钮形制：高 34、圆角 6、细边框，icon + 文案居中）；
/// 对应弹窗开着亮 accent，点击切换开/收。
fn ctx_entry_button(
    id: &'static str,
    icon_name: &'static str,
    label: &'static str,
    on: bool,
    kind: crate::TopPanel,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::Stateful<gpui::Div> {
    let hover_fg = if on { t.accent } else { t.text };
    div()
        .id(id)
        .flex_1()
        .h(px(34.))
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .cursor_pointer()
        .text_size(crate::appearance::ui_size(12.))
        .text_color(rgb(if on { t.accent } else { t.text_muted }))
        .hover(move |s| s.bg(rgb(t.bg_hover)).text_color(rgb(hover_fg)))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &gpui::MouseDownEvent, _w, cx| {
                this.open_session_info(kind, cx);
            }),
        )
        .child(crate::ui::icon(
            icon_name,
            13.,
            if on { t.accent } else { t.text_muted },
        ))
        .child(SharedString::from(label))
}

/// 行：label 左、value 右（mono）。上下文用量弹窗的全部文字 = 面板设置
/// 值，title 与正文同号（字体大小设置.md §1）。
fn ctx_row(label: &str, value: String, t: &'static crate::appearance::theme::Theme) -> gpui::Div {
    div()
        .flex()
        .items_center()
        .child(
            div()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(label.to_string())),
        )
        .child(
            div()
                .ml_auto()
                .text_size(crate::appearance::ui_size(12.))
                .font_family(crate::editor::markdown::MONO_FAMILY)
                .text_color(rgb(t.text))
                .child(SharedString::from(value)),
        )
}

/// 小节：SEMIBOLD 标题 + 行列表。
fn ctx_section(
    title: &str,
    rows: Vec<gpui::AnyElement>,
    t: &'static crate::appearance::theme::Theme,
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(
            div()
                .text_size(crate::appearance::ui_size(12.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(rgb(t.text))
                .child(SharedString::from(title.to_string())),
        )
        .children(rows)
        .into_any_element()
}

/// 二列数据网格（用户定夺：面板 460 宽、每行 2 列砍高度）：cells 按行
/// 填充，每格半宽，左格 pr 出列缝——两列的百分比值各自贴半宽右缘对齐。
fn ctx_grid2(cells: Vec<gpui::Div>) -> gpui::AnyElement {
    div()
        .flex()
        .flex_wrap()
        .gap_y(px(8.))
        .children(cells.into_iter().enumerate().map(|(i, c)| {
            div()
                .w(relative(0.5))
                .when(i % 2 == 0, |d| d.pr(px(16.)))
                .child(c.w_full())
        }))
        .into_any_element()
}

/// 分类占比文案：一位小数，尾数 .0 去掉（59.7% / 2%）。
fn cat_pct(v: u64, total: u64) -> String {
    if total == 0 {
        return "0%".into();
    }
    let p = v as f64 / total as f64 * 100.0;
    if (p * 10.).round() % 10. == 0. {
        format!("{p:.0}%")
    } else {
        format!("{p:.1}%")
    }
}

/// 分类行数据：(标签, 类别色, 估算 token)。系统侧 7 桶来自 breakdown
/// 估算，「消息」= 官方总量 − 系统侧合计（反推，clamp 0）；零行跳过，
/// 按 token 降序（稳定排序：同值保持桶序、「消息」殿后）。
fn cat_rows(
    sys: &pi_link::transcript::TranscriptSystem,
    context_total: u64,
) -> Vec<(&'static str, u32, u64)> {
    if context_total == 0 {
        return Vec::new();
    }
    let agent_dir = pi_link::paths::pi_agent_dir();
    let est = pi_link::transcript::breakdown(sys, agent_dir.as_deref());
    let mut rows: Vec<(&'static str, u32, u64)> = pi_link::transcript::SYSTEM_BUCKETS
        .iter()
        .enumerate()
        .filter(|(i, _)| est[*i] > 0)
        .map(|(i, b)| {
            (
                crate::top_panels::bucket_label(*b),
                crate::appearance::theme::bucket_color(*b),
                est[i],
            )
        })
        .collect();
    let msgs = context_total.saturating_sub(est.iter().sum());
    if msgs > 0 {
        rows.push((
            tr("stats.messages"),
            crate::appearance::theme::CTX_MESSAGES_COLOR,
            msgs,
        ));
    }
    rows.sort_by(|a, b| b.2.cmp(&a.2));
    rows
}

/// token 值统一 K 口径 + 千分位（1,000K）。
fn fmt_k(n: u64) -> String {
    format!("{}K", fmt_thousand((n as f64 / 1000.).round() as u64))
}

/// 上下文容量块（ZCode 风格）：标题行（已用/窗口 · 百分比，窗口未知只给
/// 已用）+ 进度条 + 分类占比行（系统态已加载且官方总量已知才画；否则退
/// 回旧两行）。
/// 压缩钮（底部动作排成员，2026-10-16 与两个入口钮同排等宽 flex_1）：
/// 无门槛（点不点由用户决定），描边按钮无常态底色（hover 才出 bg_hover）；
/// 固定高 34 + items_center 保证文字在框内垂直居中。compacting 仅防连点。
fn ctx_compact_button(
    compacting: bool,
    can_compact: bool,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::Stateful<gpui::Div> {
    // 「压 缩」字间插窄空格（gpui 无 letter_spacing，字符串层处理；
    // 「压缩中…」多字，保持原样不插）
    let compact_label = {
        let raw = if compacting {
            tr("status.compacting")
        } else {
            tr("action.compact")
        };
        let mut cs = raw.chars();
        match (cs.next(), cs.next(), cs.next()) {
            (Some(a), Some(b), None) => format!("{a}\u{2009}{b}"),
            _ => raw.to_string(),
        }
    };
    let fg = if compacting {
        t.text_muted
    } else if can_compact {
        t.accent
    } else {
        t.text_faint
    };
    div()
        .id("ctx-compact")
        .flex_1()
        .h(px(34.))
        .rounded(px(6.))
        .border_1()
        .border_color(rgb(t.border))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.))
        .text_size(crate::appearance::ui_size(12.))
        .when(can_compact, |s| s.cursor_pointer())
        .text_color(rgb(fg))
        .hover(move |s| {
            if can_compact {
                s.bg(rgb(t.bg_hover))
            } else {
                s
            }
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                // 双重守卫：按钮禁用态之外再查一次 runtime（agent
                // 跑动中 compact 会中止当前 turn）
                if this.rt().read(cx).can_compact() {
                    this.rt().update(cx, |r, cx| r.compact(cx));
                }
            }),
        )
        .child(crate::ui::icon("icon-compress", 13., fg))
        .child(SharedString::from(compact_label))
}
