//! 042-1 探测样本测试 CLI：`probe_sample <base_url> [api_key]`。
//! 打印 baseUrl 推断形状、目录/兜底 URL 与真实探测结果；key 只用不回显。
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

    let api = pi_link::probe::infer_api(base);
    println!("base      : {base}");
    println!("infer api : {api}");
    if let Some((models, messages)) = pi_link::probe::anthropic_compat_urls(base) {
        println!("compat    : models  = {models}");
        println!("            messages= {messages}");
    }
    let Some((url, auth)) = pi_link::probe::custom_target(base, api) else {
        eprintln!("empty base url");
        std::process::exit(1);
    };
    println!("probe url : {url} (auth {auth:?})");
    if key.is_empty() {
        println!("key       : (none given, stop before probe)");
        return;
    }

    let outcome = match pi_link::probe::anthropic_compat_urls(base) {
        Some((models, messages)) => {
            println!("strategy  : catalog-first + messages fallback");
            pi_link::probe::probe_anthropic_compat(&models, &messages, key)
        }
        None => {
            println!("strategy  : direct");
            pi_link::probe::probe(&url, auth, key)
        }
    };
    match outcome {
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
