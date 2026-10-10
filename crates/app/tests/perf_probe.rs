//! 大文件打开路径微基准：tmp/perf/perf-80k.txt（4.59MB / 80k 行随机文本）。
//!
//! 数据段（纯 ropey/String 成本）+ InputState 段（#[gpui::test]，真实
//! set_value 路径）。与 app 的 open_file_tab → ensure_file_editor →
//! set_value → Change 脏比较逐段对应，用来对账「打开冻结」时间去向。
//!
//! 运行：
//!   cargo test -p app --test perf_probe --release -- --nocapture
use gpui::{AppContext as _, Context, IntoElement, Render, TestAppContext, Window};
use gpui_component::input::RopeExt as _;
use std::time::{Duration, Instant};

const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tmp/perf/perf-80k.txt");

fn ms(d: Duration) -> String {
    format!("{:>8.2} ms", d.as_secs_f64() * 1000.0)
}

/// 打开路径各阶段在 app 侧的对应关系：
/// fs::read / from_utf8_lossy → open_file_tab（仅首开；已打开 tab 只切换不重读）；
/// Rope::from + reset → set_value 首次（多行走专用 reset 路径，无 utf16 往返）；
/// replace(全量) + clone → set_value 重载（watcher 自动重载 / 冲突 banner「重新加载」）；
/// Rope==String 比较 → Change 事件里的脏比较（原先 value().to_string() 全文物化）。
///
/// 数据段里的 utf16 统计 / replace / 逐行 Rope 物化是「旧路径」各段的留档测量，
/// 供与 050-滚动优化 §二 的历史账单对照；现行代码已不走这些步骤。
#[test]
fn probe_data_stages() {
    let t = Instant::now();
    let bytes = std::fs::read(PATH).unwrap();
    let d_read = t.elapsed();

    let t = Instant::now();
    let content = String::from_utf8_lossy(&bytes).to_string();
    let d_utf8 = t.elapsed();
    drop(bytes);

    let t = Instant::now();
    let mut rope = ropey::Rope::from(content.as_str());
    let d_rope = t.elapsed();

    let t = Instant::now();
    let u16len: usize = rope.chars().map(|c| c.len_utf16()).sum();
    let d_u16 = t.elapsed();

    // 旧 set_value 首次：text 从空 Rope replace 进 4.6MB
    let mut empty = ropey::Rope::from("");
    let t = Instant::now();
    empty.replace(0..0, content.as_str());
    let d_replace_empty = t.elapsed();
    assert_eq!(empty.len(), rope.len());

    // 旧 set_value 重载：old_text.clone() + 全量 range replace
    let t = Instant::now();
    let old = rope.clone();
    let d_clone = t.elapsed();
    let t = Instant::now();
    rope.replace(0..u16len, content.as_str());
    let d_replace_full = t.elapsed();
    drop(old);

    // 旧 text_wrapper._update 内循环：逐行 Rope::from 物化
    let t = Instant::now();
    let mut n_lines = 0usize;
    for line in rope.iter_lines() {
        let item = ropey::Rope::from(line);
        n_lines += 1;
        std::hint::black_box(item);
    }
    let d_lines = t.elapsed();
    assert!(n_lines >= 80_000);

    // 旧 Change 脏比较：value().to_string() 全文物化 + memcmp
    let t = Instant::now();
    let s = rope.to_string();
    let d_to_string = t.elapsed();
    let t = Instant::now();
    let _eq = s == content;
    let d_cmp = t.elapsed();

    println!("== 数据段（80k 行 / {} 字节，旧路径留档） ==", rope.len());
    println!("fs::read                {}", ms(d_read));
    println!("from_utf8_lossy+to_s    {}", ms(d_utf8));
    println!("Rope::from(全文)        {}", ms(d_rope));
    println!("utf16 len 统计          {}", ms(d_u16));
    println!("replace(0..0) 首次灌入  {}", ms(d_replace_empty));
    println!("Rope::clone             {}", ms(d_clone));
    println!("replace(全量) 重载      {}", ms(d_replace_full));
    println!("80k 行 Rope::from 物化  {}", ms(d_lines));
    println!("全文 to_string          {}", ms(d_to_string));
    println!("String memcmp           {}", ms(d_cmp));
}

struct Probe {
    _input: gpui::Entity<gpui_component::input::InputState>,
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui::Empty
    }
}

