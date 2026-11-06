//! 工具卡（032）：args 展示双 LRU 缓存（PREVIEW/READ_PATH）、
//! [`render_tool_card`] 与子代理卡 [`render_subagent_card`]、alert 提示盒。
//! 自 cards.rs 拆出（090 重构）。

use super::diff_view::split_files_view;
use super::*;

use std::collections::HashMap;

use gpui::{
    Animation, AnimationExt, FontWeight, MouseButton, SharedString, div, prelude::*, px, relative,
    rgb, rgba,
};

use crate::Chat;
use crate::appearance::theme;
use crate::i18n::tr;
use crate::ui::WithTip as _;
use crate::ui::icon;

// 两个 args 缓存的线程局部 LRU（同 MD_PARSE_CACHE 的理由，元素构建只在
// 主线程；放模块级是为了让探针 [`tool_args_cache_probe`] 够得着）。
// key = (源文哈希, 长度)，命中后全等校验防碰撞。
thread_local! {
    pub(crate) static PREVIEW_CACHE: std::cell::RefCell<
        std::collections::VecDeque<((u64, usize), (String, String))>,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
    pub(crate) static READ_PATH_CACHE: std::cell::RefCell<
        std::collections::VecDeque<((u64, usize), (String, Option<String>))>,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
/// 字节预算（args 源文各 16MB）：write/edit 的 args 内嵌整个文件内容
/// （几十 KB～MB 级），只按条数封顶时大文件会话稳态可到几十 MB
pub(crate) const ARGS_CACHE_BUDGET_BYTES: usize = 16 * 1024 * 1024;

/// pi-web getToolPreview：command/path/file_path/pattern/query 键序取值
/// 截 120 字符，都没有取首个键的值。
///
/// 结果带线程局部 LRU 缓存（key = 源文哈希 + 长度，命中全等校验防碰撞）：
/// List 每帧重建元素树，折叠卡也会每帧重算预览，而 serde 解析整段 args
/// （write/edit 的 args 内嵌整个文件内容，几十 KB 起）是巨轮会话打字延迟
/// 的大头——缓存后折叠帧只剩一次哈希扫描。
pub(crate) mod subagent;
pub(crate) mod tool;
pub(crate) use tool::{alert_box, render_tool_card};
/// pi-web getToolPreview：command/path/file_path/pattern/query 键序取值
/// 截 120 字符，都没有取首个键的值。
///
/// 结果带线程局部 LRU 缓存（key = 源文哈希 + 长度，命中全等校验防碰撞）：
/// List 每帧重建元素树，折叠卡也会每帧重算预览，而 serde 解析整段 args
/// （write/edit 的 args 内嵌整个文件内容，几十 KB 起）是巨轮会话打字延迟
/// 的大头——缓存后折叠帧只剩一次哈希扫描。
fn tool_preview(args: &str) -> String {
    const CAP: usize = 256;
    let key = (crate::editor::markdown::hash_str(args), args.len());
    PREVIEW_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1.0.as_str() == args {
                let out = entry.1.1.clone();
                cache.push_back(entry); // 刷新到队尾（LRU）
                return out;
            }
        }
        let out = tool_preview_uncached(args);
        cache.push_back((key, (args.to_string(), out.clone())));
        while cache.len() > CAP
            || cache.iter().map(|(k, _)| k.1).sum::<usize>() > ARGS_CACHE_BUDGET_BYTES
        {
            if cache.pop_front().is_none() {
                break;
            }
        }
        out
    })
}

fn tool_preview_uncached(args: &str) -> String {
    const KEYS: &[&str] = &["command", "path", "file_path", "pattern", "query"];
    let Ok(v) = serde_json::from_str::<serde_json::Value>(args) else {
        return String::new();
    };
    let Some(obj) = v.as_object() else {
        return String::new();
    };
    if obj.is_empty() {
        return String::new();
    }
    let value_of = |k: &str| -> Option<String> {
        obj.get(k).map(|x| match x {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    };
    let raw = KEYS.iter().find_map(|k| value_of(k)).or_else(|| {
        obj.iter().next().and_then(|(_, v)| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            other => Some(other.to_string()),
        })
    });
    raw.map(|r| r.chars().take(120).collect())
        .unwrap_or_default()
}

/// pi-web summarizeApplyPatchInput：apply_patch 目标文件路径 join 截 120。
fn summarize_apply_patch(args: &str) -> String {
    let text = serde_json::from_str::<serde_json::Value>(args)
        .ok()
        .and_then(|v| v.get("input").and_then(|x| x.as_str()).map(str::to_string))
        .unwrap_or_else(|| args.to_string());
    let paths = crate::session::diff::extract_apply_patch_paths(&text);
    let joined = paths.join(", ");
    joined.chars().take(120).collect()
}

/// 参数展开体文本：流式原始串；完整参数 pretty JSON（pi-web
/// JSON.stringify(input, null, 2) parity）。
fn tool_args_display(args: &str, partial: bool) -> String {
    if partial {
        return args.to_string();
    }
    serde_json::from_str::<serde_json::Value>(args)
        .map(|v| serde_json::to_string_pretty(&v).unwrap_or_else(|_| args.to_string()))
        .unwrap_or_else(|_| args.to_string())
}

/// 结果空文案判定：空串或 pi 的 "(no output)"。
pub(super) fn result_is_empty(result: &str) -> bool {
    let r = result.trim();
    r.is_empty() || r == "(no output)"
}
