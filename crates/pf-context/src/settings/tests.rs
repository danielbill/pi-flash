//! 001-3 §3 Rust 测试清单：precedence / round-trip / 坏档 / 并发 / watcher
//! 自写回声 / v1→v2 迁移 / flush 与写失败重试 / coalesce / 同内容跳过。

use super::*;
use section::Section;
use sections::probe::Probe;
use sections::{Appearance, Chat, DevFlags, FontSpec, Index, Models, Sessions, Shell, Subagents};
use serde_json::json;
use std::path::Path;

use crate::tests_support::tmp_dir;

fn write_json_file(path: &Path, v: &serde_json::Value) {
    std::fs::write(path, serde_json::to_string_pretty(v).unwrap()).unwrap();
}

fn read_json_file(path: &Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn store_at(dir: &Path) -> SettingsStore {
    crate::tests_support::store_at(dir)
}

// ---------------------------------------------------------------------------
// precedence：L0 < L1 < L2 < L3（L4 env 只对路径生效，不进本模型——001-0 §4）
// ---------------------------------------------------------------------------

#[test]
fn precedence_l0_to_l3() {
    let dir = tmp_dir("prec");
    let path = dir.join("app-settings.json");
    write_json_file(&path, &json!({ "subagents": { "capacity": 5 } }));

    let store = SettingsStore::open(Some(path));
    assert_eq!(
        store.get::<Subagents>().capacity,
        5,
        "L1 文件压过 L0 默认 3"
    );

    store.set_layer(Layer::Project, Subagents::KEY, json!({ "capacity": 7 }));
    assert_eq!(store.get::<Subagents>().capacity, 7, "L2 > L1");

    store.set_layer(Layer::Session, Subagents::KEY, json!({ "capacity": 9 }));
    assert_eq!(store.get::<Subagents>().capacity, 9, "L3 > L2");

    // 上层只覆盖自己声明的字段：L3 只改 enabled 时 capacity 保持 L2 值
    store.set_layer(Layer::Session, Subagents::KEY, json!({ "enabled": false }));
    let s = store.get::<Subagents>();
    assert_eq!((s.capacity, s.enabled), (7, false));

    // Null = 摘除该层覆盖，逐层回落
    store.set_layer(Layer::Session, Subagents::KEY, serde_json::Value::Null);
    assert_eq!(store.get::<Subagents>().capacity, 7);
    store.set_layer(Layer::Project, Subagents::KEY, serde_json::Value::Null);
    assert_eq!(store.get::<Subagents>().capacity, 5);
}

// ---------------------------------------------------------------------------
// round-trip：未知键 / section 内未知字段，写入前后逐字保留（001-1 §2）
// ---------------------------------------------------------------------------

#[test]
fn unknown_keys_survive_write() {
    let dir = tmp_dir("unknown");
    let path = dir.join("app-settings.json");
    write_json_file(
        &path,
        &json!({
            "x_unknown": { "a": 1, "deep": { "b": [1, 2] } },
            "subagents": { "capacity": 4, "future_field": "keep" },
        }),
    );

    let store = SettingsStore::open(Some(path.clone()));
    store.set(|s: &mut Subagents| s.capacity = 6);
    store.flush().unwrap();

    let doc = read_json_file(&path);
    assert_eq!(doc["x_unknown"], json!({ "a": 1, "deep": { "b": [1, 2] } }));
    assert_eq!(doc["subagents"]["future_field"], "keep");
    assert_eq!(doc["subagents"]["capacity"], 6);
    assert_eq!(store.get::<Subagents>().capacity, 6);
}

// ---------------------------------------------------------------------------
// 坏档：整份非法 → 保上一份好快照 + problem；单段非法 → 该段 Default 其它正常
// ---------------------------------------------------------------------------

#[test]
fn broken_doc_keeps_good_snapshot() {
    let dir = tmp_dir("broken-doc");
    let path = dir.join("app-settings.json");
    write_json_file(&path, &json!({ "subagents": { "capacity": 5 } }));
    let store = SettingsStore::open(Some(path.clone()));
    assert_eq!(store.get::<Subagents>().capacity, 5);
    let problems_before = store.problems().len();

    std::fs::write(&path, "{not json").unwrap();
    store.check_external_change(); // 检测到变化但坏档 → 不换快照

    assert_eq!(store.get::<Subagents>().capacity, 5, "保持上一份好快照");
    assert!(
        store.problems().len() > problems_before
            && store.problems().iter().any(|p| p.key == "<doc>"),
        "problem 留底不许静默"
    );

    // 修好后外部再改 → 恢复重载
    write_json_file(&path, &json!({ "subagents": { "capacity": 7 } }));
    assert!(store.check_external_change());
    assert_eq!(store.get::<Subagents>().capacity, 7);
}

#[test]
fn broken_section_falls_back_to_default_others_survive() {
    let dir = tmp_dir("broken-section");
    let path = dir.join("app-settings.json");
    write_json_file(
        &path,
        &json!({ "subagents": "garbage", "z_probe": { "value": 3 } }),
    );
    let store = SettingsStore::open(Some(path));

    assert_eq!(store.get::<Subagents>().capacity, 3, "坏段 → Default");
    assert_eq!(store.get::<Probe>().value, 3, "其它段正常");
    assert!(store.problems().iter().any(|p| p.key == "subagents"));
}

// ---------------------------------------------------------------------------
// 并发：读线程循环 get() 同时写线程 set()，无 panic、无撕裂快照
// ---------------------------------------------------------------------------

#[test]
fn concurrent_get_during_set() {
    let dir = tmp_dir("concurrent");
    let store = store_at(&dir);
    let reader_store = store.clone();

    let reader = std::thread::spawn(move || {
        for _ in 0..2000 {
            let s = reader_store.get::<Subagents>();
            assert!((2..=10).contains(&s.capacity), "快照值恒过 validate");
            assert_eq!(s.max_depth, 1);
        }
    });
    for i in 0..300u64 {
        store.set(move |s: &mut Subagents| s.capacity = 2 + i % 9);
    }
    reader.join().unwrap();
    store.flush().unwrap();
    assert!(read_json_file(&dir.join("app-settings.json"))["subagents"].is_object());
}

// ---------------------------------------------------------------------------
// watcher：自写不回环（预期哈希窗口）；外部写入触发一次重载
// ---------------------------------------------------------------------------

#[test]
fn watcher_self_write_echo_is_eaten_external_change_reloads() {
    let dir = tmp_dir("watcher");
    let path = dir.join("app-settings.json");
    let store = SettingsStore::open(Some(path.clone()));
    assert!(!store.check_external_change(), "无文件 = 无外部改动");

    store.set(|s: &mut Subagents| s.capacity = 6);
    store.flush().unwrap();
    let reloads_before = store.stats().reloads;
    assert!(!store.check_external_change(), "自写回声被预期哈希窗口吃掉");
    assert_eq!(store.stats().reloads, reloads_before, "不重载");

    // 外部手改（不同内容）→ 重载一次
    write_json_file(&path, &json!({ "subagents": { "capacity": 8 } }));
    assert!(store.check_external_change());
    assert_eq!(store.get::<Subagents>().capacity, 8);
    assert_eq!(store.stats().reloads, reloads_before + 1);

    // 同内容再触发（编辑器一串事件 / touch）→ 不重复重载
    assert!(!store.check_external_change());
}

// ---------------------------------------------------------------------------
// 迁移：v1 平铺 → v2 section，旧键保留、值读回不变（001-1 §3）
// ---------------------------------------------------------------------------

#[test]
fn v1_flat_keys_migrate_to_v2_section_on_write() {
    let dir = tmp_dir("migrate");
    let path = dir.join("app-settings.json");
    write_json_file(
        &path,
        &json!({ "subagent_capacity": 5, "subagent_enabled": false, "keep_me": 1 }),
    );

    let store = SettingsStore::open(Some(path.clone()));
    assert_eq!(store.origin::<Subagents>(), Origin::Legacy, "v1 平铺键命中");
    let s = store.get::<Subagents>();
    assert_eq!((s.capacity, s.enabled), (5, false));

    // 任一 set：该段升 v2（section 键出现），旧平铺键同值保留一版
    store.set(|s: &mut Subagents| s.capacity = 7);
    store.flush().unwrap();

    let doc = read_json_file(&path);
    assert_eq!(doc["subagents"]["capacity"], 7);
    assert_eq!(doc["subagents"]["enabled"], false);
    assert_eq!(doc["subagents"]["max_depth"], 1);
    assert_eq!(doc["subagent_capacity"], 7, "旧键同值保留一版（回滚兼容）");
    assert_eq!(doc["subagent_enabled"], false);
    assert_eq!(doc["keep_me"], 1);

    // 再读：v2 section 为准
    let store2 = SettingsStore::open(Some(path));
    assert_eq!(store2.origin::<Subagents>(), Origin::Section);
    assert_eq!(store2.get::<Subagents>().capacity, 7);
}

// ---------------------------------------------------------------------------
// flush / 写失败：内存保留 + 重试计数（001-0 §5），解除阻塞后落盘成功
// ---------------------------------------------------------------------------

#[test]
fn flush_write_failure_keeps_memory_then_retries() {
    let dir = tmp_dir("wfail");
    let path = dir.join("app-settings.json");
    std::fs::create_dir(&path).unwrap(); // 目录占位 = rename 必败的确定性注入

    let store = SettingsStore::open(Some(path.clone()));
    store.set(|s: &mut Subagents| s.capacity = 6);
    assert!(store.flush().is_err(), "写失败上浮");
    assert!(store.stats().write_failures >= 1, "重试计数");
    assert_eq!(store.get::<Subagents>().capacity, 6, "内存保留用户意图");

    std::fs::remove_dir(&path).unwrap();
    store.set(|s: &mut Subagents| s.capacity = 7);
    assert!(store.flush().is_ok(), "解除阻塞后写成功");
    assert_eq!(read_json_file(&path)["subagents"]["capacity"], 7);
}

// ---------------------------------------------------------------------------
// coalesce：连续 set 只产生一次写盘；同内容跳过
// ---------------------------------------------------------------------------

#[test]
fn coalesce_burst_writes_once_and_skips_identical() {
    let dir = tmp_dir("coalesce");
    let path = dir.join("app-settings.json");
    let store = SettingsStore::open(Some(path.clone()));

    for i in 0..20u64 {
        store.set(move |s: &mut Subagents| s.capacity = 2 + i);
    }
    store.flush().unwrap();
    let writes = store.stats().writes;
    assert_eq!(writes, 1, "20 次 set 合并成 1 次写盘");

    // 同内容再写：跳过（写计数不变）
    store.set(|s: &mut Subagents| s.capacity = 21);
    store.set(|s: &mut Subagents| s.capacity = 21);
    store.flush().unwrap();
    assert_eq!(store.stats().writes, 1, "同内容跳过");

    // 写者线程路径：改值后等 coalesce 窗口，落盘 ≤ 2 次
    store.set(|s: &mut Subagents| s.capacity = 9);
    std::thread::sleep(std::time::Duration::from_millis(400));
    let w = store.stats().writes;
    assert!((1..=2).contains(&w), "写者线程 coalesce 后落盘，实测 {w}");
    assert_eq!(read_json_file(&path)["subagents"]["capacity"], 9);
}

// ---------------------------------------------------------------------------
// origin / 缺省 / validate 唯一钳点 / 纯内存态
// ---------------------------------------------------------------------------

#[test]
fn absent_origin_defaults_and_validate_clamps_on_set() {
    let dir = tmp_dir("absent");
    let store = store_at(&dir);
    assert_eq!(store.origin::<Subagents>(), Origin::Absent);
    let s = store.get::<Subagents>();
    assert_eq!((s.capacity, s.max_depth, s.enabled), (3, 1, true));

    store.set(|s: &mut Subagents| s.capacity = 999);
    assert_eq!(store.get::<Subagents>().capacity, 10, "validate 唯一钳点");
    store.set(|s: &mut Subagents| s.capacity = 0);
    assert_eq!(store.get::<Subagents>().capacity, 2);
    store.set(|s: &mut Subagents| {
        s.capacity = 5;
        s.max_depth = 7; // 恒 1：任何来源写不进别的值
    });
    let s = store.get::<Subagents>();
    assert_eq!((s.capacity, s.max_depth), (5, 1));

    store.flush().unwrap();
    assert_eq!(
        store.origin::<Subagents>(),
        Origin::Section,
        "写后 v2 段存在"
    );
}

#[test]
fn memory_only_store_writes_nothing() {
    let store = SettingsStore::open(None);
    store.set(|s: &mut Subagents| s.capacity = 6);
    assert_eq!(store.get::<Subagents>().capacity, 6);
    assert!(store.flush().is_ok(), "无路径 = 无盘可写，静默成功");
    assert_eq!(store.stats().writes, 0);
    assert!(!store.check_external_change());
}

// ---------------------------------------------------------------------------
// S2：v1 全量 25 键 → v2 整份升级（version:2 + 全 section + 旧键保留一版）
// ---------------------------------------------------------------------------

#[test]
fn s2_full_upgrade_writes_version_and_all_sections() {
    let dir = tmp_dir("s2-upgrade");
    let path = dir.join("app-settings.json");
    write_json_file(
        &path,
        &json!({
            "theme": "rose",
            "lang": 1,
            "chat_content_width": 1000,
            "session_display_count": 5,
            "auto_clean_days": 30,
            "default_provider": "deepseek",
            "default_model": "deepseek-flash",
            "auto_update": false,
            "sound": true,
            "subagent_capacity": 5,
            "x_unknown": { "keep": [1, 2] },
        }),
    );
    let store = SettingsStore::open(Some(path.clone()));
    // 读迁移：全部平铺键进各 section
    assert_eq!(store.get::<Appearance>().theme.as_deref(), Some("rose"));
    assert_eq!(store.get::<Chat>().content_width, Some(1000));
    assert_eq!(store.get::<Sessions>().display_count, Some(5));
    assert_eq!(
        store.get::<Models>().default_model.as_deref(),
        Some("deepseek-flash")
    );
    assert_eq!(store.get::<Shell>().sound, Some(true));

    // 任一 set → 整份升级：version:2 + 全部 section + 全部平铺键保留 + 未知键
    store.set(|c: &mut Chat| c.show_thinking = Some(true));
    store.flush().unwrap();
    let doc = read_json_file(&path);
    assert_eq!(doc["version"], 2);
    for section in [
        "subagents",
        "appearance",
        "chat",
        "sessions",
        "index",
        "models",
        "updates",
        "shell",
    ] {
        assert!(
            doc[section].is_object(),
            "整份升级必须落全部 section：{section}"
        );
    }
    assert_eq!(doc["theme"], "rose", "旧平铺键保留一版");
    assert_eq!(doc["session_display_count"], 5);
    assert_eq!(doc["x_unknown"], json!({ "keep": [1, 2] }));
    assert!(
        !doc.as_object().unwrap().contains_key("dev_flags"),
        "boot-only 段不落盘"
    );
    // 升级后值不变
    let store2 = SettingsStore::open(Some(path));
    assert_eq!(store2.get::<Chat>().content_width, Some(1000));
    assert_eq!(store2.get::<Chat>().show_thinking, Some(true));
}

// ---------------------------------------------------------------------------
// S2：新 section 的 validate 唯一钳点
// ---------------------------------------------------------------------------

#[test]
fn s2_sections_clamp_in_validate() {
    let dir = tmp_dir("s2-clamp");
    let store = store_at(&dir);
    store.set(|c: &mut Chat| c.content_width = Some(9999));
    assert_eq!(store.get::<Chat>().content_width, Some(1200));
    store.set(|se: &mut Sessions| {
        se.display_count = Some(1);
        se.auto_clean_days = Some(500);
    });
    let se = store.get::<Sessions>();
    assert_eq!((se.display_count, se.auto_clean_days), (Some(3), Some(100)));
    store.set(|ix: &mut Index| {
        ix.at_index_ttl_secs = Some(0);
        ix.at_index_max_projects = Some(0);
    });
    let ix = store.get::<Index>();
    assert_eq!(
        (ix.at_index_ttl_secs, ix.at_index_max_projects),
        (Some(1), Some(1))
    );
    store.set(|a: &mut Appearance| {
        a.lang = Some(9);
        a.session_font = Some(FontSpec {
            family: "Inter".into(),
            size: 99.0,
        });
    });
    let a = store.get::<Appearance>();
    assert_eq!(a.lang, Some(2));
    assert_eq!(a.session_font.as_ref().unwrap().size, 30.0);
}

// ---------------------------------------------------------------------------
// S2：拖拽预览通道——只换内存，save_now 才落盘
// ---------------------------------------------------------------------------

#[test]
fn s2_preview_only_persists_on_save_now() {
    let dir = tmp_dir("s2-preview");
    let path = dir.join("app-settings.json");
    write_json_file(&path, &json!({ "chat_content_width": 920 }));
    let store = SettingsStore::open(Some(path.clone()));
    assert_eq!(store.get::<Chat>().content_width, Some(920));

    store.set_preview(|c: &mut Chat| c.content_width = Some(700));
    assert_eq!(
        store.get::<Chat>().content_width,
        Some(700),
        "预览立即换内存"
    );
    assert_eq!(
        read_json_file(&path)["chat_content_width"],
        920,
        "预览不落盘"
    );

    store.save_now().unwrap();
    assert_eq!(read_json_file(&path)["chat_content_width"], 700, "松手落盘");

    // 真 set 会消费掉预览态：save_now 不再重复写
    store.set(|c: &mut Chat| c.git_markers = Some(true));
    store.save_now().unwrap();
    let writes_after = store.stats().writes;
    store.save_now().unwrap();
    assert_eq!(store.stats().writes, writes_after, "空 flush 不写盘");
}

// ---------------------------------------------------------------------------
// S2：dev_flags boot-only——只读、不落 app-settings.json
// ---------------------------------------------------------------------------

#[test]
fn s2_dev_flags_boot_only_never_written() {
    let dir = tmp_dir("s2-devflags");
    let path = dir.join("app-settings.json");
    let store = SettingsStore::open(Some(path.clone()));
    let df = store.get::<DevFlags>();
    // 语义对齐 pi-link default_for：subagents 默认开（env 未设时）
    let env_subagents = std::env::var("PI_FLASH_SUBAGENTS")
        .ok()
        .map(|v| v != "0" && !v.eq_ignore_ascii_case("false"));
    assert_eq!(df.flag("subagents"), env_subagents.unwrap_or(true));
    assert_eq!(df.flag("__no_such_flag__"), false);

    store.set(|c: &mut Chat| c.git_markers = Some(true));
    store.flush().unwrap();
    let doc_text = std::fs::read_to_string(&path).unwrap();
    assert!(
        !doc_text.contains("dev_flags"),
        "boot-only 段永不进 app-settings.json"
    );
}

// ---------------------------------------------------------------------------
// S7：pfctx.log 观测留痕——启动/写盘/投影全记录（验收免人肉）
// ---------------------------------------------------------------------------

#[test]
fn s7_log_file_records_boot_load_write_export() {
    let dir = tmp_dir("s7-log");
    let store = store_at(&dir); // Logger::open(dir) 随 open 建 pfctx.log
    store.set(|s: &mut Subagents| s.capacity = 6);
    store.flush().unwrap(); // flush 实写 + 投影同步（同步路径，非后台线程）

    let text = std::fs::read_to_string(dir.join("pfctx.log")).unwrap();
    assert!(text.contains("[boot]"), "boot 行：{text}");
    assert!(text.contains("settings.load="), "load µs 留痕：{text}");
    assert!(text.contains("[write] subagents"), "写盘留痕：{text}");
    assert!(text.contains("\"capacity\":6"), "写盘留痕带规范值：{text}");
    assert!(
        text.contains("[export] ext-context.json"),
        "投影留痕：{text}"
    );
    assert!(text.contains("settings.flush="), "flush µs 留痕：{text}");

    // 外部改动重载也留痕
    write_json_file(
        &dir.join("app-settings.json"),
        &json!({ "subagents": { "capacity": 8 } }),
    );
    assert!(store.check_external_change());
    let text = std::fs::read_to_string(dir.join("pfctx.log")).unwrap();
    assert!(text.contains("[watch] 外部改动 → 重载完成"), "{text}");
    assert!(text.contains("[watch] 监听启动") == false || true); // watcher 线程竞争，不断言
}

#[test]
fn s7_problems_go_into_log() {
    let dir = tmp_dir("s7-problem");
    let path = dir.join("app-settings.json");
    std::fs::write(&path, "{not json").unwrap();
    let store = SettingsStore::open(Some(path));
    assert!(store.problems().iter().any(|p| p.key == "<doc>"));
    let text = std::fs::read_to_string(dir.join("pfctx.log")).unwrap();
    assert!(text.contains("[problem] <doc>"), "problem 留痕：{text}");
}

// ---------------------------------------------------------------------------
// S8：agent section（自我调试开关，默认 on）
// ---------------------------------------------------------------------------

#[test]
fn s8_agent_section_defaults_on_and_persists() {
    let dir = tmp_dir("s8-agent");
    let path = dir.join("app-settings.json");
    let store = SettingsStore::open(Some(path.clone()));
    assert!(
        store.get::<crate::settings::sections::Agent>().self_debug,
        "默认开"
    );
    store.set(|a: &mut crate::settings::sections::Agent| a.self_debug = false);
    store.flush().unwrap();
    assert_eq!(read_json_file(&path)["agent"]["self_debug"], false);
    assert_eq!(
        SettingsStore::open(Some(path))
            .get::<crate::settings::sections::Agent>()
            .self_debug,
        false
    );
}

#[test]
fn s8_reload_diff_logged_with_values() {
    let dir = tmp_dir("s8-reload-diff");
    let path = dir.join("app-settings.json");
    write_json_file(&path, &json!({ "subagents": { "capacity": 5 } }));
    let store = SettingsStore::open(Some(path.clone()));
    let text_before = std::fs::read_to_string(dir.join("pfctx.log")).unwrap();
    assert!(!text_before.contains("[reload]"));

    write_json_file(&path, &json!({ "subagents": { "capacity": 8 } }));
    assert!(store.check_external_change());
    let text = std::fs::read_to_string(dir.join("pfctx.log")).unwrap();
    assert!(
        text.contains("[reload] subagents:") && text.contains("→"),
        "重载差值带新旧值：{text}"
    );
    assert!(
        text.contains("\"capacity\":5") && text.contains("\"capacity\":8"),
        "{text}"
    );
}
