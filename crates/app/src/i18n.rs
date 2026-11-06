//! i18n (pi-flash-04k): three languages — zh-CN (source strings, identity),
//! zh-TW, en. `t()` maps a source literal through the table; `tf()` renders
//! `{key}` templates. The language index is runtime-switchable and persisted
//! in the workspace memory file (pi-web i18n parity: user preference).

use std::sync::atomic::{AtomicUsize, Ordering};

/// 0 zh-CN · 1 zh-TW · 2 en
static LANG_IX: AtomicUsize = AtomicUsize::new(0);

pub const LANG_LABELS: [&str; 3] = ["简体中文", "繁體中文", "English"];

mod table;

use self::table::TABLE;
pub use self::table::{ABOUT_DESC, ABOUT_NOTE};

pub fn set_lang(ix: usize) {
    let ix = ix.min(2);
    LANG_IX.store(ix, Ordering::Relaxed);
    // gpui-component（rust_i18n）词表同步：搜索条按钮文案/tooltip 用组件
    // 自己的 locales/ui.yml（繁体落组件的 zh-HK 桶）
    gpui_component::set_locale(match ix {
        1 => "zh-HK",
        2 => "en",
        _ => "zh-CN",
    });
}

pub fn lang_ix() -> usize {
    LANG_IX.load(Ordering::Relaxed)
}

/// Translate a **semantic key** for the active language.
///
/// - 键不存在 → 原样返回（未知字符串穿透，新 UI 优雅降级）
/// - **译文为空 → 回落 zh-CN**（存量漏译的行为与迁移前一致：繁/英界面显示
///   中文，而不是空白、也不是裸键名。漏译清单由 src/i18n_audit.rs 盯着）
pub fn tr<'a>(s: &'a str) -> &'a str {
    let row = TABLE.iter().find(|e| e.0 == s);
    match lang_ix() {
        0 => row.map(|e| e.1).unwrap_or(s),
        ix => row
            .map(|e| match ix {
                1 => e.2,
                _ => e.3,
            })
            .filter(|t| !t.is_empty())
            .or_else(|| row.map(|e| e.1))
            .unwrap_or(s),
    }
}

/// Render a `{key}` template in the active language.
pub fn tf(template: &str, pairs: &[(&str, String)]) -> String {
    let mut out = tr(template).to_string();
    for (k, v) in pairs {
        out = out.replace(&format!("{{{k}}}"), v);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// tests mutate the process-global language; serialize them
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn identity_for_source_language() {
        let _g = LOCK.lock();
        set_lang(0);
        assert_eq!(tr("action.save"), "保存");
        assert_eq!(tr("something.new"), "something.new");
    }

    #[test]
    fn translations_for_other_languages() {
        let _g = LOCK.lock();
        set_lang(1);
        assert_eq!(tr("action.save"), "儲存");
        assert_eq!(tr("session.fork"), "新分支");
        assert_eq!(tr("session.creating"), "建立中…");
        assert_eq!(tr("new_session.greeting"), "Hi，打算讓我做點什麼？");
        assert_eq!(tr("project.search_placeholder"), "搜尋項目…");
        assert_eq!(tr("action.open_folder"), "開啟資料夾");
        set_lang(2);
        assert_eq!(tr("action.save"), "Save");
        assert_eq!(tr("session.fork"), "Fork");
        assert_eq!(tr("session.creating"), "Creating…");
        assert_eq!(tr("new_session.greeting"), "Hi, what shall we work on?");
        assert_eq!(tr("project.search_placeholder"), "Search projects…");
        assert_eq!(tr("action.open_folder"), "Open folder");
        assert_eq!(tr("not.in.table"), "not.in.table");
        set_lang(0);
    }

    /// 存量漏译：繁/英单元格为空 → 回落 zh-CN（**不能**空白、不能回落裸键名）
    #[test]
    fn untranslated_falls_back_to_zh() {
        let _g = LOCK.lock();
        set_lang(2);
        assert_eq!(tr("git.stage_all"), "Stage All");
        set_lang(1);
        assert_eq!(tr("git.stage_all"), "Stage All");
        set_lang(0);
    }

    /// 键与文案解耦：改译文不动键（迁移的核心收益）
    #[test]
    fn key_is_independent_of_copy() {
        let _g = LOCK.lock();
        set_lang(0);
        assert_eq!(tr("settings.show_thinking"), "展示思考");
        set_lang(2);
        assert_eq!(tr("settings.show_thinking"), "Show thinking");
        set_lang(0);
    }

    #[test]
    fn template_replacement() {
        let _g = LOCK.lock();
        set_lang(2);
        assert_eq!(
            tf("extensions.installed", &[("source", "npm:x".to_string())]),
            "Installed npm:x"
        );
        set_lang(0);
        assert_eq!(
            tf("extensions.installed", &[("source", "npm:x".to_string())]),
            "已安装 npm:x"
        );
    }
}
