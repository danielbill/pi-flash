//! 组件状态广播（契约 v2.4 `broadcast_events`）：宿主标准组件的状态变化，
//! 以**中立公共事件**推给所有订阅插件。宿主只管广播，不认识订阅者、不定向
//! ——零信任红线（宿主代码零插件名、零侵入组件改动点）。
//!
//! 事件集（封闭）：`inputpanel` —— 标准输入/设置面板的 `{project, model}`。
//! 发拍时机：**面板打开首拍**（面板打开/插件装载时清快照）+ **值变化拍**。
//!
//! 实现：observe Chat 自身（notify 即拍）+ **差分后才派发**——组件改动点
//! 不埋任何调用（热路径 O(1) diff，不 change 不派发）。广播处理器约定为
//! **纯数据回调**：窗口相关动作（run/edit/终端）在无窗口面不可达即丢弃，
//! Toast 照走（状态栏无需窗口）；处理完照重渲染规则刷新面板。

use serde_json::json;

use super::PluginUi;
use crate::Chat;

/// 广播事件名（封闭集，契约 broadcast_events）。
pub(crate) const INPUTPANEL: &str = "inputpanel";
/// 认领结果回推事件（new_draft 意图调用的返回通道）：负载 {key, name, cwd}。
pub(crate) const CLAIM: &str = "claim";
/// 会话标签激活/切换（契约 v3）：负载 {key}（活跃会话的 pi sessionId，与
/// fn_bar env、pi getSessionId() 同源）。页面未激活不产生。
pub(crate) const SESSION: &str = "session";

/// inputpanel 负载快照：(project, model)。
pub(crate) type Snap = (Option<String>, Option<(String, String)>);

/// 挂到 Chat 自身 observe（`spawn_plugin_scan` 期一次性接入）。
pub(crate) fn attach(cx: &mut gpui::Context<Chat>) {
    cx.observe(&cx.entity(), |this, _entity, cx| {
        tick(this, cx);
        tick_session(this, cx); // 契约 v3 session 事件（差分守门）
    })
    .detach();
}

/// 面板打开/插件（重）装载：清快照 → 下一拍必发（「激活首拍」语义）。
pub(crate) fn reset(state: &mut PluginUi) {
    state.broadcast_last = None;
}

/// 每拍：读当前负载 → 差分 → 派发给所有就绪插件。
pub(crate) fn tick(chat: &mut Chat, cx: &mut gpui::Context<Chat>) {
    let cur = current(chat, cx);
    if chat.plugins.broadcast_last.as_ref() == Some(&cur) {
        return; // 无变化不派发（差分守门，流式渲染期零开销路径）
    }
    chat.plugins.broadcast_last = Some(cur.clone());
    let payload = payload_json(&cur);
    for ix in 0..chat.plugins.plugins.len() {
        chat.plugin_broadcast_fire(ix, INPUTPANEL, &payload, cx);
    }
}

/// 契约 v3 session 事件：活跃会话键变化时派发（差分守门；空键不派——
/// 还没进任何会话）。负载 {"key": …}。
pub(crate) fn tick_session(chat: &mut Chat, cx: &mut gpui::Context<Chat>) {
    // **PF 会话键**（active_key）= 插件面统一 id，与 fn_bar env / page_actived /
    // 列表行同一把键。曾用 runtime.state.session_id（pi 的 uuid）→ 插件侧拿到
    // 的 id 与渲染面不一致，被迫引入 session_file/uuid 双口径。
    let sess = Some(chat.pool.active_key.clone()).filter(|k| !k.is_empty());
    let changed = chat.plugins.session_last.as_ref() != Some(&sess);
    if !changed {
        return;
    }
    // 先派发再记账：首拍插件 JS 可能还没装载（plugin_ready 才懒加载），
    // 先记账会导致这一拍被吞、page_id 永远不落地。
    let Some(key) = sess.clone() else {
        chat.plugins.session_last = None;
        return;
    };
    // PF 钩子 page_actived：同一时机把当前页 key 推给插件（PF 独有概念）
    let notified = chat.plugin_page_active(&key, cx);
    if notified == 0 {
        // 插件 JS 尚未装载完：不记账，下一拍重试（否则 page_id 永远不落地）
        return;
    }
    let payload = serde_json::json!({ "key": key }).to_string();
    for ix in 0..chat.plugins.plugins.len() {
        chat.plugin_broadcast_fire(ix, SESSION, &payload, cx);
    }
    chat.plugins.session_last = Some(Some(key.clone()));
}

/// 当前 inputpanel 负载（纯读取）：项目 = 桌面工作目录；模型 = 活跃会话
/// （草稿 pending 优先，在线取 pi 回报状态）。两者均可空（契约口径）。
fn current(chat: &Chat, cx: &gpui::App) -> Snap {
    let project = chat
        .ws
        .cwd
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    let model = chat
        .pool
        .runtimes
        .get(&chat.pool.active_key)
        .and_then(|rt| {
            let r = rt.read(cx);
            r.pending_model.clone().or_else(|| {
                r.state.as_ref().and_then(|s| {
                    let m = s.model.as_ref()?;
                    (!m.id.is_empty()).then(|| (m.provider.clone(), m.id.clone()))
                })
            })
        });
    (project, model)
}

/// 快照 → 负载 JSON（空串项目按未设处理）。
fn payload_json(snap: &Snap) -> String {
    let (project, model) = snap;
    json!({
        "project": project.as_deref().filter(|p| !p.is_empty()),
        "model": model.as_ref().map(|(p, m)| json!({"provider": p, "id": m})),
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_shape_project_and_model() {
        let s: Snap = (
            Some("D:/work/demo".into()),
            Some(("anthropic".into(), "claude-x".into())),
        );
        let v: serde_json::Value = serde_json::from_str(&payload_json(&s)).unwrap();
        assert_eq!(
            v,
            json!({"project":"D:/work/demo","model":{"provider":"anthropic","id":"claude-x"}})
        );
    }

    #[test]
    fn payload_shape_nullables() {
        let s: Snap = (Some(String::new()), None); // 空串项目 = 未设
        let v: serde_json::Value = serde_json::from_str(&payload_json(&s)).unwrap();
        assert_eq!(v, json!({"project":null,"model":null}));
        let s: Snap = (None, None);
        let v: serde_json::Value = serde_json::from_str(&payload_json(&s)).unwrap();
        assert_eq!(v, json!({"project":null,"model":null}));
    }

    #[test]
    fn diff_first_change_and_stable() {
        let cur: Snap = (Some("p".into()), Some(("prov".into(), "m".into())));
        // 首拍（从未发过）必发
        assert_ne!(None::<Snap>, Some(cur.clone()));
        // 同值不重发
        let last: Option<Snap> = Some(cur.clone());
        assert_eq!(last.as_ref(), Some(&cur));
        // 模型变化 → 差分成立
        let next: Snap = (Some("p".into()), None);
        assert_ne!(last.as_ref(), Some(&next));
    }
}
