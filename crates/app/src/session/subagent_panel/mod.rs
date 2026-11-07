//! 子代理运行状态面板（044-1 §1 / 044-0 验收⑥改版）——当前会话子代理
//! catalog 的聚合视图，**依附右侧导航面板的次级弹窗**。
//!
//! 2026-11-07 用户定稿（fn_bar 独立浮层退役）：
//! - 导航轮卡有子代理的轮 → 第三行 `[bot] ●●●●`（点数 = 子代理数，色 =
//!   会话列表青红点：运行 accent / 完成 NOTICE / 异常 danger）；
//! - 悬浮该卡 → 左侧弹 500px【子代理详情扩展菜单】（无头部统计行），
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

/// 扩展菜单几何（044-0 验收⑥定稿）：宽 500、状态卡高 70、菜单最小高 90。
/// 500 = 用户定稿（2026-11-07）：统计数据怕多走形，导航面板同步 +50 补偿。
const MENU_W: f32 = 500.;
const CARD_H: f32 = 70.;
const MENU_MIN_H: f32 = 90.;
/// 菜单与导航 flyout 的间距 2px（用户定稿：视觉上连成一体）。定位基准 =
/// flyout 实测 bounds（layout.nav_flyout_bounds），不再用跨容器常量
/// ——chat-wrap 右缘 ≠ flyout 容器右缘，常量曾把 2px 缝实出 ~19px。

/// 状态色（照抄 cards.rs 的 subagent 卡片；中止橙按 044-1 §1.3 交互稿）。
const C_RUNNING: u32 = 0xeab308;
const C_DONE: u32 = 0x22c55e;
const C_ERROR: u32 = 0xf87171;
const C_ABORTED: u32 = 0xf97316;

/// 面板宿主状态：持 catalog 缓存（offset+rows）+ 按轮归组投影。
///
/// `polling` 守卫常驻轮询任务只拉起一份——轮询从会话激活跑到 Chat 消亡，
/// 换会话在 poll 内自愈归零重扫。菜单可见性 = `menu_anchor`（hover 的轮卡
/// bounds，nav_card 测量写入；收起走 250ms 离开宽限，进菜单/回卡取消）。
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
    /// 收起走 250ms 离开宽限（[`Self::menu_hide_at`]）——鼠标要能从卡/面板
    /// 跨进菜单，立即清锚菜单永远进不去（真机 2026-11-07）。
    pub menu_anchor: Option<(usize, Bounds<Pixels>)>,
    /// 菜单收起宽限的挂钟起点（flyout/菜单 hover 离开时挂，pumps 到期清锚；
    /// 进菜单/回卡即取消）。None = 无待执行的收起。
    pub menu_hide_at: Option<std::time::Instant>,
    /// 鼠标是否在菜单上（menu on_hover 维护）。宽限到期与 flyout 离开都看
    /// 它：导航 flyout 宽限到期收起、元素卸载时可能补发 hover(false)——
    /// 无此旗标会给正被悬停的菜单重新挂上收起 deadline（鼠标底下消失）。
    pub menu_hovered: bool,
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
                menu_hide_at: None,
                menu_hovered: false,
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
            panel.menu_hide_at = None;
            panel.menu_hovered = false;
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
/// 【子代理详情扩展菜单】（044-0 验收⑥）：导航轮卡 hover 左弹的 500px
/// 次级浮层。可见性 = `menu_anchor`（hover 的轮卡 bounds；收起走 250ms
/// 离开宽限——flyout/菜单离开挂 deadline，进菜单/回卡取消，pumps 到期清）。
/// 垂直定位：默认上沿齐卡上沿向下伸（至多面板底）；卡在屏幕下半（向下
/// 不足最小高）翻转——下沿齐卡下沿向上伸（至多面板顶）；内容超长滚动。
/// **无头部统计行**（用户定稿）。
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
    // 仍在，自愈）。水平定位基准 = flyout 实测左缘（paint 期 canvas 写入
    // layout.nav_flyout_bounds）——菜单右缘 = flyout 左缘 − 2px
    let wrap = chat.dlg.pill_btn.chat_wrap.borrow().clone()?;
    let flyout = chat.layout.nav_flyout_bounds.borrow().clone()?;
    let menu_right = wrap.right() - flyout.origin.x + px(2.);
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
        // 左右 padding 10px（用户定稿 2026-11-07：两侧对称），行内不再
        // 自带水平 padding——hover 高亮同样内缩 10px，边缘干净
        .px(px(10.))
        .py(px(6.));
    for row in &rows_snapshot {
        list = list.child(panel_row(&weak_rows, row));
    }
    let weak_menu = weak.clone();
    let mut menu = div()
        .id("subagent-menu")
        .absolute()
        .right(menu_right)
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
        .overflow_hidden()
        // 悬停即菜单存活：进菜单取消收起宽限，离开挂 250ms deadline
        //（pumps 到期清锚）——occlude 隔断下层消息区的 hover/click 串扰
        .occlude()
        .on_hover(move |h: &bool, _w, cx| {
            let _ = weak_menu.update(cx, |this, cx| {
                if let Some(p) = this.dlg.subagent_panel.as_mut() {
                    p.menu_hovered = *h;
                    // 离开菜单且未回面板 → 挂 250ms 宽限（pumps 到期清锚）；
                    // enter 一律取消。绘制逆序的迟到 leave 若鼠标已回面板
                    //（flyout_hovered=true）不上 deadline，防悬停中被收
                    p.menu_hide_at = if *h || this.layout.nav_flyout_hovered {
                        None
                    } else {
                        Some(std::time::Instant::now())
                    };
                }
                cx.notify();
            });
        });
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
pub(crate) mod rows;
pub(crate) use rows::duration_text;
use rows::{panel_row, row_dot_color};
