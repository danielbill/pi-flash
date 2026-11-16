//! 文件树多选（021，Zed project_panel 选择模型挪植）：`ws.tree_selected`
//! = 活动条目（Zed `selection`：键盘/改名/打开的落点，shift 扩选锚点）、
//! `ws.tree_marked` = 标记集（Zed `marked_entries`：shift+点范围扩选、
//! ctrl+点切换的成员）。纯函数收在本文件可单测，Chat 入口在下方 impl 块。
//!
//! 交互对齐 Zed project_panel.rs（on_click 6313-6415 / 右键 6620-6639）：
//! - shift+点：以活动条目为锚的展平行区间**并集**扩选（不清旧标记）；
//!   锚/目标不在展平行里 → 退化为点选目标
//! - ctrl+点（`Modifiers::secondary`，Windows=Ctrl / macOS=Cmd）：点击行
//!   进出标记集
//! - 裸点：清标记走原 082 语义（目录翻面 / 文件打开）
//! - 右键：点击行不在标记集 → 先折叠标记集（Zed 同款：防右键动作误伤
//!   看不见的多选成员）
//! 动作对象走 [`effective_sel`] / [`disjoint`] 的合并规则，删除/复制路径/
//! 拖拽整组生效（调用方见 tree_menu / file_actions_delete / tree_drag）。

use std::path::{Path, PathBuf};

use crate::same_path;
use crate::services::file_tree::TreeRow;

use crate::Chat;

/// Zed `effective_entries` 挪植：动作对象 = 活动条目与标记集的合并——
/// 无标记 → 单活动条目；唯一标记且不含活动条目 → 单活动条目（方便
/// 「标记着一组、只想动当前行」的一次性动作）；否则 = 标记集。两者皆空
/// → 空集（调用方自行兜底）。
pub(crate) fn effective_sel(active: Option<&Path>, marked: &[PathBuf]) -> Vec<PathBuf> {
    if let Some(a) = active {
        if marked.is_empty() || (marked.len() == 1 && !marked.iter().any(|m| same_path(m, a))) {
            return vec![a.to_path_buf()];
        }
    }
    marked.to_vec()
}

/// shift+点扩选（Zed 6323-6360 挪植）：锚 = 活动条目、目标 = 点击行，按
/// 展平行下标取闭区间，返回区间内全部路径（调用方并集进标记集，不去
/// 重排）。锚或目标任一不在当前展平行里（外部改动/切项目残留）→ 退化为
/// 只点选目标。
pub(crate) fn range_union(rows: &[TreeRow], active: &Path, clicked: &Path) -> Vec<PathBuf> {
    let anchor = rows.iter().position(|r| same_path(&r.path, active));
    let target = rows.iter().position(|r| same_path(&r.path, clicked));
    let (Some(a), Some(c)) = (anchor, target) else {
        return vec![clicked.to_path_buf()];
    };
    (a.min(c)..=a.max(c))
        .map(|ix| rows[ix].path.clone())
        .collect()
}

/// Zed `disjoint_entries` 挪植：集合内被其它成员（祖先目录）覆盖的子孙
/// 剪掉——删父已连带整棵，逐个投递会重复报数甚至 fs 竞态。保持原序。
/// （`is_under` 含自身，同路径成员不算覆盖。）
pub(crate) fn disjoint(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|p| {
            !paths
                .iter()
                .any(|q| !same_path(q, p) && crate::services::workspace::is_under(p, q))
        })
        .cloned()
        .collect()
}

impl Chat {
    /// 当前动作对象集（Zed `effective_entries` 合并规则）：删除/复制路径
    /// 等整组动作的取数入口。
    pub(crate) fn tree_effective_sel(&self) -> Vec<PathBuf> {
        effective_sel(self.ws.tree_selected.as_deref(), &self.ws.tree_marked)
    }

    /// 菜单动作对象集：右键打开菜单时活动条目 = 点击行（右键折叠已保证
    /// 合并结果非空且含槽路径）；根菜单（空白/操作栏右键）树选中可能为
    /// 空，兜底落菜单槽路径。
    pub(crate) fn tree_targets_or_slot(&self, slot: &Path) -> Vec<PathBuf> {
        let eff = self.tree_effective_sel();
        if eff.is_empty() {
            vec![slot.to_path_buf()]
        } else {
            eff
        }
    }

    /// 右键选中语义（Zed on_secondary_mouse_down 6620-6639 挪植）：活动
    /// 条目移到点击行；点击行不在标记集 → 折叠标记集（防右键动作误伤
    /// 看不见的多选成员）。行右键与自动化 `file.delete` op 共用（op 无
    /// 真实右键前置，显式补齐同一段状态迁移再走 from_slot 链）。
    pub(crate) fn tree_right_select(&mut self, path: &Path) {
        self.ws.tree_selected = Some(path.to_path_buf());
        if !self.ws.tree_marked.iter().any(|m| same_path(m, path)) {
            self.ws.tree_marked.clear();
        }
    }

