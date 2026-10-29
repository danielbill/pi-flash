//! dev 部署后立刻预编译 cwasm（`link:dev --wasm` 调用，8tb.4 真机 10-29）。
//!
//! 动机：重新部署 wasm → sha256 变 → cwasm 缓存失效 → 下次启动揭幕后
//! 后台编译数秒抢满 CPU（实测 UI 冻结 7.6s）。部署时预编一次，启动恒命中。
//!
//! 用法：pf-precompile <plugin-wasm> <cache-dir>
//! 输出：precompiled（真编）/ cached（已命中）/ 失败 exit 1。

use std::path::Path;

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(wasm), Some(cache)) = (args.next(), args.next()) else {
        eprintln!("用法：pf-precompile <plugin-wasm> <cache-dir>");
        std::process::exit(2);
    };
    match pf_plugin_host::PluginInstance::precompile_to_cache(Path::new(&wasm), Path::new(&cache)) {
        Ok(true) => println!("precompiled"),
        Ok(false) => println!("cached"),
        Err(e) => {
            eprintln!("precompile failed: {e}");
            std::process::exit(1);
        }
    }
}
