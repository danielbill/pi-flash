//! 大文件路径微基准留档：代码 parse 段（自合成数据，不依赖任何测试数据文件）。
//!
//! 历史背景：本文件原用 test/perf/perf-80k.txt（5.2MB / 80k 行随机文本）
//! 拆解「打开冻结」时间去向（数据段 / InputState 段 / reset 路径段），
//! 该问题已在 050-滚动优化收口，数据文件与那三段测量已一并移除。
//!
//! 运行：
//!   cargo test -p app --test perf_probe --release -- --nocapture
use gpui_component::input::RopeExt as _;
use std::time::{Duration, Instant};

fn ms(d: Duration) -> String {
    format!("{:>8.2} ms", d.as_secs_f64() * 1000.0)
}

/// 代码语言大文件：全量 tree-sitter parse 的主线程成本（现已转后台，
/// 这里留档「如果不转会冻结多少」），以及打字路径 utf16↔offset 换算的
/// 实测量级（bead pi-flash-5z6 里怀疑 O(偏移)/键，实为 ropey 树内
/// O(log N)，见 utf16 段数字）。
#[test]
fn probe_code_parse_stages() {
    // 合成 ~80k 行 Rust 源码（重复函数块），量级对齐当年的 perf-80k 数据
    let unit = r#"fn generated_item(n: usize) -> usize {
    // 计算伪哈希：轮转累乘，模仿真实代码的分支与字符串
    let mut acc: u64 = 0x9E37_79B9_7F4A_7C15 ^ (n as u64);
    let label = format!("item-{n:06}");
    for (i, b) in label.bytes().enumerate() {
        acc ^= (b as u64) << (i % 32);
        acc = acc.rotate_left(7).wrapping_mul(0x100_0000_001B3);
    }
    match acc % 7 {
        0 => acc as usize,
        1 => acc.wrapping_add(1) as usize,
        2 => (!acc) as usize,
        3 => acc >> 3 as usize,
        4 => acc << 3 as usize,
        5 => label.len() * 31,
        _ => n.wrapping_mul(acc as usize),
    }
}
"#;
    let mut src = String::with_capacity(unit.len() * 2600);
    for i in 0..2600 {
        src.push_str(
            unit.replace("generated_item", &format!("generated_item_{i}"))
                .as_str(),
        );
    }
    let rope = ropey::Rope::from(src.as_str());

    // Query 编译（SyntaxHighlighter::new）
    let t = Instant::now();
    let mut hl = gpui_component::highlighter::SyntaxHighlighter::new("rust");
    let d_new = t.elapsed();

    // 全文 parse（首开/set_value reset 后的主线程冻结项，现走 background_spawn）
    let t = Instant::now();
    hl.update(None, &rope);
    let d_parse = t.elapsed();
    assert!(hl.is_parsed());

    // 打字路径的 utf16 换算：GPUI/IME 每键走 range_to_utf16 / range_from_utf16
    let t = Instant::now();
    let mut acc = 0usize;
    for _ in 0..1000 {
        acc += rope.offset_to_offset_utf16(rope.len());
    }
    let d_to_u16 = t.elapsed();
    let t = Instant::now();
    for _ in 0..1000 {
        acc += rope.offset_utf16_to_offset(rope.len_utf16());
    }
    let d_from_u16 = t.elapsed();
    std::hint::black_box(acc);

    println!(
        "== 代码 parse 段（{} 行 / {} 字节） ==",
        rope.len_lines(ropey::LineType::LF),
        rope.len()
    );
    println!("SyntaxHighlighter::new(rust) {}", ms(d_new));
    println!("全文 tree-sitter parse       {}", ms(d_parse));
    println!(
        "utf16 换算 ×1000（文末）     to:{} from:{}",
        ms(d_to_u16),
        ms(d_from_u16)
    );
}
