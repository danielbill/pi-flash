//! 083 项目搜索——Zed `crates/search/src/project_search.rs` 面板的 PF 复刻。
//!
//! 三层职责：
//! - [`query`]：**Zed 原码**（`crates/project/src/search.rs` + `util/paths.rs`
//!   的 `PathMatcher`），逐字抄，只做 `RelPath`→`PathBuf` 的机械替换；
//! - [`engine`]：执行层（候选路径 → `match_path` 过滤 → 读盘扫 → 流式回传），
//!   函数逐条对照 Zed `project_search.rs`；
//! - [`panel`] / [`results`]：搜索条（Zed `ProjectSearchBar::render` 版式）与
//!   结果面板（Zed 多缓冲结果编辑器的视觉等价物）；
//! - [`sessions`]：**会话查询**（2026-10-21 把 013 的弹窗版改成同版式面板，
//!   只查询、不替换/过滤）——组头复用 [`results::card_header`]，正文同一条
//!   单色 + 命中底色口径。见 `docs/模块设计/084-会话查询.md`。
//!
//! 产品级唯一差异：Zed 的「找文件」（Ctrl+P 文件查找器）与「找内容」
//! （Ctrl+Shift+F 项目搜索）是两个面板，PF 合并进同一结果列表——因为它替换
//! 掉的正是原来那个「搜索文件」弹窗（Ctrl+L），用户的心智是「一次输入、文件
//! 与内容一起搜」。文件名命中排最前的独立分组（[`engine::MAX_PATH_HITS`] 封顶）。
//!
//! 全部状态住 [`SearchUi`]（挂在 `WsUi.search`）：搜索是工作区级状态，切会话
//! 不丢、切项目重搜。

pub(crate) mod engine;
mod panel;
mod query;
mod results;
mod sessions;

pub(crate) use panel::{opts_labels, view};
pub(crate) use query::PathMatcher;
pub(crate) use sessions::view as session_view;

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{Context, Task};

use crate::*;
use engine::{FileHit, SearchMsg, SearchOpts};
use query::SearchQuery;

/// 结果列表行高（与 Zed 结果编辑器一致：命中等高行）。
pub(crate) const LINE_H: f32 = 22.;
/// 文件组头**块**行高 = Zed `FILE_HEADER_HEIGHT`(2 行)；条 = 块 − 2×4px 留白
pub(crate) const HEADER_H: f32 = LINE_H * 2.;
/// 搜索联动的防抖（Zed `SEARCH_ON_TYPE_DEBOUNCE` = 250ms）
const DEBOUNCE_MS: u64 = 250;

/// 搜索阶段（Zed `SearchState` 的简化版：PF 没有 Remote/OpenBuffersOnly）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Phase {
    Idle,
    Running,
    Done,
}

/// 一个文件的结果块（Zed 一个 Multibuffer excerpt 组的等价物）。
#[derive(Clone, Debug)]
pub(crate) struct ResultBlock {
    /// 相对 cwd 的 `/` 分隔路径
    pub(crate) rel: String,
    /// 文件名命中（该文件内容里没有命中，只有路径匹配）
    pub(crate) path_only: bool,
    /// 命中行数（path_only 时为 0）
    pub(crate) count: usize,
    /// 摘录段（每段若干连续行；多段 = 一段代码里的多个命中窗口）
    pub(crate) segments: Vec<Vec<ResultLine>>,
}

/// 摘录里的一行。
#[derive(Clone, Debug)]
pub(crate) struct ResultLine {
    /// 1-based 行号
    pub(crate) line: u32,
    pub(crate) text: String,
    /// 行内命中区间（byte）
    pub(crate) spans: Vec<Range<usize>>,
    /// 命中行 = true；上下文行 = false
    pub(crate) hit: bool,
    /// 全场命中序号（0-based；上下文行为 None）
    pub(crate) ordinal: Option<usize>,
}

