//! 子代理运行状态面板（044-1 §1 / 044-0 验收⑥改版）——当前会话子代理
//! catalog 的聚合视图，**依附右侧导航面板的次级弹窗**。
//!
//! 2026-11-07 用户定稿（fn_bar 独立浮层退役）：
//! - 导航轮卡有子代理的轮 → 第三行 `[bot] ●●●●`（点数 = 子代理数，色 =
//!   会话列表青红点：运行 accent / 完成 NOTICE / 异常 danger）；
//! - 悬浮该卡 → 左侧弹 400px【子代理详情扩展菜单】（无头部统计行），
//!   上沿齐卡上沿向下伸；卡在屏幕下半则下沿齐卡下沿向上伸，至多不超过
//!   导航面板；内容超长滚动；
//! - 状态卡点击 = 新 tab 打开子会话（观察窗，无输入面板）。
//!
//! 数据面不变：事实源 = 父会话 JSONL 的 `subagent/descriptor`
//! （[`pi_link::subagent_catalog`]），常驻轮询（1s/5s），会话激活即建缓存；
//! poll 顺带按轮归组（第 n 条 User 消息 = 第 n 轮，与 nav.rs 轮构建同口径；
//! descriptor 的 parentToolCallId 对上该轮 `agent` 工具调用 id）。

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gpui::{Bounds, MouseButton, Pixels, SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::session::Block;
use pi_link::subagent_catalog::{
    CatalogRow, StatusKind, merge_rows, parse_iso_secs, read_catalog, row_duration_secs,
};

/// 打开期轮询间隔（044-1 §1.4：1s 增量读）。
const POLL_TICK: std::time::Duration = std::time::Duration::from_secs(1);

/// 扩展菜单几何（044-0 验收⑥定稿）：宽 400、状态卡高 70、菜单最小高 90。
const MENU_W: f32 = 400.;
const CARD_H: f32 = 70.;
const MENU_MIN_H: f32 = 90.;
/// 菜单与导航 flyout（400px）的间距；flyout 锚在 gutter（14px）左侧。
const MENU_RIGHT: f32 = 14. + 400. + 6.;

/// 状态色（照抄 cards.rs 的 subagent 卡片；中止橙按 044-1 §1.3 交互稿）。
const C_RUNNING: u32 = 0xeab308;
const C_DONE: u32 = 0x22c55e;
const C_ERROR: u32 = 0xf87171;
const C_ABORTED: u32 = 0xf97316;

/// 面板宿主状态：持 catalog 缓存（offset+rows）+ 按轮归组投影。
///
/// `polling` 守卫常驻轮询任务只拉起一份——轮询从会话激活跑到 Chat 消亡，
/// 换会话在 poll 内自愈归零重扫。菜单可见性 = `menu_anchor`（hover 的轮卡
/// bounds，nav_card 测量写入；flyout hover 离开时清）。
#[derive(Clone)]
pub(crate) struct SubagentPanel {
    /// 父会话文件（descriptor 的落盘处）；没落盘的会话无 catalog。
    pub file: Option<PathBuf>,
    /// 下一轮增量续读的字节偏移。
    pub offset: u64,
    /// 累积的 descriptor 投影（同 childId 末条覆盖）。
    pub rows: Vec<CatalogRow>,
    /// 按轮归组（turn 序 = 第 n 条 User 消息）：导航卡第三行 + 菜单数据源。
    pub turn_agents: Vec<Vec<CatalogRow>>,
    /// hover 的导航轮卡（turn 序 + 卡窗口坐标）：菜单锚；None = 菜单收起。
    pub menu_anchor: Option<(usize, Bounds<Pixels>)>,
    /// 常驻轮询任务是否在跑（防重复 spawn）。
    pub polling: bool,
}

/// hover 轮卡 bounds 的测量槽位（nav_card 挂 tracker canvas 每帧写，
/// on_hover 事件读——同 PillBtns::tracker 模式，但生命周期只在单帧渲染
/// 闭包内，不入 Chat）。
pub(crate) type BoundsSlot = Rc<RefCell<Option<Bounds<Pixels>>>>;

impl Chat {
    /// 会话激活时常备面板缓存——导航卡状态点从切换起就准，不依赖「先
    /// hover 过」。switch_pool_rt 调；空结构起步，首轮 poll 全量扫，不拖
    /// 慢切换热路径。
    pub(crate) fn ensure_subagent_panel(&mut self, cx: &mut gpui::Context<Self>) {
        let file = self.rt().read(cx).file.clone();
        let already = self
            .dlg
            .subagent_panel
            .as_ref()
            .is_some_and(|p| p.file == file);
        if !already {
            let polling = self.dlg.subagent_panel.as_ref().is_some_and(|p| p.polling);
            self.dlg.subagent_panel = Some(SubagentPanel {
                file,
                offset: 0,
                rows: Vec::new(),
                turn_agents: Vec::new(),
                menu_anchor: None,
                polling,
            });
        }
        ensure_poll(self, cx);
    }

    /// 一轮增量轮询：只读上次 offset 之后的新增字节，合并后重绘 + 按轮
    /// 归组重算。返回下一轮间隔；None = 面板没了（Chat 消亡）→ 任务退场。
    /// 会话切换在这里自愈：缓存挂的还是旧会话 → 归零重扫新会话。
    fn subagent_panel_poll(&mut self, cx: &mut gpui::Context<Self>) -> Option<std::time::Duration> {
        let rt = self.rt().clone();
        let current = rt.read(cx).file.clone();
        let panel = self.dlg.subagent_panel.as_mut()?;
        if panel.file != current {
            panel.file = current;
            panel.offset = 0;
            panel.rows.clear();
            panel.turn_agents.clear();
            panel.menu_anchor = None;
        }
        let Some(file) = panel.file.clone() else {
            // 会话还没落盘：没东西可读，慢速等文件出现
            return Some(POLL_TICK);
        };
        let scan = read_catalog(&file, panel.offset);
        if scan.next_offset != panel.offset {
            panel.offset = scan.next_offset;
            merge_rows(&mut panel.rows, scan.rows);
        }
        // 按轮归组（044-0 验收⑥）：轮序 = 第 n 条 User 消息（nav.rs 构建
        // 同口径）；该轮 assistant 消息里的 agent 工具调用 id → descriptor
        // 的 parentToolCallId 对号入座。messages 变化（新轮/新调用）随 poll
        // 1s 内跟上，渲染帧零扫描。
        let mut call_turn: HashMap<&str, usize> = HashMap::new();
        let mut n_turns = 0usize;
        for m in rt.read(cx).messages.iter() {
            if m.role == crate::session::Role::User {
                n_turns += 1;
                continue;
            }
            if m.role != crate::session::Role::Assistant {
                continue;
            }
            for b in &m.blocks {
                if let Block::ToolCall { id, name, .. } = b {
                    if name == "agent" {
                        call_turn.insert(id.as_str(), n_turns.saturating_sub(1));
                    }
                }
            }
        }
        let mut turn_agents: Vec<Vec<CatalogRow>> = vec![Vec::new(); n_turns];
        for row in &panel.rows {
            if let Some(pid) = row.parent_tool_call_id.as_deref() {
                if let Some(&ti) = call_turn.get(pid) {
                    if let Some(slot) = turn_agents.get_mut(ti) {
                        slot.push(row.clone());
                        continue;
                    }
                }
            }
            // 无 parentToolCallId（老 descriptor）或轮已滚出：不进导航卡，
            // 菜单也不孤儿展示——面板行数（徽标）仍计它
        }
        let changed = panel.turn_agents != turn_agents;
        panel.turn_agents = turn_agents;
        if changed {
            cx.notify();
        }
        Some(POLL_TICK)
    }

    /// 整行点击：子会话是普通 pi 会话，直接 open_session（只读观察窗 +
    /// 可续跑），并收菜单（缓存留着）。
    pub(crate) fn open_subagent_child(&mut self, path: PathBuf, cx: &mut gpui::Context<Self>) {
        if let Some(p) = self.dlg.subagent_panel.as_mut() {
            p.menu_anchor = None;
        }
        self.open_session(path, false, cx);
        cx.notify();
    }
}

/// 拉起常驻轮询（幂等：`polling` 已在跑则不动）。
fn ensure_poll(chat: &mut Chat, cx: &mut gpui::Context<Chat>) {
    if let Some(p) = chat.dlg.subagent_panel.as_mut() {
        if p.polling {
            return;
        }
        p.polling = true;
    }
    spawn_poll(cx);
}

/// 常驻轮询任务：间隔由上轮 poll 的返回值决定（开 1s / 关 5s，同 psp_drag
/// 看门狗范式）；poll 返回 None（Chat 消亡）才退场。
fn spawn_poll(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        let mut tick = POLL_TICK;
        loop {
            cx.background_executor().timer(tick).await;
            let next = this
                .update(cx, |chat, cx| chat.subagent_panel_poll(cx))
                .unwrap_or(None);
            let Some(next_tick) = next else { break };
            tick = next_tick;
        }
    })
    .detach();
}

