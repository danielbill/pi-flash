//! 大文件打开路径微基准：tmp/perf/perf-80k.txt（4.4MB / 80k 行随机文本）。
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
/// fs::read / from_utf8_lossy → open_file_tab；
/// Rope::from + replace(空) + utf16 统计 + iter_lines 物化 → set_value 首次；
/// replace(全量) + clone → set_value 重载（reload_pending / 已打开重读）；
/// to_string → Change 事件里的 value().to_string() 脏比较。
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

    // set_value 首次：text 从空 Rope replace 进 4.4MB
    let mut empty = ropey::Rope::from("");
    let t = Instant::now();
    empty.replace(0..0, content.as_str());
    let d_replace_empty = t.elapsed();
    assert_eq!(empty.len(), rope.len());

    // set_value 重载：old_text.clone() + 全量 range replace
    let t = Instant::now();
    let old = rope.clone();
    let d_clone = t.elapsed();
    let t = Instant::now();
    rope.replace(0..u16len, content.as_str());
    let d_replace_full = t.elapsed();
    drop(old);

    // text_wrapper._update 内循环：不软换行也要逐行 Rope::from 物化 LineItem
    let t = Instant::now();
    let mut n_lines = 0usize;
    let mut n_bytes = 0usize;
    for line in rope.iter_lines() {
        let item = ropey::Rope::from(line);
        n_bytes += item.len();
        n_lines += 1;
    }
    let d_lines = t.elapsed();
    assert!(n_lines >= 80_000);

    // Change 脏比较：value().to_string() 全文物化 + memcmp
    let t = Instant::now();
    let s = rope.to_string();
    let d_to_string = t.elapsed();
    let t = Instant::now();
    let _eq = s == content;
    let d_cmp = t.elapsed();

    println!("== 数据段（80k 行 / {} 字节） ==", rope.len());
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
                .soft_wrap(false)
        });
        let d_new = t.elapsed();

        // app 首开：ensure_file_editor 里 content.clone() → set_value
        let clone = content.clone();
        let t = Instant::now();
        input.update(cx, |st, scx| st.set_value(clone, window, scx));
        let d_set1 = t.elapsed();

        // Change 事件里 ed.read(cx).value().to_string() 脏比较
        let t = Instant::now();
        let v = input.read(cx).value().to_string();
        let d_val = t.elapsed();
        assert_eq!(v.len(), content.len());

        // 已打开再点 tab（不脏重读）：consume_file_reload / set_value 全量替换
        let t = Instant::now();
        input.update(cx, |st, scx| st.set_value(content.clone(), window, scx));
        let d_set2 = t.elapsed();

        println!("== InputState 段 ==");
        println!("InputState::new+code_editor {}", ms(d_new));
        println!("set_value #1（空→全文）      {}", ms(d_set1));
        println!("value().to_string()          {}", ms(d_val));
        println!("set_value #2（全文替换）     {}", ms(d_set2));

        Probe { _input: input }
    });
    cx.run_until_parked();
}