/// 项目搜索的界面状态。
pub(crate) struct SearchUi {
    /// 四个输入框（首次进入搜索面板时懒建，跨开关复用）
    pub(crate) query: Option<gpui::Entity<TextInput>>,
    pub(crate) replace: Option<gpui::Entity<TextInput>>,
    pub(crate) include: Option<gpui::Entity<TextInput>>,
    pub(crate) exclude: Option<gpui::Entity<TextInput>>,
    pub(crate) opts: SearchOpts,
    /// 查询词镜像（`on_change` 同步；渲染帧不读输入框实体）
    pub(crate) query_text: String,
    /// 替换词镜像（同上）
    pub(crate) replace_text: String,
    pub(crate) show_replace: bool,
    pub(crate) show_filters: bool,
    pub(crate) phase: Phase,
    /// 结果块模型（跨帧只克隆 Arc；追加走 Arc::make_mut）
    pub(crate) blocks: Arc<Vec<ResultBlock>>,
    /// 被收起（只留文件头）的块：**按相对路径记**——块下标会随查询重排，
    /// 路径不会（Zed 的收起也是挂在 excerpt 头上，不跟序号走）
    pub(crate) collapsed: Arc<std::collections::HashSet<String>>,
    /// 结果列表状态（变高块列表：每块 = 文件头 + 若干行）
    pub(crate) list: gpui::ListState,
    /// 结果区竖向滚动条状态（gpui-component `Scrollbar`；与 md 预览同款）
    pub(crate) scrollbar: gpui_component::scroll::ScrollbarState,
    /// 命中的内容文件数 / 文件名命中数 / 命中行总数 / 已读盘文件数
    pub(crate) files: usize,
    pub(crate) path_files: usize,
    pub(crate) matches: usize,
    pub(crate) scanned: usize,
    /// 当前选中的命中序号（对应 `ResultLine.ordinal`）
    pub(crate) active: Option<usize>,
    /// 查询/过滤器的编译错误
    pub(crate) error: Option<String>,
    /// 触到结果上限（计数后缀 `+`）
    pub(crate) limit: bool,
    /// 替换结果提示（文本, 是否失败）
    pub(crate) notice: Option<(String, bool)>,
    /// 代际：每次发起搜索 +1，陈旧批次按代际丢弃
    pub(crate) generation: u64,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) pending: Option<Task<()>>,
    /// 防抖任务（新输入 drop 旧任务 = 取消旧定时器）
    pub(crate) debounce: Option<Task<()>>,
}

