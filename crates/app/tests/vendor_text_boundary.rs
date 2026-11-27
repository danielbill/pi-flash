//! vendor gpui 文本切片边界修复（pi-flash-t2hb，2026-11-27）的回归测试。
//! vendor 测试目标因 workspace 排除（dev-dependencies 无法解析）跑不了，
//! 按惯例放 app 侧（同 vendor_highlight_diff.rs）。
//!
//! 事故：会话右键详情卡 line_clamp 标题排版时，gpui `elements/text.rs` 的
//! 测量闭包**跨趟复用**被 `truncate_line` 原地改写过的 runs——「第一趟按窄
//! 宽截断、第二趟不再截断」时，第二趟拿 122 字节的短 run 去排 123 字节原文，
//! `direct_write.rs` 按 run.len 字节切片正切进尾部 '…'（字节 120..123）→
//! `byte index 122 is not a char boundary` panic，真机六连崩（2026-11-27）。
//! 修复两层：
//! 1. 根修 `elements/text.rs`：每趟测量重建 runs（截断只污染本趟副本）；
//! 2. 兜底 `platform/windows/direct_write.rs`：run 边界钳回最近字符边界
//!    （floor_char_boundary），同类错排降级而非崩进程。
//! 本文件走 `TextSystem::platform_text_system()`（PF 补丁访问器）直达事故
//! 切片点（DirectWrite internal layout_line），钉死兜底行为：错位 run 必
//! 须降级出有效布局，修复前此处直接 panic。

use gpui::{FontRun, PlatformTextSystem, TestAppContext, TestDispatcher, font, px};
use rand::prelude::*;

/// 事故字符串原件：123 字节；pi preview 按 120 字节截断后补的 '…' 占
/// 字节 120..123，陈旧 run 边界 122 恰落在它中间。
const CRASH_TITLE: &str = "认证都3天了，进去结果看不到结果，一直在加载， 为什么？  [Image: original 2198x786, displayed a…";

#[test]
fn misaligned_run_degrades_instead_of_panicking() {
    assert_eq!(CRASH_TITLE.len(), 123);
    assert!(
        !CRASH_TITLE.is_char_boundary(122),
        "事故签名自检：122 必须落在 '…'（120..123）中间"
    );
    let dispatcher = TestDispatcher::new(StdRng::seed_from_u64(0));
    let cx = TestAppContext::build(dispatcher, None);
    let ts = cx.text_system();
    let platform = ts.platform_text_system();
    let fid = ts.resolve_font(&font("Segoe UI"));
    let runs = [FontRun {
        len: 122, // 修复前：切片 [0..122] panic；修复后：钳回 120 降级排版
        font_id: fid,
    }];
    let layout = platform.layout_line(CRASH_TITLE, px(12.), &runs);
    // trait 包装层对 Err 落到默认空布局（零 run），故「非空 run + 正宽」
    // 同时证明 internal 走的是 Ok(降级排版) 而非 Err 兜底
    assert!(layout.width > px(0.));
    assert!(!layout.runs.is_empty());
}

#[test]
fn boundary_variants_all_degrade_safely() {
    // DirectWrite 对全文排版、run 只管分段字体：宽度不随 run 长度变化，
    // 兜底的可观察行为就是「不再 panic、产出有效布局」。三个变体：
    // 122 = 切进多字节字符（事故原件）；123 = 整长对齐（正常路径）；
    // 999 = 越界（钳到 len）。
    let dispatcher = TestDispatcher::new(StdRng::seed_from_u64(0));
    let cx = TestAppContext::build(dispatcher, None);
    let ts = cx.text_system();
    let platform = ts.platform_text_system();
    let fid = ts.resolve_font(&font("Segoe UI"));
    for len in [122usize, 123, 999] {
        let layout = platform.layout_line(CRASH_TITLE, px(12.), &[FontRun { len, font_id: fid }]);
        assert!(layout.width > px(0.), "len={len} 必须产出有效布局");
        assert!(!layout.runs.is_empty(), "len={len} 必须有字形 run");
    }
}
