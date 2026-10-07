//! 微信纯文本菜单与权限选项语义 —— ZCode `botsService.ts` 移植。
//!
//! 三块原文（ZCode v3.14.3）：
//!
//! | Rust | ZCode | 说明 |
//! |---|---|---|
//! | [`render_selection`] | `formatSelectionFallback` (botsService.ts:488) | 纯文本编号菜单 |
//! | [`display_kind`] 等 | botsService.ts:513-640 | 权限选项 5 个私有 helper |
//! | [`option_index`] | `permission.respond` 分支 (botsService.ts:6137) | 1 起编号 → 0 起下标 |
//!
//! **为什么微信要单独一套**：ZCode `createSelectionReply` 里
//! `supportsStructuredSelection = actor.provider !== "weixin"` —— 微信没有原生
//! 选项卡，走的是 `formatSelectionFallback` 这条纯文本分支；飞书/Telegram 走
//! 结构化选项。pi-flash 只做微信，因此**只有这一条路径**。
//!
//! 权限选项的**编号不带 0 取消**：`permission_request` 的 `SelectionPrompt`
//! 硬编码 `showCancel: false`（botsService.ts:4021），只有通用选择菜单才有 0。

use super::messages::{t, Lang};
use super::permission::Scope;
use super::reply::contains_any_word;

/// `formatBotMessage(locale, ...)` 的三个键（表按 zh-CN 源串作键）。
const CANCEL: &str = "取消";
const SELECTION_TEXT_HINT: &str = "回复数字选择，0 取消。";
const SELECTION_TEXT_HINT_NO_CANCEL: &str = "回复数字选择。";

// ────────────────────────────── 纯文本菜单渲染 ──────────────────────────────

/// 待渲染的一条选项。
///
/// `description` 为 `Some` 时按 ZCode 拼成 `{index}. {label} {description}`
/// （label 与 description 之间**一个空格**，由 description 分支自己带）。
pub struct MenuOption<'a> {
    pub label: &'a str,
    pub description: Option<&'a str>,
}

impl<'a> MenuOption<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            description: None,
        }
    }

    pub fn with_desc(label: &'a str, description: &'a str) -> Self {
        Self {
            label,
            description: Some(description),
        }
    }
}

/// ZCode `formatSelectionFallback(selection, locale)`。
///
/// ```text
/// {title}
/// 0. 取消                ← show_cancel 时才有，且在**第一行**
/// 1. {label} {description}
/// 2. ...
///
/// {回复数字选择，0 取消。}
/// ```
///
/// `cancel_label` 对应 `selection.cancelLabel`（`None` 取 `取消`）。
/// ZCode 原注释：纯文本通道只展示编号，完整 slash command 与长路径会把
/// 消息刷得很长 —— 所以调用方给的 `label` 应当已经是短文案。
pub fn render_selection(
    title: &str,
    options: &[MenuOption<'_>],
    show_cancel: bool,
    cancel_label: Option<&str>,
    lang: Lang,
) -> String {
    let lines: Vec<String> = options
        .iter()
        .enumerate()
        .map(|(i, o)| {
            let desc = match o.description {
                Some(d) => format!(" {d}"),
                None => String::new(),
            };
            format!("{}. {}{}", i + 1, o.label, desc)
        })
        .collect();
    let body = lines.join("\n");

    if !show_cancel {
        return format!(
            "{title}\n{body}\n\n{}",
            t(lang, SELECTION_TEXT_HINT_NO_CANCEL)
        );
    }
    let cancel_owned;
    let cancel: &str = match cancel_label {
        Some(s) => s,
        None => {
            cancel_owned = t(lang, CANCEL);
            &cancel_owned
        }
    };
    format!(
        "{title}\n0. {cancel}\n{body}\n\n{}",
        t(lang, SELECTION_TEXT_HINT)
    )
}

// ────────────────────────────── 权限选项语义 ──────────────────────────────

/// ZCode `BotPermissionOptionDisplayKind`（botsService.ts:505）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
    Custom,
}

impl DisplayKind {
    /// `BOT_PERMISSION_OPTION_PRIORITY`（botsService.ts:513）。
    pub fn priority(self) -> u8 {
        match self {
            DisplayKind::AllowOnce => 0,
            DisplayKind::AllowAlways => 1,
            DisplayKind::RejectOnce => 2,
            DisplayKind::RejectAlways => 3,
            DisplayKind::Custom => 4,
        }
    }
}

