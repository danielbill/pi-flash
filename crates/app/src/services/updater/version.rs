//! 版本比较工具（081）：逐段数值比较（非字典序）+ 当前版本。
//! 自 updater.rs 拆出（090 重构）。

// ---------------------------------------------------------------------------
// 版本号比较（x.y.z 逐段数值；缺段/非数按 0）
// ---------------------------------------------------------------------------

pub(crate) fn version_key(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches('v')
        .split('.')
        .map(|p| p.trim().parse().unwrap_or(0))
        .collect()
}

/// a 是否严格大于 b。
pub(crate) fn version_gt(a: &str, b: &str) -> bool {
    let (ka, kb) = (version_key(a), version_key(b));
    let n = ka.len().max(kb.len());
    for i in 0..n {
        let (x, y) = (
            ka.get(i).copied().unwrap_or(0),
            kb.get(i).copied().unwrap_or(0),
        );
        if x != y {
            return x > y;
        }
    }
    false
}

pub(crate) fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub(crate) fn version_key_string(v: &str) -> String {
    v.trim().trim_start_matches('v').to_string()
}

/// `x.y.z` 形状（≥2 段、全数字）。用来在目录名里认出「版本」段。
pub(crate) fn is_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() >= 2
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare() {
        assert!(version_gt("1.2.3", "1.2.2"));
        assert!(version_gt("v1.2.3", "1.2.2"));
        assert!(version_gt("1.10.0", "1.9.9"), "逐段数值比较，不是字典序");
        assert!(version_gt("0.2.0", "0.1.99"));
        assert!(!version_gt("1.2.3", "1.2.3"));
        assert!(!version_gt("1.2", "1.2.0"), "缺段按 0");
        assert!(!version_gt("0.1.0", "0.2.0"));
        assert!(!version_gt("abc", "0.0.1"), "非数段按 0");
    }
}
