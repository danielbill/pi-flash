//! PF 插件宿主——会话功能栏（契约 v3 fn_bar）：chrome 批量取数（1s TTL 缓存）
//! 、点击回环（绑定当前会话 → 挂载 pi 扩展 → command env 回环）、页面 icon
//! 元素装配（session/note 渲染面消费）。

use gpui::{MouseButton, SharedString, div, prelude::*, px, rgb};
use pf_plugin_host::js::JsAction;
use serde_json::Value;

use crate::Chat;
use crate::ui::tooltip::WithTip;

use super::*;

impl Chat {
    /// 契约 v3 fn_bar：会话功能栏插件 icon 批量取数（带 1s TTL 缓存——
    /// 功能栏每帧渲染，调用不进每帧）。按能力面取（清单 fn_bar 声明 +
    /// 插件导出，载体无关）；停用/坏插件不出。
    pub(crate) fn plugin_fn_bar_chrome(
        &mut self,
        session_key: &str,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<(usize, SharedString, String, String)> {
        const TTL: std::time::Duration = std::time::Duration::from_millis(1000);
        let mut out = Vec::new();
        let mut miss: Vec<usize> = Vec::new();
        for (ix, e) in self.plugins.plugins.iter().enumerate() {
            if e.manifest.fn_bar.is_none() || e.disabled {
                continue;
            }
            let hit = self.plugins.fn_bar_cache.iter().find(|c| {
                c.pkg == e.manifest.pkg && c.session_key == session_key && c.at.elapsed() < TTL
            });
            match hit {
                Some(c) => {
                    let path = contrib_icon_path(&e.manifest.id, &c.icon);
                    out.push((ix, path, c.tone.clone(), c.tip.clone()));
                }
                None => miss.push(ix),
            }
        }
        for ix in miss {
            // 未装载的插件不阻塞功能栏（B5 就绪检查不装载：渲染帧路径禁隐
            // 式首装，不变量见 `plugin_loaded`；装载后的补拍 + 下一拍缓存
            // 失效自然补上 chrome）
            if !self.plugin_loaded(ix) {
                continue;
            }
            let pkg = self.plugins.plugins[ix].manifest.pkg.clone();
            let env = serde_json::json!({ "session": { "key": session_key } }).to_string();
            let (result, actions) = {
                let entry = &mut self.plugins.plugins[ix];
                entry.hook_fn_bar(&env)
            };
            // chrome 取数期间的动作丢弃（fn_bar 是纯 chrome，不该有副作用）。
            // 例外：会话列表徽标（SetBadge/ClearBadge）就是 chrome 的伴随声明——
            // 不放行的话，已绑会话的列表 icon 永远打不上（002 时间位徽标）。
            for a in &actions {
                match a {
                    JsAction::SetBadge {
                        session,
                        icon,
                        tone,
                    } => {
                        if !session.is_empty() {
                            let owner = self.plugins.plugins[ix].manifest.id.clone();
                            self.plugins.badges.retain(|b| b.owner != owner); // 一插件一条
                            self.plugins.badges.push(PluginBadge {
                                owner,
                                session: session.clone(),
                                icon: icon.clone(),
                                tone: tone.clone(),
                            });
                        }
                    }
                    JsAction::ClearBadge { session } => {
                        let owner = self.plugins.plugins[ix].manifest.id.clone();
                        if session.is_empty() {
                            self.plugins.badges.retain(|b| b.owner != owner);
                        } else {
                            self.plugins
                                .badges
                                .retain(|b| !(b.owner == owner && b.session == *session));
                        }
                    }
                    _ => eprintln!("[pf-plugin] fn_bar 期间的动作被忽略: {a:?}"),
                }
            }
            match result {
                Ok(Some(json)) => {
                    let v: serde_json::Value =
                        serde_json::from_str(&json).unwrap_or(serde_json::Value::Null);
                    let icon = v
                        .get("icon")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let tone = v
                        .get("tone")
                        .and_then(Value::as_str)
                        .unwrap_or("text.faint")
                        .to_string();
                    // tip 是文案对象 {zh,en} 或纯字符串，统一解成本语言串
                    let tip = match v.get("tip") {
                        Some(Value::Object(_)) => {
                            let zh = v["tip"]["zh"].as_str().unwrap_or("").to_string();
                            let en = v["tip"]["en"].as_str().map(|s| s.to_string());
                            pick_lang(&zh, &en)
                        }
                        Some(Value::String(s)) => s.clone(),
                        _ => String::new(),
                    };
                    self.plugins
                        .fn_bar_cache
                        .retain(|c| !(c.pkg == pkg && c.session_key == session_key));
                    self.plugins.fn_bar_cache.push(FnBarCache {
                        pkg: pkg.clone(),
                        session_key: session_key.to_string(),
                        icon: icon.clone(),
                        tone: tone.clone(),
                        tip: tip.clone(),
                        at: std::time::Instant::now(),
                    });
                    if !icon.is_empty() {
                        let path = contrib_icon_path(&self.plugins.plugins[ix].manifest.id, &icon);
                        out.push((ix, path, tone, tip));
                    }
                }
                // 未导出 fn_bar：负缓存一轮（TTL 内不再问）
                Ok(None) => self.plugins.fn_bar_cache.push(FnBarCache {
                    pkg: pkg.clone(),
                    session_key: session_key.to_string(),
                    icon: String::new(),
                    tone: String::new(),
                    tip: String::new(),
                    at: std::time::Instant::now(),
                }),
                Err(e) => self.plugin_broken(ix, e, cx),
            }
        }
        // 尾闸：同包只留第一颗（2026-12 真机：内存条目异常重复时 fn_bar 双
        // icon——miss 循环对每条目各查一次 push，行徽标有 row_cache 按 pkg
        // 兑底，这里补上同款防线；触发即打取证日志）。
        let before = out.len();
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        out.retain(|(ix, _, _, _)| {
            let Some(e) = self.plugins.plugins.get(*ix) else {
                return false;
            };
            seen.insert(e.manifest.pkg.as_str())
        });
        if out.len() != before && cfg!(debug_assertions) {
            eprintln!(
                "[pf-plugin] 异常：fn_bar 同包重复条目 {}→{}（重复装载来源待查）",
                before,
                out.len()
            );
        }
        out
    }

    /// fn_bar icon 点击：command 回环（env 带 {session:{key}}，契约 v3）。
    /// 绑定目标 = 当前会话：先把包内 pi 扩展写进该会话扩展清单（出码/收发
    /// 本体在 ext，004 §5.2），进程空闲即重启挂载，再回环命令。
    pub(crate) fn plugin_fn_bar_click(
        &mut self,
        ix: usize,
        session_key: &str,
        window: &mut gpui::Window,
        cx: &mut gpui::Context<Self>,
    ) {
        // v3 面（回滚）：绑定目标 = 当前会话——先把包内 pi 扩展写进该会话
        // 扩展清单（出码/收发本体在 ext），进程空闲即重启挂载，再回环命令。
        // 执行域（v4）代码保留但仅对声明 contributes.service 的包生效；
        // v3 包走 plugin_attach_pi_exts（v3 镜像协议回滚，2026-10-30 用户令）。
        {
            let has_service = self
                .plugins
                .plugins
                .get(ix)
                .map(|e| !e.manifest.service.is_empty())
                .unwrap_or(false);
            if !has_service {
                let pf_key = self.pool.active_key.clone();
                self.plugin_attach_pi_exts(ix, &pf_key, false, cx);
                self.ensure_active_process(cx);
            }
        }
        let Some(cmd) = self
            .plugins
            .plugins
            .get(ix)
            .and_then(|e| e.manifest.fn_bar.as_ref())
            .map(|f| f.command.clone())
        else {
            return;
        };
        let env = serde_json::json!({ "session": { "key": session_key } }).to_string();
        self.plugin_command_fire_env(ix, &cmd, &env, window, cx);
    }
}

/// 会话功能栏插件 icon（契约 v3 fn_bar）：每插件一颗 icon 钮（tone 着色、
/// tip 悬浮），点击 = fn_bar.command 回环（env 带当前会话键）。
pub(crate) fn fn_bar_plugin_elements(
    chat: &mut Chat,
    cx: &mut gpui::Context<Chat>,
) -> Vec<gpui::AnyElement> {
    // 当前会话键：**pool.active_key 规范键**（005 §②）——与挂载台账键、
    // spawn 注入的 PF_SESSION_KEY、命令 env 三通道同源。此前「标签路径 +
    // pi 侧 session_file/session_id 兜底」三形态漂移：草稿会话空键、draft-N
    // 与文件路径互不相认（真机 2026-11-03，出码归属丢失）。
    let session_key = chat.pool.active_key.clone();
    let chromes = chat.plugin_fn_bar_chrome(&session_key, cx);
    let weak = cx.entity().downgrade();
    chromes
        .into_iter()
        .map(|(ix, icon_path, tone, tip)| {
            let t = crate::appearance::theme::theme();
            // accent（插件的「已绑定/激活」态语义）固定映射主题绿
            // （appearance::UNREAD）——kitchen 等主题 accent 是红，直接透传
            // 会与错误色混淆。其余按 token 原样映射（text.faint = 未绑默认
            // 灰，2026-10-29 用户定夺；曾一律 text_muted 近黑，灰不可辨）。
            let color = match tone.as_str() {
                "accent" => crate::appearance::theme::UNREAD,
                "text.faint" => t.text_faint,
                "text.muted" => t.text_muted,
                _ => t.text_muted,
            };
            let weak = weak.clone();
            let key = session_key.clone();
            let mut el = div()
                .id(SharedString::from(format!("pf-fnbar-{ix}")))
                .flex()
                .items_center()
                .justify_center()
                .w(px(32.))
                .h(px(32.))
                .mr(px(20.)) // 按钮右侧留 20px（不压缩 icon，用户定夺）
                .rounded(px(6.))
                .cursor_pointer()
                .hover(move |s| s.bg(rgb(t.bg_hover)));
            if !tip.is_empty() {
                el = el.with_tip_text(tip);
            }
            el.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                let _ = weak.update(cx, |c, cx| {
                    c.plugin_fn_bar_click(ix, &key, window, cx);
                    cx.notify();
                });
            })
            .child(
                gpui::svg()
                    .path(icon_path)
                    .text_color(rgb(color))
                    .size(px(24.)), // fn 槽 icon = XL 档 24（用户定夺 2026-10-28）
            )
            .into_any_element()
        })
        .collect()
}
