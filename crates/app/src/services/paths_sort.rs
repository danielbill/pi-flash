//! 路径排序 —— 挪自 Zed `crates/util/src/paths.rs`（自然排序 / SortMode /
//! SortOrder / 叶子级条目比较），去掉了 RelPath/WSL 等基建依赖：本仓树形
//! 展平按「单目录内条目」排序，即 Zed `compare_rel_paths_by` 的叶子分支。
//! 语义保持一致：目录先（DirectoriesFirst）、文件按 stem 比较、扩展名做
//! 次级键、数字段按数值比较（file2 < file10）、大小写不敏感且小写优先。

use std::cmp::Ordering;
use std::path::Path;

// ---------------------------------------------------------------------------
// natural_sort —— 逐字搬自 Zed paths.rs:916-1035（compare_numeric_segments /
// natural_sort / natural_sort_no_tiebreak）
// ---------------------------------------------------------------------------

fn compare_numeric_segments<I>(a_iter: &mut std::iter::Peekable<I>, b_iter: &mut std::iter::Peekable<I>) -> Ordering
where
    I: Iterator<Item = char>,
{
    // Collect all consecutive digits into strings
    let mut a_num_str = String::new();
    let mut b_num_str = String::new();

    while let Some(&c) = a_iter.peek() {
        if !c.is_ascii_digit() {
            break;
        }

        a_num_str.push(c);
        a_iter.next();
    }

    while let Some(&c) = b_iter.peek() {
        if !c.is_ascii_digit() {
            break;
        }

        b_num_str.push(c);
        b_iter.next();
    }

    // First compare lengths (handle leading zeros)
    match a_num_str.len().cmp(&b_num_str.len()) {
        Ordering::Equal => {
            // Same length, compare digit by digit
            match a_num_str.cmp(&b_num_str) {
                Ordering::Equal => Ordering::Equal,
                ordering => ordering,
            }
        }

        // Different lengths but same value means leading zeros
        ordering => {
            // Try parsing as numbers first
            if let (Ok(a_val), Ok(b_val)) = (a_num_str.parse::<u128>(), b_num_str.parse::<u128>()) {
                match a_val.cmp(&b_val) {
                    Ordering::Equal => ordering, // Same value, longer one is greater (leading zeros)
                    ord => ord,
                }
            } else {
                // If parsing fails (overflow), compare as strings
                a_num_str.cmp(&b_num_str)
            }
        }
    }
}

/// Performs natural sorting comparison between two strings.
///
/// Natural sorting is an ordering that handles numeric sequences in a way that matches human expectations.
/// For example, "file2" comes before "file10" (unlike standard lexicographic sorting).
///
/// # Characteristics
///
/// * Case-sensitive with lowercase priority: When comparing same letters, lowercase comes before uppercase
/// * Numbers are compared by numeric value, not character by character
/// * Leading zeros affect ordering when numeric values are equal
/// * Can handle numbers larger than u128::MAX (falls back to string comparison)
/// * When strings are equal case-insensitively, lowercase is prioritized (lowercase < uppercase)
pub fn natural_sort(a: &str, b: &str) -> Ordering {
    let mut a_iter = a.chars().peekable();
    let mut b_iter = b.chars().peekable();

    loop {
        match (a_iter.peek(), b_iter.peek()) {
            (None, None) => {
                return b.cmp(a);
            }
            (None, _) => return Ordering::Less,
            (_, None) => return Ordering::Greater,
            (Some(&a_char), Some(&b_char)) => {
                if a_char.is_ascii_digit() && b_char.is_ascii_digit() {
                    match compare_numeric_segments(&mut a_iter, &mut b_iter) {
                        Ordering::Equal => continue,
                        ordering => return ordering,
                    }
                } else {
                    match a_char.to_ascii_lowercase().cmp(&b_char.to_ascii_lowercase()) {
                        Ordering::Equal => {
                            a_iter.next();
                            b_iter.next();
                        }
                        ordering => return ordering,
                    }
                }
            }
        }
    }
}

/// Case-insensitive natural sort without applying the final lowercase/uppercase tie-breaker.
/// This is useful when comparing individual path components where we want to keep walking
/// deeper components before deciding on casing.
fn natural_sort_no_tiebreak(a: &str, b: &str) -> Ordering {
    if a.eq_ignore_ascii_case(b) {
        Ordering::Equal
    } else {
        natural_sort(a, b)
    }
}

