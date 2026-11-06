//! 查询替换层——Zed `replacement_for` / `with_replacement`（逐字移植）。
//!
//! `escaped` 查询直接替换，真正则模式走捕获组展开 + 把 `\\`/`\n`/`\t`
//! 还原（Zed 原文照搬）。

use std::borrow::Cow;
use std::ops::Range;
use std::sync::LazyLock;

use fancy_regex::{Captures, Regex};

use super::SearchQuery;

impl SearchQuery {
    pub(crate) fn with_replacement(mut self, new_replacement: String) -> Self {
        match self {
            Self::Text {
                ref mut replacement,
                ..
            }
            | Self::Regex {
                ref mut replacement,
                ..
            } => {
                *replacement = Some(new_replacement);
                self
            }
        }
    }

    /// Zed `replacement_for`：把命中处的替换文本展开成最终字符串。
    pub(crate) fn replacement_for<'a>(
        &self,
        line: &'a str,
        hit: Range<usize>,
    ) -> Option<Cow<'a, str>> {
        match self {
            SearchQuery::Text { replacement, .. }
            | SearchQuery::Regex {
                replacement,
                escaped: true,
                ..
            } => replacement.clone().map(Cow::from),

            SearchQuery::Regex {
                regex,
                replacement: Some(replacement),
                escaped: false,
                ..
            } => {
                static TEXT_REPLACEMENT_SPECIAL_CHARACTERS_REGEX: LazyLock<Regex> =
                    LazyLock::new(|| Regex::new(r"\\\\|\\n|\\t").unwrap());
                let replacement = TEXT_REPLACEMENT_SPECIAL_CHARACTERS_REGEX.replace_all(
                    replacement,
                    |c: &Captures<str>| match c.get(0).unwrap().as_str() {
                        r"\\" => "\\",
                        r"\n" => "\n",
                        r"\t" => "\t",
                        x => unreachable!("Unexpected escape sequence: {}", x),
                    },
                );
                let captures = regex
                    .captures_from_pos(line, hit.start)
                    .ok()
                    .flatten()
                    .filter(|captures| captures.get(0).is_some_and(|m| m.range() == hit));
                let Some(captures) = captures else {
                    return Some(regex.replace(line.get(hit)?, replacement));
                };
                let mut replaced = String::new();
                captures.expand(&replacement, &mut replaced);
                Some(Cow::Owned(replaced))
            }

            SearchQuery::Regex {
                replacement: None, ..
            } => None,
        }
    }
}
