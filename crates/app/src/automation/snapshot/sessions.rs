//! sessions 面：会话清单/项目分组/伪标签/置顶/内存探针。

use gpui::Context;
use serde_json::{Value, json};
use std::time::UNIX_EPOCH;

use super::{Chat, cwd_json, opt_path, truncate};

pub(super) fn sessions_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
    let sessions: Vec<Value> = chat
        .pool
        .sessions
        .iter()
        .map(|s| {
            json!({
                "path": s.path.display().to_string(),
                "id": s.id,
                "cwd": s.cwd,
                "name": s.name,
                "preview": truncate(&s.preview, 120),
                "message_count": s.message_count,
                "modified_ms": s
                    .modified
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as u64)
                    .unwrap_or(0),
                "unread": chat.psp.unread.contains(&s.path),
                "pinned": chat
                    .psp
                    .session_pins
                    .iter()
                    .any(|p| crate::services::workspace::same_path(p, &s.path)),
                "loaded": chat.pool.runtimes.contains_key(&s.path.display().to_string()),
            })
        })
        .collect();
    let projects: Vec<Value> = chat
        .psp.projects
        .iter()
        .map(|g| json!({"name": g.name, "path": g.path.display().to_string(), "sessions": g.sessions.len()}))
        .collect();
    // 伪标签（draft 已发送、pi 未落盘）：验收断言「发送瞬间标签出现」用
    let pending_tabs: Vec<Value> = chat
        .psp
        .pending_tabs
        .iter()
        .map(|e| json!({"key": e.key, "preview": truncate(&e.preview, 120), "cwd": e.cwd}))
        .collect();
    // 会话置顶（020）：有序表（index 0 = 最近置顶），pin/unpin 事务断言用
    let session_pins: Vec<String> = chat
        .psp
        .session_pins
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    // 内存探针：各缓存（条数, 字节）+ 队列 + 活跃会话 transcript 规模——
    // 长跑内存回归验收用（退役/驱逐前后对照）
    let (active_msgs, active_sys_msgs) = chat
        .pool
        .runtimes
        .get(&chat.pool.active_key)
        .map(|rt| rt.read(cx).mem_probe())
        .unwrap_or((0, 0));
    let (img_n, img_bytes) = crate::session::messages::image_cache_probe();
    let (md_n, md_bytes) = crate::editor::markdown::md_parse_probe();
    let (hl_n, hl_bytes) = crate::editor::markdown::hl_cache_probe();
    let (limg_n, limg_bytes) = crate::editor::markdown::local_img_probe();
    let (args_n, args_bytes) = crate::session::messages::tool_args_cache_probe();
    let (mm_n, mm_bytes) = crate::editor::markdown::render::mermaid::mermaid_probe();
    let (math_n, math_bytes) = crate::editor::markdown::render::math::math_probe();
    let mem = json!({
        "runtimes": chat.pool.runtimes.len(),
        "active_msgs": active_msgs,
        "active_system_msgs": active_sys_msgs,
        "asset_evict_pending": crate::services::asset_evict::pending_len(),
        "img_cache": {"len": img_n, "bytes": img_bytes},
        "md_parse": {"len": md_n, "bytes": md_bytes},
        "hl_cache": {"len": hl_n, "bytes": hl_bytes},
        "local_img": {"len": limg_n, "bytes": limg_bytes},
        "tool_args": {"len": args_n, "bytes": args_bytes},
        "mermaid": {"len": mm_n, "bytes": mm_bytes},
        "math": {"len": math_n, "bytes": math_bytes},
    });
    json!({
        "cwd": cwd_json(chat),
        "active_key": chat.pool.active_key,
        "active_file": opt_path(chat.psp.active_file.as_ref()),
        "runtimes": chat.pool.runtimes.keys().cloned().collect::<Vec<_>>(),
        "sessions": sessions,
        "projects": projects,
        "pending_tabs": pending_tabs,
        "session_pins": session_pins,
        "mem": mem,
    })
}
