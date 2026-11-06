//! updater 测试（itlq.9 自 updater.rs 外移，纯结构搬移）：布局/交接/清理
//! 单测 + 下载引擎本地假源功能测试 + 更新管线端到端（假源 + 真 zip）。

use super::*;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// 测试 spawn powershell 必须 CREATE_NO_WINDOW——否则每跑一轮 cargo test
/// 就弹一个 PowerShell 控制台窗抢屏幕（Compress-Archive 有进度条输出，
/// 控制台会实体化成可见窗口；agent 代跑测试时尤甚）。
fn hidden_powershell() -> std::process::Command {
    let mut cmd = std::process::Command::new("powershell.exe");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 独立临时目录（测试进程共用 target/，不能污染真安装根）。
fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pf-upd-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 造一个「完整载荷目录」：exe（假文件）+ 版本戳。
fn fake_payload(dir: &Path, ver: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join(exe_rel()), b"fake-exe").unwrap();
    std::fs::write(dir.join(".payload-version"), format!("{ver}\n")).unwrap();
}

#[test]
fn base_name_strips_version_suffix() {
    assert_eq!(base_name("payload-0.1.11"), "payload");
    assert_eq!(base_name("pi-flash-0.1.11"), "pi-flash");
    assert_eq!(base_name("payload"), "payload");
    assert_eq!(base_name("pi-flash"), "pi-flash", "绿色包基名不能被吃掉");
    assert_eq!(base_name("my-app"), "my-app", "非数字尾巴不算版本");
    assert_eq!(base_name("pi-flash-0.1"), "pi-flash");
}