fn stem_and_extension(filename: &str) -> (Option<&str>, Option<&str>) {
    if filename.is_empty() {
        return (None, None);
    }

    match filename.rsplit_once('.') {
        // Case 1: No dot was found. The entire name is the stem.
        None => (Some(filename), None),

        // Case 2: A dot was found.
        Some((before, after)) => {
            // This is the crucial check for dotfiles like ".bashrc".
            // If `before` is empty, the dot was the first character.
            // In that case, we revert to the "whole name is the stem" logic.
            if before.is_empty() {
                (Some(filename), None)
            } else {
                // Otherwise, we have a standard stem and extension.
                (Some(before), Some(after))
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SortOrder / SortMode + 叶子级比较 —— 搬自 Zed paths.rs:1061-1241
// （compare_rel_paths_by 收敛为「同目录两兄弟条目」的比较）
// ---------------------------------------------------------------------------

/// Controls the lexicographic sorting of file and folder names.
/// （Zed 全量枚举搬入，设置页接 sort_mode 时用 Upper/Lower/Unicode）
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum SortOrder {
    /// Case-insensitive natural sort with lowercase preferred in ties.
    /// Numbers in file names are compared by value (e.g., `file2` before `file10`).
    #[default]
    Default,
    /// Uppercase names are grouped before lowercase names, with case-insensitive
    /// natural sort within each group. Dot-prefixed names sort before both groups.
    Upper,
    /// Lowercase names are grouped before uppercase names, with case-insensitive
    /// natural sort within each group. Dot-prefixed names sort before both groups.
    Lower,
    /// Pure Unicode codepoint comparison. No case folding, no natural number sorting.
    /// Uppercase ASCII sorts before lowercase. Accented characters sort after ASCII.
    Unicode,
}

/// Controls how files and directories are ordered relative to each other.
/// （Zed 全量枚举搬入，设置页接 sort_mode 时用 Mixed/FilesFirst）
#[allow(dead_code)]
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum SortMode {
    /// Directories are listed before files at each level.
    #[default]
    DirectoriesFirst,
    /// Files and directories are interleaved alphabetically.
    Mixed,
    /// Files are listed before directories at each level.
    FilesFirst,
}

fn case_group_key(name: &str, order: SortOrder) -> u8 {
    let first = match name.chars().next() {
        Some(c) => c,
        None => return 0,
    };
    match order {
        SortOrder::Upper => {
            if first.is_lowercase() {
                1
            } else {
                0
            }
        }
        SortOrder::Lower => {
            if first.is_uppercase() {
                1
            } else {
                0
            }
        }
        _ => 0,
    }
}

fn compare_strings(a: &str, b: &str, order: SortOrder) -> Ordering {
    match order {
        SortOrder::Unicode => a.cmp(b),
        _ => natural_sort(a, b),
    }
}

fn compare_strings_no_tiebreak(a: &str, b: &str, order: SortOrder) -> Ordering {
    match order {
        SortOrder::Unicode => a.cmp(b),
        _ => natural_sort_no_tiebreak(a, b),
    }
}

/// 同目录两个兄弟条目的比较（Zed `compare_rel_paths_by` 的叶子分支）：
/// `a`/`b` 是条目名，`is_file` 标记是否为文件（目录传 false）。
pub fn compare_entry_names(
    (a, a_is_file): (&str, bool),
    (b, b_is_file): (&str, bool),
    mode: SortMode,
    order: SortOrder,
) -> Ordering {
    let file_dir_ordering = match mode {
        SortMode::DirectoriesFirst => a_is_file.cmp(&b_is_file),
        SortMode::FilesFirst => b_is_file.cmp(&a_is_file),
        SortMode::Mixed => Ordering::Equal,
    };
    if !file_dir_ordering.is_eq() {
        return file_dir_ordering;
    }

    let (a_stem, a_ext) = if a_is_file {
        stem_and_extension(a)
    } else {
        (Some(a), None)
    };
    let (b_stem, b_ext) = if b_is_file {
        stem_and_extension(b)
    } else {
        (Some(b), None)
    };

    let mut name_cmp = match (a_stem, b_stem) {
        (Some(a), Some(b)) => {
            let name_cmp = case_group_key(a, order)
                .cmp(&case_group_key(b, order))
                .then_with(|| match mode {
                    SortMode::DirectoriesFirst => compare_strings(a, b, order),
                    _ => compare_strings_no_tiebreak(a, b, order),
                });

            let name_cmp = if mode == SortMode::Mixed {
                name_cmp.then_with(|| match (a_is_file, b_is_file) {
                    (true, false) if a.eq_ignore_ascii_case(b) => Ordering::Greater,
                    (false, true) if a.eq_ignore_ascii_case(b) => Ordering::Less,
                    _ => Ordering::Equal,
                })
            } else {
                name_cmp
            };

            name_cmp.then_with(|| {
                if a_is_file && b_is_file {
                    match order {
                        SortOrder::Unicode => a_ext.unwrap_or_default().cmp(b_ext.unwrap_or_default()),
                        _ => {
                            let a_ext_str = a_ext.unwrap_or_default().to_lowercase();
                            let b_ext_str = b_ext.unwrap_or_default().to_lowercase();
                            a_ext_str.cmp(&b_ext_str)
                        }
                    }
                } else {
                    Ordering::Equal
                }
            })
        }
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    };
    // DirectoriesFirst 模式下同名同大小写时保留稳定序（Zed 最终
    // tiebreak 走整路径比较；这里条目名已唯一，无需再比）。
    if mode == SortMode::DirectoriesFirst && name_cmp == Ordering::Equal {
        return Ordering::Equal;
    }
    if name_cmp == Ordering::Equal {
        name_cmp = compare_strings(a, b, order);
    }
    name_cmp
}

// ---------------------------------------------------------------------------
// PathExt 精选 —— 搬自 Zed paths.rs:123-179（file_icons 匹配用）
// ---------------------------------------------------------------------------

/// Returns a file's extension or, if the file is hidden, its name without the leading dot
pub fn extension_or_hidden_file_name(path: &Path) -> Option<&str> {
    let file_name = path.file_name()?.to_str()?;
    if file_name.starts_with('.') {
        return file_name.strip_prefix('.');
    }

    path.extension()
        .and_then(|e| e.to_str())
        .or_else(|| path.file_stem()?.to_str())
}

/// Returns a file's "full" joined collection of extensions, in the case where a file does not
/// just have a singular extension but instead has multiple (e.g File.tar.gz, Component.stories.tsx)
///
/// Will provide back the extensions joined together such as tar.gz or stories.tsx
pub fn multiple_extensions(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_str()?;

    let parts: Vec<&str> = file_name
        .split('.')
        // Skip the part with the file name extension
        .skip(1)
        .collect();

    if parts.len() < 2 {
        return None;
    }

    Some(parts.join("."))
}

// ---------------------------------------------------------------------------
// 测试 —— test_natural_sort / test_compare_numeric_segments 逐字搬自 Zed
// paths.rs:2756-2876（#[perf] 改 #[test]）；条目比较为叶子分支等价适配
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compare_numeric_segments() {
        // Helper function to create peekable iterators and test
        fn compare(a: &str, b: &str) -> Ordering {
            let mut a_iter = a.chars().peekable();
            let mut b_iter = b.chars().peekable();

            let result = compare_numeric_segments(&mut a_iter, &mut b_iter);

            // Verify iterators advanced correctly
            assert!(
                !a_iter.next().is_some_and(|c| c.is_ascii_digit()),
                "Iterator a should have consumed all digits"
            );
            assert!(
                !b_iter.next().is_some_and(|c| c.is_ascii_digit()),
                "Iterator b should have consumed all digits"
            );

            result
        }

        // Basic numeric comparisons
        assert_eq!(compare("0", "0"), Ordering::Equal);
        assert_eq!(compare("1", "2"), Ordering::Less);
        assert_eq!(compare("9", "10"), Ordering::Less);
        assert_eq!(compare("10", "9"), Ordering::Greater);
        assert_eq!(compare("99", "100"), Ordering::Less);

        // Leading zeros
        assert_eq!(compare("0", "00"), Ordering::Less);
        assert_eq!(compare("00", "0"), Ordering::Greater);
        assert_eq!(compare("01", "1"), Ordering::Greater);
        assert_eq!(compare("001", "1"), Ordering::Greater);
        assert_eq!(compare("001", "01"), Ordering::Greater);

        // Same value different representation
        assert_eq!(compare("000100", "100"), Ordering::Greater);
        assert_eq!(compare("100", "0100"), Ordering::Less);
        assert_eq!(compare("0100", "00100"), Ordering::Less);

        // Large numbers
        assert_eq!(compare("9999999999", "10000000000"), Ordering::Less);
        assert_eq!(
            compare(
                "340282366920938463463374607431768211455", // u128::MAX
                "340282366920938463463374607431768211456"
            ),
            Ordering::Less
        );
        assert_eq!(
            compare(
                "340282366920938463463374607431768211456", // > u128::MAX
                "340282366920938463463374607431768211455"
            ),
            Ordering::Greater
        );

        // Iterator advancement verification
        let mut a_iter = "123abc".chars().peekable();
        let mut b_iter = "456def".chars().peekable();

        compare_numeric_segments(&mut a_iter, &mut b_iter);

        assert_eq!(a_iter.collect::<String>(), "abc");
        assert_eq!(b_iter.collect::<String>(), "def");
    }

    #[test]
    fn test_natural_sort() {
        // Basic alphanumeric
        assert_eq!(natural_sort("a", "b"), Ordering::Less);
        assert_eq!(natural_sort("b", "a"), Ordering::Greater);
        assert_eq!(natural_sort("a", "a"), Ordering::Equal);

        // Case sensitivity
        assert_eq!(natural_sort("a", "A"), Ordering::Less);
        assert_eq!(natural_sort("A", "a"), Ordering::Greater);
        assert_eq!(natural_sort("aA", "aa"), Ordering::Greater);
        assert_eq!(natural_sort("aa", "aA"), Ordering::Less);

        // Numbers
        assert_eq!(natural_sort("1", "2"), Ordering::Less);
        assert_eq!(natural_sort("2", "10"), Ordering::Less);
        assert_eq!(natural_sort("02", "10"), Ordering::Less);
        assert_eq!(natural_sort("02", "2"), Ordering::Greater);

        // Mixed alphanumeric
        assert_eq!(natural_sort("a1", "a2"), Ordering::Less);
        assert_eq!(natural_sort("a2", "a10"), Ordering::Less);
        assert_eq!(natural_sort("a02", "a2"), Ordering::Greater);
        assert_eq!(natural_sort("a1b", "a1c"), Ordering::Less);

        // Multiple numeric segments
        assert_eq!(natural_sort("1a2", "1a10"), Ordering::Less);
        assert_eq!(natural_sort("1a10", "1a2"), Ordering::Greater);
        assert_eq!(natural_sort("2a1", "10a1"), Ordering::Less);

        // Special characters
        assert_eq!(natural_sort("a-1", "a-2"), Ordering::Less);
        assert_eq!(natural_sort("a_1", "a_2"), Ordering::Less);
        assert_eq!(natural_sort("a.1", "a.2"), Ordering::Less);

        // Unicode
        assert_eq!(natural_sort("文1", "文2"), Ordering::Less);
        assert_eq!(natural_sort("文2", "文10"), Ordering::Less);
        assert_eq!(natural_sort("🔤1", "🔤2"), Ordering::Less);

        // Empty and special cases
        assert_eq!(natural_sort("", ""), Ordering::Equal);
        assert_eq!(natural_sort("", "a"), Ordering::Less);
        assert_eq!(natural_sort("a", ""), Ordering::Greater);
        assert_eq!(natural_sort(" ", "  "), Ordering::Less);

        // Mixed everything
        assert_eq!(natural_sort("File-1.txt", "File-2.txt"), Ordering::Less);
        assert_eq!(natural_sort("File-02.txt", "File-2.txt"), Ordering::Greater);
        assert_eq!(natural_sort("File-2.txt", "File-10.txt"), Ordering::Less);
        assert_eq!(natural_sort("File_A1", "File_A2"), Ordering::Less);
        assert_eq!(natural_sort("File_a1", "File_A1"), Ordering::Less);
    }

    fn cmp(
        a: &str,
        a_is_file: bool,
        b: &str,
        b_is_file: bool,
        mode: SortMode,
    ) -> Ordering {
        compare_entry_names((a, a_is_file), (b, b_is_file), mode, SortOrder::Default)
    }

    #[test]
    fn test_compare_entry_names() {
        use SortMode::*;
        // 目录先（DirectoriesFirst）
        assert_eq!(cmp("z", false, "a", true, DirectoriesFirst), Ordering::Less);
        assert_eq!(cmp("a", true, "z", false, DirectoriesFirst), Ordering::Greater);
        // 数字按值
        assert_eq!(cmp("2", true, "10", true, DirectoriesFirst), Ordering::Less);
        // stem 相同比扩展名（Zed 语义：a.md < a.txt）
        assert_eq!(cmp("a.md", true, "a.txt", true, DirectoriesFirst), Ordering::Less);
        // 大小写不敏感，小写优先
        assert_eq!(cmp("B", true, "a", true, DirectoriesFirst), Ordering::Greater);
        assert_eq!(cmp("a", true, "A", true, DirectoriesFirst), Ordering::Less);
        // Mixed：文件目录混排，同名时文件靠后
        assert_eq!(cmp("z", true, "a", false, Mixed), Ordering::Greater);
        assert_eq!(cmp("a", true, "A", false, Mixed), Ordering::Greater);
        // FilesFirst
        assert_eq!(cmp("z", true, "a", false, FilesFirst), Ordering::Less);
    }
}
