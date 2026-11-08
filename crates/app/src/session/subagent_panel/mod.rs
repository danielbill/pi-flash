//! 子代理导航数据面（044-0 验收⑥）——**完全由主会话消息推导**
//! （pi-flash-9eck，2026-11-08 用户定夺：descriptor 落盘 + 父会话 catalog
//! 读取整链删除；主会话对子代理只有无状态引用，子会话不回补任何东西）。
//!
//! 事实源只有一个：**消息流**。
//! - caller 交付：`agent` 工具卡自身 result details（运行中 status=running /
//!   终态 stop_reason 齐全，扩展 tool.ts `success()`）；
//! - background 交付：派发 result 只有 status=started，终态在父会话
//!   `subagent-completion` custom 消息里（扩展 onCompleted 落盘，5554fa4a）。
//!
//! 设计判据（2026-11-08 用户定夺，bug「重启后点阵先黑后青」）：
//! **一个子会话只有一个状态**。终态进内存时一次性折进卡片
//! （[`crate::session::messages::apply_completion_messages`]——装载路径
//! `project_rpc_messages` 与 custom 消息摄入各一处），此后卡片 spinner、
//! 导航第三行点、点开子会话读到的都是同一份 details。不存在"先灰后青"两段
//! 式渲染，也没有任何轮询：点阵是导航缓存 [`NavData`] 的一个字段，跟着
//! `notify_list` 的消息版本一起重建。
//!
//! [`NavData`]: crate::session::runtime::NavData

use std::path::{Path, PathBuf};

use gpui::{div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme::theme as T;
use crate::session::Block;

/// 导航数据面宿主标记。
///
/// 数据面曾挂在这里（`turn_agents` + 落点映射 + 1s 常驻轮询），2026-11-08
/// 全数收回：点阵进 [`crate::session::runtime::NavData`]（与 turns 同缓存
/// 同版本），终态回灌进装载/摄入路径。保留类型只为 `dlg.subagent_panel`
/// 的存在语义（自动化快照 `subagent_panel` 字段与「会话激活过」判定）。
#[derive(Clone, Default)]
pub(crate) struct SubagentPanel;

impl Chat {
    /// 设置-子代理页模块开关翻转 → 进程面生效。工具清单是 spawn 参数
    /// （RPC 无运行时切换）：
    /// - 全池标记 [`SessionRuntime::pending_subagents_reload`]——运行中的
    ///   会话由 `rebind_pending` 推迟到下一轮空闲发送；
    /// - 活跃会话已起进程且空闲：立刻重绑（同 `/reload` 的 `--session`
    ///   复接，消息不丢）——工具定义面板立即可见工具消失/回归，不用等
    ///   下一次发送；草稿（无进程）不为了翻开关凭空 spawn。
    ///
    /// 为什么必须重绑：扩展在 `spawn` 时以 `-e` 挂载（[`pi_link::subagents::
    /// mount_target`]），`--tools` allowlist 也只在那时拼——活进程上的工具
    /// 注册不会自更新。2026-11-08 真机 bug 的另一半。
    pub(crate) fn apply_subagent_switch(&mut self, cx: &mut gpui::Context<Self>) {
        for rt in self.pool.runtimes.values() {
            rt.update(cx, |r, _| r.pending_subagents_reload = true);
        }
        let rt = self.rt();
        let rebind_now = {
            let r = rt.read(cx);
            // 独立 `!` bash 也在活进程里跑；重绑会把它连带杀掉——同样推迟
            !r.observer && !r.agent_running && !r.bash_running && r.agent.session.is_some()
        };
        if rebind_now {
            rt.update(cx, |r, cx| {
                r.rebind_pending(cx);
            });
        }
        cx.notify();
    }

    /// 会话激活：数据面常备（纯内存推导，无需按会话归档）。switch_pool_rt 调。
    pub(crate) fn ensure_subagent_panel(&mut self, _cx: &mut gpui::Context<Self>) {
        if self.dlg.subagent_panel.is_none() {
            self.dlg.subagent_panel = Some(SubagentPanel);
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

/// 按轮投影第三行状态点：第 n 条 User 消息 = 第 n 轮，`agent` 调用归它
/// **所属**的那一轮（与 [`crate::session::runtime::NavData::turns`] 同口径）。
///
/// 历史上的差一错（2026-11-08 报障：第 2 轮派的 3 个子代理画在第 3 轮卡
/// 上）：旧实现把攒下的点留到**下一条** User 消息出现时才 push，于是每轮
/// 的点都落到下一轮的槽位；末轮无后继 User 消息，只能靠尾部的补 push 兜
/// 到 `len` 下标——那个下标没有卡片，单轮会话的第三行因此整行不显。
pub(crate) fn project_turn_agents(msgs: &[crate::session::messages::Msg]) -> Vec<Vec<DotTone>> {
    let mut turn_agents: Vec<Vec<DotTone>> = Vec::new();
    for m in msgs {
        match m.role {
            // 见到 User 先立本轮槽位，后续 agent 调用往它里面填
            crate::session::Role::User => turn_agents.push(Vec::new()),
            crate::session::Role::Assistant => {
                // 首条 User 之前的 assistant 不属于任何轮（会话首帧的
                // 系统提示 / 回放残片），丢弃
                let Some(slot) = turn_agents.last_mut() else {
                    continue;
                };
                for b in &m.blocks {
                    if let Block::ToolCall { name, .. } = b {
                        if name == "agent" {
                            slot.push(dot_tone_of(b));
                        }
                    }
                }
            }
            _ => {}
        }
    }
    turn_agents
}

/// 导航轮卡第三行（044-0 验收⑥①）：`[bot] ●●●●`——点数 = 该轮子代理数，
/// 点色 = 会话列表状态槽口径（[`DotTone`]，7px 同列表彩点）；bot icon =
/// MD(16)（2026-11-08 用户定夺）。
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
        // 与 agent 摘要行同构：25px 标号列对齐（bot icon 居位）
        .child(
            div()
                .w(px(25.))
                .flex_shrink_0()
                .flex()
                .justify_end()
                .child(crate::ui::icon(
                    "bot",
                    crate::ui::tokens::icon::MD,
                    t.text_dim,
                )),
        )
        .child(dots)
        .into_any_element()
}

/// 第三行状态点色（会话列表状态槽口径）。
pub(crate) mod rows;
pub(crate) use rows::DotTone;
use rows::{dot_color, dot_tone_of};

#[cfg(test)]
mod dots_tests;
