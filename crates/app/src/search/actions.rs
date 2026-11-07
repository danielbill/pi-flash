//! 搜索动作：入口开合、输入框懒建、发起/防抖/流式回填（入口与发起搜索段）。

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::Context;

use super::engine::{FileHit, SearchMsg};
use super::*;
use crate::*;

use super::DEBOUNCE_MS;

impl Chat {
    // -------------------------------------------------------------------
    // 入口
    // -------------------------------------------------------------------

    /// Ctrl+L / 文件树「搜索文件」钮：打开搜索面板（单例
    /// tab），已打开则聚焦查询框。**替换掉了原来的「搜索文件」弹窗**
    /// （`Dialog::FileSearch`，2026-10-21 用户定案：一个入口，文件+内容）。
    pub(crate) fn open_project_search(&mut self, cx: &mut Context<Self>) {
        self.ensure_search_inputs(cx);
        // 焦点**当帧**交给查询框：`TextInput::focus_soon` 的请求在它自己那帧
        // render 里兑现（内件建好即 `InputState::focus`，光标 blink 同帧启动）。
        // 原来的「置 flag → 下一帧由 panel 补推焦点」会让光标晚一两帧才出现，
        // 用户实测「打开时像卡顿、像丢了焦点」（2026-10-21 定案）。
        if let Some(q) = self.ws.search.query.clone() {
            q.update(cx, |input, cx| input.focus_soon(cx));
        }
        let existing = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::Search));
        match existing {
            Some(ix) => {
                self.activate_panel_tab(ix, cx);
                self.set_content_view(ContentView::Search);
            }
            None => {
                self.ws.panel_tabs.push(PanelTab::Search);
                let ix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
                self.set_content_view(ContentView::Search);
            }
        }
        cx.notify();
    }

    /// 关搜索 tab（头上 ✕ / Esc / 标签 ×）：搜索态保留，再开还是上次的查询。
    pub(crate) fn close_search_tab(&mut self, cx: &mut Context<Self>) {
        let Some(ix) = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::Search))
        else {
            return;
        };
        self.ws.search.cancel.store(true, Ordering::Relaxed);
        self.ws.search.pending = None;
        self.ws.search.debounce = None;
        self.close_panel_tab(ix, cx);
        if self.layout.content_view == ContentView::Search {
            self.fallback_after_closes(cx);
        }
        cx.notify();
    }

    /// 懒建四个输入框（`TextInput::new` 不需要 Window，可在动作里建）。
    pub(super) fn ensure_search_inputs(&mut self, cx: &mut Context<Self>) {
        if self.ws.search.query.is_some() {
            return;
        }
        let weak_q = cx.weak_entity();
        let weak_enter = cx.weak_entity();
        let weak_esc = cx.weak_entity();
        let query = cx.new(|cx| {
            let mut ti = TextInput::new(cx)
                .flat()
                .placeholder(tr("search.placeholder"))
                .on_change(Box::new(move |q: &str, cx: &mut App| {
                    let q = q.to_string();
                    let _ = weak_q.update(cx, |c, cx| {
                        c.ws.search.query_text = q;
                        c.search_debounced(cx);
                    });
                }))
                .on_escape(Box::new(move |cx: &mut App| {
                    // Esc = 收起搜索（defer 出帧再关：关 tab 会 drop 正在派发
                    // 事件的输入框实体）
                    let weak = weak_esc.clone();
                    cx.defer(move |cx| {
                        let _ = weak.update(cx, |c, cx| c.close_search_tab(cx));
                    });
                }));
            // Enter = 立即搜索（Zed ConfirmSearch：不等防抖）
            ti.set_on_submit(Box::new(move |_q: &str, cx: &mut App| {
                let _ = weak_enter.update(cx, |c, cx| c.search_start(cx));
            }));
            ti
        });
        let weak_r = cx.weak_entity();
        let replace = cx.new(|cx| {
            TextInput::new(cx)
                .flat()
                .placeholder(tr("search.replace_placeholder"))
                .on_change(Box::new(move |q: &str, cx: &mut App| {
                    // 替换词只影响「替换」动作，不触发重搜（Zed 同）
                    let q = q.to_string();
                    let _ = weak_r.update(cx, |c, cx| {
                        c.ws.search.replace_text = q;
                        c.ws.search.notice = None;
                        cx.notify();
                    });
                }))
        });
        let weak_i = cx.weak_entity();
        let include = cx.new(|cx| {
            TextInput::new(cx)
                .flat()
                .placeholder(tr("search.include_placeholder"))
                .on_change(Box::new(move |_q: &str, cx: &mut App| {
                    let _ = weak_i.update(cx, |c, cx| c.search_debounced(cx));
                }))
        });
        let weak_e = cx.weak_entity();
        let exclude = cx.new(|cx| {
            TextInput::new(cx)
                .flat()
                .placeholder(tr("search.exclude_placeholder"))
                .on_change(Box::new(move |_q: &str, cx: &mut App| {
                    let _ = weak_e.update(cx, |c, cx| c.search_debounced(cx));
                }))
        });
        let ui = &mut self.ws.search;
        ui.query = Some(query);
        ui.replace = Some(replace);
        ui.include = Some(include);
        ui.exclude = Some(exclude);
    }

    // -------------------------------------------------------------------
    // 发起搜索
    // -------------------------------------------------------------------

    /// 防抖重搜（输入变更路径；Zed `schedule_search_on_type` 同构）。旧定时器
    /// 随任务 drop 取消——逐字输入不会排出 N 轮搜索。
    fn search_debounced(&mut self, cx: &mut Context<Self>) {
        let weak = cx.weak_entity();
        self.ws.search.debounce = Some(cx.spawn(async move |_, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(DEBOUNCE_MS))
                .await;
            let _ = weak.update(cx, |c, cx| c.search_start(cx));
        }));
    }

    /// 立即搜索（Enter / 防抖到点 / 开关变化）。
    pub(crate) fn search_start(&mut self, cx: &mut Context<Self>) {
        self.ensure_search_inputs(cx);
        let ui = &mut self.ws.search;
        // 旧的一轮先取消（旗标 + drop 泵任务；后台循环每文件查一次旗标）
        ui.cancel.store(true, Ordering::Relaxed);
        ui.pending = None;
        ui.generation += 1;
        let generation = ui.generation;
        ui.cancel = Arc::new(AtomicBool::new(false));
        ui.blocks = Arc::new(Vec::new());
        // 新查询 → 收起状态作废（旧展开/收起对不上新结果集）
        ui.collapsed = Arc::new(std::collections::HashSet::new());
        ui.list.reset(0);
        ui.files = 0;
        ui.path_files = 0;
        ui.matches = 0;
        ui.scanned = 0;
        ui.active = None;
        ui.limit = false;
        ui.error = None;
        ui.notice = None;

        let text = ui.query_text.clone();
        if text.is_empty() {
            ui.phase = Phase::Idle;
            cx.notify();
            return;
        }
        let Some(cwd) = self.ws.cwd.clone() else {
            ui.phase = Phase::Idle;
            ui.error = Some(tr("search.no_project").to_string());
            cx.notify();
            return;
        };
        let include = split_globs(
            ui.include
                .as_ref()
                .map(|e| e.read(cx).value().to_string())
                .unwrap_or_default(),
        );
        let exclude = split_globs(
            ui.exclude
                .as_ref()
                .map(|e| e.read(cx).value().to_string())
                .unwrap_or_default(),
        );
        let opts = ui.opts;
        let query = match build_query(&text, opts, include, exclude) {
            Ok(q) => q,
            Err(e) => {
                ui.phase = Phase::Idle;
                ui.error = Some(e);
                cx.notify();
                return;
            }
        };
        ui.phase = Phase::Running;
        let cancel = ui.cancel.clone();
        let (tx, mut rx) = futures::channel::mpsc::unbounded::<SearchMsg>();
        cx.background_spawn(async move {
            engine::run_search(cwd, query, opts, cancel, tx);
        })
        .detach();

        // 前台泵：收拢批次，一帧一次 notify（避免逐文件重绘）
        self.ws.search.pending = Some(cx.spawn(async move |weak, cx| {
            use futures::StreamExt as _;
            while let Some(first) = rx.next().await {
                let mut batch = vec![first];
                while let Ok(more) = rx.try_recv() {
                    batch.push(more);
                }
                let mut done = false;
                let alive = weak
                    .update(cx, |chat, cx| {
                        if chat.ws.search.generation != generation {
                            // 陈旧批次：整批丢弃（新一轮已接管）
                            return false;
                        }
                        for msg in batch {
                            done |= chat.search_apply(msg);
                        }
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !alive || done {
                    break;
                }
            }
        }));
        cx.notify();
    }

    /// 消费一条后台消息；返回 true = 本轮结束。
    fn search_apply(&mut self, msg: SearchMsg) -> bool {
        let ui = &mut self.ws.search;
        match msg {
            SearchMsg::PathHits(paths) => {
                ui.path_files = paths.len();
                let blocks = Arc::make_mut(&mut ui.blocks);
                for rel in paths {
                    blocks.push(ResultBlock {
                        rel,
                        path_only: true,
                        count: 0,
                        segments: Vec::new(),
                    });
                }
                self.sync_search_list();
                false
            }
            SearchMsg::Hit(hit) => {
                self.search_push_hit(hit);
                false
            }
            SearchMsg::Limit => {
                ui.limit = true;
                false
            }
            SearchMsg::Done { scanned } => {
                ui.scanned = scanned;
                ui.phase = Phase::Done;
                // 首个命中自动选中（Zed `update_match_index` 同义：完成即有
                // active，prev/next 才有起点）
                if ui.active.is_none() && ui.matches > 0 {
                    ui.active = Some(0);
                }
                true
            }
        }
    }

    fn search_push_hit(&mut self, hit: FileHit) {
        let ui = &mut self.ws.search;
        ui.files += 1;
        // 显示规则（2026-10-21 用户定案）：**标题命中且内容也命中 → 只显示
        // 内容组；只有标题命中才单独显示文件栏**。路径命中在扫描前就发过来了
        // （顶部文件栏），这里把同名的 path-only 块摘掉，避免同一个文件出现
        // 两次（Zed 也是这样：它知道两者都命中就不重复展示）。
        if ui.blocks.iter().any(|b| b.path_only && b.rel == hit.rel) {
            let blocks = Arc::make_mut(&mut ui.blocks);
            drop_path_only_for(blocks, &hit.rel);
            ui.path_files = ui.path_files.saturating_sub(1);
            self.sync_search_list();
        }
        let ui = &mut self.ws.search;
        let mut block = ResultBlock {
            rel: hit.rel.clone(),
            path_only: false,
            count: hit.hits.len(),
            segments: Vec::with_capacity(hit.segments.len()),
        };
        for segment in &hit.segments {
            let mut lines = Vec::with_capacity(segment.len());
            for (line, text) in segment {
                // 命中行集合里有它才是命中行（上下文行同段混排）
                let spans = hit
                    .hits
                    .iter()
                    .find(|h| h.line == *line)
                    .map(|h| h.spans.clone())
                    .unwrap_or_default();
                let is_hit = !spans.is_empty();
                let ordinal = is_hit.then(|| {
                    let o = ui.matches;
                    ui.matches += 1;
                    o
                });
                lines.push(ResultLine {
                    line: *line,
                    text: text.clone(),
                    spans,
                    hit: is_hit,
                    ordinal,
                });
            }
            block.segments.push(lines);
        }
        Arc::make_mut(&mut ui.blocks).push(block);
        self.sync_search_list();
    }

    /// 文件头左侧的收展钮：收起 = 只留文件头（内容行不渲染）。状态按 `rel`
    /// 记，切查询也不串门；只重测这一块（`ListState::splice` 同数量替换 =
    /// 逼 gpui 重新量高，否则收起后列表还按旧高度留白）。
    pub(crate) fn search_toggle_block(&mut self, block_ix: usize, cx: &mut Context<Self>) {
        let ui = &mut self.ws.search;
        let Some(rel) = ui.blocks.get(block_ix).map(|b| b.rel.clone()) else {
            return;
        };
        let set = Arc::make_mut(&mut ui.collapsed);
        if !set.remove(&rel) {
            set.insert(rel);
        }
        ui.list.splice(block_ix..block_ix + 1, 1);
        cx.notify();
    }

    /// 结果块数变化 → 同步 `ListState`（块数变了才动，滚动位按 Zed 的
    /// 阅读锚点语义保留：追加在尾部时不动视口）。
    fn sync_search_list(&mut self) {
        let ui = &mut self.ws.search;
        let count = ui.blocks.len();
        if ui.list.item_count() == count {
            return;
        }
        if ui.list.item_count() == 0 || ui.list.logical_scroll_top().item_ix == 0 {
            ui.list.reset(count);
        } else {
            let anchor = ui.list.logical_scroll_top();
            ui.list.reset(count);
            ui.list.scroll_to(gpui::ListOffset {
                item_ix: anchor.item_ix.min(count.saturating_sub(1)),
                offset_in_item: anchor.offset_in_item,
            });
        }
    }

    /// 搜索条任一输入框是否持焦——render 每帧焦点仲裁的白名单判据
    /// （同 040-1 市场搜索框/023 文件编辑器：漏了每帧被推回中性锚点，
    /// 搜索框打不进字）。
    pub(crate) fn search_input_focused(&self, window: &gpui::Window, cx: &gpui::App) -> bool {
        let ui = &self.ws.search;
        // 会话查询（PanelTab::SessionSearch）的查询框同享白名单：它的面板与
        // 项目搜索同版式，焦点仲裁走同一条判据
        [
            &ui.query,
            &ui.replace,
            &ui.include,
            &ui.exclude,
            &self.psp.session_query,
        ]
        .iter()
        .any(|slot| {
            slot.as_ref().is_some_and(|e| {
                e.read(cx).has_inner() && e.read(cx).focus_handle_in(cx).is_focused(window)
            })
        })
    }
}
