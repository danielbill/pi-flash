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

fn cmp(a: &str, a_is_file: bool, b: &str, b_is_file: bool, mode: SortMode) -> Ordering {
    compare_entry_names((a, a_is_file), (b, b_is_file), mode, SortOrder::Default)
}

#[test]
fn test_compare_entry_names() {
    use SortMode::*;
    // 目录先（DirectoriesFirst）
    assert_eq!(cmp("z", false, "a", true, DirectoriesFirst), Ordering::Less);
    assert_eq!(
        cmp("a", true, "z", false, DirectoriesFirst),
        Ordering::Greater
    );
    // 数字按值
    assert_eq!(cmp("2", true, "10", true, DirectoriesFirst), Ordering::Less);
    // stem 相同比扩展名（Zed 语义：a.md < a.txt）
    assert_eq!(
        cmp("a.md", true, "a.txt", true, DirectoriesFirst),
        Ordering::Less
    );
    // 大小写不敏感，小写优先
    assert_eq!(
        cmp("B", true, "a", true, DirectoriesFirst),
        Ordering::Greater
    );
    assert_eq!(cmp("a", true, "A", true, DirectoriesFirst), Ordering::Less);
    // Mixed：文件目录混排，同名时文件靠后
    assert_eq!(cmp("z", true, "a", false, Mixed), Ordering::Greater);
    assert_eq!(cmp("a", true, "A", false, Mixed), Ordering::Greater);
    // FilesFirst
    assert_eq!(cmp("z", true, "a", false, FilesFirst), Ordering::Less);
}
