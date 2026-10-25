//! op 分发：method → 直调 Chat 方法（app 是方法调用驱动的 shell，这是
//! 最贴近真实用户操作、也最稳的执行路径）。改状态的 op 一律以 cx.notify()
//! 收尾（GPUI 铁律：状态变 UI 动）。

use super::snapshot;
use crate::{Chat, ContentView, DockPanel};
use gpui::{Context, Focusable, Keystroke, Window};
use pi_link::automation::method;
use serde_json::{Value, json};
use std::path::PathBuf;

type OpResult = Result<Value, (String, String)>;

fn bad(msg: impl Into<String>) -> (String, String) {
    ("bad_params".into(), msg.into())
}

fn not_found(msg: impl Into<String>) -> (String, String) {
    ("not_found".into(), msg.into())
}

fn ok() -> OpResult {
    Ok(json!({"ok": true}))
}

fn params_path(params: &Value, key: &str) -> Result<PathBuf, (String, String)> {
    params
        .get(key)
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| bad(format!("需要 {{\"{key}\": \"路径\"}}")))
}

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
        method::SEARCH_OPEN => {
            chat.open_project_search(cx);
            Ok(snapshot::search_surface(chat))
        }
        method::SEARCH_QUERY => {
            let text = params
                .get("text")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            chat.set_search_query(text, cx);
            Ok(snapshot::search_surface(chat))
        }
        method::SEARCH_TOGGLE => {
            let which = params
                .get("which")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string();
            chat.search_toggle_by_name(&which, cx);
            Ok(snapshot::search_surface(chat))
        }
        method::SEARCH_STEP => {
            let forward = params
                .get("forward")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            chat.search_step(forward, cx);
            Ok(snapshot::search_surface(chat))
        }
        method::SEARCH_REPLACE_ALL => {
            chat.search_replace_all(cx);
            Ok(snapshot::search_surface(chat))
        }
        method::INPUT_KEYS => {
            let keys = params
                .get("keys")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"keys\": \"ctrl-s\"}"))?;
            let ks =
                Keystroke::parse(keys).map_err(|e| ("bad_params".to_string(), e.to_string()))?;
            // keystroke 会进真实 UI 事件链（输入框/弹层回调里 weak.update
            // Chat），而本 op 自身已包在 chat.aux.update 里——同步派发必触发
            // entity_map 重入断言（entity_map.rs "already being updated"，
            // 004 项目菜单弹窗的 ESC 首次暴露）。推迟到本效果周期尾（逃出
            // chat.aux.update）再派发；真实用户按键本就不在 chat.aux.update 里，
            // 此改动只是让自动化路径与之一致。dispatched 无法同步取得，
            // 改报 deferred（事务范式照旧：exec → wait/snapshot 断言效果）。
            window.defer(cx, move |window, cx| {
                window.dispatch_keystroke(ks, cx);
            });
            Ok(json!({"deferred": true}))
        }
        method::INPUT_MOUSE => {
            // 走真实 gpui 事件链的鼠标注入：hover/on_click/起选守卫全按
            // 线上 hit_test 语义响应，是穿透/命中类 bug 的唯一可信模拟。
            // 与 INPUT_KEYS 同因 defer：本 op 包在 chat.aux.update 里，
            // 同步派发必触 entity_map 重入断言。
            use gpui::{
                MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PlatformInput, Point, px,
            };
            let action = params
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("click")
                .to_string();
            let size = window.viewport_size();
            let coord = |abs: &str, rel: &str, total: f32, dflt: f32| -> f32 {
                if let Some(v) = params.get(abs).and_then(Value::as_f64) {
                    v as f32
                } else if let Some(v) = params.get(rel).and_then(Value::as_f64) {
                    (v as f32).clamp(0., 1.) * total
                } else {
                    dflt
                }
            };
            let position = Point::new(
                px(coord(
                    "x",
                    "rx",
                    f32::from(size.width),
                    f32::from(size.width) / 2.,
                )),
                px(coord(
                    "y",
                    "ry",
                    f32::from(size.height),
                    f32::from(size.height) / 2.,
                )),
            );
            if !matches!(action.as_str(), "click" | "down" | "up" | "move" | "rclick") {
                return Err(bad(format!(
                    "未知 action: {action:?}（可用 click|down|up|move|rclick）"
                )));
            }
            let button = match params.get("button").and_then(Value::as_str) {
                Some("right") => MouseButton::Right,
                _ => MouseButton::Left,
            };
            let count = params.get("count").and_then(Value::as_u64).unwrap_or(1) as usize;
            window.defer(cx, move |window, cx| {
                let send = |window: &mut Window, cx: &mut gpui::App, down: bool| {
                    let ev = if down {
                        PlatformInput::MouseDown(MouseDownEvent {
                            button,
                            position,
                            click_count: count,
                            first_mouse: false,
                            ..Default::default()
                        })
                    } else {
                        PlatformInput::MouseUp(MouseUpEvent {
                            button,
                            position,
                            click_count: count,
                            ..Default::default()
                        })
                    };
                    window.dispatch_mouse_input(ev, cx);
                };
                match action.as_str() {
                    "move" => {
                        window.dispatch_mouse_input(
                            PlatformInput::MouseMove(MouseMoveEvent {
                                position,
                                ..Default::default()
                            }),
                            cx,
                        );
                    }
                    "down" => send(window, cx, true),
                    "up" => send(window, cx, false),
                    // rclick = 右键单击（开上下文菜单类）
                    "rclick" | "click" => {
                        send(window, cx, true);
                        send(window, cx, false);
                    }
                    _ => unreachable!(),
                }
            });
            Ok(
                json!({"deferred": true, "position": {"x": f32::from(position.x), "y": f32::from(position.y)}}),
            )
        }
        method::INPUT_FOCUS => {
            // 元素级 focus（非坐标点击）：焦点/键位类 bug 的自动化入口。
            // window.focus 只设焦点 id + refresh，无同步回调，chat.aux.update
            // 内直调安全（与 dispatch_keystroke 的 defer 不同因）。
            let target = params
                .get("target")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    bad("需要 {\"target\": \"composer|chat|git_commit|terminal|editor\"}")
                })?;
            let handle = match target {
                "composer" => chat
                    .composer
                    .composer
                    .as_ref()
                    .map(|c| c.read(cx).focus_handle_in(cx))
                    .unwrap_or_else(|| chat.layout.focus.clone()),
                "chat" => chat.layout.focus.clone(),
                "git_commit" => chat.ws.git_commit_input.read(cx).focus_handle(),
                "terminal" => {
                    // 优先给**当前显示**的终端（渲染面 = active_panel_tab），
                    // 没显示终端时退到 active_terminal 记录的那个
                    chat.shown_terminal_focus()
                        .or_else(|| {
                            let ix = chat.ws.active_terminal.unwrap_or(0);
                            chat.ws.terminals.get(ix).map(|t| t.focus.clone())
                        })
                        .ok_or_else(|| not_found("没有打开的终端"))?
                }
                // 文件编辑器：Ctrl+F 搜索替换条这类「焦点在编辑器才生效」的
                // 键位/弹层断言入口（file.open 后焦点仍可能停在 composer）
                "editor" => {
                    let path = chat
                        .active_file_path()
                        .ok_or_else(|| not_found("没有打开的文件（先 file.open）"))?;
                    let ed = chat
                        .ws
                        .file_cache
                        .get(&path)
                        .and_then(|f| f.editor.clone())
                        .ok_or_else(|| not_found("文件没有编辑器"))?;
                    ed.read(cx).focus_handle(cx)
                }
                other => {
                    return Err(bad(format!(
                        "未知 focus 目标: {other:?}（composer|chat|git_commit|terminal|editor）"
                    )));
                }
            };
            window.focus(&handle);
            cx.notify();
            Ok(json!({"ok": true, "target": target}))
        }
        method::TERMINAL_OPEN => {
            // 打开（或聚焦）工作区终端；键入回显断言读 term 快照面
            chat.open_terminal(None, window, cx);
            Ok(json!({
                "ok": true,
                "terminals": chat.ws.terminals.len(),
                "active_terminal": chat.ws.active_terminal,
                "content_view": match chat.layout.content_view {
                    ContentView::Chat => "chat",
                    ContentView::Term => "term",
                    ContentView::File => "file",
                    ContentView::Changelog => "changelog",
                    ContentView::PiExtMarket => "pi-ext-market",
                    ContentView::PiNativeMarket => "pi-native-market",
                    ContentView::PfMarket => "pf-market",
                    ContentView::Search => "search",
                    ContentView::SessionSearch => "session_search",
                    ContentView::Wx => "wx",
                },
            }))
        }

        // ---- 会话 ----
        method::SESSION_NEW => {
            chat.new_session(cx);
            let key = chat.pool.active_key.clone();
            Ok(json!({"ok": true, "key": key}))
        }
        method::SESSION_OPEN => {
            let path = params_path(params, "path")?;
            if !path.is_file() {
                return Err(not_found(format!("会话文件不存在: {}", path.display())));
            }
            chat.open_session(path.clone(), false, cx);
            Ok(json!({"ok": true, "key": path.display().to_string()}))
        }
        method::SESSION_SWITCH => {
            let key = params
                .get("key")
                .or_else(|| params.get("path"))
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"key\": \"…\"} 或 {\"path\": \"…\"}"))?
                .to_string();
            let rt = chat.pool.runtimes.get(&key).cloned().ok_or_else(|| {
                not_found(format!(
                    "池里没有 key={key:?} 的会话（runtimes 见 ui.snapshot sessions 面）"
                ))
            })?;
            chat.switch_pool_rt(rt, cx);
            ok()
        }
        method::SESSION_DELETE => {
            let path = params_path(params, "path")?;
            chat.delete_session(path, cx);
            ok()
        }
        method::SESSION_PIN => {
            let path = params_path(params, "path")?;
            // **幂等**：缺省 = 置顶（pinned: true），显式 false 才取消。
            // 早期缺省是 toggle，脚本/测试连着调两次会互相撤销（e2e 实测
            // 三条 pin 只剩最后一条）——自动化 op 必须可重复调用。
            // UI 的按钮语义仍是 toggle（那是点击语义，与本 op 无关）。
            let want = params
                .get("pinned")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let now = chat.session_pinned(&path);
            if want != now {
                chat.session_pin_toggle(path, cx);
            }
            Ok(json!({"pinned": want}))
        }
        method::SESSION_SEND => {
            if let Some(t) = params.get("text").and_then(Value::as_str) {
                chat.set_input(t.to_string(), cx);
            }
            chat.send_input(cx);
            ok()
        }
        method::SESSION_STEER => {
            if let Some(t) = params.get("text").and_then(Value::as_str) {
                chat.set_input(t.to_string(), cx);
            }
            chat.steer_input(cx);
            ok()
        }
        method::SESSION_FOLLOWUP => {
            if let Some(t) = params.get("text").and_then(Value::as_str) {
                chat.set_input(t.to_string(), cx);
            }
            chat.follow_up_input(cx);
            ok()
        }
        method::SESSION_ABORT => {
            chat.abort_stream(cx);
            ok()
        }

        // ---- 输入与面板 ----
        method::COMPOSER_SET_TEXT => {
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            chat.set_input(text.to_string(), cx);
            ok()
        }
        method::PANEL_DOCK => {
            let panel = params
                .get("panel")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"panel\": \"default|git|plugin|wx\"}"))?;
            // "wx" 已非 dock 面板（2026-10-23 迁内容区全屏页）：重定向到
            // 开页，老脚本不破。
            if panel == "wx" {
                chat.open_wx(cx);
                return ok();
            }
            let plugin_ix = params.get("ix").and_then(Value::as_u64).unwrap_or(0) as usize;
            if panel == "plugin" {
                // 走 open_plugin_dock（与点状态栏 tab 同语义）：触发插件懒加载链
                // （建 ctx/装 wasm → onPanelOpen → render 缓存）；直接赋值
                // dock_panel 会绕过它，插件面板永远停在「…」占位。
                if chat.plugins.plugins.get(plugin_ix).is_none_or(|e| e.disabled) {
                    return Err(bad(format!("插件 ix={plugin_ix} 不存在或已停用")));
                }
                chat.open_plugin_dock(plugin_ix, cx);
                return ok();
            }
            chat.layout.dock_panel = DockPanel::from_dock_arg(panel, plugin_ix)
                .ok_or_else(|| bad(format!("未知 dock 面板: {panel:?}")))?;
            cx.notify();
            ok()
        }
        method::PLUGIN_MENU => {
            // 文件树右键贡献项回调（与右键菜单点击同一条路：plugin_menu_fire）
            let ix = params.get("ix").and_then(Value::as_u64).unwrap_or(0) as usize;
            let id = params
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"id\": \"set-quick\"}"))?;
            let path = params_path(params, "path")?;
            chat.plugin_menu_fire(ix, id, &path, window, cx);
            ok()
        }
        method::PLUGIN_DOCK => {
            let ix = params.get("ix").and_then(Value::as_u64).unwrap_or(0) as usize;
            if chat.plugins.plugins.get(ix).is_none_or(|e| e.disabled) {
                return Err(bad(format!("插件 ix={ix} 不存在或已停用")));
            }
            chat.open_plugin_dock(ix, cx);
            ok()
        }
        method::CONTENT_VIEW => {
            let view = params
                .get("view")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"view\": \"chat|term|file\"}"))?;
            match view {
                "chat" => chat.set_content_view(ContentView::Chat),
                "term" => chat.set_content_view(ContentView::Term),
                "file" => chat.set_content_view(ContentView::File),
                // 040-1：市场页走真开页路径（建搜索框 + ensure_loaded 拉首页）
                "pi-ext-market" => chat.open_pi_ext_market(cx),
                "pi-native-market" => chat.open_pi_native_market(cx),
                // 090-4：PF 插件市场同口径（建输入框 + 重读台账）
                "pf-market" => chat.open_pf_market(cx),
                // 060：微信通信全屏页（三横菜单唯一 UI 入口）
                "wx" => chat.open_wx(cx),
                "search" => chat.open_project_search(cx),
                other => return Err(bad(format!("未知内容视图: {other:?}"))),
            }
            cx.notify();
            ok()
        }
        method::FILE_OPEN => {
            let path = params_path(params, "path")?;
            if !path.is_file() {
                return Err(not_found(format!("文件不存在: {}", path.display())));
            }
            chat.open_file_tab(path, cx);
            ok()
        }
        // 023 fileView：保存 / 关闭。close 对脏缓冲会转成确认弹窗——事务
        // 范式里用 ui.snapshot 的 dialogs surface + file.close 收尾。
        method::FILE_SAVE => {
            let path = params_path(params, "path")?;
            chat.save_file(&path, cx);
            ok()
        }
        method::FILE_CLOSE => {
            let path = params_path(params, "path")?;
            if !chat
                .ws.panel_tabs
                .iter()
                .any(|t| matches!(t, crate::PanelTab::File(p) if crate::services::workspace::same_path(p, &path)))
            {
                return Err(not_found(format!("文件 tab 未打开: {}", path.display())));
            }
            chat.close_file_tab(&path, cx);
            ok()
        }
        method::FILE_SET_TEXT => {
            let path = params_path(params, "path")?;
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"text\": \"内容\"}"))?
                .to_string();
            let ed = chat
                .ws
                .file_cache
                .get(&path)
                .and_then(|f| f.editor.clone())
                .ok_or_else(|| not_found("文件没有编辑器（未渲染）"))?;
            ed.update(cx, |st, scx| st.set_value(text, window, scx));
            // set_value 发 Change → dirty 按「编辑器值 != 磁盘真值」比较落位
            cx.notify();
            ok()
        }
        // 冲突横幅「重新加载」：磁盘为准（丢弃本地未保存修改）——与 banner
        // 按钮同一方法，让冲突裁决路径可被自动化断言
        method::FILE_RELOAD => {
            let path = params_path(params, "path")?;
            if !chat.ws.file_cache.contains_key(&path) {
                return Err(not_found(format!("文件 tab 未打开: {}", path.display())));
            }
            chat.file_reload_from_disk(&path, cx);
            ok()
        }
        // 编辑区右键菜单（自动化验证入口：状态与真右键同槽，配合 shot
        // 断言菜单渲染/收起；x/y 缺省给视口中上部）
        method::FILE_CONTEXT_MENU => {
            let path = params_path(params, "path")?;
            if !chat.ws.file_cache.contains_key(&path) {
                return Err(not_found(format!("文件 tab 未打开: {}", path.display())));
            }
            let x = params.get("x").and_then(Value::as_f64).unwrap_or(600.) as f32;
            let y = params.get("y").and_then(Value::as_f64).unwrap_or(160.) as f32;
            chat.dlg.editor_menu = Some((path, x, y));
            cx.notify();
            ok()
        }
        // 编辑区右键菜单收起（file.context_menu 的对向 op，与点菜单项外
        // 区域同效——清槽）。幂等：无菜单开着也 ok，回 had_menu 供断言
        method::FILE_CONTEXT_MENU_CLOSE => {
            let had_menu = chat.dlg.editor_menu.is_some();
            chat.dlg.editor_menu = None;
            cx.notify();
            Ok(json!({"ok": true, "had_menu": had_menu}))
        }
        // 031 输入面板右键菜单（与真右键同槽；x/y 缺省给视口中下部——
        // 胶囊常驻底部）。压缩锁期间真右键不弹，这里同样守卫（错误回包
        // 供断言「锁住不弹」）
        method::INPUT_CONTEXT_MENU => {
            if chat.rt().read(cx).compacting {
                return Err(bad("上下文压缩中，输入面板锁定"));
            }
            let x = params.get("x").and_then(Value::as_f64).unwrap_or(600.) as f32;
            let y = params.get("y").and_then(Value::as_f64).unwrap_or(600.) as f32;
            chat.dlg.input_menu = Some((x, y));
            cx.notify();
            ok()
        }
        // 输入面板右键菜单收起（input.context_menu 的对向 op，与点菜单项
        // 外区域同效——清槽）。幂等：无菜单开着也 ok，回 had_menu 供断言
        method::INPUT_CONTEXT_MENU_CLOSE => {
            let had_menu = chat.dlg.input_menu.is_some();
            chat.dlg.input_menu = None;
            cx.notify();
            Ok(json!({"ok": true, "had_menu": had_menu}))
        }
        method::FILE_VIEW_MODE => {
            let path = params_path(params, "path")?;
            let mode = params
                .get("mode")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"mode\": \"source|preview\"}"))?;
            let source = match mode {
                "source" => true,
                "preview" => false,
                other => return Err(bad(format!("未知 mode: {other}"))),
            };
            match chat.ws.file_cache.get_mut(&path) {
                Some(ft) => {
                    // 双态类型才有视图模式可切（仅 md）：其余静默成功（op
                    // 语义同前——不报错，只是没有可切换的态）
                    if crate::editor::kind::kind_of(&path).supports_view_mode() {
                        ft.md_source = source;
                    }
                }
                None => return Err(not_found(format!("文件 tab 未打开: {}", path.display()))),
            }
            cx.notify();
            ok()
        }
        // 文件树删除流（自动化验证入口）：与右键菜单「删除」同槽同方法——
        // 弹确认浮层（真删在 file.delete_confirm 应答后）
        method::FILE_DELETE => {
            let path = params_path(params, "path")?;
            if !path.exists() {
                return Err(not_found(format!("文件不存在: {}", path.display())));
            }
            chat.confirm_tree_delete(path, cx);
            ok()
        }
        // 删除确认浮层的「确认」（与确认按钮同一方法）：清槽 + 投递后台
        // trash 真删（大目录回收站搬运可达分钟级，不再阻塞主线程；完成
        // 后由 watcher/git 刷新回灌树）
        method::FILE_DELETE_CONFIRM => {
            if chat.dlg.tree_confirm_del.is_none() {
                return Err(bad("没有待确认的删除（tree_confirm_del 为空）"));
            }
            chat.tree_delete_confirm_commit(cx);
            ok()
        }
        method::FILES_TOGGLE_DIR => {
            let path = params_path(params, "path")?;
            if !chat.ws.expanded_dirs.remove(&path) {
                chat.ws.expanded_dirs.insert(path);
            }
            chat.rebuild_tree();
            cx.notify();
            ok()
        }
        // 021 行内改名：打开编辑行（可选 name 预填新名）。提交/取消走
        // input.keys（enter/escape）或 input.focus（失焦收场），断言读
        // files 面快照的 tree_edit 字段
        method::FILES_RENAME => {
            let path = params_path(params, "path")?;
            if !path.exists() {
                return Err(not_found(format!("条目不存在: {}", path.display())));
            }
            let is_dir = path.is_dir();
            chat.open_file_rename(path, is_dir, false, cx);
            tree_edit_prefill(chat, params, cx);
            ok()
        }
        // 021 行内新建：在 dir 下开编辑行（is_dir = 建目录；尾随 `/` 名字
        // 同样会升级为目录）。文件/目录菜单的行内语义直通。
        method::FILES_NEW_ENTRY => {
            let dir = params_path(params, "dir")?;
            if !dir.is_dir() {
                return Err(not_found(format!("目录不存在: {}", dir.display())));
            }
            let is_dir = params
                .get("is_dir")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            chat.open_file_new(dir, is_dir, cx);
            tree_edit_prefill(chat, params, cx);
            ok()
        }
        method::PROJECT_SWITCH => {
            let cwd = params_path(params, "cwd")?;
            if !cwd.is_dir() {
                return Err(not_found(format!("目录不存在: {}", cwd.display())));
            }
            chat.switch_project(cwd, cx);
            ok()
        }

        method::PROJECT_NEW_SESSION_IN => {
            let cwd = params_path(params, "cwd")?;
            if !cwd.is_dir() {
                return Err(not_found(format!("目录不存在: {}", cwd.display())));
            }
            chat.new_session_in(cwd, cx);
            ok()
        }

        // ---- git ----
        method::GIT_STAGE => {
            let r = git_stage_op(chat, params, true);
            chat.ws.git_error = r.err();
            chat.refresh_git(cx);
            cx.notify();
            ok()
        }
        method::GIT_UNSTAGE => {
            let r = git_stage_op(chat, params, false);
            chat.ws.git_error = r.err();
            chat.refresh_git(cx);
            cx.notify();
            ok()
        }
        method::GIT_COMMIT => {
            let msg = params
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or_default();
            chat.ws
                .git_commit_input
                .update(cx, |ti, cx| ti.set_value(msg.to_string(), cx));
            chat.git_commit_staged(cx);
            Ok(json!({"ok": chat.ws.git_error.is_none(), "error": chat.ws.git_error}))
        }
        method::GIT_PUSH => {
            chat.git_push_branch(cx);
            Ok(json!({"ok": true, "note": "推送异步进行，git 面看 error 字段"}))
        }
        method::GIT_REFRESH => {
            chat.refresh_git(cx);
            cx.notify();
            ok()
        }
        method::GIT_SET_TAB => {
            let tab = params
                .get("tab")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"tab\": \"changes|history\"}"))?;
            chat.git_set_tab(
                match tab {
                    "changes" => crate::function_panel::git_panel::GitTab::Changes,
                    "history" => crate::function_panel::git_panel::GitTab::History,
                    other => return Err(bad(format!("未知 git 页: {other:?}"))),
                },
                cx,
            );
            ok()
        }

        // ---- 设置/弹窗 ----
        method::SETTINGS_OPEN => {
            let tab = params.get("tab").and_then(Value::as_u64).unwrap_or(0) as u8;
            chat.open_settings(tab, cx);
            ok()
        }
        method::SETTINGS_CLOSE => {
            chat.dlg.settings = None;
            cx.notify();
            ok()
        }
        // 040 扩展页安装直调：走与安装按钮完全相同的 mc_install_package 链路
        method::SETTINGS_INSTALL_EXT => {
            let source = params
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"source\": \"npm:… 或整条 pi install …\"}"))?
                .to_string();
            chat.mc_install_package(source, cx);
            ok()
        }
        // 040 卸载确认流：先弹居中确认浮层，再应答确认
        method::SETTINGS_REMOVE_EXT => {
            let source = params
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"source\": \"npm:…\"}"))?
                .to_string();
            chat.mc_ask_remove_package(source, cx);
            ok()
        }
        method::SETTINGS_PKG_REMOVE_CONFIRM => {
            let confirmed = params.get("ok").and_then(Value::as_bool).unwrap_or(false);
            chat.mc_remove_dialog_close(confirmed, cx);
            ok()
        }
        method::WX_QR_OPEN => {
            // 扫码弹窗已废（2026-10-23：码直接绘在【微信通信】页内）——
            // 动作改道：开页 + 幂等起扫码，老脚本语义不变。
            chat.aux.remote.begin_qr();
            chat.open_wx(cx);
            ok()
        }
        method::WX_ENABLE => {
            // 开/关微信渠道 = 开/关【微信通话】标签（002：标签开 = on，
            // 关 = off）。缺省 on=true；返回渠道是否真的起来了（无 token
            // 起不来，调用方看 running 字段）。
            let on = params.get("on").and_then(Value::as_bool).unwrap_or(true);
            if on {
                chat.open_wx(cx);
            } else {
                chat.close_wx_tab(cx);
            }
            Ok(json!({ "ok": true, "running": chat.aux.remote.is_running() }))
        }
        method::WX_NEW_SESSION => {
            // 002 §6 [新建会话]：confirm=false = 按钮同路径（运行守卫 + 弹
            // 确认框）；confirm=true = 确认按钮同路径（清 wx_cwd + wx_session
            // 引用 → 设置会话页）。
            let confirm = params.get("confirm").and_then(Value::as_bool).unwrap_or(false);
            let dialog_opened = if confirm {
                chat.wx_new_session_reset(cx);
                false
            } else {
                if chat.aux.remote.active_running {
                    let lang = wxprobe::format::messages::Lang::from_ix(crate::i18n::lang_ix());
                    chat.aux
                        .remote
                        .send(&wxprobe::format::messages::t(
                            lang,
                            crate::remote_control::TASK_RUNNING,
                        ));
                    false
                } else {
                    chat.dlg.dialog = Some(crate::Dialog::WxNewSession);
                    true
                }
            };
            cx.notify();
            Ok(json!({
                "ok": true,
                "dialog_opened": dialog_opened,
                "active_key": chat.pool.active_key,
            }))
        }
        method::DIALOG_CLOSE => {
            chat.dlg.dialog = None;
            cx.notify();
            ok()
        }
        // 081 自动更新：「下载完成，点击重启更新」的点击（spawn 新载荷 exe 后
        // 本进程退出；观察者看进程树/新版目录落位）
        method::UPDATE_RESTART => {
            chat.restart_to_apply_update(cx);
            ok()
        }
        method::PROJECT_PICKER_OPEN => {
            let fresh = params
                .get("fresh")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            chat.open_project_picker(fresh, cx);
            ok()
        }
        method::MODEL_PICKER_OPEN => {
            // 042：模型菜单纯 pill 化后的无坐标入口（锚点走右下兜底）；
            // 列表内容本身来自 Chat 的共享目录，不依赖本会话进程
            chat.open_model_picker(cx);
            ok()
        }
        method::FILES_PROJECT_MENU => {
            // 文件树标题项目下拉的无坐标入口（model.picker_open 同款）：
            // toggle 开/收；↑/↓/Enter 走 input.keys，状态读 files 面 proj_menu
            chat.toggle_tree_proj_menu(cx);
            ok()
        }
        method::FILES_PIN_VIEW => {
            // 021 Pin 收藏夹视图的无坐标入口：toggle 树体区域视图开关；
            // 开合读 files 面 pin_view，视图行读 rows
            chat.toggle_pin_view(cx);
            ok()
        }
        method::FILES_PIN => {
            // 与右键菜单「Pin/取消固定」同一方法（toggle）；清单读 files 面 pins
            let path = params_path(params, "path")?;
            if !path.exists() {
                return Err(not_found(format!("条目不存在: {}", path.display())));
            }
            chat.tree_pin_toggle(path, cx);
            ok()
        }
        method::SESSION_TOOLS_PRESET => {
            let preset = params
                .get("preset")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    bad("需要 {\"preset\": \"full|custom|default|read-only|chat-only\"}")
                })?;
            chat.mc_set_tools_preset(preset, cx);
            ok()
        }
        method::PI_EXTENSION_PICKER_OPEN => {
            chat.open_pi_extension_picker(cx);
            ok()
        }
        method::PI_EXTENSION_PICKER_TOGGLE => {
            let src = params
                .get("source")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"source\": \"npm:…\"}"))?;
            chat.pi_extension_picker_toggle(src, cx);
            ok()
        }
        method::PI_EXTENSION_PICKER_CANCEL => {
            chat.pi_extension_picker_cancel(cx);
            ok()
        }
        method::MCP_PICKER_OPEN => {
            chat.open_mcp_picker(cx);
            ok()
        }
        method::MCP_PICKER_TOGGLE => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"name\": \"…\"}"))?;
            chat.mcp_picker_toggle(name, cx);
            ok()
        }
        method::MCP_PICKER_CANCEL => {
            chat.dlg.mcp_picker = None;
            cx.notify();
            ok()
        }
        method::SETTINGS_MCP_SAVE => {
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("需要 {\"text\": \"粘贴内容\"}"))?
                .to_string();
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            if let Some(st) = chat.dlg.settings.clone() {
                st.update(cx, |s, cx| {
                    s.mcp_add.update(cx, |ti, cx| ti.set_value(text, cx));
                    s.mcp_name.update(cx, |ti, cx| ti.set_value(name, cx));
                    cx.notify();
                });
            }
            chat.mcp_save_submit(cx);
            ok()
        }
        method::UI_SCREENSHOT => super::screenshot::shot(window, params),
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

