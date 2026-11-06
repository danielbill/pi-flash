//! app/ui/theme/lang/dialog/update 组：shell 级 op——查（app.info /
//! ui.snapshot）、生命周期（app.quit）、外观（theme/lang）、弹窗收尾、
//! 自动更新重启，以及 ui.screenshot / ui.hit_probe / ui.debug_hover 三个
//! 调试面。

use gpui::{Context, Window};
use serde_json::{Value, json};

use super::{OpResult, bad, not_found, ok};
use crate::Chat;
use crate::automation::{screenshot, snapshot};
use pi_link::automation::method;

pub(super) fn dispatch(
    chat: &mut Chat,
    window: &mut Window,
    cx: &mut Context<Chat>,
    m: &str,
    params: &Value,
) -> OpResult {
    match m {
        // ---- 查 ----
        method::APP_INFO => Ok(snapshot::app_info(chat, window, cx)),
        method::UI_SNAPSHOT => {
            let name = params.get("surface").and_then(Value::as_str);
            match snapshot::surface(chat, window, cx, name) {
                Some(mut v) => {
                    // --only 裁剪：只留指定顶层键（files 面整棵树能到几千行，
                    // 断言往往只要一两个键；CLI snapshot --only 走这里）
                    let keys: Vec<String> = match params.get("only") {
                        Some(Value::String(s)) => vec![s.clone()],
                        Some(Value::Array(a)) => a
                            .iter()
                            .filter_map(Value::as_str)
                            .map(String::from)
                            .collect(),
                        _ => vec![],
                    };
                    if !keys.is_empty() {
                        if let Value::Object(map) = &mut v {
                            map.retain(|k, _| keys.iter().any(|x| x == k));
                        }
                    }
                    Ok(v)
                }
                None => Err(bad(format!(
                    "未知 surface {:?}（可用: {:?}）",
                    name,
                    snapshot::SURFACES
                ))),
            }
        }

        // ---- app ----
        method::APP_QUIT => {
            // 与标题栏 ✕ 同序（90-9 §6）：unload → 存窗口状态 → cx.quit
            //（停 pi 随进程）。gpui Windows 的 cx.quit 不触发 on_should_close。
            crate::shutdown_for_close(chat, window);
            cx.quit();
            Ok(json!({"quitting": true}))
        }
        method::THEME_SET => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"name\": \"mist|default|dark|rose\"}"))?;
            if !crate::appearance::theme::set_by_name(name) {
                return Err(bad(format!("未知主题: {name:?}")));
            }
            crate::appearance::sync_gpui_tokens(cx);
            cx.notify();
            ok()
        }
        method::LANG_SET => {
            let ix = match params.get("ix").and_then(Value::as_u64) {
                Some(ix) => ix as usize,
                None => {
                    let name = params
                        .get("name")
                        .and_then(Value::as_str)
                        .ok_or_else(|| bad("需要 {\"ix\": 0} 或 {\"name\": \"English\"}"))?;
                    crate::i18n::LANG_LABELS
                        .iter()
                        .position(|l| *l == name)
                        .ok_or_else(|| {
                            bad(format!(
                                "未知语言: {name:?}（可用 {:?}，或 ix 0/1/2）",
                                crate::i18n::LANG_LABELS
                            ))
                        })?
                }
            };
            crate::i18n::set_lang(ix);
            cx.notify();
            ok()
        }

        // ---- 弹窗 / 更新 ----
        method::DIALOG_CLOSE => {
            chat.dlg.dialog = None;
            cx.notify();
            ok()
        }
        // 扩展阻塞弹窗点选项（#7 验收：与选项行 on_mouse_down 同链）；
        // 不带 value = 取消（ext_dialog 被 take，无请求时报「无挂起」）
        method::DIALOG_EXT_RESPOND => {
            let value = params
                .get("value")
                .and_then(Value::as_str)
                .map(str::to_string);
            let cancelled = value.is_none();
            if chat.aux.ext_dialog.is_none() {
                return Err(not_found(format!("无挂起扩展弹窗")));
            }
            chat.ext_respond(value, None, cancelled, cx);
            ok()
        }
        // 081 自动更新：「下载完成，点击重启更新」的点击（spawn 新载荷 exe 后
        // 本进程退出；观察者看进程树/新版目录落位）
        method::UPDATE_RESTART => {
            chat.restart_to_apply_update(cx);
            ok()
        }

        // ---- 调试面 ----
        method::UI_SCREENSHOT => screenshot::shot(window, params),
        // 调试专用：直置 psp 项目行 hover/收起态（绕过鼠标合成）。project
        // 缺省 = 清除 hover；collapse = 强制该项目收起(true)/展开(false)。
        // project_card = 项目行右键详情卡直开（confirm=true 直进删除确认态）。
        // 命中探针：报指定逻辑坐标处的 hit-test 链（穿透/命中类 bug 定位）
        method::UI_HIT_PROBE => {
            use gpui::px;
            let get = |k: &str| -> Result<f32, (String, String)> {
                params
                    .get(k)
                    .and_then(serde_json::Value::as_f64)
                    .map(|v| v as f32)
                    .ok_or_else(|| bad(format!("需要 {{\"{k}\": number}}")))
            };
            let (x, y) = (get("x")?, get("y")?);
            let chain = window.debug_hit_test_chain(gpui::Point::new(px(x), px(y)));
            let items: Vec<serde_json::Value> = chain
                .iter()
                .map(|(id, bounds, inter, behavior, hovered)| {
                    serde_json::json!({
                        "id": format!("{:?}", id),
                        "bounds": {
                            "x": f32::from(bounds.origin.x), "y": f32::from(bounds.origin.y),
                            "w": f32::from(bounds.size.width), "h": f32::from(bounds.size.height),
                        },
                        "inter": {
                            "x": f32::from(inter.origin.x), "y": f32::from(inter.origin.y),
                            "w": f32::from(inter.size.width), "h": f32::from(inter.size.height),
                        },
                        "behavior": format!("{:?}", behavior),
                        "hovered": hovered,
                    })
                })
                .collect();
            let hover_count = items
                .iter()
                .filter(|it| it["hovered"] == serde_json::json!(true))
                .count();
            Ok(serde_json::json!({ "x": x, "y": y, "hover_count": hover_count, "chain": items }))
        }
        method::UI_DEBUG_HOVER => {
            // 会话详情卡直开（自动化复现右键详情卡路径；rename=true 直进
            // 改名态——与标题点击同一 Chat::hover_card_begin_rename 槽）。
            // 锚点取固定调试坐标（真路径 = 右键点视口坐标）。
            if let Some(sp) = params.get("session").and_then(Value::as_str) {
                let path = std::path::PathBuf::from(sp);
                let found = chat.psp.projects.iter().any(|g| {
                    g.sessions
                        .iter()
                        .any(|s| crate::services::workspace::same_path(&s.path, &path))
                });
                if !found {
                    return Err(not_found(format!("psp 列表没有该会话: {sp}")));
                }
                chat.dlg.hover_card = Some(crate::function_panel::HoverCard {
                    path: path.clone(),
                    x: 300.,
                    y: 200.,
                    confirming: false,
                    renaming: false,
                    rename_input: None,
                });
                if params.get("rename").and_then(Value::as_bool) == Some(true) {
                    chat.hover_card_begin_rename(cx);
                }
                cx.notify();
                return Ok(json!({
                    "hovered_session": sp,
                    "renaming": chat
                        .dlg.hover_card
                        .as_ref()
                        .is_some_and(|c| c.renaming),
                }));
            }
            // 项目行右键详情卡直开（自动化复现右键路径；confirm=true 直进
            // 删除确认态）。锚点取固定调试坐标（真路径 = 右键点视口坐标）。
            if let Some(sp) = params.get("project_card").and_then(Value::as_str) {
                let path = std::path::PathBuf::from(sp);
                let found = chat
                    .psp
                    .projects
                    .iter()
                    .any(|g| crate::services::workspace::same_path(&g.path, &path));
                if !found {
                    return Err(not_found(format!("psp 列表没有该项目: {sp}")));
                }
                chat.dlg.proj_card = Some(crate::function_panel::ProjCard {
                    path: path.clone(),
                    x: 300.,
                    y: 200.,
                    confirming: params.get("confirm").and_then(Value::as_bool) == Some(true),
                });
                cx.notify();
                return Ok(json!({
                    "project_card": sp,
                    "confirming": chat
                        .dlg.proj_card
                        .as_ref()
                        .is_some_and(|c| c.confirming),
                }));
            }
            match params.get("project") {
                Some(Value::Number(n)) => {
                    let pi = n.as_u64().ok_or_else(|| bad("project 必须是非负整数"))? as usize;
                    if pi >= chat.psp.projects.len() {
                        return Err(bad(format!(
                            "project {pi} 越界（共 {} 个）",
                            chat.psp.projects.len()
                        )));
                    }
                    chat.psp.hovered_project = Some(pi);
                    let key = crate::services::workspace::same_ws_key(
                        &chat.psp.projects[pi].path.to_string_lossy(),
                    );
                    match params.get("collapse") {
                        Some(Value::Bool(true)) => {
                            chat.psp.collapsed_keys.insert(key);
                        }
                        Some(Value::Bool(false)) => {
                            chat.psp.collapsed_keys.remove(&key);
                        }
                        _ => {}
                    }
                }
                _ => chat.psp.hovered_project = None,
            }
            cx.notify();
            Ok(json!({
                "hovered": chat.psp.hovered_project,
                "collapsed": chat
                    .psp.projects
                    .iter()
                    .enumerate()
                    .map(|(pi, g)| {
                        (
                            format!("p{pi}"),
                            json!(chat.psp.collapsed_keys.contains(&crate::services::workspace::same_ws_key(
                                &g.path.to_string_lossy(),
                            ))),
                        )
                    })
                    .collect::<serde_json::Map<String, Value>>(),
            }))
        }

        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}
