//! tabs 域测试外移（激活跟随移动 / 快照瞬态映射）。

use super::shifted_active;

/// 换位平移穷举：[t0..t4] 里枚举全部 (from, to, a)，用「模拟序列
/// remove/insert 后查同元素下标」做 oracle 对照。
#[test]
fn active_index_follows_tab_move() {
    for n in 1..=5usize {
        let tabs: Vec<usize> = (0..n).collect();
        for from in 0..n {
            for to in 0..n {
                let mut moved = tabs.clone();
                let t = moved.remove(from);
                moved.insert(to, t);
                for a in 0..n {
                    let elem = tabs[a];
                    let want = moved.iter().position(|&e| e == elem).unwrap();
                    assert_eq!(
                        shifted_active(a, from, to),
                        want,
                        "n={n} from={from} to={to} a={a}"
                    );
                }
            }
        }
    }
}

/// 存档快照（pi-flash-weia）：瞬态页不进存档且激活位随之前移映射；
/// 终端 id 换 cwd；CAP 截尾后激活位越界被读侧滤掉。
#[test]
fn tabs_state_snapshot_maps_active_past_transient() {
    use super::tabs_state_from_parts;
    use crate::PanelTab;
    use std::path::PathBuf;

    let file = |s: &str| PanelTab::File(PathBuf::from(s));
    let strip = [
        file("D:\\a\\x.rs"),
        PanelTab::Search, // 瞬态：不进存档
        PanelTab::Term(7),
        file("D:\\a\\y.rs"),
    ];
    let term_cwd = |id: usize| (id == 7).then(|| PathBuf::from("D:\\a"));

    // 激活位指向瞬态 Search：存档里无对应条目 → active=None
    let st = tabs_state_from_parts(&strip, term_cwd, Some(1), false);
    assert_eq!(st.tabs.len(), 3, "瞬态页不进存档");
    assert_eq!(st.active, None, "指向瞬态页的激活位不采信");
    assert!(!st.chat);

    // 激活位指向 Term(7)：映射到存档下标 1；激活位是 term → chat=false
    let st = tabs_state_from_parts(&strip, term_cwd, Some(2), false);
    assert_eq!(st.active, Some(1));
    assert_eq!(st.tabs[1].kind, "term");
    assert_eq!(st.tabs[1].path, "D:\\a");

    // 前台是会话 → chat=true，激活位照存（回到会话一键）
    let st = tabs_state_from_parts(&strip, term_cwd, Some(3), true);
    assert!(st.chat);
    assert_eq!(st.active, Some(2), "瞬态页剪掉后 y.rs 前移到 2");

    // 未知终端 id（terminals 已丢）：条目整体跳过
    let st = tabs_state_from_parts(&strip, |_| None, None, true);
    assert_eq!(st.tabs.len(), 2, "无 cwd 的终端不进存档");

    // CAP 截尾：截到 TABS_SAVE_CAP 条，不多不少
    let cap = crate::services::workspace::TABS_SAVE_CAP;
    let many: Vec<PanelTab> = (0..cap + 5)
        .map(|i| file(&format!("D:\\a\\f{i}.rs")))
        .collect();
    let st = tabs_state_from_parts(&many, |_| None, None, true);
    assert_eq!(st.tabs.len(), cap);
}

/// 047-P2-2 soft cap 逐出选择（纯函数口径锁）：有序表头起第一个非脏
/// File——脏/冲突跳过、非 File tab 不拦路、全脏 None 放行。
#[test]
fn eviction_victim_picks_oldest_clean_file() {
    use super::open::eviction_victim;
    use crate::PanelTab;
    use std::path::{Path, PathBuf};

    let file = |s: &str| PanelTab::File(PathBuf::from(s));
    let dirty_set: &[&str] = &["D:\\a\\dirty.rs", "D:\\a\\conflict.rs"];
    let dirty = |p: &Path| dirty_set.iter().any(|d| Path::new(d) == p);

    // 最旧干净者胜：表头脏 tab 跳过
    let tabs = [
        file("D:\\a\\dirty.rs"),
        file("D:\\a\\old.rs"),
        file("D:\\a\\new.rs"),
    ];
    assert_eq!(
        eviction_victim(&tabs, &dirty),
        Some(Path::new("D:\\a\\old.rs")),
        "从最旧起第一个非脏"
    );

    // 非 File tab（终端/搜索等瞬态页）不参与也不拦路
    let tabs = [PanelTab::Term(3), PanelTab::Search, file("D:\\a\\x.rs")];
    assert_eq!(
        eviction_victim(&tabs, &dirty),
        Some(Path::new("D:\\a\\x.rs"))
    );

    // 全脏 → None（soft cap 放行，不硬拒）
    let tabs = [file("D:\\a\\dirty.rs"), file("D:\\a\\conflict.rs")];
    assert_eq!(eviction_victim(&tabs, &dirty), None);

    // 空 tab 带 → None
    assert_eq!(eviction_victim(&[], &dirty), None);
}