/// 交接自证：目录名版本必须等于进程版本，否则拒绝交接（防 spawn 风暴）。
#[test]
fn self_version_guard() {
    let root = temp_dir("selfver");
    let mine = root.join(format!("payload-{}", current_version()));
    fake_payload(&mine, current_version());
    assert!(self_version_matches(&mine), "版本一致应放行");
    let faked = root.join("payload-9.9.9");
    fake_payload(&faked, "9.9.9");
    assert!(!self_version_matches(&faked), "戳与进程版本不符应拒交");
    assert!(
        self_version_matches(&root.join("payload")),
        "无版本名照常交接"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 布局核心：目录名带版本 + 戳吻合 + exe 在位 = 可交接的新载荷；半截目录、
/// 名字与戳不符、非版本目录一律落选。
#[test]
fn staged_newer_picks_complete_payload_only() {
    let root = temp_dir("layout");
    std::fs::create_dir_all(root.join("payload")).unwrap();
    fake_payload(&root.join("payload-0.1.10"), "0.1.10");
    // 半截：目录名对、戳缺失
    let broken = root.join("payload-0.1.11");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join(exe_rel()), b"fake-exe").unwrap();
    // 名字与戳不符
    fake_payload(&root.join("payload-0.1.12"), "0.1.10");

    let got = staged_newer(&root, "0.1.9").unwrap();
    assert_eq!(got.0, "0.1.10", "只有完整目录算数");
    assert_eq!(got.1, root.join("payload-0.1.10"));
    assert!(staged_newer(&root, "0.1.10").is_none(), "同版本不算更新");
    let _ = std::fs::remove_dir_all(&root);
}

/// 清理：只删比自己旧的完整载荷目录，且不碰基名无版本的安装目录 /
/// 自己所在目录 / 更高的新载荷。版本全部相对 `current_version()` 推导：
/// cleanup_older 比的是编译期 crate 版本，写死具体号必随版本上涨碎掉
///（0.1.12 起 0.1.11 已不算「更新的」）。
#[test]
fn cleanup_removes_only_older_payloads() {
    let root = temp_dir("cleanup");
    let cur = current_version();
    let older = "0.0.9"; // 任何现实版本之下
    let newer = format!("9{cur}"); // 首段 9 > 0，恒在当前版本之上
    std::fs::create_dir_all(root.join("payload")).unwrap();
    fake_payload(&root.join(format!("payload-{older}")), older);
    fake_payload(&root.join(format!("payload-{cur}")), cur);
    fake_payload(&root.join(&format!("payload-{newer}")), &newer);
    cleanup_older(root.clone(), root.join(format!("payload-{cur}")));
    assert!(
        !root.join(format!("payload-{older}")).exists(),
        "旧版本目录被清"
    );
    assert!(root.join(format!("payload-{cur}")).exists(), "自己不动");
    assert!(root.join(format!("payload-{newer}")).exists(), "更新的不动");
    assert!(root.join("payload").exists(), "安装目录（无版本）不动");
    let _ = std::fs::remove_dir_all(&root);
}

/// 选择：排除自己所在目录（否则「点击重启」会拉起自己）。
#[test]
fn pick_next_excludes_self() {
    let root = temp_dir("pick-next");
    let me = root.join("payload-0.1.10");
    let next = root.join("payload-0.1.11");
    fake_payload(&me, "0.1.10");
    fake_payload(&next, "0.1.11");
    assert_eq!(pick_next(&root, "0.1.10", &me).unwrap().1, next);
    assert!(
        pick_next(&root, "0.1.10", &next).is_none(),
        "自己就是最新的 → 没有可切的更高版本"
    );
    assert_eq!(
        pick_next(&root, "0.1.10", &root.join("payload")).unwrap().1,
        next,
        "安装目录（无版本名）不是任何人"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// 老落位方案（0.1.6~0.1.10 整目录 swap 时代）的残留必须被启动清理带走：
/// helper 日志 / staged 标记 / `payload.old-*`（升级过来时一份就是 300MB）。
/// `payload.stage-<版本>` 是例外——那是完整载荷，走版本规则。
#[test]
fn cleanup_removes_legacy_swap_leftovers() {
    let root = temp_dir("legacy");
    let me = root.join("payload");
    fake_payload(&me, "0.1.10");
    let old = root.join("payload.old-20261014215500");
    std::fs::create_dir_all(&old).unwrap();
    std::fs::write(old.join(exe_rel()), b"x").unwrap();
    std::fs::write(root.join("update-swap.log"), b"log").unwrap();
    std::fs::write(root.join("update-staged.json"), b"{}").unwrap();
    let old_stage = root.join("payload.stage-0.1.8");
    fake_payload(&old_stage, "0.1.8");

    cleanup_older(root.clone(), me.clone());

    assert!(
        !old.exists(),
        "payload.old-* 应被清（升级过来的 300MB 孤儿）"
    );
    assert!(
        !root.join("update-swap.log").exists(),
        "老 helper 日志应被清"
    );
    assert!(!root.join("update-staged.json").exists(), "老标记应被清");
    assert!(!old_stage.exists(), "更旧的 staging 走版本规则清掉");
    assert!(me.exists(), "当前载荷不动");
    let _ = std::fs::remove_dir_all(&root);
}

/// **清理绝不删活目录**（0.1.6~0.1.10 死循环的教训）：目录里有进程
/// share-none 持有文件时，`rename` 探针必败 → 跳过本次；占用释放后
/// 再跑一次即可删掉。若改用 `remove_dir_all` 硬删，就会把正在运行的
/// 旧载荷从磁盘上掏空。
#[cfg(windows)]
#[test]
fn cleanup_skips_live_payload_dir() {
    let root = temp_dir("cleanup-live");
    let live = root.join("payload-0.1.8");
    let me = root.join("payload-0.1.10");
    fake_payload(&live, "0.1.8");
    fake_payload(&me, "0.1.10");

    // 占用者：share-none 打开 live 内的文件，抓住后写哨兵文件（测试侧靠它
    // 同步，不靠碰运气的 sleep）
    let sentinel = root.join("holding.txt");
    let mut holder = hidden_powershell()
        .args([
            "-NoProfile",
            "-Command",
            &format!(
                "$f=[System.IO.File]::Open('{}','Open','Read','None'); \
                 Set-Content -LiteralPath '{}' -Value ok; \
                 Start-Sleep -Seconds 30",
                live.join(exe_rel()).display(),
                sentinel.display()
            ),
        ])
        .spawn()
        .unwrap();
    for _ in 0..50 {
        if sentinel.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert!(sentinel.exists(), "占用者未就位（powershell 启动失败？）");

    cleanup_older(root.clone(), me.clone());
    assert!(live.exists(), "活目录（文件被占用）绝不能删");
    assert!(me.exists(), "自己不动");

    let _ = holder.kill();
    let _ = holder.wait();
    std::thread::sleep(Duration::from_millis(300));
    cleanup_older(root.clone(), me.clone());
    assert!(!live.exists(), "占用释放后应能删掉");
    assert!(me.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn hash_line_parse() {
    let text = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  pi-flash-0.1.2-win32-x64.zip\n\
                011f68a3a5e3c1fda41e7f68a9a4f1654eeab5d5f4d19c8e13a7d411d1a25b35 *other.zip\n";
    assert_eq!(
        parse_hash_for(text, "pi-flash-0.1.2-win32-x64.zip").unwrap(),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert!(parse_hash_for(text, "missing.zip").is_none());
    assert!(parse_hash_for("garbage line\n", "x.zip").is_none());
}

#[test]
fn zip_path_sanitize() {
    let dest = Path::new("D:\\pkg");
    assert!(safe_zip_path(dest, "pi-flash/vendor/pi/x.js").is_some());
    assert!(safe_zip_path(dest, "pi-flash\\vendor\\x.js").is_some());
    assert!(safe_zip_path(dest, "/abs/path").is_none(), "绝对路径拒绝");
    assert!(safe_zip_path(dest, "a/../../etc").is_none(), "上跳拒绝");
    assert!(safe_zip_path(dest, "C:\\evil").is_none(), "盘符拒绝");
    assert!(safe_zip_path(dest, "a/./b").is_some(), "点段折叠");
}

// --- 下载引擎功能测试（本地裸 TCP 假源，真下载真哈希真守卫） ------------

/// 起一次性 HTTP 服务返回 base；trickle_ms = 每 1KB 间隔（模拟慢源）；
/// with_length = false 时无 content-length（close 分隔）。
fn serve(body: Vec<u8>, trickle_ms: Option<u64>, with_length: bool) -> String {
    use std::io::Write as _;
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    std::thread::spawn(move || {
        let Ok((mut s, _)) = listener.accept() else {
            return;
        };
        let mut req = [0u8; 4096];
        let _ = std::io::Read::read(&mut s, &mut req);
        let len_head = if with_length {
            format!("Content-Length: {}\r\n", body.len())
        } else {
            String::new()
        };
        let _ = write!(s, "HTTP/1.1 200 OK\r\n{len_head}Connection: close\r\n\r\n");
        for chunk in body.chunks(1024) {
            if s.write_all(chunk).is_err() {
                return; // 测试端测速中止断开属预期
            }
            if let Some(ms) = trickle_ms {
                std::thread::sleep(Duration::from_millis(ms));
            }
        }
    });
    format!("http://{addr}")
}

/// 已关闭的端口（连接拒绝），当挂掉的源。
fn dead_base() -> String {
    use std::net::TcpListener;
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    format!("http://{addr}")
}

/// 单源下载的落盘目标（每次开头清一次；**测试结束必须自己删**——
/// 之前只在开头清，每次 `cargo test` 都在 %TEMP% 漏三份最多 256KB 的残渣）。
fn test_dest(name: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("pf-upd-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

/// 顺序跑三种场景（共用全局百分比原子量，避免并行互扰）：
/// ① 测速守卫弃慢源；② 死源在前自动切换，哈希正确、百分比到 100；
/// ③ 无 content-length 成功但无百分比。
#[test]
fn download_speed_guard_and_failover() {
    let _guard = PROGRESS_LOCK.lock().unwrap();
    use sha2::Digest as _;
    let probe = Duration::from_millis(300);
    let min_bps = 128.0 * 1024.0;
    let (d_slow, d_fo, d_nl) = (test_dest("slow"), test_dest("fo"), test_dest("nl"));

    // ① 20KB/s 慢源 → 快速中止，错误信息含"测速不过"
    let slow = serve(vec![0xAB; 64 * 1024], Some(50), true);
    let t0 = std::time::Instant::now();
    let r = download_and_hash_cfg(&format!("{slow}/v0.0.1/x.zip"), &d_slow, probe, min_bps);
    assert!(r.is_err(), "慢源应被测速守卫拒绝");
    assert!(r.unwrap_err().contains("测速不过"));
    assert!(
        t0.elapsed() < Duration::from_secs(5),
        "应秒级中止而非拖到整体超时"
    );

    // ② 死端口在前 → 切到可用源；命中源正确、SHA-256 正确、百分比 100
    let body = vec![0x5Au8; 256 * 1024];
    let good = serve(body.clone(), None, true);
    let bases = vec![dead_base(), good.clone()];
    let (hash, chosen) =
        download_mirrored_cfg("x.zip", &d_fo, "0.0.1", &bases, probe, min_bps).unwrap();
    assert_eq!(chosen, good, "应命中可用源");
    let mut h = sha2::Sha256::new();
    h.update(&body);
    assert_eq!(hash, format!("{:x}", h.finalize()), "边下边算哈希正确");
    assert_eq!(download_percent(), Some(100));

    // ③ 无 content-length：下载成功但百分比保持未知
    let stream = serve(b"no-length-body".to_vec(), None, false);
    let (_h3, chosen3) =
        download_mirrored_cfg("x.zip", &d_nl, "0.0.1", &[stream.clone()], probe, min_bps)
            .unwrap();
    assert_eq!(chosen3, stream);
    assert_eq!(download_percent(), None, "无 content-length 无百分比");

    for d in [&d_slow, &d_fo, &d_nl] {
        let _ = std::fs::remove_file(d);
    }
}

/// 真实网络端到端（发版前手动跑一次）：
/// `cargo test -p app updater_real_download -- --ignored --nocapture`
#[test]
#[ignore = "真实网络下载整个载荷（约 85MB），手动验证用"]
fn updater_real_download() {
    let rel = fetch_latest().unwrap_or_else(|e| panic!("fetch_latest 失败：{e}"));
    let asset = asset_name(&rel.version);
    let dir = std::env::temp_dir().join(format!("pf-upd-real-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dest = dir.join(&asset);
    let (hash, chosen) = download_mirrored(&asset, &dest, &rel.version)
        .unwrap_or_else(|e| panic!("下载失败：{e}"));
    eprintln!(
        "v{} 命中源：{chosen}\nsha256={hash}\n文件：{}",
        rel.version,
        dest.display()
    );
    assert_eq!(download_percent(), Some(100));
    assert!(dest.metadata().unwrap().len() > 0);
    let _ = std::fs::remove_dir_all(&dir);
}

// --- 更新管线端到端（假源 + 真 zip，无 GUI 无 powershell） ----------------
// 进度全局量（DOWNLOAD_PCT/EXTRACT_PCT/UPDATE_PHASE）跨测试共享：凡断言
// 或触发它们的用例一律持锁，防 cargo 并行线程互扰。

static PROGRESS_LOCK: Mutex<()> = Mutex::new(());

/// 多请求静态文件站（serve() 只接一条连接；管线要 zip、sidecar、SUMS 多
/// 次请求）。URL 路径直接映射 root 下相对文件，缺失 404。
fn serve_dir(root: &Path) -> String {
    use std::io::{Read as _, Write as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let root = root.to_path_buf();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { return };
            let mut req = [0u8; 4096];
            if s.read(&mut req).is_err() {
                continue;
            }
            let req = String::from_utf8_lossy(&req);
            let Some(path) = req.split_whitespace().nth(1) else {
                continue;
            };
            match std::fs::read(root.join(path.trim_start_matches('/'))) {
                Ok(data) => {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        data.len()
                    );
                    let _ = s.write_all(head.as_bytes());
                    let _ = s.write_all(&data);
                }
                Err(_) => {
                    let _ = s.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found",
                    );
                }
            }
        }
    });
    format!("http://{addr}")
}

/// 真 zip 夹具（复刻 release.sh：Compress-Archive 包装目录 pi-flash/），
/// 附 sidecar 与 SHA256SUMS，落 site/v<ver>/。
fn build_release_fixture(site: &Path, ver: &str, exe_bytes: &[u8], marker: &str) {
    let fix_root = site.join(format!("fix-{ver}"));
    let pkg = fix_root.join("pi-flash");
    std::fs::create_dir_all(&pkg).unwrap();
    std::fs::write(pkg.join(exe_rel()), exe_bytes).unwrap();
    std::fs::write(pkg.join("marker.txt"), marker).unwrap();

    let ver_dir = site.join(format!("v{ver}"));
    std::fs::create_dir_all(&ver_dir).unwrap();
    let zip = ver_dir.join(format!("pi-flash-{ver}-win32-x64.zip"));
    let ps = format!(
        "Compress-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
        pkg.display(),
        zip.display()
    );
    let out = hidden_powershell()
        .args(["-NoProfile", "-Command", &ps])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "夹具压缩失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );
    use sha2::Digest as _;
    let mut h = sha2::Sha256::new();
    h.update(std::fs::read(&zip).unwrap());
    let hash = format!("{:x}", h.finalize());
    let name = zip.file_name().unwrap().to_string_lossy().to_string();
    std::fs::write(
        ver_dir.join(format!("{name}.sha256")),
        format!("{hash}  {name}\n"),
    )
    .unwrap();
    std::fs::write(ver_dir.join("SHA256SUMS"), format!("{hash}  {name}\n")).unwrap();
}

/// 用户实录链路（无 GUI 主防线）：假源 latest → stage 落位新载荷目录
/// → 目录/戳/内容齐全 → `staged_newer` 认账（启动交接与 Ready 态的数据源）
/// → 清理旧版本目录时不动它。
#[cfg(windows)]
#[test]
fn update_pipeline_end_to_end() {
    let _guard = PROGRESS_LOCK.lock().unwrap();
    let root = temp_dir("pipe-e2e");
    let me = root.join("payload"); // 当前载荷目录（模拟 npm 薄壳布局）
    fake_payload(&me, "0.1.8");

    let ver = "9.9.9";
    let site = root.join("site");
    build_release_fixture(&site, ver, b"new", "payload-v9.9.9\n");
    let base = serve_dir(&site);
    // SAFETY：活跃测试里只有本测试与 bad_hash 用例读 PI_FLASH_MIRROR，
    // 其余下载测试走 _cfg 显式注入，无并行读者
    unsafe { std::env::set_var("PI_FLASH_MIRROR", &base) };

    let rel = ReleaseInfo {
        version: ver.to_string(),
        body: String::new(),
    };
    let staged = stage_update_in(&me, &rel).unwrap();
    assert_eq!(staged, ver);

    let target = root.join(format!("payload-{ver}"));
    assert_eq!(
        std::fs::read_to_string(target.join(".payload-version")).unwrap(),
        format!("{ver}\n")
    );
    assert_eq!(
        std::fs::read(target.join("marker.txt")).unwrap(),
        b"payload-v9.9.9\n",
        "载荷内容为新夹具"
    );
    assert_eq!(
        staged_newer(&root, "0.1.8").map(|(v, _)| v),
        Some(ver.to_string()),
        "启动交接认账"
    );
    // 旧版本清理不碰新载荷；旧载荷目录（无版本名）也不动
    cleanup_older(root.clone(), me.clone());
    assert!(target.exists(), "新载荷目录不能被清理碰掉");
    assert!(me.exists(), "安装目录不动");
    let tmp_left: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("update.tmp-"))
        .collect();
    assert!(tmp_left.is_empty(), "半截下载目录已清：{tmp_left:?}");

    unsafe { std::env::remove_var("PI_FLASH_MIRROR") };
    let _ = std::fs::remove_dir_all(&root);
}

/// 哈希不符的失败路径：stage 报 SHA-256 错；旧载荷原样，无新目录/无 tmp 残留。
#[cfg(windows)]
#[test]
fn stage_update_bad_hash_leaves_no_trace() {
    let _guard = PROGRESS_LOCK.lock().unwrap();
    let root = temp_dir("pipe-bad");
    let me = root.join("payload");
    fake_payload(&me, "0.1.8");

    let ver = "9.9.9";
    let site = root.join("site");
    build_release_fixture(&site, ver, b"new", "payload-v9.9.9\n");
    // 篡改 sidecar 与 SUMS 为错哈希（64 位十六进制保证能被解析后比对）
    let bad = format!("{}\n", "0".repeat(64));
    std::fs::write(
        site.join(format!("v{ver}/pi-flash-{ver}-win32-x64.zip.sha256")),
        &bad,
    )
    .unwrap();
    std::fs::write(site.join(format!("v{ver}/SHA256SUMS")), &bad).unwrap();
    let base = serve_dir(&site);
    // SAFETY：见 update_pipeline_end_to_end
    unsafe { std::env::set_var("PI_FLASH_MIRROR", &base) };

    let rel = ReleaseInfo {
        version: ver.to_string(),
        body: String::new(),
    };
    let err = stage_update_in(&me, &rel).unwrap_err();
    assert!(err.contains("SHA-256"), "应报校验不符：{err}");

    assert_eq!(
        std::fs::read_to_string(me.join(".payload-version")).unwrap(),
        "0.1.8\n",
        "旧载荷原样"
    );
    assert!(
        !root.join(format!("payload-{ver}")).exists(),
        "无新载荷目录"
    );
    let residue: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("update.tmp-"))
        .collect();
    assert!(residue.is_empty(), "tmp 已清：{residue:?}");

    unsafe { std::env::remove_var("PI_FLASH_MIRROR") };
    let _ = std::fs::remove_dir_all(&root);
}

/// 半截残留目录（上次解压中断：有目录、无戳）必须落选，且下次落位要把它
/// 换成完整的（而不是因目录已存在而失败）。
#[cfg(windows)]
#[test]
fn stage_replaces_stale_incomplete_target() {
    let _guard = PROGRESS_LOCK.lock().unwrap();
    let root = temp_dir("stage-stale");
    let me = root.join("payload");
    fake_payload(&me, "0.1.9");

    let ver = "9.9.9";
    let stale = root.join(format!("payload-{ver}"));
    std::fs::create_dir_all(&stale).unwrap();
    std::fs::write(stale.join(exe_rel()), b"half-extracted").unwrap();
    std::fs::write(stale.join("junk.txt"), b"leftover").unwrap();
    assert!(
        staged_newer(&root, "0.1.9").is_none(),
        "无版本戳 = 半截 → 落选（绝不能拿来交接）"
    );

    let site = root.join("site");
    build_release_fixture(&site, ver, b"new", "payload-v9.9.9\n");
    let base = serve_dir(&site);
    // SAFETY：见 update_pipeline_end_to_end
    unsafe { std::env::set_var("PI_FLASH_MIRROR", &base) };

    let rel = ReleaseInfo {
        version: ver.to_string(),
        body: String::new(),
    };
    stage_update_in(&me, &rel).unwrap();

    assert_eq!(
        std::fs::read_to_string(stale.join(".payload-version")).unwrap(),
        format!("{ver}\n")
    );
    assert_eq!(
        std::fs::read(stale.join("marker.txt")).unwrap(),
        b"payload-v9.9.9\n"
    );
    assert!(!stale.join("junk.txt").exists(), "旧半截内容被整目录替换掉");
    assert_eq!(
        staged_newer(&root, "0.1.9").map(|(v, _)| v),
        Some(ver.to_string())
    );

    unsafe { std::env::remove_var("PI_FLASH_MIRROR") };
    let _ = std::fs::remove_dir_all(&root);
}

/// 拒绝落位到当前运行目录（防止自覆盖）。
#[test]
fn stage_refuses_running_dir() {
    let root = temp_dir("self-target");
    let me = root.join("payload-0.1.10");
    fake_payload(&me, "0.1.10");
    let rel = ReleaseInfo {
        version: "0.1.10".to_string(),
        body: String::new(),
    };
    let err = stage_update_in(&me, &rel).unwrap_err();
    assert!(err.contains("当前运行目录"), "{err}");
    let _ = std::fs::remove_dir_all(&root);
}

/// 并行解压树完整性：Compress-Archive 造真 zip（嵌套目录 + PS 5.1 的
/// 反斜杠条目名——safe_zip_path 归一化正为此），并行解压后逐文件比对
/// 内容；进度终值必须到 100（pif-ui snapshot progress 的数据源）。
#[cfg(windows)]
#[test]
fn extract_zip_parallel_tree_intact() {
    let _guard = PROGRESS_LOCK.lock().unwrap();
    let dir = temp_dir("extract-par");
    let src = dir.join("pi-flash");
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for i in 0..120 {
        let rel = if i % 3 == 0 {
            format!("vendor/pi/deep/dir{i}/module_{i}.js")
        } else if i % 3 == 1 {
            format!("vendor/node_modules/pkg{i}/lib/index.js")
        } else {
            format!("assets/i18n/msg_{i}.json")
        };
        let body = format!("payload {i}: {}", "x".repeat(i * 37 % 900));
        let p = src.join(&rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, &body).unwrap();
        files.push((rel, body.into_bytes()));
    }
    let zip_path = dir.join("t.zip");
    let ps = format!(
        "Compress-Archive -Path '{}' -DestinationPath '{}' -Force",
        src.display(),
        zip_path.display()
    );
    let out = hidden_powershell()
        .args(["-NoProfile", "-Command", &ps])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "Compress-Archive 失败：{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let dest = dir.join("out");
    extract_zip(&zip_path, &dest).unwrap();
    // Compress-Archive -Path <目录> 连包装目录一起进 zip：条目名带
    // pi-flash/ 前缀（与 release.sh 产物同形，locate_content_root 消费）
    for (rel, body) in &files {
        let got = std::fs::read(dest.join("pi-flash").join(rel.replace('/', "\\")))
            .unwrap_or_else(|e| panic!("{rel}: {e}"));
        assert_eq!(&got, body, "{rel} 内容不符");
    }
    assert_eq!(extract_percent(), Some(100), "解压进度终值应为 100");
    let _ = std::fs::remove_dir_all(&dir);
}

/// 真机计时（`cargo test -p app --bin pi-flash extract_real_dist -- --ignored
/// --nocapture`）：对 dist/ 里最新真 zip 跑并行解压，打印耗时与条目数。
/// 基线对照 = PowerShell Expand-Archive（单线程）。dist/ 不存在时跳过。
#[cfg(windows)]
#[test]
#[ignore]
fn extract_real_dist_zip_timing() {
    // cargo test 的 cwd 是 crate 根；CARGO_MANIFEST_DIR = <ws>/crates/app，
    // 剥两级到工作区根（同 vendor.rs base_candidates 惯例）
    let dist = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .unwrap()
        .join("dist");
    let mut best: Option<(PathBuf, std::time::Duration)> = None;
    if let Ok(entries) = std::fs::read_dir(&dist) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if name.starts_with("pi-flash-") && name.ends_with("-win32-x64.zip") {
                let score = e
                    .metadata()
                    .and_then(|m| m.modified())
                    .map(|t| t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default())
                    .unwrap_or_default();
                if best.as_ref().map(|(_, s)| score > *s).unwrap_or(true) {
                    best = Some((e.path(), score));
                }
            }
        }
    }
    let Some((zip_path, _)) = best else {
        eprintln!("dist/ 无 zip，跳过");
        return;
    };
    let dest = std::env::temp_dir().join(format!("pf-extract-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dest);
    let t0 = std::time::Instant::now();
    extract_zip(&zip_path, &dest).unwrap();
    let dt = t0.elapsed();
    let count = count_files(&dest);
    eprintln!(
        "并行解压 {:?}：{} 个文件，{:?}（{:?}/s）",
        zip_path.file_name().unwrap(),
        count,
        dt,
        (count as f64 / dt.as_secs_f64()) as u64
    );
    let _ = std::fs::remove_dir_all(&dest);
}

fn count_files(root: &Path) -> usize {
    let mut n = 0;
    if let Ok(entries) = std::fs::read_dir(root) {
        for e in entries.flatten() {
            if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                n += count_files(&e.path());
            } else {
                n += 1;
            }
        }
    }
    n
}
