//! composer (v57 输入组件化)：真输入框 = gpui-component InputState 多行
//! 模式（ui/composer_input.rs 门面）——点击定位光标、左右键移动、多行上下
//! 键、选区、IME、undo、超限纵向滚动条全为组件内置，不再手搓 String+绘制
//! 光标。单容器 16px 圆角 1px 边框，宽 = 消息列宽（外层同 px(15) 内边距 +
//! 设置项「会话内容宽度」封顶，见 `workspace::CHAT_WIDTH_*`），窗口缩放时
//! 始终贴合，不再用百分比近似
//! 居中，悬浮胶囊上浮叠在聊天区上（0 高 wrapper 不吞点击/滚轮）。控件行：
//! 左 = 图片 + 工具预设「默认∨」；右 = 上下文用量环 + 模型∨ + 思考∨ +
//! 圆形发送 ↑ / 主题色停止块（运行中）。
//!
//! 按键路由（详见 composer_input.rs 头注）：
//! - Enter → 组件 PressEnter{secondary:false} → on_submit（菜单开=接受补全；
//!   运行中=引导；空闲=发送）
//! - Shift+Enter → wrapper on_key_down（运行中=排队 follow-up，空闲=换行）
//! - ↑/↓/Tab → "Input" 上下文覆盖绑定截获为 ComposerUp/Down/Tab（菜单导航
//!   /历史回溯/补全），非空多行重新派发组件 MoveUp/MoveDown
//! - Escape → wrapper on_key_down（菜单开=取消补全，运行中=中止）

use gpui::Context;

use crate::Chat;
use crate::*;

mod area;
mod bar;
mod ctx_tip;
mod queue_cards;

pub(crate) use area::*;
use bar::*;
use ctx_tip::*;
pub(crate) use queue_cards::*;

// ---------------------------------------------------------------------------
// composer 方法：输入写入/@ 索引/发送与中止（P3 自 main.rs impl Chat 拆入）
// ---------------------------------------------------------------------------

impl Chat {
    /// chat.composer.input 唯一写入口：同步镜像进输入组件（组件是渲染真值源，
    /// 手写 input 不再存在）。历史/草稿/清空/补全改写都走这里。
    pub(crate) fn set_input(&mut self, v: String, cx: &mut Context<Self>) {
        self.composer.input = v;
        self.composer.menu_dismissed = false;
        if let Some(c) = &self.composer.composer {
            c.update(cx, |f, fcx| f.set_value(&self.composer.input, fcx));
        }
        cx.notify();
    }

    /// set_input 的光标落点版（031 @ 补全：确认后光标停在插入 token 之后）。
    /// cursor_byte = 新值的字节偏移。
    pub(crate) fn set_input_with_cursor(
        &mut self,
        v: String,
        cursor_byte: usize,
        cx: &mut Context<Self>,
    ) {
        self.composer.input = v;
        self.composer.menu_dismissed = false;
        if let Some(c) = &self.composer.composer {
            let v = self.composer.input.clone();
            c.update(cx, |f, fcx| f.set_value_with_cursor(v, cursor_byte, fcx));
        }
        cx.notify();
    }

