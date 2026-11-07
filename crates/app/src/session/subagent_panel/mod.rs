//! 子代理导航数据面（044-0 验收⑥）——**完全由主会话消息推导**
//! （pi-flash-9eck，2026-11-08 用户定夺：descriptor 落盘 + 父会话 catalog
//! 读取整链删除；主会话对子代理只有无状态引用，子会话不回补任何东西）。
//!
//! 事实源：
//! - caller 交付：`agent` 工具卡自身 result details（运行中 status=running /
//!   终态 stop_reason 齐全，扩展 tool.ts `success()`）；
//! - background 交付：派发 result 只有 status=started，终态由父会话
//!   `subagent-completion` custom 消息（扩展 onCompleted 落盘，5554fa4a）
//!   回灌 ToolCall details（[`Chat::sync_cards_from_completions`]）。
//!
//! 导航轮卡第三行（第 n 条 User 消息 = 第 n 轮，与 nav.rs 轮构建同口径）的
//! 点阵 = 该轮 agent 工具调用状态点。常驻增量任务（1s）纯内存扫描，零 IO。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui::{div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::session::Block;

/// 打开期轮询间隔。
const POLL_TICK: std::time::Duration = std::time::Duration::from_secs(1);

/// 导航数据面宿主状态：agent 调用落点映射 + 按轮状态点投影。
///
/// `polling` 守卫常驻任务只拉起一份——轮询从会话激活跑到 Chat 消亡；
/// 换会话在 [`TurnScan`] 身份锚失效时自愈重扫。
#[derive(Clone, Default)]
pub(crate) struct SubagentPanel {
    /// 常驻增量任务是否在跑（防重复 spawn）。
    pub polling: bool,
    /// 按轮的第三行状态点（轮序 = 第 n 条 User 消息）：导航卡数据源。
    pub turn_agents: Vec<Vec<DotTone>>,
    /// agent 工具调用 toolCallId → 会话流落点（增量维护）。
    calls: HashMap<String, CallSite>,
    /// 子代理 childId（details.child_id）→ 会话流落点：completion 回灌
    /// 与「点卡片开子会话」反查用。
    by_child: HashMap<String, CallSite>,
    /// 增量游标（消息数 / 轮数 / 首条消息时间戳）。
    turn_scan: TurnScan,
}

/// 一个 `agent` 工具调用在会话流里的落点：轮序号（导航归组）+ 消息/块下标
/// （completion 回灌按此就地改写）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct CallSite {
    pub turn: usize,
    pub msg: usize,
    pub block: usize,
}

/// 增量游标。`first_ts` 是会话身份锚：换会话 / fork / 回退时首条消息时间戳
/// 变（或消息数变短）→ 整份游标与映射作废重扫。
#[derive(Clone, Default)]
struct TurnScan {
    len: usize,
    turns: usize,
    first_ts: Option<i64>,
}

