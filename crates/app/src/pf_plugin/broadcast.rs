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
    let t0 = std::time::Instant::now();
    for ix in 0..chat.plugins.plugins.len() {
        chat.plugin_broadcast_fire(ix, INPUTPANEL, &payload, cx);
    }
    // [perf] 铁律：热路径实测数字（2026-11-06 卡死修复附带——面板打开首拍
    // 的全体扇出含各插件装载检查+broadcast+全量 render，流式期间这个数字
    // 就是「切面板卡不卡」的直接证据）
    if std::env::var("PI_FLASH_PERF").is_ok() {
        eprintln!(
            "[perf] broadcast tick 扇出 {} 插件耗时 {:?}",
            chat.plugins.plugins.len(),
            t0.elapsed()
        );
    }
}

/// 契约 v3 session 事件：活跃会话键变化时派发（差分守门；空键不派——
/// 还没进任何会话）。负载 {"key": …}。
pub(crate) fn tick_session(chat: &mut Chat, cx: &mut gpui::Context<Chat>) {
    // **PF 会话键**（active_key）= 插件面统一 id，与 fn_bar env / page_actived /
    // 列表行同一把键。曾用 runtime.state.session_id（pi 的 uuid）→ 插件侧拿到
    // 的 id 与渲染面不一致，被迫引入 session_file/uuid 双口径。
    let sess = Some(chat.pool.active_key.clone()).filter(|k| !k.is_empty());
    if chat.plugins.session_last.as_ref() == Some(&sess) {
        return;
    }
    let Some(key) = sess else {
        chat.plugins.session_last = None;
        return;
    };
    // 派发后即记账（B7，2026-11-06 卡死修复）：晚装插件收不到这一拍的问题
    // 改由装载补拍（plugin_catchup_page_active）兜住，不再「notified==0 不
    // 记账下一拍重试」——那会在插件装载失败/未就绪时退化成每 notify 全量
    // 扇出风暴（流式期间 116 拍/s × 全体插件 = UI 线程饱和，本 bug 主因之
    // 一）。扇出本身走就绪检查（plugin_loaded），未装载插件零成本跳过。
    // PF 钩子 page_actived：同一时机把当前页 key 推给已装载插件（PF 独有概念）
    chat.plugin_page_active(&key, cx);
    let payload = serde_json::json!({ "key": key }).to_string();
    for ix in 0..chat.plugins.plugins.len() {
        chat.plugin_broadcast_fire(ix, SESSION, &payload, cx);
    }
    chat.plugins.session_last = Some(Some(key));
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
