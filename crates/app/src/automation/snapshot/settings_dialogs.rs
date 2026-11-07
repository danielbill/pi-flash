//! settings/dialogs 面：设置面板六页表单态 + 弹窗/浮层清单。

use crate::Dialog;
use gpui::Context;
use serde_json::{Value, json};

use super::{Chat, opt_path, truncate};

pub(super) fn settings_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
    match &chat.dlg.settings {
        None => json!({"open": false}),
        Some(panel) => {
            let p = panel.read(cx);
            // 内存目录里的商名去重（改名后旧名残留断言用，P2-4 观测点）
            let mut mj_catalog_providers: Vec<String> = chat
                .catalog_for(chat.project())
                .iter()
                .map(|m| m.provider.clone())
                .collect();
            mj_catalog_providers.sort();
            mj_catalog_providers.dedup();
            json!({
                "open": true,
                "tab": p.tab,
                "section": p.section,
                "error": p.error,
                // 040 扩展页：op 进行中 + 当前列表（安装后刷新断言用）
                "busy": chat.cfg.pkg_op.is_some(),
                "pkg_sources": chat
                    .cfg.mc_pkgs_global
                    .iter()
                    .map(pi_link::skills::entry_source)
                    .collect::<Vec<String>>(),
                "mc_state": format!("{:?}", chat.cfg.mc_state),
                "mc_project_scope": chat.cfg.mc_project_scope,
                // 技能页：列表 + 休眠态（开关断言用）
                "skills": chat
                    .cfg
                    .mc_skills
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "scope": format!("{:?}", s.scope),
                            "disabled": s.disable_invocation,
                            "path": s.path.to_string_lossy(),
                        })
                    })
                    .collect::<Vec<_>>(),
                // 043 MCP 页：列表 + 表单字段快照（列表断言 / 保存回读用）
                "mcp_servers": chat
                    .cfg.mcp_servers
                    .iter()
                    .map(|s| {
                        json!({
                            "name": s.name,
                            "scope": format!("{:?}", s.scope),
                            "enabled": s.enabled,
                            "exposure": s.exposure,
                        })
                    })
                    .collect::<Vec<_>>(),
                "mcp_name_value": p.mcp_name.read(cx).value().to_string(),
                "mcp_add_value": p.mcp_add.read(cx).value().to_string(),
                // 051 自定义 Provider 页：表单值 / 测速-测试-填参状态 / 账本与
                // 内存目录商名（UI 测试断言用；key 只报字符数不报明文）
                "mj_providers": pi_link::models_json::read()
                    .map(|doc| {
                        pi_link::models_json::providers(&doc)
                            .into_iter()
                            .map(|(n, _)| n)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
                "mj_catalog_providers": mj_catalog_providers,
                "mj_speed": chat.cfg.mj_speed.as_ref().map(|s| format!("{s:?}")),
                "mj_test": chat.cfg.mj_test.as_ref().map(|s| format!("{s:?}")),
                "mj_fill": format!("{:?}", chat.cfg.mj_models_dev_state),
                "mj_name_value": p.mj_name.read(cx).value().to_string(),
                "mj_base_value": p.mj_base.read(cx).value().to_string(),
                "mj_key_len": p.mj_key.read(cx).value().chars().count(),
                "mj_id_value": p.mj_id.read(cx).value().to_string(),
                "mj_ctx_value": p.mj_ctx.read(cx).value().to_string(),
                "mj_max_tokens_value": p.mj_max_tokens.read(cx).value().to_string(),
                "mj_cost_in_value": p.mj_cost_in.read(cx).value().to_string(),
                "mj_cost_out_value": p.mj_cost_out.read(cx).value().to_string(),
                "mj_cost_cr_value": p.mj_cost_cr.read(cx).value().to_string(),
                "mj_cost_cw_value": p.mj_cost_cw.read(cx).value().to_string(),
                "mj_cost_source": chat
                    .cfg
                    .mj_fill_entry
                    .as_ref()
                    .map(|(_, e)| e.source.clone())
                    .unwrap_or_default(),
                "mj_image": p.mj_image,
                "mj_reasoning": p.mj_reasoning,
                "mj_delete_confirm": chat.cfg.mc_mj_delete_confirm.clone(),
            })
        }
    }
}

