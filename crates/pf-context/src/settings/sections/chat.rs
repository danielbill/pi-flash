//! `chat` section（001-1 §3）：内容列宽 / Live Preview / 发送方式 / 思考 / git 标识。
//! v1 平铺键 `chat_content_width` → `content_width`（去冗余前缀），其余同名。

use serde::{Deserialize, Serialize};

use crate::settings::section::Section;

/// 会话内容宽度阈值（2026-10-29 用户定案：1200 封顶、步进 20；pi-web
/// `--chat-content-max-width` parity）。app 侧经 `workspace::CHAT_WIDTH_*`
/// re-export 消费，调用点路径不变。
pub const CHAT_WIDTH_MIN: u32 = 560;
pub const CHAT_WIDTH_DEFAULT: u32 = 920;
pub const CHAT_WIDTH_MAX: u32 = 1200;
/// 滑块步进（20px；min/default/max 均为 20 的整数倍）
pub const CHAT_WIDTH_STEP: u32 = 20;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "snake_case")]
pub struct Chat {
    /// 会话内容宽度 px（消息列 / composer 胶囊 / 新会话页 / ask 卡共用单一列宽）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_width: Option<u64>,
    /// 024 Markdown 所见即所得（默认关——已知回车崩溃未修，显式开启才挂装饰）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub md_live_preview: Option<bool>,
    /// 090 排队护栏："queue"（默认）/ "steer"；非法值落 queue（读侧策略）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub send_mode: Option<String>,
    /// 展示思考（默认关 = 新思考块收成一行）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub show_thinking: Option<bool>,
    /// 023 文件树 git 标识（默认关 = 清爽目录树）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_markers: Option<bool>,
}

impl Section for Chat {
    const KEY: &'static str = "chat";

    fn validate(&mut self) {
        if let Some(w) = self.content_width.as_mut() {
            *w = (*w).clamp(CHAT_WIDTH_MIN as u64, CHAT_WIDTH_MAX as u64);
        }
    }

    fn from_legacy(map: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        let content_width = map.get("chat_content_width").and_then(|v| v.as_u64());
        let md_live_preview = map.get("md_live_preview").and_then(|v| v.as_bool());
        let send_mode = map
            .get("send_mode")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let show_thinking = map.get("show_thinking").and_then(|v| v.as_bool());
        let git_markers = map.get("git_markers").and_then(|v| v.as_bool());
        if content_width.is_none()
            && md_live_preview.is_none()
            && send_mode.is_none()
            && show_thinking.is_none()
            && git_markers.is_none()
        {
            return None;
        }
        Some(Self {
            content_width,
            md_live_preview,
            send_mode,
            show_thinking,
            git_markers,
        })
    }

    fn to_legacy(&self, map: &mut serde_json::Map<String, serde_json::Value>) {
        let mut insert = |k: &str, v: Option<serde_json::Value>| {
            if let Some(v) = v {
                map.insert(k.to_string(), v);
            }
        };
        insert(
            "chat_content_width",
            self.content_width.map(serde_json::Value::from),
        );
        insert(
            "md_live_preview",
            self.md_live_preview.map(serde_json::Value::from),
        );
        insert(
            "send_mode",
            self.send_mode.clone().map(serde_json::Value::String),
        );
        insert(
            "show_thinking",
            self.show_thinking.map(serde_json::Value::from),
        );
        insert("git_markers", self.git_markers.map(serde_json::Value::from));
    }
}
