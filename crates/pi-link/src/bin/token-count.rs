//! token 估算的独立工具脚本 —— `estimate::estimate_tokens` 的 CLI 壳。
//!
//! 用法：
//! ```text
//! cargo run -p pi-link --bin token-count            # 读 stdin
//! cargo run -p pi-link --bin token-count FILE...    # 读文件
//! echo -n "你好世界" | cargo run -p pi-link --bin token-count
//! ```
//!
//! 与界面「此会话系统提示词」标题后的小字同源同实现（算法与测试都在
//! [`pi_link::estimate`]），用于在终端侧独立复核任意文本/文件的估算值。
//! 输出每输入一行 `tokens`（人读时加 `--verbose` 附加字符数）。

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
    let files: Vec<&String> = args.iter().filter(|a| !a.starts_with('-')).collect();

    if files.is_empty() {
        let text = std::io::read_to_string(std::io::stdin()).expect("read stdin");
        report("<stdin>", &text, verbose);
    } else {
        for path in files {
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|e| panic!("read {path}: {e}"));
            report(path, &text, verbose);
        }
    }
}

fn report(label: &str, text: &str, verbose: bool) {
    let tokens = pi_link::estimate::estimate_tokens(text);
    if verbose {
        println!("{label}: {} tokens ({} chars)", tokens, text.chars().count());
    } else {
        println!("{tokens}");
    }
}