/// ZCode `getBotPermissionOptionDisplayKind`（botsService.ts:521）。
///
/// 把 provider 给的原始选项（`optionId` + `kind` + `name` 拼串、先 `toLowerCase`）
/// 用**中英关键词嗅探**成 5 类。顺序与 ZCode 一致：**先判允许、再判拒绝**，
/// 两边都命中时允许赢（ZCode 原文 `if (isAllow) ... if (isReject) ...`）。
///
/// 英文词用 JS `\b(word)\b`（此处由 [`contains_any_word`] 等价实现，
/// 直接在原始字节上比较、不做 `to_lowercase()` 以免搞乱非 ASCII 下标）；
/// 中文词无 `\b`，就是裸子串。
///
/// 注意 ZCode 先把整串 `.toLowerCase()` 再匹配 —— 这里对英文部分靠
/// `eq_ignore_ascii_case` 等价，中文不受大小写影响。
pub fn display_kind(option_id: &str, kind: &str, name: &str) -> DisplayKind {
    let text = format!("{option_id} {kind} {name}").to_lowercase();
    let is_always = contains_any_word(&text, &["always", "persistent", "permanent", "remember"])
        || ["始终", "永久", "记住", "不再询问"].iter().any(|w| text.contains(w));
    let is_allow = contains_any_word(&text, &["allow", "approve", "accept", "yes"])
        || ["允许", "同意", "批准"].iter().any(|w| text.contains(w));
    let is_reject = contains_any_word(&text, &["deny", "reject", "decline", "no"])
        || ["拒绝", "不允许", "否"].iter().any(|w| text.contains(w));

    if is_allow {
        if is_always {
            DisplayKind::AllowAlways
        } else {
            DisplayKind::AllowOnce
        }
    } else if is_reject {
        if is_always {
            DisplayKind::RejectAlways
        } else {
            DisplayKind::RejectOnce
        }
    } else {
        DisplayKind::Custom
    }
}

/// ZCode `sortBotPermissionOptions`（botsService.ts:539）。
///
/// `[...options].sort(...)` 在 ES2019 起是**稳定排序**，Rust `sort_by` 同样
/// 稳定 —— 同优先级保持 provider 原始顺序。
pub fn sort_options<T>(items: &mut [T], key: impl Fn(&T) -> DisplayKind) {
    items.sort_by(|a, b| key(a).priority().cmp(&key(b).priority()));
}

/// ZCode `isBotPermissionRejectOption`（botsService.ts:629）。
///
/// 决定回填走 `/deny` 还是 `/approve`（pi-flash 侧对应 `Command::ExtensionUiResponse`
/// 的 confirm 位）。
pub fn is_reject(kind: DisplayKind) -> bool {
    matches!(kind, DisplayKind::RejectOnce | DisplayKind::RejectAlways)
}

/// ZCode `formatBotPermissionOptionLabel`（botsService.ts:549）。
///
/// **保真点**：ZCode 只判 `locale === "en-US"`，其余一律中文 —— 它的
/// `Locale` 类型是 `"zh-CN" | "en-US"`，**没有 zh-TW**（`protocol.ts:75`）。
/// 因此这里 `Lang::ZhTw` 也走中文分支，不自造繁体。
///
/// `Custom` 不做本地化，原样返回 provider 的 `name`。
pub fn option_label(lang: Lang, kind: DisplayKind, custom_name: &str) -> String {
    if matches!(lang, Lang::En) {
        match kind {
            DisplayKind::AllowOnce => "Allow".to_string(),
            DisplayKind::AllowAlways => "Always Allow".to_string(),
            DisplayKind::RejectOnce => "Deny".to_string(),
            DisplayKind::RejectAlways => "Always Deny".to_string(),
            DisplayKind::Custom => custom_name.to_string(),
        }
    } else {
        match kind {
            DisplayKind::AllowOnce => "允许".to_string(),
            DisplayKind::AllowAlways => "始终允许".to_string(),
            DisplayKind::RejectOnce => "拒绝".to_string(),
            DisplayKind::RejectAlways => "始终拒绝".to_string(),
            DisplayKind::Custom => custom_name.to_string(),
        }
    }
}