/// stage/unstage 共用体：{"path": "..."} 单文件 或 {"all": true}（unstage
/// 无现成全量函数，逐个 reset 已登记的 staged 文件；批量只保留最后一个
/// 错误——更早的失败被吞，详情走 git_error 进 git 面可见）。
fn git_stage_op(chat: &mut Chat, params: &Value, stage: bool) -> Result<(), String> {
    if params.get("all").and_then(Value::as_bool).unwrap_or(false) && !stage {
        let staged: Vec<PathBuf> = chat
            .ws
            .git_files
            .iter()
            .filter(|f| f.staged)
            .map(|f| f.path.clone())
            .collect();
        let mut last = Ok(());
        if let Some(cwd) = chat.ws.cwd.as_deref() {
            for p in staged {
                last = crate::services::git::git_unstage(cwd, &p);
            }
        }
        last
    } else if let Some(cwd) = chat.ws.cwd.as_deref() {
        if params.get("all").and_then(Value::as_bool).unwrap_or(false) {
            crate::services::git::git_stage_all(cwd)
        } else {
            let path = params_path(params, "path").map_err(|e| e.1)?;
            if stage {
                crate::services::git::git_stage(cwd, &path)
            } else {
                crate::services::git::git_unstage(cwd, &path)
            }
        }
    } else {
        // 无项目态：没有可操作的仓库
        Err("no project".into())
    }
}

/// 021 行内编辑的预填（files.rename / files.new_entry 的可选 `name`）：
/// 打开编辑行后直接把新名灌进输入框，省一次 input.keys 逐字键入。
fn tree_edit_prefill(chat: &mut Chat, params: &Value, cx: &mut gpui::Context<Chat>) {
    let Some(name) = params.get("name").and_then(Value::as_str) else {
        return;
    };
    if let Some(edit) = chat.dlg.tree_edit.as_ref() {
        let name = name.to_string();
        edit.input.update(cx, |ti, cxi| ti.set_value(name, cxi));
    }
}
