//! 模型引用判别：官方 id（发给商家的接口名）与官方 name（版本名）谁更适合
//! 当「查参数的检索键」。
//!
//! 背景（2026-11-01 用户定稿）：商家 /models 返回的 id 常常是**家族名**
//!（DeepSeek 的 `deepseek-flash`，以后所有 flash 版本都叫这个），而版本信息
//! 只在 name 里（`DeepSeek-V4.1-Flash`）。OpenRouter 目录对家族名往往只有
//! 「…-latest」别名通配条目（参数是聚合值，不精确），对版本名才有具体型号
//! 条目（ctx/maxTokens 是真值）。所以【自动填参】要多带一条「name 派生的
//! 检索键」，**只用于后台查询**——`models.json` 里保存的 id 永远是官方列表
//! 值，UI 也只显示官方值，用户不需要知道这层。
//!
//! 判别不靠字符串长相猜（`deepseek-flash` 与 `deepseek-v4.1-flash-1017`
//! 都像 id，猜必错）：这里只按「id → name 派生」产出**并列候选**，由目录
//! 结果裁决（具体型号优先、别名兜底，见 `openrouter::resolve_many`）。

/// name → 目录 slug：剥厂商前缀（`DeepSeek: `）、小写、空白/下划线转 `-`、
/// 剔非法字符、压重复 `-`。
///
/// `"DeepSeek: DeepSeek V4.1 Flash 1017"` → `"deepseek-v4.1-flash-1017"`；
/// `"DeepSeek-V4.1-Flash"` → `"deepseek-v4.1-flash"`。
pub fn slugify(name: &str) -> String {
    let s = name.trim();
    // 厂商前缀（"Z.ai: GLM 5.3" / "DeepSeek: DeepSeek V4.1 Flash"）
    let s = match s.split_once(": ") {
        Some((_, rest)) if !rest.trim().is_empty() => rest.trim(),
        _ => s,
    };
    let mut out = String::with_capacity(s.len());
    for ch in s.to_lowercase().chars() {
        let c = match ch {
            ch if ch.is_ascii_alphanumeric() => ch,
            // 版本点、变体冒号、author 斜杠、分隔符原样保留
            '.' | ':' | '/' | '-' => ch,
            c if c.is_whitespace() || c == '_' => '-',
            _ => continue,
        };
        if c == '-' && (out.is_empty() || out.ends_with('-')) {
            continue;
        }
        out.push(c);
    }
    out.trim_end_matches('-').to_string()
}

/// 【自动填参】的检索键候选（按尝试顺序，去空去重）：
/// 1. 官方 id（本身能命中具体型号时就用它，行为与今天一致）；
/// 2. `slugify(name)`（版本名 → 目录 slug，具体型号条目）。
///
/// 命中优先级由 `openrouter::resolve_many` 裁决：**具体型号 > 别名通配**，
/// 同等档位才看候选顺序。name 已与 slug 同形时不重复出现。
pub fn query_candidates(id: &str, name: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |v: &str| {
        let v = v.trim();
        if !v.is_empty() && !out.iter().any(|x| x.eq_ignore_ascii_case(v)) {
            out.push(v.to_string());
        }
    };
    push(id);
    if !name.trim().is_empty() {
        push(&slugify(name));
    }
    out
}

/// 账本条目是否就是这个模型引用（点列表进详情时的兜底匹配）。
///
/// 精确 id 优先由调用方先试一遍；本函数兜的是「目录 id 与账本 id 形态不同」
/// 的三种情况：忽略大小写、slug 等价（`DeepSeek V4.1 Flash` ↔
/// `deepseek-v4.1-flash`）、目录拿 name 当 id（与账本 name 相等）。
pub fn matches_ref(id: &str, name: &str, needle: &str) -> bool {
    let n = needle.trim();
    if n.is_empty() {
        return false;
    }
    if id.eq_ignore_ascii_case(n) || (!name.is_empty() && name.eq_ignore_ascii_case(n)) {
        return true;
    }
    let ns = slugify(n);
    if ns.is_empty() {
        return false;
    }
    slugify(id) == ns || (!name.is_empty() && slugify(name) == ns)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_name_to_catalog_slug() {
        assert_eq!(slugify("DeepSeek-V4.1-Flash"), "deepseek-v4.1-flash");
        assert_eq!(
            slugify("DeepSeek: DeepSeek V4.1 Flash 1017"),
            "deepseek-v4.1-flash-1017"
        );
        assert_eq!(slugify("Z.ai: GLM 5.3"), "glm-5.3");
        assert_eq!(
            slugify("  NVIDIA_Nemotron 3.5   Lightning "),
            "nvidia-nemotron-3.5-lightning"
        );
        assert_eq!(slugify("Qwen2.5 72B Instruct"), "qwen2.5-72b-instruct");
        // 已经是 id 形态：幂等
        assert_eq!(slugify("deepseek-flash"), "deepseek-flash");
        assert_eq!(
            slugify("deepseek/deepseek-flash:free"),
            "deepseek/deepseek-flash:free"
        );
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("—"), "");
    }

    #[test]
    fn query_candidates_id_then_version_name() {
        // 实测原型：官方 id 是家族名，name 才是版本
        assert_eq!(
            query_candidates("deepseek-flash", "DeepSeek-V4.1-Flash"),
            vec![
                "deepseek-flash".to_string(),
                "deepseek-v4.1-flash".to_string()
            ]
        );
        // 带厂商前缀 / 空格的版本名
        assert_eq!(
            query_candidates("deepseek-flash", "DeepSeek: DeepSeek V4.1 Flash 1017"),
            vec![
                "deepseek-flash".to_string(),
                "deepseek-v4.1-flash-1017".to_string()
            ]
        );
        // 无 name（手输新模型）→ 只有官方 id，行为与旧版一致
        assert_eq!(query_candidates("glm-5.3", ""), vec!["glm-5.3".to_string()]);
        // id 空、只有 name → 用 name 兜底
        assert_eq!(
            query_candidates("", "DeepSeek V4.1 Flash"),
            vec!["deepseek-v4.1-flash".to_string()]
        );
        // name 已是 slug 形态 → 不重复
        assert_eq!(
            query_candidates("deepseek-v4.1-flash", "deepseek-v4.1-flash"),
            vec!["deepseek-v4.1-flash".to_string()]
        );
    }

    #[test]
    fn matches_ref_fallback_forms() {
        // 精确 id
        assert!(matches_ref(
            "deepseek-flash",
            "DeepSeek-V4.1-Flash",
            "deepseek-flash"
        ));
        // 忽略大小写
        assert!(matches_ref("GLM-5.3", "GLM 5.3", "glm-5.3"));
        // 目录拿 name 当 id：与账本 name 相等
        assert!(matches_ref(
            "deepseek-flash",
            "DeepSeek V4.1 Flash",
            "DeepSeek V4.1 Flash"
        ));
        // slug 等价
        assert!(matches_ref(
            "deepseek-flash",
            "DeepSeek-V4.1-Flash",
            "deepseek v4.1 flash"
        ));
        // 不相干的模型不能被误配
        assert!(!matches_ref(
            "deepseek-flash",
            "DeepSeek-V4.1-Flash",
            "deepseek-v4-pro"
        ));
        assert!(!matches_ref("deepseek-flash", "DeepSeek-V4.1-Flash", ""));
    }
}
