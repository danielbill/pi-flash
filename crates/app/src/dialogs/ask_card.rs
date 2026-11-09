//! ask_user_question 整卷问卷卡（v2 重设计）。
//!
//! 链路：transcript 流里的 `Block::ToolCall{args}`（name = ask_user_question）
//! 携带完整 questions[]，先于包逐题发来的 `extension_ui_request` 可见 ——
//! 在此整卷解析成 tab 卡先行渲染。请求 → 题目的对应关系**按 title 含题面
//! 文本匹配**（不按下标/条数）：同一卷（= 同一条 tool_call id）里一题可能
//! 来两条请求 —— 多选走 `input`、单选走 `select`、自定义答案还有一条
//! `input` 追问（rpc-fallback 的 "Type your answer:" 分支）；已作答的题
//! 来请求就静默应答，未作答才挂起弹卡。确认后卡仍持卷（不弹窗），专为
//! 接住最后那类追问；换新 tool_call id 或取消时才作废。
//! 回传契约与 @juicesharp/rpiv-ask-user-question 的 rpc-fallback 严格对齐：
//! 单选回 options 原行 / 自定义回纯文本；多选回 "N[,N…][; 文本]"，空 = 全不选。
//!
//! 交互结构移植自 my-mastra-bot AskCard.tsx：tab 自由切题（鼠标点题签）、
//! 每题独立作答态、tab ●→✓ 答完标记、确认 [n/N] 门控、Enter/数字键键盘模型。
//! Tab 键已砍（2026-11-05 用户拍板）：与 Enter 表观重叠（Enter 跳下一未
//! 答题、全答完即提交，按序作答时与 Tab 同效），键盘模型只留 Enter/数字键；
//! 且 Root 层 `tab→Tab` 绑定会抢在 on_key_down 前吞键（gpui.md keymap 节），
//! 监听器路线本就不通。
//! 视觉 = tmp/ask-user-redesign.html 评审稿（MIST 主题逐值对译）。

use crate::*;
use pi_link::protocol::{ExtUiMethod, ExtensionUiRequest};

// ---------------------------------------------------------------------------
// 状态
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(crate) struct AskCard {
    /// 本卷的 tool_call id（卷标识）：同一 id 的请求都归这一卷（含自定义
    /// 答案 "Type your answer:" 追问）；换 id 说明开了新卷，旧卡作废。
    pub call_id: String,
    pub questions: Vec<AskQ>,
    pub active: usize,
    /// 正在等用户作答的阻塞请求 + 它对应的题目下标。题目按请求 title 包含
    /// 题面定位，**不假设「一题一请求、严格串行」**：扩展多选走 input、
    /// 单选走 select，自定义答案还有一条 input 追问（rpc-fallback
    /// `askSingleSelect` 的 sentinel 分支），同一题可能来两个请求。
    pub pending: Option<(usize, ExtensionUiRequest)>,
    /// 卡片可见（有请求在等作答时打开；确认后收起，后续追问静默应答）
    pub open: bool,
    /// 预览正文滚动句柄（Rc 共享：AskCard 每帧 clone，滚动位置跨帧存续）
    pub pv_scroll: gpui::ScrollHandle,
}

#[derive(Debug, Clone)]
pub(crate) struct AskQ {
    pub header: String,
    pub question: String,
    pub multi: bool,
    pub options: Vec<AskOpt>,
    /// 与 options 等长的选中标记
    pub sel: Vec<bool>,
    /// 「自定义答案」行选中态（单选与选项互斥；多选并存）
    pub other_sel: bool,
    pub other_text: String,
}

#[derive(Debug, Clone)]
pub(crate) struct AskOpt {
    pub label: String,
    pub desc: String,
    pub preview: Option<String>,
}

impl AskCard {
    /// 卷已收口：每题都有答案且没有挂起请求。卡**不销毁**（不弹窗），仅
    /// 留着静默应答本卷的追问（自定义答案 "Type your answer:" input 可能
    /// 在最后一题应答后才到）；换卷（新 tool_call id）/取消时才作废。
    pub(crate) fn finished(&self) -> bool {
        self.pending.is_none() && self.questions.iter().all(|q| q.answered())
    }

    /// 请求对应的题目：title 含题面即为命中（题面唯一）；同文多题时优先
    /// 未作答的那题。标题 = `[header] question` + 折叠的 preview 块 / 选项
    /// 列表（追问 input 的标题会重复题面 + 选项 + "Type your answer:"）。
    pub(crate) fn match_question(&self, req: &ExtensionUiRequest) -> Option<usize> {
        let mut answered_hit: Option<usize> = None;
        for (i, q) in self.questions.iter().enumerate() {
            if !parse::title_matches(req, q) {
                continue;
            }
            if !q.answered() {
                return Some(i);
            }
            answered_hit.get_or_insert(i);
        }
        answered_hit
    }
}

impl AskQ {
    pub(crate) fn answered(&self) -> bool {
        if self.sel.iter().any(|s| *s) {
            return true;
        }
        self.other_sel && !self.other_text.trim().is_empty()
    }

    /// 按请求类型序列化本题答案（与包 rpc-fallback 解析器对齐）：
    /// 单选回 options 原行 / 自定义回纯文本；多选回 "N[,N…][; 文本]"
    fn serialize_for(&self, req: &ExtensionUiRequest) -> String {
        if self.multi {
            return self.pick_text();
        }
        if self.other_sel {
            return self.other_text.trim().to_string();
        }
        if let Some(i) = self.sel.iter().position(|s| *s) {
            if let ExtUiMethod::Select { options, .. } = &req.method {
                if let Some(line) = options.get(i) {
                    return line.clone();
                }
            }
            return self.options[i].label.clone();
        }
        String::new()
    }

    /// 提交序列化预览（多选摘要条 / 调试用）；与 serialize() 同构
    fn pick_text(&self) -> String {
        if !self.multi {
            return self
                .sel
                .iter()
                .position(|s| *s)
                .map(|i| self.options[i].label.clone())
                .or_else(|| {
                    self.other_sel
                        .then(|| self.other_text.trim().to_string())
                        .filter(|s| !s.is_empty())
                })
                .unwrap_or_default();
        }
        let nums: Vec<String> = self
            .sel
            .iter()
            .enumerate()
            .filter(|(_, s)| **s)
            .map(|(i, _)| (i + 1).to_string())
            .collect();
        let text = self.other_text.trim();
        match (!nums.is_empty(), !text.is_empty()) {
            (true, true) => format!("{}; {}", nums.join(","), text),
            (true, false) => nums.join(","),
            (false, true) => text.to_string(),
            _ => String::new(),
        }
    }

    fn has_preview(&self) -> bool {
        self.options.iter().any(|o| o.preview.is_some())
    }
}

// ---------------------------------------------------------------------------
// 子模块：解析 / 交互动作 / 渲染 / 渲染件
// ---------------------------------------------------------------------------

mod actions;
mod parse;
mod render;
mod widgets;

pub(crate) use render::render_ask_card;
