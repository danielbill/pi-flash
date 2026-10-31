//! 执行域宿主侧（90-6 §5）：状态集 + 未运行排队 + outbox 泵 + 实例配置同步。
//!
//! 状态与队列住进程级单例（呈现面 post/query 线程直答，不经 Chat 借用）；
//! Chat 侧只做两件事：outbox 泵（投递呈现面 onMessage 回调 + 重渲）与
//! 实例配置同步（装/卸/禁用、会话创建后重写各会话 config）。

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

use serde_json::json;

use pi_link::service_ext::{self, SvcInstanceSpec};

#[derive(Default)]
struct SvcShared {
    /// 见过信箱事件的 token 集（区分 absent / stopped）
    seen: HashSet<String>,
    /// ready 集（shim 挂载成功后播报）
    ready: HashSet<String>,
    /// 未运行排队：token → [(plugin_id, payload_json)]
    queues: HashMap<String, Vec<(String, String)>>,
}

fn svc() -> &'static Mutex<SvcShared> {
    static SVC: OnceLock<Mutex<SvcShared>> = OnceLock::new();
    SVC.get_or_init(|| Mutex::new(SvcShared::default()))
}

/// 锚点或会话键 → token。锚点自带 token（`<token>/<id>`）；裸键查注册表（不铸）。
fn resolve_token(base: &std::path::Path, key_or_anchor: &str) -> Option<String> {
    if let Some((t, rest)) = key_or_anchor.split_once('/') {
        if !rest.is_empty() && t.starts_with("s-") {
            return Some(t.to_string());
        }
    }
    service_ext::token_if_any(base, key_or_anchor)
}

/// 呈现面 `ctx.service.post`：ready → 直投信箱；未运行 → 排队（溢出 false）。
/** wasm 载体入口（无插件 id 参数）：id 从锚点 `<token>/<pluginId>` 派生。 */
pub(crate) fn svc_post_anon(key_or_anchor: &str, payload: &str) -> bool {
    let id = key_or_anchor
        .split_once('/')
        .map(|(_, rest)| rest)
        .unwrap_or("");
    svc_post(id, key_or_anchor, payload)
}

pub(crate) fn svc_post(id: &str, key_or_anchor: &str, payload: &str) -> bool {
    let Some(base) = service_ext::base() else {
        return false;
    };
    // 插件级（全局）操作——绑定/出码刷新就是这一类：它和「当前在哪个会话」
    // 无关，目标由插件自己的状态决定。键为空/未注册时投给该插件的**任意活实例**
    // （出码态是插件全局单写者，投给谁都一样），不再因缺会话键丢失意图。
    let token = match resolve_token(&base, key_or_anchor) {
        Some(t) => Some(t),
        None => any_live_token(&base, id),
    };
    let Some(token) = token else {
        eprintln!("[pf-svc] post 无实例（{key_or_anchor}），丢弃");
        return false;
    };
    let payload_value: serde_json::Value = serde_json::from_str(payload).unwrap_or(json!(payload));
    service_ext::append_event(
        &service_ext::in_path(&base, &token),
        &json!({"id": if id.is_empty() { "svc" } else { id }, "payload": payload_value}),
    )
    .is_ok()
}

/// 该插件的任意活实例（优先 ready；其次排队中的；都没有 = 无）。
/// 用于插件级操作（出码/刷新/开关）——不绑定具体会话。
fn any_live_token(base: &std::path::Path, plugin_id: &str) -> Option<String> {
    let st = svc().lock().unwrap();
    // ready 集里挑一个（token 不区分插件，用实例配置里的 id 校对）
    let pick = st
        .ready
        .iter()
        .chain(st.seen.iter())
        .find(|t| service_ext::has_instance(base, t, plugin_id))
        .cloned();
    pick
}

/// 呈现面 `ctx.service.query`：宿主生命周期事实，插件不得自记。
pub(crate) fn svc_query(key_or_anchor: &str) -> String {
    let Some(base) = service_ext::base() else {
        return "absent".into();
    };
    let Some(token) = resolve_token(&base, key_or_anchor) else {
        return "absent".into();
    };
    let st = svc().lock().unwrap();
    if !st.seen.contains(&token) {
        return "absent".into();
    }
    if st.ready.contains(&token) {
        "running".into()
    } else {
        "stopped".into()
    }
}

pub(crate) fn svc_mark_ready(token: &str) {
    let mut st = svc().lock().unwrap();
    st.seen.insert(token.to_string());
    st.ready.insert(token.to_string());
}