impl Default for SearchUi {
    fn default() -> Self {
        Self {
            query: None,
            replace: None,
            include: None,
            exclude: None,
            opts: SearchOpts::default(),
            query_text: String::new(),
            replace_text: String::new(),
            show_replace: false,
            show_filters: false,
            phase: Phase::Idle,
            blocks: Arc::new(Vec::new()),
            collapsed: Arc::new(std::collections::HashSet::new()),
            list: gpui::ListState::new(0, gpui::ListAlignment::Top, px(1000.)),
            scrollbar: gpui_component::scroll::ScrollbarState::default(),
            files: 0,
            path_files: 0,
            matches: 0,
            scanned: 0,
            active: None,
            error: None,
            limit: false,
            notice: None,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            pending: None,
            debounce: None,
        }
    }
}

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
    fn ensure_search_inputs(&mut self, cx: &mut Context<Self>) {
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

    // -------------------------------------------------------------------
    // 开关 / 导航 / 跳转
    // -------------------------------------------------------------------

    /// 自动化入口：设置查询词并立即开搜（UI 测试 `search.query`）。
    pub(crate) fn set_search_query(&mut self, text: String, cx: &mut Context<Self>) {
        self.ensure_search_inputs(cx);
        self.ws.search.query_text = text.clone();
        if let Some(q) = self.ws.search.query.clone() {
            q.update(cx, |s, cx| s.set_value(text, cx));
        }
        self.open_project_search(cx);
    }

    /// 自动化入口：按名字切开关（UI 测试 `search.toggle`）。
    pub(crate) fn search_toggle_by_name(&mut self, which: &str, cx: &mut Context<Self>) {
        let w = match which {
            "case_sensitive" => SearchToggle::CaseSensitive,
            "whole_word" => SearchToggle::WholeWord,
            "regex" => SearchToggle::Regex,
            "include_ignored" => SearchToggle::IncludeIgnored,
            "filters" => SearchToggle::Filters,
            "replace" => SearchToggle::Replace,
            _ => return,
        };
        self.search_toggle(w, cx);
    }

    /// 三个匹配开关（大小写 / 整词 / 正则）+ 过滤/替换行开关。切匹配项立刻
    /// 重搜（Zed 同）。
    pub(crate) fn search_toggle(&mut self, which: SearchToggle, cx: &mut Context<Self>) {
        let ui = &mut self.ws.search;
        match which {
            SearchToggle::CaseSensitive => ui.opts.case_sensitive = !ui.opts.case_sensitive,
            SearchToggle::WholeWord => ui.opts.whole_word = !ui.opts.whole_word,
            SearchToggle::Regex => ui.opts.regex = !ui.opts.regex,
            SearchToggle::IncludeIgnored => ui.opts.include_ignored = !ui.opts.include_ignored,
            SearchToggle::Filters => {
                ui.show_filters = !ui.show_filters;
                let show = ui.show_filters;
                if !show {
                    // 关过滤器 = 清空 include/exclude 并重搜：输入框隐藏后还留着
                    // 条件会让结果莫名变少
                    if let Some(inc) = ui.include.clone() {
                        inc.update(cx, |s, cx| s.set_value(String::new(), cx));
                    }
                    if let Some(exc) = ui.exclude.clone() {
                        exc.update(cx, |s, cx| s.set_value(String::new(), cx));
                    }
                    self.search_start(cx);
                } else {
                    cx.notify();
                }
                return;
            }
            SearchToggle::Replace => {
                ui.show_replace = !ui.show_replace;
                cx.notify();
                return;
            }
        }
        self.search_start(cx);
    }

    /// 命中序号从 (block_ix, segment_ix, line_ix) 反查（导航用）。
    fn search_ordinal_positions(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (block_ix, block) in self.ws.search.blocks.iter().enumerate() {
            for segment in &block.segments {
                for line in segment {
                    if let Some(o) = line.ordinal {
                        out.push((block_ix, o));
                    }
                }
            }
        }
        out
    }

    /// prev/next 命中（Zed SelectNextMatch/SelectPreviousMatch，默认环绕）。
    pub(crate) fn search_step(&mut self, forward: bool, cx: &mut Context<Self>) {
        let positions = self.search_ordinal_positions();
        if positions.is_empty() {
            return;
        }
        let pos = self
            .ws
            .search
            .active
            .and_then(|a| positions.iter().position(|(_, o)| *o == a));
        let next = match (pos, forward) {
            (None, true) => 0,
            (None, false) => positions.len() - 1,
            (Some(i), true) => (i + 1) % positions.len(),
            (Some(i), false) => (i + positions.len() - 1) % positions.len(),
        };
        let (block_ix, ordinal) = positions[next];
        self.ws.search.active = Some(ordinal);
        self.scroll_to_block(block_ix);
        cx.notify();
    }

    /// 把某个结果块滚进视口（块内偏移 0 = 文件头可见，Zed 结果编辑器
    /// 选中命中时的最小滚动同义）。
    fn scroll_to_block(&mut self, block_ix: usize) {
        let list = self.ws.search.list.clone();
        list.scroll_to(gpui::ListOffset {
            item_ix: block_ix,
            offset_in_item: px(0.),
        });
    }

    /// 打开一个结果块对应的文件：`line` 为 0 表示「文件名命中，开到首行」。
    pub(crate) fn search_open_block(
        &mut self,
        block_ix: usize,
        line: u32,
        ordinal: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let Some(block) = self.ws.search.blocks.get(block_ix) else {
            return;
        };
        let rel = block.rel.clone();
        if let Some(o) = ordinal {
            self.ws.search.active = Some(o);
        }
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        let abs = cwd.join(&rel);
        // 行跳转请求：文件 tab 渲染帧消费（view.rs 灌进编辑器光标）
        self.ws.pending_jump = Some((abs.clone(), line.max(1)));
        self.open_file_tab(abs, cx);
        cx.notify();
    }

    // -------------------------------------------------------------------
    // 替换
    // -------------------------------------------------------------------

    /// 替换全部命中（Zed ReplaceAll）。活跃文件有未保存修改时跳过并计数。
    pub(crate) fn search_replace_all(&mut self, cx: &mut Context<Self>) {
        self.search_replace(cx, false);
    }

    /// 替换下一处（Zed ReplaceNext）：只动当前活跃命中所在文件的第一个命中，
    /// 然后重搜并推进选中。
    pub(crate) fn search_replace_next(&mut self, cx: &mut Context<Self>) {
        self.search_replace(cx, true);
    }

    fn search_replace(&mut self, cx: &mut Context<Self>, next_only: bool) {
        let text = self.ws.search.query_text.clone();
        let replacement = self.ws.search.replace_text.clone();
        if text.is_empty() {
            return;
        }
        let include = split_globs(
            self.ws
                .search
                .include
                .as_ref()
                .map(|e| e.read(cx).value().to_string())
                .unwrap_or_default(),
        );
        let exclude = split_globs(
            self.ws
                .search
                .exclude
                .as_ref()
                .map(|e| e.read(cx).value().to_string())
                .unwrap_or_default(),
        );
        let Ok(q) = build_query(&text, self.ws.search.opts, include, exclude) else {
            return;
        };
        let q = q.with_replacement(replacement.clone());
        let Some(cwd) = self.ws.cwd.clone() else {
            return;
        };
        // 待改文件：ReplaceAll = 每个内容命中文件；ReplaceNext = 活跃命中所在文件
        let active_block: Option<usize> = if next_only {
            let active = self.ws.search.active;
            let mut found = None;
            for (block_ix, block) in self.ws.search.blocks.iter().enumerate() {
                for segment in &block.segments {
                    for line in segment {
                        if line.ordinal.is_some() && line.ordinal == active {
                            found = Some(block_ix);
                        }
                    }
                }
            }
            found
        } else {
            None
        };
        let mut files: Vec<String> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for (block_ix, block) in self.ws.search.blocks.iter().enumerate() {
            if block.path_only {
                continue;
            }
            if let Some(target) = active_block
                && block_ix != target
            {
                continue;
            }
            if seen.insert(block.rel.clone()) {
                files.push(block.rel.clone());
            }
        }
        if files.is_empty() {
            self.ws.search.notice = Some((tr("search.replace_none").to_string(), true));
            cx.notify();
            return;
        }
        let mut changed_files = 0usize;
        let mut changed_hits = 0usize;
        let mut skipped_dirty = 0usize;
        for rel in &files {
            let abs = cwd.join(rel);
            // 有未保存修改的文件不动（写盘会触发冲突横幅，改动归属不明）
            if self
                .ws
                .file_cache
                .get(&abs)
                .is_some_and(|f| f.dirty || f.conflict.is_some())
            {
                skipped_dirty += 1;
                continue;
            }
            let Ok(bytes) = std::fs::read(&abs) else {
                continue;
            };
            let Ok(decoded) = crate::editor::file_content::decode_text(bytes) else {
                continue;
            };
            let src = decoded.text;
            let (out, hits) = replace_in_text(&q, &src, next_only);
            if hits == 0 {
                continue;
            }
            let encoded = crate::editor::file_content::encode_text(
                out.clone(),
                decoded.encoding,
                decoded.has_bom,
            );
            if std::fs::write(&abs, encoded).is_err() {
                continue;
            }
            changed_files += 1;
            changed_hits += hits;
            // 已开 tab 同步（内容 + 磁盘基准），避免 watcher 把它判成外部改动；
            // 编辑器值由渲染帧经 reload_pending 灌入（set_value 要 &mut Window）
            if let Some(ft) = self.ws.file_cache.get_mut(&abs) {
                ft.content = out;
                ft.conflict = None;
                ft.dirty = false;
                ft.disk_sig = crate::services::workspace::file_sig(&abs);
                if ft.editor.is_some() {
                    ft.reload_pending = true;
                }
            }
            if next_only {
                break;
            }
        }
        let mut msg = crate::i18n::tf(
            "search.replaced",
            &[
                ("files", changed_files.to_string()),
                ("hits", changed_hits.to_string()),
            ],
        );
        if skipped_dirty > 0 {
            msg.push_str(&crate::i18n::tf(
                "search.replace_skipped_dirty",
                &[("n", skipped_dirty.to_string())],
            ));
        }
        let failed = changed_files == 0;
        self.ws.search.notice = Some((msg, failed));
        let advance = next_only && !failed;
        self.search_start(cx);
        if advance {
            self.search_step(true, cx);
        }
        cx.notify();
    }
}