    /// @ 文件索引惰性构建（031 对齐 pi-web file-index：TTL 内复用，过期/
    /// 缺失在后台线程重建；旧清单在重建期间先顶上。TTL/容量读
    /// app-settings.json，默认 60s / 10 项目）。
    pub(crate) fn ensure_at_index(&mut self, cx: &mut Context<Self>) {
        // 无项目态：无文件索引可言（发送已被 send 门拦在选目录之前）
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let ttl = std::time::Duration::from_secs(services::workspace::at_index_ttl_secs());
        let key = cwd.to_string_lossy().to_string();
        let fresh = match self.composer.at_index.get(&key) {
            Some(e) => e.building || e.built.is_some_and(|t| t.elapsed() < ttl),
            None => false,
        };
        if fresh {
            return;
        }
        let entry = self.composer.at_index.entry(key.clone()).or_default();
        entry.building = true;
        entry.built = None;
        // 缓存上限（pi-web：超限整表清空——每 cwd 一份，重建很便宜）
        if self.composer.at_index.len() >= services::workspace::at_index_max_projects() {
            self.composer.at_index.clear();
        }
        cx.spawn(async move |this, cx| {
            // entries 派生也在后台：大仓库（20 万条）下主线程只做 Arc 换手
            let (files, entries) = cx
                .background_executor()
                .spawn(async move {
                    let listing = crate::services::file_index::load_listing(&cwd);
                    let entries =
                        crate::services::at_file::build_entries_from_files(&listing.files);
                    (listing.files, entries)
                })
                .await;
            let _ = this.update(cx, |chat, cx| {
                // key = 构建时的 cwd（期间切项目也不会写错条目）
                let e = chat.composer.at_index.entry(key.clone()).or_default();
                e.files = std::sync::Arc::new(files);
                e.entries = std::sync::Arc::new(entries);
                e.built = Some(std::time::Instant::now());
                e.building = false;
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn with_active_editor<
        Act: FnOnce(&mut SessionRuntime, &mut Context<SessionRuntime>),
    >(
        &mut self,
        cx: &mut Context<Self>,
        act: Act,
    ) {
        let rt = self.rt();
        let (input, images, history) = (
            self.composer.input.clone(),
            self.composer.pending_images.clone(),
            self.composer.history.clone(),
        );
        rt.update(cx, |r, cx| {
            r.input = input;
            r.pending_images = images;
            r.history = history;
            act(r, cx);
        });
        // pull post-action state back (history push, input clear)
        let (input, images, history) = {
            let r = rt.read(cx);
            (r.input.clone(), r.pending_images.clone(), r.history.clone())
        };
        self.set_input(input, cx);
        self.composer.pending_images = images;
        self.composer.history = history;
        self.composer.history_ix = None;
        cx.notify();
    }

    pub(crate) fn send_input(&mut self, cx: &mut Context<Self>) {
        // 无项目态 send 门（010 重设计）：cwd 是会话的属性，发送前必须先有
        // 目录。文字留在 composer（取消选择不丢），副本进 pending_send；
        // 选定目录后 new_session_in → flush_pending_send 回填发送
        if self.ws.cwd.is_none() {
            if self.dlg.pending_send.is_none() {
                self.dlg.pending_send = Some(self.composer.input.clone());
            }
            self.open_project_picker(true, cx);
            return;
        }
        // 单一语义（2026-10-18 定案）：当前项目 ≡ 活跃会话的项目——全部
        // ws.cwd 写入点（switch_project / open_session / switch_pool_rt /
        // 树侧两处）都同步移动注意力，发送只认活跃会话，没有第二个判断
        // 依据（曾加过「cwd 不一致则重落草稿」的兑底判断，核查全部路径后
        // 确认分叉不可达，判断本身即第二语义残渣，已删）。
        self.with_active_editor(cx, |r, cx| r.send_input(cx));
    }

    /// send 门回填（new_session_in 落新草稿后调）：switch_to 会把 composer
    /// 文字随旧 runtime 换走，先把暂存文字放回；期间用户真有新输入则以新
    /// 输入为准（暂存弃掉，不自动发）。
    pub(crate) fn flush_pending_send(&mut self, cx: &mut Context<Self>) {
        let Some(stashed) = self.dlg.pending_send.take() else {
            return;
        };
        if self.composer.input.is_empty() {
            self.set_input(stashed.clone(), cx);
        }
        if self.composer.input == stashed {
            self.send_input(cx);
        }
    }

    pub(crate) fn steer_input(&mut self, cx: &mut Context<Self>) {
        self.with_active_editor(cx, |r, cx| r.steer_input(cx));
    }

    /// 090：steer 模式下 Ctrl+Enter 显式入队（与 send_input 排队分支同一
    /// 漏斗：入本地队列 + 空闲即 flush）。
    pub(crate) fn queue_enqueue_direct(&mut self, cx: &mut Context<Self>) {
        if self.ws.cwd.is_none() {
            return;
        }
        self.with_active_editor(cx, |r, cx| {
            let text = r.input.trim().to_string();
            if text.is_empty() && r.pending_images.is_empty() {
                return;
            }
            if !r.ensure_spawned(cx) {
                return;
            }
            r.enqueue_from_composer(text, cx);
            r.flush_local_queue(cx);
        });
    }

    pub(crate) fn follow_up_input(&mut self, cx: &mut Context<Self>) {
        self.with_active_editor(cx, |r, cx| r.follow_up_input(cx));
    }

    pub(crate) fn abort_stream(&mut self, cx: &mut Context<Self>) {
        self.rt().update(cx, |r, cx| r.abort_stream(cx));
        cx.notify();
    }

    /// `!` bash 执行中 Esc / 停止按钮（031）：rpc abort_bash
    pub(crate) fn abort_bash(&mut self, cx: &mut Context<Self>) {
        self.rt().update(cx, |r, cx| r.abort_bash(cx));
        cx.notify();
    }

    /// 思考档换挡入口（胶囊菜单 / 操作栏菜单同源）。`key` 是 pi 的档位键，
    /// 特例 `"auto"` = 「使用 pi 默认设置」：pi 没有 auto 档，得把设置里
    /// 解出来的默认档显式下发（只清本地 override 等于点了没反应）。
    pub(crate) fn set_thinking_level(&mut self, key: &str, cx: &mut Context<Self>) {
        if key == "auto" {
            let def = self.pi_default_thinking(cx);
            self.rt().update(cx, |r, cx| r.set_thinking_auto(&def, cx));
            return;
        }
        self.rt().update(cx, |r, cx| r.set_thinking_level(key, cx));
    }

    /// pi 侧对当前模型生效的默认思考档：enabledModels pin > 设置页记录的
    /// per-model 档 > 全局 defaultThinkingLevel > pi 内置缺省（medium）。
    /// 与 012 新会话默认档同一把尺子（{@link new_session_default}），只是
    /// 锚在当前模型而不是默认模型上。
    fn pi_default_thinking(&self, cx: &gpui::App) -> String {
        let rt = self.rt().read(cx);
        let model_key = rt
            .state
            .as_ref()
            .and_then(|s| s.model.as_ref())
            .map(|m| format!("{}/{}", m.provider, m.id))
            .or_else(|| rt.pending_model.as_ref().map(|(p, id)| format!("{p}/{id}")))
            .or_else(|| {
                self.cfg
                    .mc_default_model
                    .as_ref()
                    .map(|(p, id)| format!("{p}/{id}"))
            });
        let lookup = |list: &[(String, String)]| {
            model_key
                .as_ref()
                .and_then(|k| list.iter().find(|(p, _)| p == k).map(|(_, l)| l.clone()))
        };
        lookup(&self.cfg.mc_state.pins)
            .or_else(|| lookup(&self.cfg.mc_model_thinking))
            .or_else(|| self.cfg.mc_default_thinking.clone())
            .unwrap_or_else(|| pi_link::config::PI_DEFAULT_THINKING_LEVEL.to_string())
    }

    /// `/` 菜单命令清单（010-启动.md §3）：会话进程答过 → 用它（含项目 skill）；
    /// 否则 = 项目 skill 派生（`skill:<name>`）+ 全局扩展命令（内置表 + 缓存）。
    /// 草稿态（无进程）因此也有完整菜单。
    ///
    /// 返回 Cow：会话进程答过的场景（slash 菜单打开期间每帧调用）直接借用
    /// rt.commands，不再每帧深克隆整张命令表。
    pub(crate) fn slash_commands<'a>(
        &self,
        cx: &'a gpui::App,
    ) -> std::borrow::Cow<'a, [pi_link::protocol::SlashCommand]> {
        let rt = self.rt().read(cx);
        if !rt.commands.is_empty() {
            return std::borrow::Cow::Borrowed(&rt.commands);
        }
        let mut out: Vec<pi_link::protocol::SlashCommand> = self
            .pool
            .project_ctx
            .get(&self.project_key())
            .map(|ctx| ctx.skill_commands.clone())
            .unwrap_or_default();
        out.extend(self.pool.globals.commands.iter().cloned());
        std::borrow::Cow::Owned(out)
    }
}

// ---- @ 文件索引状态类型（自 shell/types.rs 下沉，composer 域自持）----

#[derive(Clone, Default)]
pub(crate) struct AtIndexState {
    pub(crate) built: Option<std::time::Instant>,
    pub(crate) files: std::sync::Arc<Vec<String>>,
    pub(crate) entries: std::sync::Arc<Vec<crate::services::at_file::FileEntry>>,
    pub(crate) building: bool,
}

/// 索引重建整体换新 Arc → `Arc::ptr_eq` 失配自然失效，无需手动清。
pub(crate) type AtFilterCache = (
    std::sync::Arc<Vec<crate::services::at_file::FileEntry>>,
    String,
    Vec<crate::services::at_file::FileEntry>,
);