/// 面板本体：浮层（044-1 §1.1 三改——二改的停靠版会把消息区下推，用户
/// 不要）。`absolute` 挂 chat-wrap（relative）顶左 = 操作栏正下方悬浮，
/// 不推挤消息区；**不挂全屏外点拦截**（那会把 composer 模型下拉等点击一起
/// 吃掉，见 buglist 第三批），关 = ✕ 或再点操作栏按钮。
/// `open=false` → None（不占渲染）。
/// 【子代理详情扩展菜单】（044-0 验收⑥）：导航轮卡 hover 左弹的 400px
/// 次级浮层。可见性 = `menu_anchor`（hover 的轮卡 bounds；flyout hover
/// 离开统一清）。垂直定位：默认上沿齐卡上沿向下伸（至多面板底）；卡在
/// 屏幕下半（向下不足最小高）翻转——下沿齐卡下沿向上伸（至多面板顶）；
/// 内容超长滚动。**无头部统计行**（用户定稿）。
/// 挂 chat-wrap（relative）；坐标 = 卡窗口坐标 − wrap 原点。
pub(crate) fn subagent_menu_element(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
) -> Option<gpui::AnyElement> {
    let t = T();
    let panel = chat.dlg.subagent_panel.as_ref()?;
    let (ti, card) = panel.menu_anchor.as_ref()?;
    let rows = panel.turn_agents.get(*ti)?;
    if rows.is_empty() {
        return None;
    }
    // wrap 原点换算（菜单挂 wrap 内 absolute）：槽位由 chat-wrap 的
    // tracker 捕获（044-0 起沿用）；未就绪首帧不出（下一帧 hover 事件
    // 仍在，自愈）
    let wrap = chat.dlg.pill_btn.chat_wrap.borrow().clone()?;
    let card_top = (card.origin.y - wrap.origin.y).max(px(0.));
    let card_bottom = (card.bottom() - wrap.origin.y).min(wrap.size.height);
    // 垂直：向下优先（上沿齐卡上沿），可用高不足最小高翻转向上（下沿齐
    // 卡下沿）；两种模式的伸展上限都 = 导航面板范围（wrap 高内收 4px）
    let avail_down = wrap.size.height - card_top - px(4.);
    let (top, bottom, max_h) = if avail_down >= px(MENU_MIN_H) {
        (Some(card_top), None, avail_down)
    } else {
        let avail_up = card_bottom - px(4.);
        (
            None,
            Some(wrap.size.height - card_bottom),
            avail_up.max(px(MENU_MIN_H)),
        )
    };
    let weak_rows = weak.clone();
    let rows_snapshot: Vec<CatalogRow> = rows.clone();
    let mut list = div()
        .id("subagent-menu-list")
        .w_full()
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .px(px(6.))
        .py(px(6.));
    for row in &rows_snapshot {
        list = list.child(panel_row(&weak_rows, row));
    }
    let mut menu = div()
        .id("subagent-menu")
        .absolute()
        .right(px(MENU_RIGHT))
        .w(px(MENU_W))
        .min_h(px(MENU_MIN_H))
        .max_h(max_h)
        .flex()
        .flex_col()
        .rounded(px(8.))
        .border_1()
        .border_color(rgb(t.border))
        .bg(rgb(t.bg))
        .shadow_lg()
        .overflow_hidden();
    if let Some(top) = top {
        menu = menu.top(top);
    }
    if let Some(bottom) = bottom {
        menu = menu.bottom(bottom);
    }
    menu = menu.child(list);
    Some(menu.into_any_element())
}