#[gpui::test]
fn probe_input_state_open(cx: &mut TestAppContext) {
    let content = std::fs::read_to_string(PATH).unwrap();

    cx.add_window(move |window, cx| {
        let t = Instant::now();
        let input = cx.new(|scx| {
            gpui_component::input::InputState::new(window, scx)
                .code_editor("text")
                // 与生产同参（023 fileView 2026-10-10 起默认 soft wrap）——
                // 注：本段量的是 set_value/reset，此时 wrap_width 未知（未布局）
                // 故折行计算不在此发生，开不开对这几个数字无影响
                .soft_wrap(true)
        });
        let d_new = t.elapsed();

        // app 首开：ensure_file_editor 里 content.clone() → set_value
        let clone = content.clone();
        let t = Instant::now();
        input.update(cx, |st, scx| st.set_value(clone, window, scx));
        let d_set1 = t.elapsed();

        // 旧 Change 脏比较对照：value().to_string() 全文物化
        let t = Instant::now();
        let v = input.read(cx).value().to_string();
        let d_val = t.elapsed();
        assert_eq!(v.len(), content.len());

        // watcher 自动重载 / 冲突 banner「重新加载」：set_value 全文替换
        // （app 侧已开 tab 再点目录树不再走这条路，只切 tab）
        let t = Instant::now();
        input.update(cx, |st, scx| st.set_value(content.clone(), window, scx));
        let d_set2 = t.elapsed();

        // Change 脏比较新路径：Rope == String（零分配 memcmp，应用内真实路径）
        let t = Instant::now();
        let dirty = *input.read(cx).text() != content;
        let d_cmp = t.elapsed();
        assert!(!dirty);

        println!("== InputState 段 ==");
        println!("InputState::new+code_editor {}", ms(d_new));
        println!("set_value #1（空→全文）      {}", ms(d_set1));
        println!("value().to_string()（旧对照）{}", ms(d_val));
        println!("set_value #2（全文替换）     {}", ms(d_set2));
        println!("Rope==String 脏比较          {}", ms(d_cmp));

        Probe { _input: input }
    });
    cx.run_until_parked();
}

/// set_value reset 路径内部的剩余成本拆解：line_wrapper 池、RopeSlice 行迭代、
/// LineItem Vec 填充——定位 wrapper reset 的剩余耗时去向。
#[gpui::test]
fn probe_reset_path_pieces(cx: &mut TestAppContext) {
    let content = std::fs::read_to_string(PATH).unwrap();

    cx.add_window(move |window, cx| {
        let font = window.text_style().font();
        let font_size = window.text_style().font_size.to_pixels(window.rem_size());

        let t = Instant::now();
        let w1 = window.text_system().line_wrapper(font.clone(), font_size);
        let d_wrapper_cold = t.elapsed();
        drop(w1);
        let t = Instant::now();
        let w2 = window.text_system().line_wrapper(font.clone(), font_size);
        let d_wrapper_warm = t.elapsed();
        drop(w2);

        let rope = ropey::Rope::from(content.as_str());
        let t = Instant::now();
        let slice = rope.slice(0..rope.len());
        let n = slice.len_lines(ropey::LineType::LF);
        let mut total = 0usize;
        for row in 0..n {
            let line = slice.line(row, ropey::LineType::LF);
            let line = if line.len() > 0 {
                let e = line.len() - 1;
                if line.is_char_boundary(e) && line.char(e) == '\n' {
                    line.slice(..e)
                } else {
                    line
                }
            } else {
                line
            };
            total += line.len();
        }
        let d_slice_iter = t.elapsed();
        assert_eq!(total, rope.len() - (n - 1));

        let t = Instant::now();
        let mut items: Vec<(usize, smallvec::SmallVec<[std::ops::Range<usize>; 1]>)> =
            Vec::with_capacity(n);
        for row in 0..n {
            let line = slice.line(row, ropey::LineType::LF);
            let line_len = line.len();
            let mut wrapped = smallvec::SmallVec::with_capacity(1);
            wrapped.push(0..line_len);
            items.push((line_len, wrapped));
        }
        let d_vec_fill = t.elapsed();
        assert_eq!(items.len(), n);

        println!("== reset 路径拆解 ==");
        println!("line_wrapper 冷创建    {}", ms(d_wrapper_cold));
        println!("line_wrapper 池复用    {}", ms(d_wrapper_warm));
        println!("80k 行 slice.line 迭代 {}", ms(d_slice_iter));
        println!("LineItem Vec 填充      {}", ms(d_vec_fill));

        Probe {
            _input: cx.new(|scx| {
                gpui_component::input::InputState::new(window, scx)
                    .code_editor("text")
                    .soft_wrap(true)
            }),
        }
    });
    cx.run_until_parked();
}

/// 代码语言大文件：全量 tree-sitter parse 的主线程成本（现已转后台，
/// 这里留档「如果不转会冻结多少」），以及打字路径 utf16↔offset 换算的
/// 实测量级（bead pi-flash-5z6 里怀疑 O(偏移)/键，实为 ropey 树内
/// O(log N)，见 utf16 段数字）。
#[test]
fn probe_code_parse_stages() {
    // 合成 ~80k 行 Rust 源码（重复函数块），量级对齐 perf-80k.txt
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
        src.push_str(unit.replace("generated_item", &format!("generated_item_{i}")).as_str());
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

    println!("== 代码 parse 段（{} 行 / {} 字节） ==", rope.len_lines(ropey::LineType::LF), rope.len());
    println!("SyntaxHighlighter::new(rust) {}", ms(d_new));
    println!("全文 tree-sitter parse       {}", ms(d_parse));
    println!("utf16 换算 ×1000（文末）     to:{} from:{}", ms(d_to_u16), ms(d_from_u16));
}