/// ZCode `formatBotPermissionOptionDescription`（botsService.ts:579）。
///
/// `Custom` 直接返回 `option.kind`；其余按 [`Scope`] 三分支（command / file /
/// generic），**与 label 同样只有 en / 中文两档**。
pub fn option_description(lang: Lang, kind: DisplayKind, scope: Scope, custom_kind: &str) -> String {
    if matches!(kind, DisplayKind::Custom) {
        return custom_kind.to_string();
    }
    if matches!(lang, Lang::En) {
        return match kind {
            DisplayKind::AllowOnce => "Allow this time only".to_string(),
            DisplayKind::RejectOnce => "Reject this time".to_string(),
            DisplayKind::AllowAlways => match scope {
                Scope::Command => "Do not ask again for the same command".to_string(),
                Scope::File => "Do not ask again for the same file operation".to_string(),
                Scope::Generic => "Do not ask again for the same permission request".to_string(),
            },
            DisplayKind::RejectAlways => match scope {
                Scope::Command => "Always reject the same command".to_string(),
                Scope::File => "Always reject the same file operation".to_string(),
                Scope::Generic => "Always reject the same permission request".to_string(),
            },
            DisplayKind::Custom => unreachable!("Custom 已在上方返回"),
        };
    }
    match kind {
        DisplayKind::AllowOnce => "仅允许这一次".to_string(),
        DisplayKind::RejectOnce => "这次先拒绝".to_string(),
        DisplayKind::AllowAlways => match scope {
            Scope::Command => "后续相同命令不再询问".to_string(),
            Scope::File => "后续相同文件操作不再询问".to_string(),
            Scope::Generic => "后续相同权限请求不再询问".to_string(),
        },
        DisplayKind::RejectAlways => match scope {
            Scope::Command => "后续相同命令也会直接拒绝".to_string(),
            Scope::File => "后续相同文件操作也会直接拒绝".to_string(),
            Scope::Generic => "后续相同权限请求也会直接拒绝".to_string(),
        },
        DisplayKind::Custom => unreachable!("Custom 已在上方返回"),
    }
}

// ────────────────────────────── 编号回填解析 ──────────────────────────────

/// ZCode `permission.respond` 的 `Number.parseInt(command.value, 10) - 1`
/// （botsService.ts:6139），返回 **0 起下标**。
///
/// 与 ZCode 逐条对齐的边界：
/// * 前导空白跳过（JS `parseInt` 自己 trim）
/// * `"1abc"` → 0、`"1.5"` → 0（JS 只取到首个非法字符为止）
/// * `"0"` / 负数 → `None`（下标越界 = ZCode 取到 `undefined` → 走
///   `permissionExpired`）
/// * `"abc"` / 空串 → `None`（JS `parseInt` 得 `NaN`，`Number.isFinite` 拒绝）
/// * `"0x10"` → `None`（ZCode 显式传了 radix 10，`0x` 不被接受，只读出 `0`
///   → 下标 -1 → 同样越界）
/// * 超长数字溢出 → `None`（JS 得到极大浮点，越界同样 expired）
pub fn option_index(text: &str) -> Option<usize> {
    let n = js_parse_int_10(text)?;
    let idx = n - 1;
    if idx < 0 {
        None
    } else {
        Some(idx as usize)
    }
}

