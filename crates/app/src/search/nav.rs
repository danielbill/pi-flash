//! 搜索交互：开关 / 导航 / 跳转段。

use gpui::Context;

use super::*;
use crate::*;

use super::SearchToggle;

impl Chat {
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
}
