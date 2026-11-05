//! 会话动作：内置斜杠命令（compact/reload/name/copy/clone…）、思考档
//! 换挡、模型选择、扩展命令发送、手动压缩、工具预设标签。

use gpui::Context;
use pi_link::protocol::Command;

use crate::i18n::{tf, tr};
use crate::session::messages::Role;

use super::*;

impl SessionRuntime {
    /// 内置斜杠命令的运行时臂（zi2s.5；Chat 级分发在 input/mod.rs
    /// `builtin_slash_dispatch`——session/mcp 开面板的在那边，不走这里）。
    /// 返回 false = 运行时侧不处理（调用方照旧走 prompt）。
    pub(crate) fn slash_builtin(&mut self, name: &str, args: &str, cx: &mut Context<Self>) -> bool {
        // draft 懒 spawn 语境（pi-web ensureNewSession parity）：内置命令要在
        // 真会话上执行，进程不在就先拉起（拦截发生在 send_input 的
        // ensure_spawned 之前）
        if !self.ensure_spawned(cx) {
            self.notify_toast(tr("status.disconnected").into(), cx);
            return true;
        }
        match name {
            "compact" => {
                // can_compact 拒绝时必须有提示（圆环按钮灰态能看出来，斜杠
                // 入口没有视觉状态——静默 return 就是「毫无反应」，真机验收
                // 实踩）。agent_running 时尤其要说明：压缩会掐当前轮次。
                if self.agent_running {
                    self.notify_toast(tr("slash.compact_running").into(), cx);
                    return true;
                }
                if self.compacting {
                    self.notify_toast(tr("slash.compact_ongoing").into(), cx);
                    return true;
                }
                self.compact(cx);
                // 受理回执（placeholder 会换「压缩中」，但斜杠入口需要即时
                // 可见的确认）
                if self.compacting {
                    self.notify_toast(tr("slash.compact_started").into(), cx);
                }
                true
            }
            "auto-compact" => {
                let next = !self.auto_compaction.unwrap_or(true);
                self.auto_compaction = Some(next);
                if let Some(s) = &self.agent.session {
                    let _ = s.send(&Command::SetAutoCompaction { enabled: next });
                }
                let msg: String = tr(if next {
                    "slash.auto_compact_on"
                } else {
                    "slash.auto_compact_off"
                })
                .into();
                self.notify_toast(msg, cx);
                true
            }
            "reload" => {
                // pi rpc-mode 无 reload 命令（rpc-mode.js 实测，pi-web 走自己
                // 的服务端调 SDK）：对齐 pi-web reloadSessionResources 的效果
                // = 重绑进程（同 --session 复接，扩展/MCP/skills 全部重读）
                // + 清单随 spawn 刷新
                self.pending_mcp_reload = true;
                if self.rebind_pending(cx) {
                    self.notify_toast(tr("slash.reloaded").into(), cx);
                }
                true
            }
            "name" => {
                if args.is_empty() {
                    self.notify_toast(tr("slash.name_usage").into(), cx);
                    return true;
                }
                let name = args.to_string();
                if let Some(s) = &self.agent.session {
                    let _ = s.send(&Command::SetSessionName { name: name.clone() });
                }
                // 乐观写状态槽（pi 回执臂再补 ListDirty 刷侧栏）
                if let Some(st) = self.state.as_mut() {
                    st.session_name = Some(name.clone());
                }
                self.notify_toast(tf("slash.named", &[("name", name)]).into(), cx);
                true
            }
            "copy" => {
                // actions_bar 同链：本地消息尾找最后一条 assistant（pi-web
                // 走 get_last_assistant_text RPC，本地链等价且零往返）
                let text = self
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == Role::Assistant)
                    .map(|m| m.plain_text().into_owned())
                    .unwrap_or_default();
                if text.is_empty() {
                    self.notify_toast(tr("slash.copy_empty").into(), cx);
                } else {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                    self.notify_toast(tr("slash.copied").into(), cx);
                }
                true
            }
            "clone" => {
                if self.agent_running || self.bash_running {
                    self.notify_toast(tr("slash.clone_running").into(), cx);
                    return true;
                }
                // 尾部轮 = 整段复制（fork.rs fork_from_turn(None) 的 Clone 链）
                self.fork_from_turn(None, cx);
                true
            }
            _ => false,
        }
    }

    /// 思考档换挡（胶囊菜单 / 操作栏菜单同源）。
    ///
    /// `level` 是 pi 的档位键。pi 会按模型能力**截断**（deepseek-flash 只有
    /// off/low/high/max），真值随 `thinking_level_changed` 事件回来，本地这
    /// 里先乐观回填状态槽（不然胶囊要等下一次 `get_state` 才动，看起来像
    /// 「没换过去」）。
    pub(crate) fn set_thinking_level(&mut self, level: &str, cx: &mut Context<Self>) {
        self.thinking_override = Some(level.to_string());
        self.push_thinking_level(level);
        cx.notify();
    }

    /// 「使用 pi 默认设置」（菜单 auto）：override 清空（打勾留在 auto），但
    /// pi 端**没有** auto 档——只清本地标志等于点了没反应（真机：换到 low
    /// 再选 auto，pi 仍停在 low、胶囊也不动）。所以由调用方按 pi 设置解出
    /// 默认档（[`pi_link::config::PI_DEFAULT_THINKING_LEVEL`] 兜底）显式下发。
    pub(crate) fn set_thinking_auto(&mut self, default: &str, cx: &mut Context<Self>) {
        self.thinking_override = None;
        self.push_thinking_level(default);
        cx.notify();
    }

    /// 乐观回填状态槽 + 下发 rpc。没有进程（空闲回收/草稿）时就只落本地：
    /// 下次 spawn 用 `--thinking <override>` 带上。
    fn push_thinking_level(&mut self, level: &str) {
        if let Some(st) = self.state.as_mut() {
            st.thinking_level = Some(level.to_string());
        }
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetThinkingLevel {
                level: level.to_string(),
            });
        }
    }

    pub(crate) fn select_model(&mut self, provider: String, id: String, cx: &mut Context<Self>) {
        if let Some(session) = &self.agent.session {
            let _ = session.send(&Command::SetModel {
                provider,
                model: id,
            });
        } else {
            // draft with no process: remember the pick, spawn carries it as
            // --model (pi-web newSessionModel parity)
            self.pending_model = Some((provider, id));
        }
        // no immediate refresh_state here: pi 1.0 completes the swap
        // asynchronously, so an eager get_state reports the PREVIOUS model —
        // the label updates from the set_model response instead (see the
        // set_model response arm)
        cx.notify();
    }

    /// 034 状态带可点项：执行一条扩展命令（如 `/usage`）。
    ///
    /// 与 composer 手打**同一条漏斗**：同一个 `Command::Prompt`（命令相位、
    /// 乐观气泡、磁盘真相收场全由既有链路接管），区别只是不经输入框、
    /// 不进 ↑ 历史。运行中不发（按钮侧已置灰，这里兼底）；要排队一条命令
    /// 会让用户以为点了没反应（等待行要等上一轮才出现）。
    pub(crate) fn send_command(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.compacting || self.agent_running {
            return;
        }
        // 040/043：空闲发送前生效的扩展/MCP 重绑（与 send_input 同口径）
        if !self.rebind_pending(cx) || !self.ensure_spawned(cx) {
            return;
        }
        let cmd = Command::Prompt {
            message: text.to_string(),
            images: Vec::new(),
            streaming_behavior: Some("followUp".into()),
        };
        match self.agent.session.as_ref().map(|s| s.send(&cmd)) {
            Some(Ok(_)) => {
                self.status = "running".into();
                self.optimistic_send(text.to_string(), true, cx);
            }
            Some(Err(e)) => self.status = e,
            None => self.status = tr("status.disconnected").into(),
        }
        cx.notify();
    }

    /// 压缩按钮可用性：agent 跑动中不可（compact 会中止当前 turn）、
    /// 压缩中防连点、未连接不可。除此之外不设门槛——用不用由用户决定。
    pub(crate) fn can_compact(&self) -> bool {
        !self.compacting && !self.agent_running && self.agent.session.is_some()
    }

    /// 圆环弹窗底部手动压缩：rpc compact。`compacting` 期间输入面板锁死
    /// （placeholder 提示 + 变更丢弃）；完成后 pi 回 compact response，
    /// 由其分支清标志并刷新 stats/messages。
    pub(crate) fn compact(&mut self, cx: &mut Context<Self>) {
        if !self.can_compact() {
            return;
        }
        match self
            .agent
            .session
            .as_ref()
            .map(|s| s.send(&Command::Compact))
        {
            Some(Ok(_)) => {
                self.compacting = true;
                self.status = tr("status.compacting").into();
            }
            Some(Err(e)) => self.status = e,
            None => self.status = tr("status.disconnected").into(),
        }
        cx.emit(SessionEvent::Changed);
        cx.notify();
    }

    pub(crate) fn tool_preset_key(&self) -> &str {
        &self.tools_preset
    }

    pub(crate) fn tool_preset_label(&self) -> String {
        let key = self.tool_preset_key();
        if key.is_empty() {
            return "configured".into();
        }
        if key == "custom" {
            // 档名 full+；扩展数量由右边的「扩展(n)」按钮承载，不重复
            return crate::i18n::tr("extensions.full").to_string();
        }
        key.to_string()
    }
}
