//! `pif-secret`：把系统钥匙库里的 apikey 交给 pi 的小助手。
//!
//! pi 的 `auth.json` 里 PF 只写一行引用：
//! `{ "deepseek": { "type": "api_key", "key": "!'…/pif-secret.exe' get deepseek" } }`
//! pi 解析凭据时会执行该命令、把 stdout 当作 key（`resolveConfigValue` 的
//! `!` 分支）。于是秘密只存在钥匙库，盘上永远没有明文，且**任何前端**
//! （PF / pi TUI / pi-web）都能取到——不依赖 PF 进程在场。
//!
//! 退出码：0 = 输出秘密；1 = 钥匙库无此条目/不可用；2 = 参数错误。
//! 非 0 时 pi 会判该 provider「未配置」（安全失败，不会把命令串当 key 发出去）。

use pi_link::credentials::{env_var_name, KeyringVault, SecretVault};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[0] != "get" {
        eprintln!("usage: pif-secret get <provider>");
        std::process::exit(2);
    }
    let provider = &args[1];
    let entry = env_var_name(provider);
    match KeyringVault.get(&entry) {
        Ok(Some(secret)) if !secret.is_empty() => print!("{secret}"),
        _ => std::process::exit(1),
    }
}
