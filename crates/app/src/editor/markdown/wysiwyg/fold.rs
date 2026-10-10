//! 折叠集操作：合并、行级 reveal、doc↔vis 映射委托。
//!
//! 映射核心（`Decorations::map_doc_to_vis` / `map_vis_to_doc`）收口在
//! vendor `input/decorations.rs`（element.rs 光标、movement 方向键、点击
//! 反算都就地用它）；本模块是 app 侧入口 + **主力测试区**——所有光标/
//! 映射行为的性质测试在此，防错位 bug 散落。
//!
//! P0 reveal 粒度 = **光标所在行**（024 §6.2）：该行 folds 全部显现，
//! 其余行保持隐藏；P2 细化到光标所在行内元素。

use std::ops::Range;

use gpui_component::input::Decorations;

/// 合并重叠/相邻的折叠区间（parse 产出的嵌套强调 fold 会重叠）。
/// 输入不要求有序。
pub fn merge(mut folds: Vec<Range<usize>>) -> Vec<Range<usize>> {
    folds.retain(|f| f.start < f.end); // 丢空段
    folds.sort_by_key(|f| f.start);
    let mut out: Vec<Range<usize>> = Vec::with_capacity(folds.len());
    for f in folds {
        match out.last_mut() {
            Some(last) if f.start <= last.end => {
                last.end = last.end.max(f.end);
            }
            _ => out.push(f),
        }
    }
    out
}

/// 容器级 reveal（024 P2）：选区/光标与容器 content **相交或接触**
/// → 该容器全部定界 folds 显现（`**粗体**` 整对标记同进退，不会
/// 左显右不显）；merge 段与任一命中容器的 folds 相交即丢。空选区
/// （光标）落在段内/两端停靠位同样触发（编辑进入态）；选区强制
/// reveal：选中的文本必须可见原文（Obsidian 行为）。
pub fn reveal_containers(
    containers: &[super::parse::Container],
    merged: &[Range<usize>],
    sel: Range<usize>,
) -> Vec<Range<usize>> {
    let touched: Vec<&super::parse::Container> = containers
        .iter()
        .filter(|c| c.content.end >= sel.start && c.content.start <= sel.end)
        .collect();
    merged
        .iter()
        .filter(|m| {
            !touched.iter().any(|c| {
                c.folds
                    .iter()
                    .any(|f| f.start < m.end && m.start < f.end)
            })
        })
        .cloned()
        .collect()
}

/// doc 偏移 → 折叠后偏移（委托 vendor 单点实现）。
pub fn doc_to_vis(folds: &[Range<usize>], off: usize) -> usize {
    Decorations::map_doc_to_vis(folds, off)
}