/// 显示规则（纯函数便于单测）：内容命中到达时，摘掉同名的「仅文件名命中」
/// 文件栏——**内容与标题都命中的文件只展示内容组**（2026-10-21 用户定案：
/// 「只有标题命中肯定显示文件栏，内容和标题都命中了就不重复展示」）。
/// 返回是否真的摘掉了东西。
pub(crate) fn drop_path_only_for(blocks: &mut Vec<ResultBlock>, rel: &str) -> bool {
    let before = blocks.len();
    blocks.retain(|b| !(b.path_only && b.rel == rel));
    blocks.len() != before
}

/// include/exclude 输入框 → glob 列表（Zed 用逗号分隔；PF 额外支持换行）。
pub(crate) fn split_globs(text: String) -> Vec<String> {
    text.split([',', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

/// 建查询（Zed `SearchQuery::text` / `regex` 的分流 + `PathMatcher` 构造）。
pub(crate) fn build_query(
    text: &str,
    opts: SearchOpts,
    include: Vec<String>,
    exclude: Vec<String>,
) -> Result<SearchQuery, String> {
    let is_posix = !cfg!(windows);
    let inc = PathMatcher::new(include, is_posix).map_err(|e| e.to_string())?;
    let exc = PathMatcher::new(exclude, is_posix).map_err(|e| e.to_string())?;
    let built = if opts.regex {
        SearchQuery::regex(
            text,
            opts.whole_word,
            opts.case_sensitive,
            false,
            inc,
            exc,
            false,
        )
    } else {
        SearchQuery::text(
            text,
            opts.whole_word,
            opts.case_sensitive,
            false,
            inc,
            exc,
            false,
        )
    };
    built.map_err(|e| e.to_string())
}

/// 对整段文本做替换，返回 (新文本, 替换处数)。
///
/// 逐行调用 Zed 的 `replacement_for`（它按命中区间精确重排捕获组，等价于
/// Zed 结果编辑器里的 `ReplaceAll` 施加在每个 range 上），再拼回文本——这样
/// `$1`/`\n` 等语义与 Zed 完全一致。
fn replace_in_text(q: &SearchQuery, src: &str, next_only: bool) -> (String, usize) {
    let mut out = String::with_capacity(src.len());
    let mut count = 0usize;
    let mut done = false;
    // split_inclusive 让每行（含行尾换行）原样拼回：CRLF 与「末尾无换行」
    // 都不会被改写
    for chunk in src.split_inclusive('\n') {
        let (raw, had_lf) = match chunk.strip_suffix('\n') {
            Some(rest) => (rest, true),
            None => (chunk, false),
        };
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let crlf = raw.len() != line.len();
        if done {
            out.push_str(chunk);
            continue;
        }
        let spans = q.search_str(line);
        if spans.is_empty() {
            out.push_str(chunk);
            continue;
        }
        let mut cursor = 0usize;
        for span in spans {
            out.push_str(&line[cursor..span.start]);
            match q.replacement_for(line, span.clone()) {
                Some(rep) => out.push_str(&rep),
                None => out.push_str(&line[span.clone()]),
            }
            cursor = span.end;
            count += 1;
            if next_only {
                done = true;
                break;
            }
        }
        out.push_str(&line[cursor..]);
        if crlf {
            out.push('\r');
        }
        if had_lf {
            out.push('\n');
        }
    }
    (out, count)
}

/// 搜索面板上的六个开关。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchToggle {
    CaseSensitive,
    WholeWord,
    Regex,
    IncludeIgnored,
    Filters,
    Replace,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(src: &str, replacement: &str) -> SearchQuery {
        build_query(src, SearchOpts::default(), Vec::new(), Vec::new())
            .unwrap()
            .with_replacement(replacement.to_string())
    }

    #[test]
    fn replace_all_per_line() {
        let query = q("foo", "bar");
        let (out, n) = replace_in_text(&query, "foo a\nb foo\n", false);
        assert_eq!(out, "bar a\nb bar\n");
        assert_eq!(n, 2);
    }

    #[test]
    fn replace_preserves_crlf() {
        let query = q("foo", "bar");
        let (out, n) = replace_in_text(&query, "foo\r\nnext\r\n", false);
        assert_eq!(out, "bar\r\nnext\r\n");
        assert_eq!(n, 1);
    }

    #[test]
    fn replace_next_only_touches_first() {
        let query = q("foo", "bar");
        let (out, n) = replace_in_text(&query, "foo\nfoo\n", true);
        assert_eq!(out, "bar\nfoo\n");
        assert_eq!(n, 1);
    }

    #[test]
    fn path_only_row_is_dropped_once_content_hits() {
        let mut blocks = vec![
            ResultBlock {
                rel: "a/dup.md".into(),
                path_only: true,
                count: 0,
                segments: Vec::new(),
            },
            ResultBlock {
                rel: "b/only_name.md".into(),
                path_only: true,
                count: 0,
                segments: Vec::new(),
            },
        ];
        // 内容命中到达 a/dup.md → 摘掉它的文件栏，只留内容组
        assert!(drop_path_only_for(&mut blocks, "a/dup.md"));
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].rel, "b/only_name.md", "仅文件名命中的文件栏保留");
        // 再摘一次：没有可摘的，计数与内容不变
        assert!(!drop_path_only_for(&mut blocks, "a/dup.md"));
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn globs_split_on_comma_and_newline() {
        assert_eq!(
            split_globs("a,b\n c ,\n".to_string()),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }
}
