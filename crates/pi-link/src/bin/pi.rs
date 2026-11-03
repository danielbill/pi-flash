//! 全局 `pi` 前置 shim（081）：把命令转发给 PF 内置 pi 内核（vendor pin，
//! 如 1.5.0），让 PF 更新时系统默认 `pi` 同步升级。
//!
//! 指针文件 `<self_dir>/pi-target.json`（`{"node":"…","cli":"…/cli.js"}`）
//! 由 pi-flash 维护（`register_to_sys::ensure_pi_shim`：启动自愈 + updater
//! 落位后指向 staged 新载荷），本体零逻辑——spawn + 透传 stdio/退出码。
//! 一个 exe 通吃 cmd/PowerShell/Git Bash/WSL（PATHEXT 优先 .exe；WSL 走
//! interop）。pi-flash 的 `pi.exe` 必须与指针同目录（bin 目录里就这一对）。
//!
//! 退出码：3 = shim 自身故障（指针缺失/损坏/内核缺席），与 pi 自身退出码区分。

use serde_json::Value;

fn fail(msg: &str) -> ! {
    eprintln!("pi (pi-flash shim): {msg}");
    std::process::exit(3);
}

fn main() {
    let exe = std::env::current_exe().unwrap_or_else(|_| fail("无法定位自身"));
    let dir = exe
        .parent()
        .unwrap_or_else(|| fail("无法定位安装目录"))
        .to_path_buf();
    let text = std::fs::read_to_string(dir.join("pi-target.json"))
        .unwrap_or_else(|_| fail("pi-target.json 缺失——请先运行一次 pi-flash 完成注册"));
    let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| {
        fail(&format!(
            "pi-target.json 损坏（{e}）——请先运行一次 pi-flash"
        ))
    });
    let node = v
        .get("node")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fail("pi-target.json 缺 node"));
    let cli = v
        .get("cli")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fail("pi-target.json 缺 cli"));
    if !std::path::Path::new(cli).is_file() {
        fail("内置内核缺席（更新中或载荷不完整）——请先运行一次 pi-flash");
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    let status = std::process::Command::new(node)
        .arg(cli)
        .args(&args)
        .status()
        .unwrap_or_else(|e| fail(&format!("无法启动 node（{e}）")));
    std::process::exit(status.code().unwrap_or(1));
}