pub(super) fn dialogs_surface(chat: &Chat, cx: &Context<Chat>) -> Value {
    let dialog = chat.dlg.dialog.as_ref().map(|d| match d {
        Dialog::ProviderPicker { .. } => "provider_picker",
        Dialog::GitDiff { .. } => "git_diff",
        Dialog::ProjectPicker { .. } => "project_picker",
        Dialog::ImagePreview { .. } => "image_preview",
        Dialog::SessionInfo { .. } => "session_info",
        Dialog::FileDirty { .. } => "file_dirty",
    });
    // 打开项目菜单：列表数据随弹窗一起上报（扫描是异步回填，UI 测试
    // 据此轮询就绪）
    let project_hits: Vec<Value> = match chat.dlg.dialog.as_ref() {
        Some(Dialog::ProjectPicker { .. }) => chat
            .psp
            .project_hits
            .iter()
            .map(|p| json!({"name": p.name, "path": p.path.display().to_string()}))
            .collect(),
        _ => Vec::new(),
    };
    // 文件搜索命中随弹窗上报（后台扫盘异步回填，UI 测试据此轮询就绪）
    json!({
        "dialog": dialog,
        // 扩展阻塞弹窗队头 id/方法（#7 排队语义 + test/ext-ui 台架断言用）。
        // 键名不能叫 ext_dialog：下方布尔（Some=有弹窗）同名会把它覆盖，
        // 富对象一直是死字段（json! 后键覆盖前键）
        "ext_dialog_head": chat.aux.ext_dialog.as_ref().map(|r| serde_json::json!({
            "id": r.id,
            "method": match &r.method {
                pi_link::protocol::ExtUiMethod::Select { .. } => "select",
                pi_link::protocol::ExtUiMethod::Confirm { .. } => "confirm",
                pi_link::protocol::ExtUiMethod::Input { .. } => "input",
                pi_link::protocol::ExtUiMethod::Editor { .. } => "editor",
                _ => "other",
            },
            // 在屏输入框当前值（input 单行 / editor 多行，zi2s.6 台架断言用；
            // prefill 多行必须原样保留——折行/丢失即回归）
            "input": match &r.method {
                pi_link::protocol::ExtUiMethod::Input { .. } => {
                    chat.aux.ext_input.read(cx).value().to_string()
                }
                pi_link::protocol::ExtUiMethod::Editor { .. } => {
                    chat.aux.ext_editor_input.read(cx).value().to_string()
                }
                _ => String::new(),
            },
            // select 键盘导航高亮行（zi2s.7 台架断言：↓↓ 后应=2）
            "sel": if matches!(r.method, pi_link::protocol::ExtUiMethod::Select { .. }) {
                serde_json::json!(chat.aux.ext_sel_ix)
            } else {
                serde_json::json!(null)
            },
        })),
        "ext_dialog_waiting": chat.aux.ext_dialog_waiting.len(),
        // ask_user_question 问卷卡（v2 整卷卡；test/ext-ui C12 键盘模型断言
        // 出口）。卡片不在 = null
        "ask": chat.aux.ask.as_ref().map(|a| {
            json!({
                "open": a.open,
                "active": a.active,
                "n": a.questions.len(),
                "done": a.questions.iter().filter(|q| q.answered()).count(),
                "submitted": a.submitted,
            })
        }),
        // 模型菜单可见行数（UI 测试断言用：空列表 = 新会话页拉不到目录；
        // 042 起数据源 = pill 化模型菜单的 picker_models）
        "model_rows": match chat.dlg.model_picker.as_ref() {
            Some(_) => chat.picker_models(cx).0.len(),
            None => 0,
        },
        // 模型菜单 ✓ 行（与 model_picker::view 同链：pending → state → 新会话
        // 默认兜底）；菜单没开 = null。「新会话点行不出 ✓」回归断言用
        "model_current_ref": match chat.dlg.model_picker.as_ref() {
            Some(_) => {
                let r = chat.rt().read(cx);
                r.pending_model
                    .as_ref()
                    .map(|(p, id)| format!("{p}/{id}"))
                    .or_else(|| {
                        r.state
                            .as_ref()
                            .and_then(|s| s.model.as_ref())
                            .map(|m| format!("{}/{}", m.provider, m.id))
                    })
                    .or_else(|| chat.new_session_default().0.map(|(p, id)| format!("{p}/{id}")))
                    .map(Value::String)
                    .unwrap_or(Value::Null)
            }
            None => Value::Null,
        },
        "project_hits": project_hits,
        "project_filter": chat.psp.project_filter,
        // 键盘选中行（0 = 打开文件夹，1.. = 项目行；默认选中断言 + ↑/↓
        // 按键导航断言的数据化出口）
        "project_sel": chat.psp.project_sel,
        "confirm_delete": opt_path(chat.dlg.confirm_delete.as_ref()),
        // 021 文件树删除确认浮层（Some=开着，值为待删路径；file.delete /
        // file.delete_confirm 两段式 op 断言开合用）+ trash 失败持久弹层
        "tree_confirm_del": opt_path(chat.dlg.tree_confirm_del.as_ref()),
        "tree_del_error": chat.dlg.tree_del_error.clone(),
        // 删除在飞清单长度（trash 已后台化：op 断言「删除完成」可轮询归零）
        "tree_deleting": chat.dlg.tree_deleting.len(),
        // 040 扩展页卸载确认浮层（Some=开着，值为待卸载来源）
        "pkg_confirm_remove": chat.cfg.pkg_confirm_remove.clone(),
        "renaming": opt_path(chat.dlg.renaming.as_ref()),
        // 编辑区右键菜单（file.context_menu 置槽；null=关）——开合/收起
        // 断言的数据化出口（此前只能开，断言靠 shot 像素）
        "editor_menu": chat.dlg.editor_menu.as_ref().map(|(p, x, y)| json!({
            "path": p.display().to_string(),
            "x": x,
            "y": y,
        })),
        // 031 输入面板右键菜单（input.context_menu 置槽；null=关）
        "input_menu": chat.dlg.input_menu.map(|(x, y)| json!({
            "x": x,
            "y": y,
        })),
        // 消息内容区右键菜单（null=关）
        "msg_menu": chat.dlg.msg_menu.map(|(x, y)| json!({
            "x": x,
            "y": y,
        })),
        // 044-0 验收⑥ 子代理数据面（导航卡第三行点阵）——turns 只列有点的轮
        // （二级菜单已删除，2026-11-07 用户定夺）；None = 缓存未建（会话未激活过）
        "subagent_panel": chat.dlg.subagent_panel.as_ref().map(|p| {
            json!({
                "rows": p.rows.len(),
                "turns": p
                    .turn_agents
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(i, v)| {
                        json!({
                            "turn": i,
                            "count": v.len(),
                            "phases": v.iter().map(|r| r.phase.clone()).collect::<Vec<_>>(),
                        })
                    })
                    .collect::<Vec<_>>(),
            })
        }),
        "ext_dialog": chat.aux.ext_dialog.is_some(),
        "toast": chat.dlg.status_toast.as_ref().map(|(t, _)| truncate(t, 200)),
        "settings_open": chat.dlg.settings.is_some(),
    })
}
