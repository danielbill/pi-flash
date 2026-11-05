use super::*;

/// 冷缓存 + 双线程并发 load_cached：编译必须只发生一次（单飞锁 + 双检）。
/// 由 inner e2e 注入 PF_TEST_V3_WASM 运行；--nocapture 下应只见一条
/// 「[perf] cwasm 编译」。
#[test]
fn concurrent_load_compiles_once() {
    let Ok(v3) = std::env::var("PF_TEST_V3_WASM") else {
        return;
    };
    let dir = std::env::temp_dir().join(format!("pf-host-single-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("plugin.wasm");
    std::fs::copy(&v3, &wasm).unwrap();
    let cache = dir.join(".cache");
    // 不 init：load_cached 只验装载，不触证明门

    let wasm1 = wasm.clone();
    let cache1 = cache.clone();
    let t1 = std::thread::spawn(move || {
        let mut i = PluginInstance::load_cached(&wasm1, &cache1).expect("t1 load");
        i.load_ctx(&test_ctx("t1")).ok();
    });
    let wasm2 = wasm.clone();
    let cache2 = cache.clone();
    let t2 = std::thread::spawn(move || {
        let mut i = PluginInstance::load_cached(&wasm2, &cache2).expect("t2 load");
        i.load_ctx(&test_ctx("t2")).ok();
    });
    t1.join().unwrap();
    t2.join().unwrap();
    // 两份实例都装载成功（文件级断言：产物存在且唯一）
    let n = std::fs::read_dir(&cache)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "cwasm"))
        .count();
    assert_eq!(n, 1, "并发装载后产物应只有一份");
}