/// JS `Number.parseInt(s, 10)` 的最小等价实现。
fn js_parse_int_10(text: &str) -> Option<i64> {
    let s = text.trim_start();
    let b = s.as_bytes();
    let mut i = 0;
    let negative = if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
        let neg = b[i] == b'-';
        i += 1;
        neg
    } else {
        false
    };
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == start {
        return None;
    }
    let v: i64 = s[start..i].parse().ok()?;
    Some(if negative { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── 菜单渲染 ──

    #[test]
    fn selection_menu_shows_cancel_first_line() {
        // ZCode 默认（showCancel 未显式 false）：0 在最前，标题不带序号
        let opts = [MenuOption::new("允许"), MenuOption::new("拒绝")];
        let got = render_selection("要执行吗", &opts, true, None, Lang::ZhCn);
        assert_eq!(
            got,
            "要执行吗\n0. 取消\n1. 允许\n2. 拒绝\n\n回复数字选择，0 取消。"
        );
    }

    #[test]
    fn permission_menu_has_no_cancel_line() {
        // botsService.ts:4021 `showCancel: false` —— 权限请求不给 0
        let opts = [
            MenuOption::with_desc("允许", "仅允许这一次"),
            MenuOption::with_desc("始终允许", "后续相同命令不再询问"),
            MenuOption::new("拒绝"),
        ];
        let got = render_selection("需要权限", &opts, false, None, Lang::ZhCn);
        assert_eq!(
            got,
            "需要权限\n1. 允许 仅允许这一次\n2. 始终允许 后续相同命令不再询问\n3. 拒绝\n\n回复数字选择。"
        );
        assert!(!got.contains("\n0."), "权限菜单不得出现 0 取消行");
    }

    #[test]
    fn selection_menu_uses_cancel_label_and_english_hint() {
        let opts = [MenuOption::new("Allow")];
        let got = render_selection("Title", &opts, true, Some("Abort"), Lang::En);
        assert_eq!(
            got,
            "Title\n0. Abort\n1. Allow\n\nReply with a number to choose, or 0 to cancel."
        );
    }

    #[test]
    fn selection_without_description_has_no_trailing_space() {
        let opts = [MenuOption::new("A"), MenuOption::with_desc("B", "d")];
        let got = render_selection("t", &opts, false, None, Lang::ZhCn);
        assert!(got.contains("1. A\n2. B d"));
    }

    // ── 选项分类 ──

    #[test]
    fn display_kind_sniffs_english_and_chinese() {
        assert_eq!(display_kind("allow", "once", "Run it"), DisplayKind::AllowOnce);
        assert_eq!(
            display_kind("always", "persistent", "allow"),
            DisplayKind::AllowAlways
        );
        assert_eq!(display_kind("deny", "once", "x"), DisplayKind::RejectOnce);
        assert_eq!(display_kind("reject", "always", "x"), DisplayKind::RejectAlways);
        assert_eq!(display_kind("custom", "override", "强制覆盖"), DisplayKind::Custom);
        // 中文关键词
        assert_eq!(display_kind("a", "b", "始终允许"), DisplayKind::AllowAlways);
        assert_eq!(display_kind("a", "b", "这次先拒绝"), DisplayKind::RejectOnce);
        assert_eq!(display_kind("a", "b", "允许"), DisplayKind::AllowOnce);
    }

    #[test]
    fn word_boundary_is_respected() {
        // \b 语义：`allowed` 不匹配 `allow\b`，`no` 不是 `denied` 的子词
        assert_eq!(display_kind("allowed", "x", "y"), DisplayKind::Custom);
        assert_eq!(display_kind("denied", "x", "y"), DisplayKind::Custom);
        // 但整词命中
        assert_eq!(display_kind("no", "x", "y"), DisplayKind::RejectOnce);
    }

    #[test]
    fn allow_wins_when_both_keywords_hit() {
        // ZCode：先 if (isAllow) 再 if (isReject) —— 允许赢
        assert_eq!(display_kind("x", "y", "允许拒绝"), DisplayKind::AllowOnce);
    }

    #[test]
    fn sort_puts_allow_before_deny_and_is_stable() {
        let mut items = vec![
            ("deny-1", DisplayKind::RejectOnce),
            ("custom-1", DisplayKind::Custom),
            ("allow-2", DisplayKind::AllowOnce),
            ("allow-1", DisplayKind::AllowOnce),
            ("deny-2", DisplayKind::RejectOnce),
        ];
        sort_options(&mut items, |it| it.1);
        let names: Vec<&str> = items.iter().map(|it| it.0).collect();
        assert_eq!(
            names,
            // 输入序里 allow-2 在 allow-1 之前 —— 稳定排序必须保留该相对顺序
            vec!["allow-2", "allow-1", "deny-1", "deny-2", "custom-1"],
            "同优先级保持原序（JS Array#sort 自 ES2019 起稳定）"
        );
    }

    #[test]
    fn reject_flag_drives_deny_vs_approve() {
        assert!(!is_reject(DisplayKind::AllowOnce));
        assert!(!is_reject(DisplayKind::AllowAlways));
        assert!(is_reject(DisplayKind::RejectOnce));
        assert!(is_reject(DisplayKind::RejectAlways));
        assert!(!is_reject(DisplayKind::Custom), "Custom 走 approve 分支");
    }

    // ── 文案 ──

    #[test]
    fn option_labels_zh_and_en() {
        for (lang, allow, always, deny, always_deny) in [
            (Lang::ZhCn, "允许", "始终允许", "拒绝", "始终拒绝"),
            // 保真点：ZCode 没有 zh-TW，非 en 一律中文
            (Lang::ZhTw, "允许", "始终允许", "拒绝", "始终拒绝"),
            (Lang::En, "Allow", "Always Allow", "Deny", "Always Deny"),
        ] {
            assert_eq!(option_label(lang, DisplayKind::AllowOnce, "n"), allow);
            assert_eq!(option_label(lang, DisplayKind::AllowAlways, "n"), always);
            assert_eq!(option_label(lang, DisplayKind::RejectOnce, "n"), deny);
            assert_eq!(option_label(lang, DisplayKind::RejectAlways, "n"), always_deny);
        }
        // Custom 不本地化
        assert_eq!(option_label(Lang::En, DisplayKind::Custom, "My Option"), "My Option");
        assert_eq!(option_label(Lang::ZhCn, DisplayKind::Custom, "自定"), "自定");
    }

    #[test]
    fn option_descriptions_depend_on_scope() {
        assert_eq!(
            option_description(Lang::ZhCn, DisplayKind::AllowOnce, Scope::Command, "k"),
            "仅允许这一次"
        );
        assert_eq!(
            option_description(Lang::ZhCn, DisplayKind::RejectOnce, Scope::File, "k"),
            "这次先拒绝"
        );
        assert_eq!(
            option_description(Lang::ZhCn, DisplayKind::AllowAlways, Scope::Command, "k"),
            "后续相同命令不再询问"
        );
        assert_eq!(
            option_description(Lang::ZhCn, DisplayKind::AllowAlways, Scope::File, "k"),
            "后续相同文件操作不再询问"
        );
        assert_eq!(
            option_description(Lang::ZhCn, DisplayKind::AllowAlways, Scope::Generic, "k"),
            "后续相同权限请求不再询问"
        );
        assert_eq!(
            option_description(Lang::ZhCn, DisplayKind::RejectAlways, Scope::Generic, "k"),
            "后续相同权限请求也会直接拒绝"
        );
        assert_eq!(
            option_description(Lang::En, DisplayKind::AllowAlways, Scope::Command, "k"),
            "Do not ask again for the same command"
        );
        assert_eq!(
            option_description(Lang::En, DisplayKind::RejectAlways, Scope::File, "k"),
            "Always reject the same file operation"
        );
        // Custom → 原样返回 option.kind，不查表
        assert_eq!(
            option_description(Lang::En, DisplayKind::Custom, Scope::Command, "sudo"),
            "sudo"
        );
    }

    // ── 编号解析 ──

    #[test]
    fn option_index_is_one_based() {
        assert_eq!(option_index("1"), Some(0));
        assert_eq!(option_index("3"), Some(2));
        assert_eq!(option_index(" 2"), Some(1), "JS parseInt 跳过前导空白");
    }

    #[test]
    fn option_index_rejects_out_of_range_and_garbage() {
        assert_eq!(option_index("0"), None, "0 → 下标 -1 → expired");
        assert_eq!(option_index("-1"), None);
        assert_eq!(option_index("abc"), None, "NaN → Number.isFinite 拒绝");
        assert_eq!(option_index(""), None);
        assert_eq!(option_index("+"), None);
        assert_eq!(option_index("0x10"), None, "radix 10 下只读出 0 → -1 越界");
    }

    #[test]
    fn option_index_matches_js_parse_int_prefix_rules() {
        assert_eq!(option_index("1abc"), Some(0), "JS 取首个非法字符前的数字");
        assert_eq!(option_index("1.5"), Some(0));
        assert_eq!(option_index("12"), Some(11));
        assert_eq!(option_index("99999999999999999999999"), None, "溢出 → 越界");
    }
}
