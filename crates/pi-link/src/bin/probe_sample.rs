//! 042-1 探测样本测试 CLI：`probe_sample <base_url> [api_key]`。
//! 打印 baseUrl 推断形状（模块①）与连通探测结果（模块②，key 只用不回显）。
//!
//! 用途：给真实 provider 样本跑通路验证（脚本测试），不启动 UI。

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: probe_sample <base_url> [api_key]");
        std::process::exit(2);
    }
    let base = args[1].as_str();
    let key = args.get(2).map(String::as_str).unwrap_or("");

    println!("base      : {base}");
    println!(
        "infer api : {}（模块①，仅 pi 注册用）",
        pi_link::probe::infer_api(base)
    );
    if key.is_empty() {
        println!("key       : (none given, stop before probe)");
        return;
    }

    println!("strategy  : probe_connect（模块②阶梯，与 api type 无关）");
    match pi_link::probe::probe_connect(base, key) {
        pi_link::probe::ProbeOutcome::Ok {
            models,
            latency_ms,
            catalog,
        } => {
            println!("probe     : OK  {latency_ms}ms  {models} entries");
            for m in catalog.iter().take(12) {
                let name = if m.name.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", m.name)
                };
                println!("  - {}{}", m.id, name);
            }
            if catalog.len() > 12 {
                println!("  ... +{} more", catalog.len() - 12);
            }
        }
        pi_link::probe::ProbeOutcome::AuthRejected { status } => {
            println!("probe     : AUTH REJECTED (HTTP {status})");
        }
        pi_link::probe::ProbeOutcome::Unreachable { error } => {
            println!("probe     : UNREACHABLE ({error})");
        }
    }
}
