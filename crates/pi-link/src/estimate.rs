//! pi-web `estimateTokens` parity：CJK 字符 ≈1 token/字，其余 ≈4 字符/token。
//!
//! pi-ai 自己的 `estimateTokens` 是朴素 `ceil(len/4)`（对中文严重低估），
//! pi-web 在 usage 展示上换成了 CJK 感知版；本实现与 pi-web 逐字对齐，
//! 是「系统提示词占多少 token」等所有估算展示的唯一来源
//! （app 侧直接复用；`src/bin/token-count.rs` 是它的独立 CLI 工具壳）。

/// 估算 `text` 的 token 数（pi-web estimateTokens parity）。
pub fn estimate_tokens(text: &str) -> u64 {
    let mut cjk: u64 = 0;
    let mut rest: u64 = 0;
    for ch in text.chars() {
        let c = ch as u32;
        let is_cjk = (0x3000..=0x30ff).contains(&c)
            || (0x3400..=0x9fff).contains(&c)
            || (0xf900..=0xfaff).contains(&c)
            || (0x20000..=0x2fa1f).contains(&c)
            || (0xac00..=0xd7af).contains(&c);
        if is_cjk {
            cjk += 1;
        } else {
            rest += 1;
        }
    }
    cjk + rest / 4
}

#[cfg(test)]
mod tests {
    use super::estimate_tokens;

    #[test]
    fn ascii_is_four_chars_per_token() {
        // 8 个 ASCII 字符 = 2 token
        assert_eq!(estimate_tokens("abcdefgh"), 2);
        // 整除向下取（JS 同款 `rest / 4`，不 ceil）
        assert_eq!(estimate_tokens("abc"), 0);
        assert_eq!(estimate_tokens(""), 0);
    }

    #[test]
    fn cjk_counts_one_token_per_char() {
        // 5 个汉字 = 5 token（朴素 len/4 会算成 1）
        assert_eq!(estimate_tokens("你好世界呀"), 5);
        // 中英混排：2 CJK + 8 ASCII → 2 + 2
        assert_eq!(estimate_tokens("你好abcdefgh"), 4);
    }

    #[test]
    fn counts_chars_not_bytes() {
        // 逐字计数，不做 UTF-8 字节除法：4 个汉字 = 4（12 字节）
        assert_eq!(estimate_tokens("钉版钉版"), 4);
    }
}
