//! 数据化界面快照：Chat/SessionRuntime 的字段 → JSON。
//!
//! 原则：只读 pub(crate) 字段与既有 read API（settings 走
//! SettingsFormData::snapshot 同源先例），绝不为了快照改 app 状态。
//! surface 名与 method::UI_SNAPSHOT 的 params.surface 对应。

use crate::Chat;
use gpui::Context;
use serde_json::{Value, json};

pub(crate) const SURFACES: [&str; 10] = [
    "app", "sessions", "session", "composer", "files", "git", "settings", "dialogs", "term",
    "search",
];

pub(crate) fn surface(
    chat: &Chat,
    window: &gpui::Window,
    cx: &Context<Chat>,
    name: Option<&str>,
) -> Option<Value> {
    match name {
        None => {
            let mut all = json!({});
            for s in SURFACES {
                all[s] = surface_one(chat, window, cx, s);
            }
            Some(all)
        }
        Some(n) => {
            if SURFACES.contains(&n) {
                Some(surface_one(chat, window, cx, n))
            } else {
                None
            }
        }
    }
}

/// snapshot 的 cwd 字段：无项目态 = 空串（不指代任何真实目录）
fn cwd_json(chat: &Chat) -> String {
    chat.ws
        .cwd
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_default()
}

fn surface_one(chat: &Chat, window: &gpui::Window, cx: &Context<Chat>, name: &str) -> Value {
    match name {
        "app" => app_surface(chat, window, cx),
        "sessions" => sessions_surface(chat, cx),
        "session" => session_surface(chat, cx),
        "composer" => composer_surface(chat, cx),
        "files" => files_surface(chat, cx),
        "git" => git_surface(chat),
        "settings" => settings_surface(chat, cx),
        "dialogs" => dialogs_surface(chat, cx),
        "term" => term_surface(chat, window, cx),
        "search" => search_surface(chat),
        _ => Value::Null,
    }
}

pub(crate) fn app_info(chat: &Chat, window: &gpui::Window, cx: &Context<Chat>) -> Value {
    let mut v = app_surface(chat, window, cx);
    v["pid"] = json!(std::process::id());
    v["surfaces"] = json!(SURFACES);
    v
}

mod app;
mod composer_files;
mod runtime;
mod sessions;
mod settings_dialogs;

use app::{app_surface, term_surface};
pub(crate) use composer_files::search_surface;
use composer_files::{composer_surface, files_surface, git_surface};
use runtime::session_surface;
use sessions::sessions_surface;
use settings_dialogs::{dialogs_surface, settings_surface};

fn content_view_str(v: crate::ContentView) -> &'static str {
    match v {
        crate::ContentView::Chat => "chat",
        crate::ContentView::Term => "term",
        crate::ContentView::File => "file",
        crate::ContentView::Changelog => "changelog",
        crate::ContentView::PiNativeMarket => "pi-native-market",
        crate::ContentView::PfMarket => "pf-market",
        crate::ContentView::Search => "search",
        crate::ContentView::SessionSearch => "session_search",
        crate::ContentView::Plugin => "plugin",
    }
}

fn opt_path(p: Option<&std::path::PathBuf>) -> Value {
    match p {
        Some(p) => json!(p.display().to_string()),
        None => Value::Null,
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let cut: String = s.chars().take(n).collect();
        format!("{cut}…")
    }
}
