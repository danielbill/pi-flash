//! pi 子进程挂接与池内切换：subscribe_runtime（RPC 事件泵→Chat 状态回
//! 灌主路）、switch_to（运行时换绑与焦点/键位迁移）。

use super::*;

impl Chat {
    pub(crate) fn subscribe_runtime(
        &mut self,
        rt: &gpui::Entity<session::runtime::SessionRuntime>,
        cx: &mut Context<Self>,
    ) {
        // 执行域实例配置随会话挂册同步（90-6；新会话铸造 token + 写实例配置）
        crate::pf_plugin::svc::svc_sync_configs(self);
        cx.subscribe(rt, |chat, rt, ev: &session::runtime::SessionEvent, cx| {
            use session::runtime::SessionEvent;
            let is_active = rt.read(cx).key == chat.pool.active_key;
            match ev {
                SessionEvent::Changed => {
                    // recents (003-session管理): message-level activity on
                    // this runtime is "recently updated" — promote its file
                    // in the recent-activity list (persist throttled)
                    if let Some(f) = rt.read(cx).file.clone() {
                        pi_link::recents::touch_recent(&f);
                    }
                    if !is_active {
                        // v54 未读绿点：非活跃会话在跑/在流式 → 记未读，
                        // 切换到它时清除
                        let (file, running) = {
                            let r = rt.read(cx);
                            (
                                r.file.clone(),
                                r.agent_running || r.state.as_ref().is_some_and(|s| s.is_streaming),
                            )
                        };
                        if let Some(f) = file {
                            if running && !chat.psp.unread.contains(&f) {
                                chat.psp.unread.insert(f);
                            }
                        }
                    }
                    cx.notify();
                }
                SessionEvent::TurnSettled => {
                    // 轮次终点（2026-10-10 规则修订）：agent 结束时**窗口没
                    // 焦点、或本会话已转后台**（用户切到同一窗口的别的对话）
                    // 就给点——三色分类：外部错误/崩溃 → 红；内部中断
                    // （length/aborted）→ 黄；正常完成 → 青蓝。在场看见不给。
                    // is_active = 该 runtime key vs 当前活跃 key，结算时刻
                    // 现算即前台/后台判定
                    if !chat.psp.window_active || !is_active {
                        let (file, turn_error, turn_interrupted) = {
                            let r = rt.read(cx);
                            (r.file.clone(), r.turn_error, r.turn_interrupted)
                        };
                        if let Some(f) = file {
                            // 通知点只表达**最近一轮**的结局：先摘掉上一轮的
                            // 档位再挂新点——否则旧红点（优先级最高）会盖住
                            // 新一轮的完成通知，反之亦然
                            chat.psp.unread.remove(&f);
                            chat.psp.turn_errors.remove(&f);
                            chat.psp.turn_warnings.remove(&f);
                            if turn_error {
                                chat.psp.turn_errors.insert(f);
                            } else if turn_interrupted {
                                chat.psp.turn_warnings.insert(f);
                            } else {
                                chat.psp.unread.insert(f);
                            }
                        }
                    }
                    cx.notify();
                }
                SessionEvent::ListDirty => {
                    chat.refresh_sessions();
                    cx.notify();
                }
                SessionEvent::PendingTab(preview) => {
                    // 伪标签生命周期（draft 首条消息乐观上侧栏）：挂/刷新按
                    // key 幂等，撤按 key 摘除。真标签由 FileBound 落地。
                    let key = rt.read(cx).key.clone();
                    match preview {
                        Some(p) => {
                            let p = p.clone();
                            let cwd = rt.read(cx).cwd.to_string_lossy().to_string();
                            if let Some(e) = chat.psp.pending_tabs.iter_mut().find(|e| e.key == key)
                            {
                                e.preview = p;
                            } else {
                                chat.psp.pending_tabs.push(PendingTab {
                                    key,
                                    preview: p,
                                    cwd,
                                });
                            }
                        }
                        None => {
                            chat.psp.pending_tabs.retain(|e| e.key != key);
                        }
                    }
                    cx.notify();
                }
                SessionEvent::ExtUi(req) => {
                    // **只接远程插件线自己的请求**（与下面 Assistant 同一条闸）。
                    // 扩展的 ui 广播是全局的：任何会话里任何扩展调
                    // `ui.notify` / `setStatus` / `setTitle` 都走这条事件，
                    if is_active {
                        pi_link::client::ext_trace(&format!(
                            "route: id={} key={}",
                            req.id,
                            rt.read(cx).key
                        ));
                        let req = req.clone();
                        chat.on_ext_ui(req, cx);
                    } else {
                        // parked session asks for permission: queue it, the
                        // badge surfaces it; popped when user switches there
                        // #问题卡不弹： parked 期间的积压请求在此兑现，留痕对账
                        // 阻塞判定收口到 protocol::ExtUiMethod::is_blocking——
                        // 侧栏橙点徽标（psp_row_views awaiting）与这里必须同一
                        // 口径：fire-and-forget（notify/setStatus/…）不叫人、
                        // 不亮点，只入队等切回时重放（2026-11-05 真机：微信桥
                        // /监控类服务会话 park 期间猛发 setStatus，全被当阻塞
                        // 件排队，橙点常亮且切回不清）
                        let blocking = req.method.is_blocking();
                        pi_link::client::ext_trace(&format!(
                            "PARKED: id={} key={} blocking={blocking} (active_key={})",
                            req.id,
                            rt.read(cx).key,
                            chat.pool.active_key
                        ));
                        rt.update(cx, |r, _| r.ext_queue.push(req.clone()));
                        if blocking {
                            // 每请求只叫一次（claimExtensionAttentionNotification
                            // parity）；切回兑现后不再打扰
                            let fresh =
                                rt.update(cx, |r, _| r.notified_ext_ids.insert(req.id.clone()));
                            if fresh {
                                let label = Self::session_label_of(chat, &rt.read(cx).key);
                                chat.set_status(format!("后台会话「{label}」在等你回答问题"), cx);
                            }
                        }
                        cx.notify();
                    }
                }
                SessionEvent::Toast(msg) => {
                    // runtime 层用户动作终局的可见出口（compact 完成/失败等；
                    // r.status 无渲染点）。仅活跃会话弹 toast，后台会话的
                    // 状态变化等切换时从状态行看。
                    if is_active {
                        chat.set_status(msg.clone(), cx);
                    }
                }
                SessionEvent::ExtUiDied => {
                    // pi 进程死亡（zi2s.3）：在屏扩展弹窗/问卷卡清空并对死管道
                    // 逐个回 cancelled（handler promise 落地）。弹窗只会来自
                    // 活跃会话（ExtUi 同一闸），后台会话死亡无需动 aux。
                    if is_active {
                        chat.ext_cancel_all(cx);
                    }
                }
                SessionEvent::PlanPresent { plan } => {
                    // 031-1 P3：present_plan 到达 → 开审批弹窗。同一时刻仅一
                    // 份挂起裁决（弹窗开着时后来的丢弃）；后台会话不弹（切
                    // 回时若裁决仍挂起，模型侧 present_plan 还挂着，但 PF 不
                    // 重放工具事件——用户切回可在输入框直接发话，裁决走
                    // /plan-approve 命令仍可完成。P4 待验收项）。
                    if is_active && chat.dlg.plan.approval.is_none() {
                        chat.dlg.plan.approval = Some(plan.clone());
                        cx.notify();
                    }
                }
                SessionEvent::Models(models) => {
                    // 进程答案写项目槽（pi-web /api/models parity：每个 runtime 只写
                    // 自己 cwd 的条目），同时**并进全局态并回写自有缓存**
                    // （010-启动.md §2/§4.1：包内联 provider 只有进程答得到，
                    // 存下来下次冷启动就有真名字）。
                    let cwd_key = rt.read(cx).cwd.to_string_lossy().to_string();
                    chat.pool.models_by_cwd.insert(cwd_key, models.clone());
                    // 首个实时答复：先淘汰缓存幽灵（已从 models.json 删除、静态
                    // 注册表也没有、实时答复也不认的商——并集合并永远删不掉它们，
                    // 还会落进设置页「扩展 provider」兜底桶上组恒入列）。
                    let mut purged = false;
                    if chat.pool.globals.catalog_provisional {
                        let doc = crate::Chat::mj_doc();
                        let mut keep: std::collections::HashSet<String> =
                            pi_link::models_json::providers(&doc)
                                .into_iter()
                                .map(|(p, _)| p)
                                .collect();
                        if let Some(dump) = chat.cfg.provider_registry.as_deref() {
                            keep.extend(dump.providers.iter().map(|p| p.id.clone()));
                        }
                        purged =
                            crate::startup::purge_ghosts(&mut chat.pool.globals, &models, &keep);
                        chat.pool.globals.catalog_provisional = false;
                    }
                    // 幽灵被删也要回写——否则并集无新增时缓存文件里的幽灵留到下次冷启动
                    if crate::startup::merge_models(&mut chat.pool.globals, &models) || purged {
                        let _ = crate::startup::write_cache(
                            &chat.pool.globals.models,
                            &chat.pool.globals.commands,
                        );
                    }
                    chat.cfg.mc_state = models_config::compute_state(
                        chat.cfg.mc_patterns.as_ref(),
                        &chat.mc_refs(),
                    );
                    cx.notify();
                }
                SessionEvent::Commands(commands) => {
                    // 扩展命令（包内注册，磁盘没有数据文件）：并进全局清单 + 回写缓存。
                    // `skill:` 前缀不入全局（skill 走磁盘派生，见 startup::merge_commands）。
                    if crate::startup::merge_commands(&mut chat.pool.globals, &commands) {
                        let _ = crate::startup::write_cache(
                            &chat.pool.globals.models,
                            &chat.pool.globals.commands,
                        );
                    }
                    cx.notify();
                }
                SessionEvent::FileBound(path) => {
                    chat.on_file_bound(&rt, path.clone(), cx);
                }
                SessionEvent::RenameReady(prefill) => {
                    if is_active {
                        if let Some(f) = rt.read(cx).file.clone() {
                            chat.start_rename(f, prefill.clone(), cx);
                        }
                    }
                }
                SessionEvent::RecallInput { text, images } => {
                    // abort 撤回的排队消息回填输入框（pi-web restoring-to-editor
                    // parity）：文本置于已有草稿之前（撤回的比正在写的旧）。
                    // 活跃会话以 composer 为真值源回填可见输入框；后台会话
                    // 没有 composer 镜像，吞进 runtime 草稿，切回时随
                    // switch_to 带回。images = 090 本地队列撤回的附件（pi 侧
                    // 撤回恒空）——runtime 已吞，活跃会话把 composer 附件条
                    // 也拼上（前置，不清用户已有）。
                    rt.update(cx, |r, _| {
                        if !text.is_empty() {
                            r.input = if r.input.is_empty() {
                                text.clone()
                            } else {
                                format!("{text}\n\n{}", r.input)
                            };
                        }
                    });
                    if is_active {
                        if !text.is_empty() {
                            let draft = chat.composer.input.clone();
                            let merged = if draft.is_empty() {
                                text.clone()
                            } else {
                                format!("{text}\n\n{draft}")
                            };
                            chat.set_input(merged, cx);
                        }
                        if !images.is_empty() {
                            let mut imgs = images.clone();
                            imgs.extend(chat.composer.pending_images.drain(..));
                            chat.composer.pending_images = imgs;
                        }
                    }
                }
            }
        })
        .detach();
    }