/// 折叠后偏移 → doc 偏移。
/// （P2 点击反算入口；当前仅测试使用）
#[allow(dead_code)]
pub fn vis_to_doc(folds: &[Range<usize>], vis: usize) -> usize {
    Decorations::map_vis_to_doc(folds, vis)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::input::{Decorations, Rope};

    fn ranges(v: &[(usize, usize)]) -> Vec<Range<usize>> {
        v.iter().map(|&(a, b)| a..b).collect()
    }

    #[test]
    fn merge_sorts_and_merges_overlap() {
        let merged = merge(ranges(&[(10, 15), (0, 3), (12, 20), (5, 7), (4, 6)]));
        assert_eq!(merged, ranges(&[(0, 3), (4, 7), (10, 20)]));
    }

    #[test]
    fn merge_drops_empty_and_adjacent_merges() {
        // 相邻（end == next.start）合并为一段
        let merged = merge(ranges(&[(0, 2), (2, 4), (9, 9)]));
        assert_eq!(merged, ranges(&[(0, 4)]));
    }

    #[test]
    fn reveal_containers_drop_whole_container() {
        use super::super::parse::Container;
        // `**粗体**`：左标记 0..2、内容 2..6、右标记 6..8 —— 一个容器
        let containers = vec![Container {
            content: 2..6,
            folds: vec![0..2, 6..8],
        }];
        let merged = ranges(&[(0, 2), (6, 8), (20, 22)]);
        // 光标落在内容/停靠位 → 整容器（两枚标记）同显
        for sel in [3..3, 2..2, 6..6, 1..7] {
            let kept = reveal_containers(&containers, &merged, sel.clone());
            assert_eq!(kept, ranges(&[(20, 22)]), "sel {sel:?} 应整容器显现");
        }
        // 光标在段外 → 原样折叠
        assert_eq!(reveal_containers(&containers, &merged, 12..12), merged);
    }

    #[test]
    fn doc_to_vis_basic() {
        let folds = ranges(&[(6, 8), (20, 22)]); // 两段各 2 字节
        assert_eq!(doc_to_vis(&folds, 0), 0);
        assert_eq!(doc_to_vis(&folds, 5), 5);
        assert_eq!(doc_to_vis(&folds, 6), 6); // 段首不动
        assert_eq!(doc_to_vis(&folds, 7), 6); // 段内收拢到段首
        assert_eq!(doc_to_vis(&folds, 8), 6); // 段尾收拢
        assert_eq!(doc_to_vis(&folds, 9), 7); // 段后前移 2
        // fold2 前已隐藏 fold1 的 2 字节 → 段首/段尾都是 18，段后 -4
        assert_eq!(doc_to_vis(&folds, 20), 18);
        assert_eq!(doc_to_vis(&folds, 22), 18);
        assert_eq!(doc_to_vis(&folds, 24), 20);
    }

    #[test]
    fn vis_to_doc_roundtrip_on_visible_offsets() {
        let folds = ranges(&[(6, 8), (20, 22), (30, 31)]);
        let total = 40;
        for off in 0..total {
            // 段 [start, end) 收拢到段首（逆像优先段尾）→ 跳过起点侧；
            // 段尾及其后往返必须恒等
            let collapses = folds
                .iter()
                .any(|f| f.start <= off && off < f.end);
            if collapses {
                continue;
            }
            let vis = doc_to_vis(&folds, off);
            assert_eq!(vis_to_doc(&folds, vis), off, "off={off} vis={vis}");
        }
    }

    #[test]
    fn mapping_monotonic() {
        let folds = ranges(&[(2, 5), (9, 13)]);
        let mut prev = 0usize;
        for off in 0..50 {
            let vis = doc_to_vis(&folds, off);
            assert!(vis >= prev, "doc_to_vis 非单调: off={off}");
            prev = vis;
        }
        let mut prev = 0usize;
        for vis in 0..50 {
            let doc = vis_to_doc(&folds, vis);
            assert!(doc >= prev, "vis_to_doc 非单调: vis={vis}");
            prev = doc;
        }
    }

    #[test]
    fn next_atomic_skips_hidden_run() {
        let dec = Decorations::new(Rope::from("x"), vec![], ranges(&[(6, 8), (10, 12)]), vec![]);
        // 右移：停在段后
        assert_eq!(dec.next_atomic(6, 1), 8);
        // 左移：停在段前
        assert_eq!(dec.next_atomic(8, -1), 6);
        // 不紧邻 → 原样
        assert_eq!(dec.next_atomic(0, 1), 0);
        assert_eq!(dec.next_atomic(20, -1), 20);
        // 连续两段右跳
        assert_eq!(dec.next_atomic(6, 1), 8);
    }

    /// 随机折叠集下映射性质（固定种子，确定性可复现）。
    #[test]
    fn property_roundtrip_random_folds() {
        // 简易 LCG，避免引 rand 依赖
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        let mut next = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _case in 0..200 {
            // 生成 0..64 内不重叠折叠段
            let mut folds = Vec::new();
            let mut cursor = 0usize;
            while cursor < 64 {
                let gap = next() % 4;
                cursor += gap;
                let len = next() % 4;
                if len == 0 || cursor + len > 64 {
                    cursor += 1.max(len);
                    continue;
                }
                folds.push(cursor..cursor + len);
                cursor += len + 1;
            }
            let folds = merge(folds);
            for off in 0..64 {
                // 段 [start, end) 收拢 → 逆像不唯一，跳过；其余恒等
                let collapses = folds
                    .iter()
                    .any(|f| f.start <= off && off < f.end);
                if collapses {
                    continue;
                }
                let vis = doc_to_vis(&folds, off);
                assert_eq!(vis_to_doc(&folds, vis), off, "folds={folds:?} off={off}");
            }
            // 单调性
            let mut prev = 0usize;
            for off in 0..64 {
                let vis = doc_to_vis(&folds, off);
                assert!(vis >= prev);
                prev = vis;
            }
        }
    }
}
