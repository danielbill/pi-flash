//! payload 测试（itlq.9 自 payload.rs 外移，纯结构搬移）：稳定入口自愈/
//! 预拷/清理判据（含真镜像锁用例）。

use super::*;

fn temp_root(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "pf-upd-{}-{}-{}",
        tag,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 造一份完整载荷：exe + 若干普通文件 + 戳（最后写，与真实布局同序）。
fn make_payload(dir: &Path, ver: &str, extra: &[&str]) {
    std::fs::create_dir_all(dir).unwrap();
    let mut files: Vec<&str> = vec!["pi-flash.exe", "vendor/pi/x.js"];
    files.extend_from_slice(extra);
    for f in files {
        let p = dir.join(f);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, format!("{f} @ {ver}")).unwrap();
    }
    std::fs::write(dir.join(".payload-version"), format!("{ver}\n")).unwrap();
}

/// 真镜像锁（windows）：把系统 cmd.exe 复制成 `exe` 并运行——运行中镜像
/// 的写打开必败（ERROR_SHARING_VIOLATION）。std 打开文件自带全共享模式，
/// 测试里持写句柄模拟不出镜像锁，必须真跑。返回 None = 环境不可用，调用
/// 方跳过用例。
#[cfg(windows)]
fn spawn_image_lock(exe: &Path) -> Option<std::process::Child> {
    std::fs::create_dir_all(exe.parent().unwrap()).ok()?;
    let cmd = std::env::var("SystemRoot")
        .map(|r| std::path::PathBuf::from(r).join("System32/cmd.exe"))
        .unwrap_or_else(|_| std::path::PathBuf::from(r"C:\Windows\System32\cmd.exe"));
    std::fs::copy(&cmd, exe).ok()?;
    let mut child = std::process::Command::new(exe)
        .args(["/c", "ping", "-n", "30", "127.0.0.1"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    // 轮询等镜像节真正建立（全量并行下 CPU 抢占，固定 sleep 会翻车）：
    // 写打开失败 = 锁已生效；5s 仍锁不上视为环境异常
    for _ in 0..50 {
        if std::fs::OpenOptions::new().write(true).open(exe).is_err() {
            return Some(child);
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = child.kill();
    let _ = child.wait();
    eprintln!("[test] 镜像锁 5s 未建立，跳过用例");
    None
}

#[test]
fn stable_entry_version_requires_stamp_and_exe() {
    let root = temp_root("sev");
    let stable = root.join("payload");
    assert_eq!(stable_entry_version(&stable), None, "啥都没有");

    std::fs::create_dir_all(&stable).unwrap();
    std::fs::write(stable.join(".payload-version"), "0.4.2\n").unwrap();
    assert_eq!(stable_entry_version(&stable), None, "只有戳没有 exe = 半残");

    std::fs::write(stable.join(exe_rel()), b"exe").unwrap();
    assert_eq!(stable_entry_version(&stable).as_deref(), Some("0.4.2"));

    std::fs::write(stable.join(".payload-version"), "垃圾\n").unwrap();
    assert_eq!(stable_entry_version(&stable), None, "非版本戳不认");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn refresh_copies_newest_and_stamps_last() {
    let root = temp_root("refresh");
    let me = root.join("payload-0.4.2");
    make_payload(&me, "0.4.2", &["extensions/a.json"]);
    // 稳定入口半残：有目录没内容（npm replacePayload 炸后的典型残骸）
    std::fs::create_dir_all(root.join("payload")).unwrap();

    assert!(refresh_stable_entry(&root, &me));
    let stable = root.join("payload");
    assert_eq!(stable_entry_version(&stable).as_deref(), Some("0.4.2"));
    assert_eq!(
        std::fs::read_to_string(stable.join("pi-flash.exe")).unwrap(),
        "pi-flash.exe @ 0.4.2"
    );
    assert!(stable.join("vendor/pi/x.js").is_file());
    assert!(stable.join("extensions/a.json").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn refresh_noop_when_already_current() {
    let root = temp_root("noop");
    let me = root.join("payload-0.4.2");
    make_payload(&me, "0.4.2", &[]);
    make_payload(&root.join("payload"), "0.4.2", &[]);

    assert!(!refresh_stable_entry(&root, &me), "已是最新 = 零动作");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn refresh_never_downgrades() {
    let root = temp_root("downgrade");
    let me = root.join("payload-0.4.2");
    make_payload(&me, "0.4.2", &[]);
    // 稳定入口更新（npm 刚装的新壳载荷），版本化目录是旧的
    make_payload(&root.join("payload"), "0.5.0", &[]);

    assert!(!refresh_stable_entry(&root, &me), "不降级");
    assert_eq!(
        std::fs::read_to_string(root.join("payload/pi-flash.exe")).unwrap(),
        "pi-flash.exe @ 0.5.0"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[cfg(windows)]
fn refresh_skips_when_target_locked() {
    let root = temp_root("locked");
    let me = root.join("payload-0.4.2");
    make_payload(&me, "0.4.2", &[]);
    let stable = root.join("payload");
    // 老版本跑在稳定入口：镜像锁 → 整体跳过
    let Some(mut child) = spawn_image_lock(&stable.join(exe_rel())) else {
        eprintln!("[test] 镜像锁不可用，跳过用例");
        return;
    };
    std::fs::write(stable.join(".payload-version"), "0.4.0\n").unwrap();

    assert!(!refresh_stable_entry(&root, &me), "被占用整体跳过");
    assert_eq!(
        stable_entry_version(&stable).as_deref(),
        Some("0.4.0"),
        "旧内容原封不动"
    );
    let _ = child.kill();
    let _ = child.wait();
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn refresh_incomplete_writes_no_stamp() {
    let root = temp_root("partial");
    let me = root.join("payload-0.4.2");
    make_payload(&me, "0.4.2", &["node.exe"]);
    let stable = root.join("payload");
    std::fs::create_dir_all(&stable).unwrap();
    // 目标侧埋雷：node.exe 位置是目录 → 源文件复制必败 → 不完整
    std::fs::create_dir_all(stable.join("node.exe")).unwrap();

    assert!(!refresh_stable_entry(&root, &me), "有失败 = 不完整");
    assert_eq!(
        stable_entry_version(&stable),
        None,
        "不完整绝不写戳冒充完整载荷"
    );
    assert!(
        stable.join("pi-flash.exe").is_file(),
        "非 exe 已先拷成功（旧 exe 仍可启动走交接）"
    );
    let marker = std::fs::read_to_string(stable.join(STAGE_MARKER)).unwrap();
    assert!(
        marker.starts_with("0.4.2\n") && marker.contains("node.exe"),
        "失败清单入标记（首行版本 + 相对路径），下次按清单续拷"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cleanup_deletes_redundant_versioned_dirs() {
    let root = temp_root("cleanup");
    let me = root.join("payload-9.9.9");
    make_payload(&me, "9.9.9", &[]);
    make_payload(&root.join("payload"), "9.9.9", &[]); // 稳定入口 = 9.9.9
    make_payload(&root.join("payload-0.4.1"), "0.4.1", &[]);
    make_payload(&root.join("payload-8.8.8"), "8.8.8", &[]);

    cleanup_older(root.clone(), me.clone());
    assert!(
        !root.join("payload-0.4.1").exists(),
        "≤ 稳定入口戳 → 冗余删除"
    );
    assert!(
        !root.join("payload-8.8.8").exists(),
        "≤ 稳定入口戳 → 冗余删除"
    );
    assert!(me.exists(), "自己所在的目录永不删（运行中）");
    assert!(
        root.join("payload").exists(),
        "稳定入口本体不在版本化清单里"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn cleanup_keeps_everything_without_stable_stamp() {
    let root = temp_root("cleanup2");
    let me = root.join("payload-9.9.9");
    make_payload(&me, "9.9.9", &[]);
    make_payload(&root.join("payload-0.0.1"), "0.0.1", &[]);
    // 没有稳定入口目录 → 扩展判据不生效
    make_payload(&root.join("payload-9.9.8"), "9.9.8", &[]);

    cleanup_older(root.clone(), me.clone());
    assert!(
        root.join("payload-9.9.8").exists(),
        "无稳定入口戳时扩展判据不删（outdated 判据对 9.9.8 不成立）"
    );
    assert!(me.exists());
    // payload-0.0.1 会被 outdated 判据删（current_version() 必大于 0.0.1），
    // 与扩展判据无关，不在此断言。
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn precopy_direct_migration_when_target_idle() {
    let root = temp_root("precopy");
    let me = root.join("payload-0.4.2");
    make_payload(&me, "0.4.2", &["node.exe"]);
    let target = me.clone();
    let stable = root.join("payload");
    std::fs::create_dir_all(&stable).unwrap(); // 空（被炸残骸）

    // 稳定入口无主（老进程跑在版本化目录）→ T1 直接完成迁移
    precopy_stable_entry(&root, &me, &target, "0.4.2");
    assert_eq!(
        stable_entry_version(&stable).as_deref(),
        Some("0.4.2"),
        "无一被锁 → 直接迁移写戳"
    );
    assert!(!stable.join(STAGE_MARKER).exists(), "完整迁移不留标记");
    assert!(stable.join("node.exe").is_file());
    assert!(stable.join("vendor/pi/x.js").is_file());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[cfg(windows)]
fn precopy_marker_then_refresh_finishes() {
    let root = temp_root("precmk");
    let me = root.join("payload-0.4.2");
    make_payload(&me, "0.4.2", &["node.exe"]);
    let stable = root.join("payload");
    // 老进程本尊跑在稳定入口：pi-flash.exe 镜像锁
    let Some(mut child) = spawn_image_lock(&stable.join(exe_rel())) else {
        eprintln!("[test] 镜像锁不可用，跳过用例");
        return;
    };

    precopy_stable_entry(&root, &me, &me.clone(), "0.4.2");
    assert_eq!(stable_entry_version(&stable), None, "有锁定剩余 = 不写戳");
    assert!(stable.join("node.exe").is_file(), "未锁文件已预拷就位");
    let marker = std::fs::read_to_string(stable.join(STAGE_MARKER)).unwrap();
    assert!(marker.starts_with("0.4.2\n"), "标记首行 = 版本");
    assert!(marker.contains("pi-flash.exe"), "锁定文件在清单里");

    // 老进程退出 → 首启自愈按清单收尾（只补 pi-flash.exe，不再全量）
    let _ = child.kill();
    let _ = child.wait();
    std::thread::sleep(std::time::Duration::from_millis(200));
    assert!(refresh_stable_entry(&root, &me), "收尾完成");
    assert_eq!(stable_entry_version(&stable).as_deref(), Some("0.4.2"));
    assert!(!stable.join(STAGE_MARKER).exists(), "收尾后清标记");
    assert_eq!(
        std::fs::read_to_string(stable.join("pi-flash.exe")).unwrap(),
        "pi-flash.exe @ 0.4.2"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn refresh_marker_version_mismatch_falls_back_to_full() {
    let root = temp_root("precmm");
    let me = root.join("payload-0.4.2");
    make_payload(&me, "0.4.2", &[]);
    let stable = root.join("payload");
    std::fs::create_dir_all(&stable).unwrap();
    // 陈旧标记（版本对不上）→ 必须走全量而非按清单
    std::fs::write(stable.join(STAGE_MARKER), "0.3.9\npi-flash.exe\n").unwrap();

    assert!(refresh_stable_entry(&root, &me));
    assert_eq!(stable_entry_version(&stable).as_deref(), Some("0.4.2"));
    assert!(!stable.join(STAGE_MARKER).exists(), "全量成功清旧标记");
    let _ = std::fs::remove_dir_all(&root);
}