/// 导航轮卡第三行（044-0 验收⑥①）：`[bot] ●●●●`——点数 = 该轮子代理
/// 数，点色 = 会话列表青红点口径（运行 accent / 完成 NOTICE / 异常 danger，
/// [`row_dot_color`]）。
pub(crate) fn turn_agents_row(rows: &[CatalogRow]) -> gpui::AnyElement {
    let t = T();
    let mut dots = div().flex().items_center().gap(px(4.));
    for r in rows {
        dots = dots.child(div().size(px(6.)).rounded_full().bg(rgb(row_dot_color(r))));
    }
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .mt(px(5.))
        // 与 agent 摘要行同构：18px 标号列对齐（bot icon 居位）
        .child(
            div()
                .w(px(18.))
                .flex()
                .justify_end()
                .child(crate::ui::icon("bot", 11., t.text_dim)),
        )
        .child(dots)
        .into_any_element()
}

/// 第三行状态点色（会话运行结果青红点）：运行 accent、完成 NOTICE、
/// 异常/中止/轮次用尽 danger。
fn row_dot_color(row: &CatalogRow) -> u32 {
    let t = T();
    if row.in_flight() {
        t.accent
    } else if row.status_kind() == StatusKind::Done {
        crate::appearance::theme::NOTICE
    } else {
        t.danger
    }
}