    /// 行点击统一入口（Zed on_click 6313-6415 挪植）：修饰点击只动选择
    /// 不触发行副作用（目录不翻面、文件不打开），裸点走原 082 语义。
    pub(crate) fn tree_click_select(
        &mut self,
        path: &Path,
        is_dir: bool,
        mods: gpui::Modifiers,
        cx: &mut gpui::Context<Self>,
    ) {
        // shift+点：锚 = 活动条目，区间并集扩选（Zed 不清旧标记，区间外
        // 成员保留）
        if mods.shift && self.ws.tree_selected.is_some() {
            let active = self.ws.tree_selected.clone().expect("checked above");
            for p in range_union(&self.ws.tree_rows, &active, path) {
                if !self.ws.tree_marked.iter().any(|m| same_path(m, &p)) {
                    self.ws.tree_marked.push(p);
                }
            }
            self.ws.tree_selected = Some(path.to_path_buf());
            cx.notify();
            return;
        }
        // ctrl+点（Zed `modifiers().secondary()` 同款）：进出标记集
        if mods.secondary() {
            match self.ws.tree_marked.iter().position(|m| same_path(m, path)) {
                Some(ix) => {
                    self.ws.tree_marked.remove(ix);
                }
                None => self.ws.tree_marked.push(path.to_path_buf()),
            }
            self.ws.tree_selected = Some(path.to_path_buf());
            cx.notify();
            return;
        }
        // 裸点：清标记；目录翻面 / 文件打开（原 082 语义原样保留）
        self.ws.tree_marked.clear();
        self.ws.tree_selected = Some(path.to_path_buf());
        if is_dir {
            if !self.ws.expanded_dirs.remove(path) {
                self.ws.expanded_dirs.insert(path.to_path_buf());
            }
            self.rebuild_tree();
        } else {
            self.open_file_tab(path.to_path_buf(), cx);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{disjoint, effective_sel, range_union};
    use crate::services::file_tree::TreeRow;
    use std::path::PathBuf;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    fn row(path: &str) -> TreeRow {
        TreeRow {
            path: p(path),
            name: "x".into(),
            depth: 0,
            is_dir: false,
            expanded: false,
            git: None,
            changed_dot: false,
            ignored: false,
        }
    }

    #[test]
    fn effective_sel_follows_zed_merge_rules() {
        // 无标记 → 单活动条目
        assert_eq!(effective_sel(Some(p("r/a").as_path()), &[]), vec![p("r/a")]);
        // 唯一标记且不含活动条目 → 单活动条目（Zed 一次性动作便利规则）
        assert_eq!(
            effective_sel(Some(p("r/a").as_path()), &[p("r/b")]),
            vec![p("r/a")]
        );
        // 唯一标记且就是活动条目 → 标记集
        assert_eq!(
            effective_sel(Some(p("r/b").as_path()), &[p("r/b")]),
            vec![p("r/b")]
        );
        // 多标记 → 标记集（活动条目是否在内不管）
        let marked = vec![p("r/b"), p("r/c")];
        assert_eq!(
            effective_sel(Some(p("r/a").as_path()), &marked),
            vec![p("r/b"), p("r/c")]
        );
        // 无活动条目 → 标记集原样（含空集）
        assert_eq!(effective_sel(None, &marked), vec![p("r/b"), p("r/c")]);
        assert_eq!(effective_sel(None, &[]), Vec::<PathBuf>::new());
    }

    #[test]
    fn range_union_covers_anchor_to_clicked_both_directions() {
        let rows: Vec<TreeRow> = ["r/a", "r/b", "r/c", "r/d", "r/e"]
            .iter()
            .map(|s| row(s))
            .collect();
        // 锚在上、点在下
        assert_eq!(
            range_union(&rows, &p("r/b"), &p("r/d")),
            vec![p("r/b"), p("r/c"), p("r/d")]
        );
        // 锚在下、点在上（反向扩选）
        assert_eq!(
            range_union(&rows, &p("r/d"), &p("r/a")),
            vec![p("r/a"), p("r/b"), p("r/c"), p("r/d")]
        );
        // 点自己 → 单元素
        assert_eq!(range_union(&rows, &p("r/c"), &p("r/c")), vec![p("r/c")]);
    }

    #[test]
    fn range_union_degrades_to_clicked_when_anchor_missing() {
        let rows: Vec<TreeRow> = ["r/a", "r/b"].iter().map(|s| row(s)).collect();
        // 锚是外部残留路径（不在展平行里）→ 只点选目标
        assert_eq!(range_union(&rows, &p("r/gone"), &p("r/b")), vec![p("r/b")]);
        // 目标不在行里（点了不存在行）→ 同样退化
        assert_eq!(
            range_union(&rows, &p("r/a"), &p("r/gone")),
            vec![p("r/gone")]
        );
    }

    #[test]
    fn disjoint_prunes_descendants_keeps_order() {
        // 标记祖先目录连带整棵：子孙剪掉、无关成员保留、原序不变
        let marked = vec![
            p("r/sub"),
            p("r/sub/child.txt"),
            p("r/other.txt"),
            p("r/sub/deep/x"),
        ];
        assert_eq!(disjoint(&marked), vec![p("r/sub"), p("r/other.txt")]);
        // 全是平级 → 原样
        let flat = vec![p("r/a"), p("r/b")];
        assert_eq!(disjoint(&flat), vec![p("r/a"), p("r/b")]);
        // 空集安全
        assert_eq!(disjoint(&[]), Vec::<PathBuf>::new());
    }
}
