//! ask_user_question 整卷问卷卡（v2 重设计）。
//!
//! 链路：transcript 流里的 `Block::ToolCall{args}`（name = ask_user_question）
//! 携带完整 questions[]，先于包逐题发来的 `extension_ui_request` 可见 ——
//! 在此整卷解析成 tab 卡先行渲染；包的第 N 个阻塞请求到达时按序映射到
//! tab N，用户点「确认」后串行应答（或 submitted 态下静默自动应答）。
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
    pub questions: Vec<AskQ>,
    pub active: usize,
    /// 已映射的阻塞请求数（= 下一个请求对应的题目下标）
    pub next_request: usize,
    /// 确认后进入自动应答态：后续请求到达即按已答序列化回填
    pub submitted: bool,
    /// 卡片可见（首个请求到达时打开；确认后收起，等剩余请求流过）
    pub open: bool,
    /// 当前挂起的阻塞请求（包严格串行，至多一个）
    pub pending: Option<ExtensionUiRequest>,
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
