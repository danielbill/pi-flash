//! input 组：合成按键、鼠标注入（走真实 gpui 事件链）、元素级聚焦、输入
//! 面板右键菜单。

use gpui::{Context, Focusable, Keystroke, Window};
use serde_json::{Value, json};

use super::{OpResult, bad, not_found, ok};
use crate::Chat;
use pi_link::automation::method;

pub(super) fn dispatch(
    chat: &mut Chat,
    window: &mut Window,
    cx: &mut Context<Chat>,
    m: &str,
    params: &Value,
) -> OpResult {
    match m {
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
            // pressed=true 的 move 事件带 pressed_button（真实拖拽语义）——
            // 编辑器拖拽划选等按住拖动的手势只有它触发（component 端
            // on_drag_move 门 pressed_button==Some(Left)，裸 move 注入划
            // 不出选区，划选类 bug 因此无法自动化复现）
            let pressed = params
                .get("pressed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
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
                                pressed_button: pressed.then_some(button),
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

        _ => Err((
            "unknown_method".into(),
            format!("未知 method: {m}（清单见 pi_link::automation::method）"),
        )),
    }
}
