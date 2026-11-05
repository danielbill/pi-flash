use super::*;

#[test]
fn dir_aliases_cover_market_and_flat_layouts() {
    // 市场安装链：scope 展开成 `__`
    assert_eq!(
        dir_aliases("@pi-flash-plugins/quick-panel"),
        vec![
            "@pi-flash-plugins__quick-panel".to_string(),
            "quick-panel".to_string()
        ]
    );
    // dev 手工平铺 / 无 scope 包：两个候选相同，不重复
    assert_eq!(dir_aliases("quick-panel"), vec!["quick-panel".to_string()]);
    // scope 自身含 `__` 时短名仍可取
    assert_eq!(dir_aliases("@a__b/x")[1], "x");
}