fn panel_row(weak: &gpui::WeakEntity<Chat>, row: &CatalogRow) -> gpui::AnyElement {
    let t = T();
    let kind = row.status_kind();
    let mark_c = mark_color(row);
    let name = row.profile.clone().unwrap_or_else(|| row.provider.clone());
    let task = row
        .label
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| row.child_id.clone());
    let open = row.session_file.clone().map(PathBuf::from);

    let mut el = div()
        .id(SharedString::from(format!("subagent-row-{}", row.child_id)))
        .w_full()
        .h(px(CARD_H))
        .px(px(12.))
        .mb(px(1.))
        .rounded(px(crate::ui::tokens::radius::INLINE))
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(6.))
        .text_size(crate::appearance::ui_size(12.))
        .hover(|s| s.bg(rgb(t.bg_hover)));
    if open.is_some() {
        let weak_row = weak.clone();
        el = el
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                let _ = weak_row.update(cx, |c, cx| {
                    if let Some(p) = open.clone() {
                        c.open_subagent_child(p, cx);
                    }
                });
            });
    }
    el.child(
        // 行一：状态标记 + 名称 + 统计（044-0 验收④：时长/轮/言/工具/token
        // icon 化上移到名字后）+ 失败分码（B3④：被拒/失败行没有子会话可
        // 点，理由内联行尾可见）
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(status_mark(kind, mark_c))
            .child(
                div()
                    .flex_shrink_0()
                    .max_w(px(140.))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(rgb(t.text))
                    .child(SharedString::from(name)),
            )
            .child(
                // stats 容器：**必须显式 .flex()**——无它子元素纵向堆叠
                //（gpui div 缺省非 row flex；验收④返工实录）
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .text_size(crate::appearance::ui_size(11.))
                    .text_color(rgb(t.text_dim))
                    .children(stats_segments(row)),
            )
            .children(
                row.error_code
                    .as_deref()
                    .filter(|_| matches!(kind, StatusKind::Error))
                    .map(|code| {
                        div()
                            .flex_shrink_0()
                            .text_size(crate::appearance::ui_size(10.))
                            .text_color(rgb(C_ERROR))
                            .child(SharedString::from(code.to_string()))
                    }),
            ),
    )
    .child(
        // 行二：任务名（label 退 childId，超长截断）
        div()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_ellipsis()
            .px(px(23.))
            .text_size(crate::appearance::ui_size(11.))
            .text_color(rgb(t.text_dim))
            .child(SharedString::from(task)),
    )
    .into_any_element()
}