pub(crate) fn svc_mark_stopped(token: &str) {
    let mut st = svc().lock().unwrap();
    st.seen.insert(token.to_string());
    st.ready.remove(token);
}

/// ready 后补投排队（保持 FIFO）。
fn svc_flush_queue(token: &str) {
    let base = match service_ext::base() {
        Some(b) => b,
        None => return,
    };
    let drained: Vec<(String, String)> = match svc().lock().unwrap().queues.remove(token) {
        Some(q) => q,
        None => return,
    };
    for (id, payload) in drained {
        let payload_value: serde_json::Value =
            serde_json::from_str(&payload).unwrap_or(json!(payload));
        let _ = service_ext::append_event(
            &service_ext::in_path(&base, token),
            &json!({"id": id, "payload": payload_value}),
        );
    }
}

/// 实例配置同步：已装且未禁用的 service 声明 → 各会话 config 重写。
/// 调用点：插件清单落位（扫描）、会话挂册（subscribe_runtime）。
pub(crate) fn svc_sync_configs(chat: &mut crate::Chat) {
    let Some(base) = service_ext::base() else {
        return;
    };
    let mut owned: Vec<(String, std::path::PathBuf, std::path::PathBuf)> = Vec::new();
    for e in chat
        .plugins
        .plugins
        .iter()
        .filter(|e| !e.disabled && !e.manifest.service.is_empty())
    {
        for sc in &e.manifest.service {
            if sc.entry.is_empty() {
                continue;
            }
            owned.push((
                e.manifest.id.clone(),
                e.manifest.dir.join(&sc.entry),
                e.manifest.dir.join("data").join("svc"),
            ));
        }
    }
    // 迁移清理（90-6）：这些入口从 v4 起是执行域面，不再是可 -e 挂载的扩展；
    // v3 时代写进 session-ext.json 的残留会让 pi 拒绝加载并连累同一批 -e
    // （含执行域 shim）——每轮同步顺手摘掉，幂等。
    for (id, _, _) in &owned {
        let _ = pi_link::session_ext::purge_exec_entries(id);
    }
    let specs: Vec<SvcInstanceSpec<'_>> = owned
        .iter()
        .map(|(id, entry, dir)| SvcInstanceSpec { id, entry, dir })
        .collect();
    for key in chat.pool.runtimes.keys() {
        if specs.is_empty() {
            if let Some(token) = service_ext::token_if_any(&base, key) {
                service_ext::remove_session_config(&base, &token);
            }
            continue;
        }
        // 有服务插件：铸造/复用 token 并写全配置（每次全量重写，幂等）
        if let Some(token) = service_ext::token_for(&base, key) {
            if let Err(e) = service_ext::write_session_config(&base, &token, &specs) {
                eprintln!("[pf-svc] 配置写入失败（{key}）: {e}");
            }
        }
    }
}

/// outbox 泵（~350ms 一拍）：ready/stopped 事件记账 + 排队补投 + 呈现面投递。
pub(crate) fn svc_pump(chat: &mut crate::Chat, cx: &mut gpui::Context<crate::Chat>) {
    let Some(base) = service_ext::base() else {
        return;
    };
    let dir = service_ext::sessions_dir(&base);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return;
    };
    for entry in rd.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(token) = name.strip_suffix("-out.jsonl") else {
            continue;
        };
        for ev in service_ext::consume_events(&entry.path()) {
            let Some(id) = ev.get("id").and_then(serde_json::Value::as_str) else {
                continue;
            };
            if let Some(kind) = ev.get("ev").and_then(serde_json::Value::as_str) {
                match kind {
                    "ready" => {
                        svc_mark_ready(token);
                        svc_flush_queue(token);
                    }
                    "stopped" => svc_mark_stopped(token),
                    _ => {}
                }
                continue;
            }
            let Some(payload) = ev.get("payload") else {
                continue;
            };
            let payload_json = serde_json::to_string(payload).unwrap_or_default();
            // 来源 meta（90-6 §4）：呈现面归档状态需要知道推送来自哪个锚点/会话
            let anchor = format!("{token}/{id}");
            let meta = json!({
                "anchor": anchor,
                "key": service_ext::key_for_token(&base, token),
            });
            let meta_json = meta.to_string();
            chat.svc_deliver(id, &payload_json, &meta_json, cx);
        }
    }
}