    /// Attention switch: flush the editor mirrors into the outgoing
    /// session, load the incoming one's inputPanel state. Zero process
    /// operations, zero IO — this is the pi-web "switch" and it is instant.
    pub(crate) fn switch_to(
        &mut self,
        rt: gpui::Entity<session::runtime::SessionRuntime>,
        cx: &mut Context<Self>,
    ) {
        if let Some(old) = self.pool.runtimes.get(&self.pool.active_key).cloned() {
            if old != rt {
                pi_link::client::ext_trace(&format!(
                    "activate: key={} (prev={})",
                    rt.read(cx).key,
                    self.pool.active_key
                ));
                let (input, images, history) = (
                    std::mem::take(&mut self.composer.input),
                    std::mem::take(&mut self.composer.pending_images),
                    std::mem::take(&mut self.composer.history),
                );
                // 草稿页切走即落盘（持久化 flush 点，2026-11-07）：出场的
                // 未晋级草稿把刚换下去的输入写进草稿文件，重启不丢；
                // FileBound 会话不落（输入属会话本体，随文件走）
                let draft_out = {
                    let r = old.read(cx);
                    (r.file.is_none() && !r.observer).then(|| r.cwd.to_string_lossy().to_string())
                };
                if let Some(cwd) = draft_out {
                    pi_link::drafts::save(&cwd, &input);
                }
                old.update(cx, |r, _| {
                    r.input = input;
                    r.pending_images = images;
                    r.history = history;
                });
            }
        }
        let (input, images, history, key, file) = rt.update(cx, |r, _| {
            (
                r.input.clone(),
                r.pending_images.clone(),
                r.history.clone(),
                r.key.clone(),
                r.file.clone(),
            )
        });
        self.composer.input = input;
        self.composer.pending_images = images;
        self.composer.history = history;
        self.composer.history_ix = None;
        self.pool.active_key = key.clone();
        // tab 姿态（044-0 验收②）：普通会话 = 主会话指针跟进 + 标题快照刷
        // 新 + 自由区全灭（chat 前台高亮归固定段）；observer 观察窗 = 激活
        // 自由区对应 tab（open_session 保证已在 panel_tabs），主会话指针
        // 不动——固定段永远显示主会话/新会话。
        let observer = rt.read(cx).observer;
        if observer {
            if let Some(ix) = self
                .ws
                .panel_tabs
                .iter()
                .position(|t| matches!(t, crate::PanelTab::Observer { key: k } if *k == key))
            {
                self.ws.active_panel_tab = Some(ix);
            }
        } else {
            self.pool.primary_key = key.clone();
            // 标题快照只写真值，兜底词绝不入快照（与 sync_primary_tab_label
            // 同规）：启动恢复路 open_session→switch_to 时 psp 分组/清单尚
            // 未建，psp_title_of 取不到——旧代码把「新会话」写进快照钉死
            // tab，直到下一次 ListDirty（发消息）才有机会自愈（2026-11-07
            // 用户复发报障）。留空 → 渲染回落活值同词，清单重建后随时可被
            // sync 治愈。
            if let Some(raw) = file
                .as_deref()
                .and_then(|f| self.psp_title_of(std::path::Path::new(f)))
            {
                self.pool
                    .tab_labels
                    .insert(key.clone(), super::tab_title(&raw));
            }
            self.ws.active_panel_tab = None;
        }
        self.psp.active_file = file.clone();
        // 注意力切到会话 ⇒ 内容区必须回到 chat 前台：否则在终端/文件编辑
        // 视图里点会话列表行（或「新会话」）后，内容区还停在 Term/File，
        // 看着像「切了会话但窗口没切过来」。browse_last 不受影响，
        // statusbar 的 files tab 仍能回到终端/文件（set_content_view 只在
        // Term/File 时更新它）。
        if self.layout.content_view != crate::ContentView::Chat {
            self.set_content_view(crate::ContentView::Chat);
        }
        if let Some(f) = &file {
            // 无项目态不存在可切会话（switch_pool_rt 已先重挂），防御式取 Some。
            // observer 观察窗不写 last-open：子会话不是恢复目标（044-0 验收
            // ①教训——旧版污染后启动会恢复成子会话，父 catalog 无从解析）
            if let Some(cwd) = &self.ws.cwd {
                if !observer && !self.plugin_claimed(&f.to_string_lossy()) {
                    set_last_open(&cwd.to_string_lossy(), &f.to_string_lossy());
                }
            }
            // v54: 切入会话清除未读/出错红点
            self.psp.unread.remove(f);
            self.psp.turn_errors.remove(f);
            self.psp.turn_warnings.remove(f);
        }
        self.dlg.pill_menu = None;
        self.composer.menu_ix = 0;
        // 「点外/Esc 收起」不能跨会话泄漏：否则上个会话收起过菜单，切过来
        // 后 / @ 永远弹不出（active_menu 第一行就短路）
        self.composer.menu_dismissed = false;
        // 气泡滚动句柄是 Chat 级共享 map，按渲染过的消息 id 只增不减——
        // 切会话即清（047-内存削减 P0-2）：回旧会话句柄按需重建（user_row
        // 的 or_insert_with），滚动位由 chat_list pager 锚点接管
        self.composer.bubble_scrolls.borrow_mut().clear();
        // surface queued permission requests of the incoming session
        let queued = rt.update(cx, |r, _| {
            let q = std::mem::take(&mut r.ext_queue);
            r.touch();
            q
        });
        if !queued.is_empty() {
            // #问题卡不弹： parked 期间的积压请求在此兑现，留痕对账。
            // 两段式兑现（2026-11-05 黄点修复）：fire-and-forget 立即全部
            // 落地（此刻 active_key 已切到本会话，on_ext_ui 写的 self.rt()
            // 就是本 runtime，不会污染别人）；阻塞件只浮出队首供作答，
            // 其余回队等逐个兑现。旧逻辑「只兑现队首、其余无差别回队」
            // 会把 fire-and-forget 永远留在队里——徽标常亮、空闲退役被卡。
            let (blocking, forget): (Vec<_>, Vec<_>) =
                queued.into_iter().partition(|q| q.method.is_blocking());
            pi_link::client::ext_trace(&format!(
                "switch flush: {} forget + {} blocking (first id={})",
                forget.len(),
                blocking.len(),
                blocking.first().map(|q| q.id.as_str()).unwrap_or("-")
            ));
            for q in forget {
                self.on_ext_ui(q, cx);
            }
            if let Some(first) = blocking.first() {
                let first = first.clone();
                cx.notify();
                self.on_ext_ui(first, cx);
            }
            let rest = blocking.into_iter().skip(1).collect::<Vec<_>>();
            if !rest.is_empty() {
                rt.update(cx, |r2, _| r2.ext_queue.extend(rest));
            }
            cx.notify();
        }
        // 子代理面板缓存随会话激活常备（open=false）：徽标从切换起就准，
        // 不依赖先开过一次面板（2026-10-19 拍板：关面板只是隐藏，轮询常驻）。
        // 模块运行时开关关着不建缓存不起轮询（零 IO）。
        if crate::services::workspace::subagent_module_active() {
            self.ensure_subagent_panel(cx);
        }
        cx.notify();
    }
}
