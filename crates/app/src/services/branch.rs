//! Session-tree helpers (pi-web lib/session-tree.ts / BranchNavigator
//! labels parity): branch detection, root→leaf path collection, chain
//! compression and labels.

use std::collections::HashSet;

use pi_link::protocol::TreeNode;

/// Iterative check: does the tree branch anywhere?
pub fn tree_has_branches(nodes: &[TreeNode]) -> bool {
    if nodes.len() > 1 {
        return true;
    }
    let mut stack: Vec<&TreeNode> = nodes.iter().collect();
    while let Some(n) = stack.pop() {
        if n.children.len() > 1 {
            return true;
        }
        for c in &n.children {
            stack.push(c);
        }
    }
    false
}

/// Ids on the root→leaf path (iterative DFS, BranchNavigator parity).
pub fn build_active_path(nodes: &[TreeNode], leaf_id: Option<&str>) -> HashSet<String> {
    let mut out = HashSet::new();
    let Some(target) = leaf_id else {
        return out;
    };
    let mut stack: Vec<(&TreeNode, Vec<String>)> =
        nodes.iter().map(|n| (n, vec![n.id.clone()])).collect();
    while let Some((node, path)) = stack.pop() {
        if node.id == target {
            out.extend(path);
            break;
        }
        for c in &node.children {
            let mut p = path.clone();
            p.push(c.id.clone());
            stack.push((c, p));
        }
    }
    out
}

/// Compress a single-child chain into its branching/leaf representative.
/// Returns (representative, skipped count, label). Label prefers the first
/// message text on the chain (40 chars, pi-web getLabel parity).
pub fn compress_chain(node: &TreeNode) -> (TreeNode, usize, String) {
    let mut current = node.clone();
    let mut label_entry: Option<String> = message_label(node);
    let mut skipped = 0usize;
    while current.children.len() == 1 {
        current = current.children[0].clone();
        if label_entry.is_none() {
            label_entry = message_label(&current);
        }
        skipped += 1;
    }
    let label = label_entry
        .or_else(|| message_label(&current))
        .unwrap_or_else(|| current.entry_type.clone());
    (current, skipped, label)
}

/// 40-char label for message entries (getLabel parity).
pub fn message_label(node: &TreeNode) -> Option<String> {
    if node.entry_type != "message" || node.role.as_deref() == Some("system") {
        return None;
    }
    let mut text = node.text.clone()?;
    if text.is_empty() {
        if node.role.as_deref() == Some("assistant") {
            text = "[assistant]".into();
        } else {
            return None;
        }
    }
    let mut t: String = text.chars().take(40).collect();
    if text.chars().count() > 40 {
        t.push('…');
    }
    Some(t)
}

/// User-message entry ids along the root→leaf path (fork anchors for the
/// per-message fork button). Ordering matches the projected user messages.
pub fn collect_path_user_ids(nodes: &[TreeNode], leaf_id: Option<&str>) -> Vec<String> {
    let Some(target) = leaf_id else {
        return Vec::new();
    };
    fn flatten<'a>(nodes: &'a [TreeNode], map: &mut std::collections::HashMap<String, &'a TreeNode>) {
        for n in nodes {
            map.insert(n.id.clone(), n);
            flatten(&n.children, map);
        }
    }
    let mut map = std::collections::HashMap::new();
    flatten(nodes, &mut map);
    let mut chain: Vec<TreeNode> = Vec::new();
    let mut cur = map.get(target);
    while let Some(n) = cur {
        chain.push((*n).clone());
        cur = n.parent_id.as_deref().and_then(|pid| map.get(pid));
    }
    chain.reverse();
    chain
        .into_iter()
        .filter(|n| n.role.as_deref() == Some("user"))
        .map(|n| n.id)
        .collect()
}

/// Top-level rows: multiple roots => the roots; otherwise children of the
/// first branching node (empty when the session is linear).
pub fn select_top_level_branches(tree: &[TreeNode]) -> Vec<TreeNode> {
    if tree.len() > 1 {
        return tree.to_vec();
    }
    if tree.is_empty() {
        return Vec::new();
    }
    let first = compress_chain(&tree[0]).0;
    if first.children.len() > 1 {
        first.children.clone()
    } else {
        Vec::new()
    }
}