/// 行一统计段（044-0 验收④）：时长恒有；stats/usage 缺项（老 descriptor）
/// 整段隐藏，不摆 "—" 占位。轮/言/工具/时长/token 全 icon+数字（右侧导航
/// 条同款 + clock-4/coins，用户供图 lucide）。
fn stats_segments(row: &CatalogRow) -> Vec<gpui::AnyElement> {
    let t = T();
    let mut segs: Vec<gpui::AnyElement> = Vec::new();
    segs.push(
        div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(4.))
            .child(crate::ui::icon("clock-4", 11., t.text_dim))
            .child(SharedString::from(live_duration_text(row)))
            .into_any_element(),
    );
    let mut push_stat = |icon_name: &'static str, n: u64| {
        segs.push(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(crate::ui::icon(icon_name, 11., t.text_dim))
                .child(SharedString::from(n.to_string()))
                .into_any_element(),
        );
    };
    if let Some(n) = row.turns {
        push_stat("message-square-text", n);
    }
    if let Some(n) = row.agent_messages {
        push_stat("bot", n);
    }
    if let Some(n) = row.tool_calls {
        push_stat("wrench", n);
    }
    if let Some(n) = row.usage_total {
        // token 走 tokens_text 的 k 格式化（306k；push_stat 是裸计数专用）
        segs.push(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(crate::ui::icon("coins", 11., t.text_dim))
                .child(SharedString::from(tokens_text(n)))
                .into_any_element(),
        );
    }
    segs
}

/// 行的运行耗时秒数——**头部合计与行时远唯一口径**（044-0 B4：原先头部
/// 用 stale 的 updatedAt−startedAt、行用 live now−startedAt，出现合计 3s
/// vs 行 13s 的矛盾）：运行中 = 实时 now−startedAt（轮间隔内补齐实时性），
/// 终态 = updatedAt−startedAt。
fn row_secs(row: &CatalogRow) -> Option<i64> {
    if row.in_flight() {
        let a = parse_iso_secs(&row.started_at)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(a);
        return Some((now - a).max(0));
    }
    row_duration_secs(row)
}

/// 行一时长（044-0 运行时长）。
fn live_duration_text(row: &CatalogRow) -> String {
    row_secs(row)
        .map(duration_text)
        .unwrap_or_else(|| "—".into())
}

/// 行一状态标记（044-0：同会话列表）——运行中 = 8 针荆棘圈 spinner；
/// 终态 = 色点（绿完成 / 橙中止与轮次用尽 / 红失败）。
fn status_mark(kind: StatusKind, color: u32) -> gpui::AnyElement {
    match kind {
        StatusKind::Running => crate::ui::spark_spinner(15., color),
        _ => div()
            .size(px(7.))
            .rounded_full()
            .bg(rgb(color))
            .into_any_element(),
    }
}

/// 状态标记用色（与行文案 status_of 同源，字体色换标记色）。
fn mark_color(row: &CatalogRow) -> u32 {
    match row.status_kind() {
        StatusKind::Running => C_RUNNING,
        StatusKind::Done => C_DONE,
        StatusKind::Aborted | StatusKind::MaxTurns => C_ABORTED,
        StatusKind::Error => C_ERROR,
    }
}

/// 秒 → 紧凑时长（pi-web 口径）：22s / 1m42s。
pub(crate) fn duration_text(secs: i64) -> String {
    if secs < 0 {
        return "—".into();
    }
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m{}s", secs / 60, secs % 60)
    }
}

/// tokens 紧凑格式（pi-web 口径）：870 / 23k / 100k（044-0 行二：k为单位）。
fn tokens_text(n: u64) -> String {
    if n < 1000 {
        n.to_string()
    } else {
        format!("{}k", (n + 500) / 1000)
    }
}
