//! wiki 图片引用展开（025 P1，自 doc.rs 外移）：`![[a.png]]` →
//! `![](a.png)`，独占一行的图片引用前后补空行（fence 感知，纯文本
//! `[[wiki链接]]` 不碰）。

/// 把源码里的 wiki 图片引用展开为标准 markdown（逐行，fence 感知）。
/// `![[a.png]]` → `![](a.png)`；`![[a.png|300]]` 取尺寸前的文件名。
/// 另：独占一行的图片引用（wiki 或标准式）前后补空行——Obsidian 习惯
/// 回车即图（图与文字间常无空行），GFM 同段落不产图片块，不补分割
/// 预览就不出图（025 P1 实翻车）。
pub(super) fn expand_wiki_images(src: &str) -> String {
    let mut out = String::with_capacity(src.len() + 32);
    let mut in_fence = false;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            out.push_str(line);
            continue;
        }
        if in_fence {
            out.push_str(line);
            continue;
        }
        let expanded = expand_wiki_line(line);
        let trimmed = expanded.trim();
        if is_standalone_image_ref(trimmed) {
            let blank_before = out.is_empty() || out.ends_with("\n\n");
            if !blank_before {
                out.push('\n');
            }
            out.push_str(&expanded);
            if !expanded.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        } else {
            out.push_str(&expanded);
        }
    }
    out
}

/// 整行恰为一条图片引用：`![[…]]` 或 `![](…)`（闭合到行尾）。
pub(super) fn is_standalone_image_ref(line: &str) -> bool {
    if let Some(rest) = line.strip_prefix("![[") {
        return rest.ends_with("]]") && !rest.contains('\n');
    }
    if let Some(rest) = line.strip_prefix("![](") {
        return rest.ends_with(')') && !rest.contains("![](") && !rest.contains('\n');
    }
    false
}

pub(super) fn expand_wiki_line(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    loop {
        let Some(s) = rest.find("![[") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..s]);
        let after = &rest[s + 3..];
        let Some(e) = after.find("]]") else {
            // 无闭合：原样保留剩余部分
            out.push_str(&rest[s..]);
            break;
        };
        let name = &after[..e];
        if name.is_empty() || name.contains('[') || name.contains(']') || name.contains('\n') {
            out.push_str(&rest[s..s + 3 + e + 2]);
        } else {
            // Obsidian 尺寸语法 `![[a.png|300]]`：取管道前文件名
            let file = name.split('|').next().unwrap_or(name).trim();
            out.push_str("![](");
            out.push_str(file);
            out.push(')');
        }
        rest = &after[e + 2..];
    }
    out
}