/// 增量维护 `agent 工具调用 / childId → 会话流落点`（第 n 条 User 消息 =
/// 第 n-1 轮，与 nav.rs 轮构建同口径）。只扫游标之后的消息，但每次都回退到
/// **最后一条**重扫：流式 / 后到的 toolCall 块会长在它上面。
fn scan_agent_calls(
    msgs: &[crate::session::messages::Msg],
    scan: &mut TurnScan,
    calls: &mut HashMap<String, CallSite>,
    by_child: &mut HashMap<String, CallSite>,
) {
    let first_ts = msgs.first().and_then(|m| m.ts);
    // 换会话 / fork / 回退：len 变小或首条时间戳变 → 游标与映射作废。
    if msgs.len() < scan.len || first_ts != scan.first_ts {
        calls.clear();
        by_child.clear();
        *scan = TurnScan {
            len: 0,
            turns: 0,
            first_ts,
        };
    }
    let scanned = scan.len.min(msgs.len());
    let from = scanned.saturating_sub(1);
    let mut n_turns = scan.turns
        - msgs[from..scanned]
            .iter()
            .filter(|m| m.role == crate::session::Role::User)
            .count();
    for (mi, m) in msgs.iter().enumerate().skip(from) {
        if m.role == crate::session::Role::User {
            n_turns += 1;
            continue;
        }
        if m.role != crate::session::Role::Assistant {
            continue;
        }
        for (bi, b) in m.blocks.iter().enumerate() {
            let Block::ToolCall {
                id, name, details, ..
            } = b
            else {
                continue;
            };
            if name != "agent" {
                continue;
            }
            let site = CallSite {
                turn: n_turns.saturating_sub(1),
                msg: mi,
                block: bi,
            };
            if !id.is_empty() {
                calls.insert(id.clone(), site);
            }
            // 派发 result details 带 child_id（扩展 tool.ts）：completion
            // 回灌按 childId 找落点。
            if let Some(cid) = details
                .as_ref()
                .and_then(|d| d.get("child_id"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
            {
                by_child.insert(cid.to_string(), site);
            }
        }
    }
    *scan = TurnScan {
        len: msgs.len(),
        turns: n_turns,
        first_ts,
    };
}

impl Chat {
    /// 会话激活：面板常备（纯内存推导，无需按会话归档）。switch_pool_rt 调。
    pub(crate) fn ensure_subagent_panel(&mut self, cx: &mut gpui::Context<Self>) {
        if self.dlg.subagent_panel.is_none() {
            self.dlg.subagent_panel = Some(SubagentPanel::default());
        }
        ensure_poll(self, cx);
    }

    /// 一轮增量任务：全内存（落点扫描 + 第三行投影 + completion 回灌）。
    /// 返回下一轮间隔；None = 面板没了（Chat 消亡）→ 任务退场。
    fn subagent_panel_poll(&mut self, cx: &mut gpui::Context<Self>) -> Option<std::time::Duration> {
        let rt = self.rt().clone();
        self.regroup_turn_agents(&rt, cx);
        self.sync_cards_from_completions(&rt, cx);
        Some(POLL_TICK)
    }

    /// 按轮状态点投影（第三行）：直接遍历主会话消息——每秒一次、无 IO、
    /// 无 JSON 解析，长会话也是微秒级；顺带自愈落点下标漂移。
    fn regroup_turn_agents(
        &mut self,
        rt: &gpui::Entity<crate::session::runtime::SessionRuntime>,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(panel) = self.dlg.subagent_panel.as_mut() else {
            return;
        };
        let msgs = rt.read(cx).messages.as_slice();
        scan_agent_calls(
            msgs,
            &mut panel.turn_scan,
            &mut panel.calls,
            &mut panel.by_child,
        );
        let mut turn_agents: Vec<Vec<DotTone>> = Vec::new();
        let mut cur: Vec<DotTone> = Vec::new();
        for m in msgs {
            if m.role == crate::session::Role::User {
                turn_agents.push(std::mem::take(&mut cur));
                continue;
            }
            if m.role != crate::session::Role::Assistant {
                continue;
            }
            for b in &m.blocks {
                if let Block::ToolCall { name, .. } = b {
                    if name == "agent" {
                        cur.push(dot_tone_of(b));
                    }
                }
            }
        }
        if !cur.is_empty() {
            turn_agents.push(cur);
        }
        let changed = panel.turn_agents != turn_agents;
        panel.turn_agents = turn_agents;
        if changed {
            cx.notify();
        }
    }

    /// `subagent-completion` custom 消息（background 终态）回灌会话流卡片：
    /// 按 childId 找到派发 ToolCall 块，就地合并 details 终态字段并置
    /// result_arrived——卡片从「运行中」切到任务完成/执行异常，第三行点
    /// 落色（不追加消息，也不动索引）。幂等：重复应用值相同不 notify。
    fn sync_cards_from_completions(
        &mut self,
        rt: &gpui::Entity<crate::session::runtime::SessionRuntime>,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(panel) = self.dlg.subagent_panel.as_mut() else {
            return;
        };
        // 收集 patch：childId → (终态字段)。details 在 custom Msg 上原样保留
        // （扩展 completion.ts：childId / stopReason / errorCode / session_file）。
        let mut patches: Vec<(String, serde_json::Value)> = Vec::new();
        for m in rt.read(cx).messages.as_slice() {
            if m.role != crate::session::Role::Custom
                || m.custom_type.as_deref() != Some("subagent-completion")
            {
                continue;
            }
            let Some(d) = &m.details else { continue };
            let Some(cid) = d.get("childId").and_then(|v| v.as_str()) else {
                continue;
            };
            let Some(stop) = d.get("stopReason").and_then(|v| v.as_str()) else {
                continue;
            };
            let mut merged = serde_json::Map::new();
            merged.insert("status".into(), stop.into());
            merged.insert("stop_reason".into(), stop.into());
            if let Some(v) = d.get("errorCode") {
                merged.insert("error_code".into(), v.clone());
            }
            if let Some(v) = d.get("session_file") {
                merged.insert("session_file".into(), v.clone());
            }
            patches.push((cid.to_string(), serde_json::Value::Object(merged)));
        }
        if patches.is_empty() {
            return;
        }
        let mut changed = false;
        rt.update(cx, |rt, _cx| {
            for (cid, fields) in &patches {
                let Some(site) = panel.by_child.get(cid).copied() else {
                    continue;
                };
                let Some(block) = rt
                    .messages
                    .get_mut(site.msg)
                    .and_then(|m| m.blocks.get_mut(site.block))
                else {
                    continue;
                };
                if let Block::ToolCall {
                    details: slot,
                    result_arrived,
                    ..
                } = block
                {
                    let base =
                        slot.get_or_insert_with(|| serde_json::Value::Object(Default::default()));
                    if let serde_json::Value::Object(map) = base {
                        if let serde_json::Value::Object(fields) = fields {
                            for (k, v) in fields {
                                if map.get(k) != Some(v) {
                                    map.insert(k.clone(), v.clone());
                                    changed = true;
                                }
                            }
                        }
                    }
                    if !*result_arrived {
                        *result_arrived = true;
                        changed = true;
                    }
                }
            }
        });
        if changed {
            cx.notify();
        }
    }

    /// 内存里反查某个子代理的子会话文件：卡片点击时 details 还没带
    /// session_file 的窗口期（background 派发尚未落 completion）用。
    pub(crate) fn session_file_for_child(
        &self,
        cx: &mut gpui::Context<Self>,
        child_id: &str,
    ) -> Option<PathBuf> {
        for m in self.rt().read(cx).messages.as_slice() {
            for b in &m.blocks {
                if let Block::ToolCall {
                    details: Some(d), ..
                } = b
                {
                    if d.get("child_id").and_then(|v| v.as_str()) == Some(child_id) {
                        if let Some(sf) = d
                            .get("session_file")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                        {
                            return Some(PathBuf::from(sf));
                        }
                    }
                }
            }
        }
        None
    }

    /// 主会话里反查某个子会话文件的展示名：agent 调用 args 的 label，退回
    /// profile，再退回 prompt 首行。打开子会话 tab 的标题源（044-0）。
    pub(crate) fn label_for_child(
        &self,
        cx: &mut gpui::Context<Self>,
        path: &Path,
    ) -> Option<String> {
        for m in self.rt().read(cx).messages.as_slice() {
            for b in &m.blocks {
                let Block::ToolCall {
                    name,
                    args,
                    details,
                    ..
                } = b
                else {
                    continue;
                };
                if name != "agent" {
                    continue;
                }
                let sf = details
                    .as_ref()
                    .and_then(|d| d.get("session_file"))
                    .and_then(|v| v.as_str());
                if !sf.is_some_and(|s| {
                    crate::services::workspace::same_path(std::path::Path::new(s), path)
                }) {
                    continue;
                }
                let Ok(v) = serde_json::from_str::<serde_json::Value>(args) else {
                    continue;
                };
                let label = v
                    .get("label")
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .or_else(|| {
                        v.get("profile")
                            .and_then(|x| x.as_str())
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                    })
                    .or_else(|| {
                        v.get("prompt")
                            .and_then(|x| x.as_str())
                            .map(|p| p.chars().take(40).collect())
                    });
                if label.is_some() {
                    return label;
                }
            }
        }
        None
    }
}

/// 拉起常驻增量任务（幂等：`polling` 已在跑则不动）。
fn ensure_poll(chat: &mut Chat, cx: &mut gpui::Context<Chat>) {
    if let Some(p) = chat.dlg.subagent_panel.as_mut() {
        if p.polling {
            return;
        }
        p.polling = true;
    }
    spawn_poll(cx);
}

/// 常驻任务：间隔由上轮返回值决定；poll 返回 None（Chat 消亡）才退场。
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

/// 导航轮卡第三行（044-0 验收⑥①）：`[bot] ●●●●`——点数 = 该轮子代理数，
/// 点色 = 会话列表状态槽口径（[`DotTone`]，7px 同列表彩点）。
/// 事后统计呈现（2026-11-07 用户定夺）；超长截断（容器裁切）。
pub(crate) fn turn_agents_row(tones: &[DotTone]) -> gpui::AnyElement {
    let t = T();
    let mut dots = div().flex().flex_shrink_0().items_center().gap(px(4.));
    for tone in tones {
        dots = dots.child(div().size(px(7.)).rounded_full().bg(rgb(dot_color(*tone))));
    }
    div()
        .min_w_0()
        .overflow_hidden()
        .flex()
        .items_center()
        .gap(px(6.))
        .mt(px(5.))
        // 与 agent 摘要行同构：18px 标号列对齐（bot icon 居位）
        .child(
            div()
                .w(px(18.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .child(crate::ui::icon("bot", 11., t.text_dim)),
        )
        .child(dots)
        .into_any_element()
}

/// 第三行状态点色（会话列表状态槽口径）。
pub(crate) mod rows;
pub(crate) use rows::DotTone;
use rows::{dot_color, dot_tone_of};

#[cfg(test)]
mod turn_scan_tests;
