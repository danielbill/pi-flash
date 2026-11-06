//! PF 插件清单——值形态（02 §2 手写极简 TOML 值）：字符串 / `{zh,en}` 文案
//! 对象 / 裸字面量三形态 + 行内注释剥离 + kebab-case 校验。解析主循环在
//! [`super::parse`]。

/// check_arch 的 fn 行数统计是括号裸计数：正文里的字符面量 `'{'`/`'}'`
/// 会把计数带偏（parse_val 曾被计成 652 行）。集中在此，正文不出现裸花括号。
const LBRACE: char = '{';
const RBRACE: char = '}';

#[derive(Debug, Clone)]
pub(super) enum TomlVal {
    Str(String),
    /// `{ zh = "…", en = "…" }`
    I18n {
        zh: String,
        en: Option<String>,
    },
    /// 裸字面量（数字 / bool）
    Raw(String),
}

impl TomlVal {
    pub(super) fn as_str(&self, what: &str, line: usize) -> Result<String, String> {
        match self {
            TomlVal::Str(s) => Ok(s.clone()),
            _ => Err(format!("pf-plugin.toml 行 {}：{what} 需为字符串", line + 1)),
        }
    }
    pub(super) fn as_i18n(&self, what: &str, line: usize) -> Result<(String, Option<String>), String> {
        match self {
            TomlVal::I18n { zh, en } => Ok((zh.clone(), en.clone())),
            TomlVal::Str(s) => Ok((s.clone(), None)), // 宽松：纯字符串当 zh
            _ => Err(format!(
                "pf-plugin.toml 行 {}：{what} 需为文案对象",
                line + 1
            )),
        }
    }
    pub(super) fn as_raw(&self, what: &str, line: usize) -> Result<String, String> {
        match self {
            TomlVal::Raw(s) => Ok(s.clone()),
            _ => Err(format!("pf-plugin.toml 行 {}：{what} 非法取值", line + 1)),
        }
    }
}

/// 解析一行值：`"…"` / `{ zh = "…", en = "…" }` / 裸字面量。
pub(super) fn parse_val(v: &str, line: usize) -> Result<TomlVal, String> {
    let v = v.trim();
    if let Some(rest) = v.strip_prefix('"') {
        let end = rest
            .rfind('"')
            .ok_or(format!("pf-plugin.toml 行 {}：字符串未闭合", line + 1))?;
        if rest[end + 1..].trim() != "" {
            return Err(format!(
                "pf-plugin.toml 行 {}：字符串后有多余内容",
                line + 1
            ));
        }
        return Ok(TomlVal::Str(
            rest[..end].replace("\\\"", "\"").replace("\\\\", "\\"),
        ));
    }
    if v.starts_with(LBRACE) {
        let inner = v
            .strip_prefix(LBRACE)
            .and_then(|s| s.strip_suffix(RBRACE))
            .ok_or(format!("pf-plugin.toml 行 {}：文案对象未闭合", line + 1))?;
        let mut zh = None;
        let mut en = None;
        // 极简扫描：逐个 `key = "value"` 对（值内不含引号——清单文案域够用）
        let mut rest = inner.trim();
        while !rest.is_empty() {
            let eq = rest
                .find('=')
                .ok_or(format!("pf-plugin.toml 行 {}：文案对象缺 =", line + 1))?;
            let key = rest[..eq].trim().trim_matches(',').to_string();
            let after = rest[eq + 1..].trim();
            if !after.starts_with('"') {
                return Err(format!(
                    "pf-plugin.toml 行 {}：文案对象值需为字符串",
                    line + 1
                ));
            }
            let close = after[1..].find('"').ok_or(format!(
                "pf-plugin.toml 行 {}：文案对象字符串未闭合",
                line + 1
            ))?;
            let val = after[1..1 + close].to_string();
            match key.as_str() {
                "zh" => zh = Some(val),
                "en" => en = Some(val),
                other => {
                    return Err(format!(
                        "pf-plugin.toml 行 {}：未知文案键 {other}",
                        line + 1
                    ));
                }
            }
            rest = after[1 + close + 1..].trim_start_matches(',');
            rest = rest.trim();
        }
        let Some(zh) = zh else {
            return Err(format!(
                "pf-plugin.toml 行 {}：文案对象缺 zh（U7）",
                line + 1
            ));
        };
        return Ok(TomlVal::I18n { zh, en });
    }
    if v.is_empty() || v.contains('"') || v.contains(LBRACE) {
        return Err(format!("pf-plugin.toml 行 {}：无法解析的值", line + 1));
    }
    Ok(TomlVal::Raw(v.to_string()))
}

pub(super) fn is_kebab(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars().last().is_some_and(|c| c.is_ascii_alphanumeric())
}

/// 剥行内注释：引号外的 `#` 起截断（02 §2 示例清单自带行内注释，必须支持；
/// 引号内的 `#` 是文案内容，保留）。
pub(super) fn strip_inline_comment(line: &str) -> &str {
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in line.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' if in_str => escaped = true,
            '"' => in_str = !in_str,
            '#' if !in_str => return line[..i].trim_end(),
            _ => {}
        }
    }
    line
}
