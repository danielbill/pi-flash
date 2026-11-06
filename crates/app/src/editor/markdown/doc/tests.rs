//! doc 域内嵌测试外移（click 手势分界 / doc_blocks 解析）。

use super::{CLICK_MAX_DRAG, click_within_drag};
use gpui::{point, px};

/// genuine click（位移 ≤ 阈值，含轻微手抖）→ 进源码编辑。
#[test]
fn genuine_click_passes() {
    let o = point(px(100.), px(200.));
    assert!(click_within_drag(o, o)); // 原地
    assert!(click_within_drag(
        o,
        point(px(100. + CLICK_MAX_DRAG), px(200.))
    )); // 恰好阈值
    assert!(click_within_drag(o, point(px(103.), px(201.)))); // 手抖
}

/// 拖拽（位移 > 阈值）不进源码——预览态划选手势留在预览（024 预览
/// 正文无选区机制，拖一下就被甩进源码 = 「预览选中复制不了」的元凶）。
#[test]
fn drag_motion_rejected() {
    let o = point(px(100.), px(200.));
    let eps = 0.01;
    assert!(!click_within_drag(
        o,
        point(px(100. + CLICK_MAX_DRAG + eps), px(200.))
    ));
    assert!(!click_within_drag(
        o,
        point(px(100.), px(200. + CLICK_MAX_DRAG + eps))
    ));
    // 斜向小分量叠加超阈值同样拒
    assert!(!click_within_drag(
        o,
        point(px(100. + CLICK_MAX_DRAG), px(200. + CLICK_MAX_DRAG))
    ));
}
