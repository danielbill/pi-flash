//! 启动扫描（02 §6）/ 会话徽标 / 资产注册表 / 宿主协议文件——单测
//! （自 manifest.rs 外移；fixture 落 temp 目录按进程号隔离）。清单解析单测在
//! [`super::tests`]。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::assets::{PLUGIN_ASSETS, plugin_asset, register_plugin_dir};
use super::parse::parse_manifest;
use super::scan::{StaleKind, StalePlugin, scan_candidates, scan_root};
use crate::pf_plugin::*;

fn write_manifest(dir: &Path, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join("pf-plugin.toml");
    std::fs::write(&p, body).unwrap();
    p
}

const GOOD: &str = r#"
schema = 4
id = "quick-panel"
title = { zh = "快捷面板", en = "Quick Panel" }
version = "0.1.0"

[[contributes.status_bar]]
icon = "assets/bolt.svg"
title = { zh = "快捷" }

[[contributes.file_menu]]
id = "set-quick"
title = { zh = "设为快捷方式", en = "Pin Quick" }
icon = "pin"

[[contributes.panels]]
id = "quick"
title = { zh = "快捷" }
entry = "main.js"
"#;

/// 两档布局都必须给出**同一个顶层包目录名**——停用过滤与「台账 ↔ 运行态」
/// 对账都靠它；下钻出来的版本目录名只是装载位置，不是身份。
#[test]
fn scan_candidates_keeps_top_pkg() {
    let tmp = std::env::temp_dir().join(format!("pf-scan-pkg-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    // market 安装链布局：plugins/<pkg>/<version>/pf-plugin.toml
    write_manifest(
        &tmp.join("@scope__quick/0.1.0"),
        &GOOD.replace("id = \"quick-panel\"", "id = \"quick\""),
    );
    // dev 手工平铺布局：plugins/<pkg>/pf-plugin.toml
    write_manifest(&tmp.join("flat"), &GOOD);

    let mut got: Vec<(String, String)> = scan_candidates(&tmp)
        .into_iter()
        .map(|(pkg, dir)| {
            (
                pkg,
                dir.strip_prefix(&tmp)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
            )
        })
        .collect();
    got.sort();
    assert_eq!(
        got,
        vec![
            (
                "@scope__quick".to_string(),
                "@scope__quick/0.1.0".to_string()
            ),
            ("flat".to_string(), "flat".to_string()),
        ]
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 把 GOOD 清单连同它引用的入口/资产一起落到目录（parse 校验文件存在）。
fn write_good_pkg(dir: &Path, id: &str) {
    write_manifest(
        dir,
        &GOOD.replace("id = \"quick-panel\"", &format!("id = \"{id}\"")),
    );
    std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
}

fn ledger_entry(name: &str, version: &str, disabled: bool) -> pi_link::pf_plugins::LedgerEntry {
    pi_link::pf_plugins::LedgerEntry {
        name: name.into(),
        version: version.into(),
        integrity: String::new(),
        installed_at: String::new(),
        source: "market".into(),
        disabled,
    }
}

/// 090-10 迁移体验：用户数据挂**顶层包目录**。版本布局 pkg_dir = 父目录，
/// 平铺布局 pkg_dir = 清单目录；两档 data_dir 都落 `<pkg>/data`——版本换代
/// 时 data 路径不变，这是「更新不丢数据」的全部依据。
#[test]
fn pkg_dir_follows_layout() {
    let tmp = std::env::temp_dir().join(format!("pf-pkgdir-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    // market 版本布局
    let ver = tmp.join("@scope__quick-panel").join("0.1.5");
    write_good_pkg(&ver, "quick-panel");
    let m = parse_manifest(&ver).expect("版本布局清单合法");
    assert_eq!(m.dir, ver, "wasm/assets 仍在版本目录");
    assert_eq!(m.pkg_dir, tmp.join("@scope__quick-panel"));
    assert_eq!(m.data_dir(), tmp.join("@scope__quick-panel").join("data"));
    // dev 平铺布局
    let flat = tmp.join("quick-panel");
    write_good_pkg(&flat, "quick-panel");
    let m = parse_manifest(&flat).expect("平铺清单合法");
    assert_eq!(m.pkg_dir, flat);
    assert_eq!(m.data_dir(), flat.join("data"));
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 090-10 迁移体验：台账启用、清单 schema 2 被本版拒 → 进 stale（kind /
/// version / title 都从 peek 来，不依赖 parse）；合法包进 manifests 不进
/// stale；台账没有的包两边都不进（野包不装载也不提示）。
#[test]
fn scan_root_collects_stale_for_rejected_ledger_pkgs() {
    let tmp = std::env::temp_dir().join(format!("pf-stale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    // ① 旧 schema 2 包（本机 0.1.4 现状）
    write_manifest(
        &tmp.join("@pi-flash-plugins__quick-panel").join("0.1.4"),
        r#"
schema = 2
id = "quick-panel"
title = { zh = "快捷面板", en = "Quick Panel" }
version = "0.1.4"
wasm = "plugin.wasm"
"#,
    );
    // ② 合法包
    write_good_pkg(&tmp.join("@s__other").join("1.0.0"), "other");
    // ③ 野包：磁盘有、台账无
    write_manifest(
        &tmp.join("@s__wild").join("0.0.1"),
        "schema = 2\nid = \"wild\"\n",
    );

    let ledger = vec![
        ledger_entry("@pi-flash-plugins/quick-panel", "0.1.4", false),
        ledger_entry("@s/other", "1.0.0", false),
    ];
    let out = scan_root(&tmp, &ledger);

    assert_eq!(out.manifests.len(), 1, "只有合法包装载");
    assert_eq!(out.manifests[0].id, "other");
    assert_eq!(
        out.stale,
        vec![StalePlugin {
            name: "@pi-flash-plugins/quick-panel".into(),
            version: "0.1.4".into(),
            title_zh: "快捷面板".into(),
            kind: StaleKind::SchemaUnsupported { got: 2 },
        }]
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 停用的包不装载也**不**进 stale——用户主动关的，不该被催更新。
#[test]
fn scan_root_skips_disabled_even_if_rejected() {
    let tmp = std::env::temp_dir().join(format!("pf-stale-dis-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    write_manifest(
        &tmp.join("@s__old").join("0.1.0"),
        "schema = 2\nid = \"old\"\ntitle = { zh = \"旧\" }\nversion = \"0.1.0\"\n",
    );
    let out = scan_root(&tmp, &[ledger_entry("@s/old", "0.1.0", true)]);
    assert!(out.manifests.is_empty());
    assert!(out.stale.is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 非 schema 类清单错误归 Invalid（原因原文），version 缺失回落版本目录名。
#[test]
fn scan_root_marks_other_parse_errors_invalid() {
    let tmp = std::env::temp_dir().join(format!("pf-stale-inv-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    // schema 合法但 id 与目录名不符 → parse 拒；没有 version 行
    write_manifest(
        &tmp.join("@s__mismatch").join("2.0.0"),
        "schema = 5\nid = \"someone-else\"\ntitle = { zh = \"错位\" }\n",
    );
    let out = scan_root(&tmp, &[ledger_entry("@s/mismatch", "2.0.0", false)]);
    assert_eq!(out.stale.len(), 1);
    let s = &out.stale[0];
    assert_eq!(s.version, "2.0.0", "version 缺失 → 版本目录名");
    assert_eq!(s.title_zh, "错位");
    assert!(
        matches!(&s.kind, StaleKind::Invalid(r) if r.contains("目录名")),
        "{:?}",
        s.kind
    );
    let _ = std::fs::remove_dir_all(&tmp);
}

/// mda8：版本层布局只认台账版本目录。更新后旧版本目录残留（被占用没删掉）
/// 且两版清单都合法时，不能双装载；残留目录也不算失效（不催更新）。
#[test]
fn scan_root_loads_only_ledger_version() {
    let tmp = std::env::temp_dir().join(format!("pf-onever-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let pkg = tmp.join("@s__quick-panel");
    write_good_pkg(&pkg.join("0.1.5"), "quick-panel");
    write_good_pkg(&pkg.join("0.1.6"), "quick-panel");
    // 字符串序陷阱：0.1.10 < 0.1.9，但台账说 0.1.6 → 只它
    write_good_pkg(&pkg.join("0.1.10"), "quick-panel");

    let out = scan_root(&tmp, &[ledger_entry("@s/quick-panel", "0.1.6", false)]);
    assert_eq!(out.manifests.len(), 1, "三个合法版本目录只装台账那一个");
    assert_eq!(out.manifests[0].dir, pkg.join("0.1.6"));
    assert!(out.stale.is_empty(), "残留目录不算失效");

    // 台账版本目录不存在（异常残局）→ 什么都不装、也不拿别的版本顶上
    let out = scan_root(&tmp, &[ledger_entry("@s/quick-panel", "9.9.9", false)]);
    assert!(out.manifests.is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 历史污染（2026-12 真机）：link:dev 平铺残留（prune 认清单不删）与市场
/// 版本层包同 id——平铺经目录别名第二形态冒名命中同一条账 → 曾双装载
/// （fn_bar 双微信）。只装版本层（精确匹配优先）；只有平铺时回落装载。
#[test]
fn scan_root_flat_dev_residue_yields_to_market_version() {
    let tmp = std::env::temp_dir().join(format!("pf-flatdev-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    // 市场版（版本层）+ dev 平铺残留（同 id、旧版本号）
    write_good_pkg(&tmp.join("@s__quick-panel/0.1.6"), "quick-panel");
    write_good_pkg(&tmp.join("quick-panel"), "quick-panel");

    let out = scan_root(&tmp, &[ledger_entry("@s/quick-panel", "0.1.6", false)]);
    assert_eq!(out.manifests.len(), 1, "同 id 只装一个");
    assert_eq!(
        out.manifests[0].dir,
        tmp.join("@s__quick-panel/0.1.6"),
        "版本精确匹配优先于平铺回落"
    );

    // 台账版本目录缺席、只剩平铺 → 平铺回落装载（dev 部署不回归）
    std::fs::remove_dir_all(&tmp.join("@s__quick-panel")).unwrap();
    let out = scan_root(&tmp, &[ledger_entry("@s/quick-panel", "9.9.9", false)]);
    assert_eq!(out.manifests.len(), 1);
    assert_eq!(out.manifests[0].dir, tmp.join("quick-panel"));
    let _ = std::fs::remove_dir_all(&tmp);
}

// ---------------------------------------------------------------------------
// 徽标/图标映射纯函数（entry.rs；crate::pf_plugin 顶层 re-export）
// ---------------------------------------------------------------------------

#[test]
fn plugin_badge_multi_owner_same_session_all_kept() {
    // 多插件同会话：全部保留、保序（从右到左 = 声明序），不裁不并
    let ses = "C:/x/a.jsonl";
    let badges = vec![
        PluginBadge {
            owner: "pf-demo".into(),
            session: ses.into(),
            icon: "assets:x.svg".into(),
            tone: "accent".into(),
        },
        PluginBadge {
            owner: "other".into(),
            session: ses.into(),
            icon: "assets:o.svg".into(),
            tone: "faint".into(),
        },
        PluginBadge {
            owner: "third".into(),
            session: "C:/x/b.jsonl".into(),
            icon: "assets:t.svg".into(),
            tone: "accent".into(),
        },
    ];
    let hit = badges_for(&badges, std::path::Path::new(ses));
    assert_eq!(hit.len(), 2);
    assert_eq!(
        hit[0],
        ("plugins/pf-demo/assets/x.svg".into(), "accent".into())
    );
    assert_eq!(
        hit[1],
        ("plugins/other/assets/o.svg".into(), "faint".into())
    );
    // 路径大小写变体也命中（same_path 归一化）
    assert_eq!(
        badges_for(&badges, std::path::Path::new("c:/X/A.jsonl")).len(),
        2
    );
    // 空列表 → 空
    assert!(badges_for(&[], std::path::Path::new(ses)).is_empty());
}

#[test]
fn row_badge_assembly_pin_first_cap_five() {
    // 终装配：PF 主体 pin 优先最右（首条），插件按注册序排其后
    let ses = "C:/x/a.jsonl";
    let mk = |owner: &str, icon: &str| PluginBadge {
        owner: owner.into(),
        session: ses.into(),
        icon: format!("assets:{icon}").into(),
        tone: "accent".into(),
    };
    let plugins = vec![mk("pf-wxbot", "wx.svg"), mk("other", "o.svg")];
    let hit = assemble_row_badges(true, badges_for(&plugins, std::path::Path::new(ses)));
    assert_eq!(
        hit,
        vec![
            ("icons/pin.svg".into(), "dim".into()),
            ("plugins/pf-wxbot/assets/wx.svg".into(), "accent".into()),
            ("plugins/other/assets/o.svg".into(), "accent".into()),
        ]
    );
    // 未置顶 → 无 pin
    assert_eq!(
        assemble_row_badges(false, badges_for(&plugins, std::path::Path::new(ses))).len(),
        2
    );
    // 上限 5 保右弃左：置顶 + 6 插件 → pin + 前 4 个插件（越靠后越靠左，弃）
    let many: Vec<PluginBadge> = (0..6)
        .map(|i| mk(&format!("p{i}"), &format!("i{i}.svg")))
        .collect();
    let hit = assemble_row_badges(true, badges_for(&many, std::path::Path::new(ses)));
    assert_eq!(hit.len(), crate::pf_plugin::ROW_BADGE_MAX);
    assert_eq!(hit[0], ("icons/pin.svg".into(), "dim".into()));
    assert_eq!(hit[1], ("plugins/p0/assets/i0.svg".into(), "accent".into()));
    assert!(hit.last().unwrap().0.contains("i3.svg"));
}

#[allow(dead_code)]
fn __anchor() {}

// ---------------------------------------------------------------------------
// 资产注册表（目录别名回退）
// ---------------------------------------------------------------------------

/// 市场卡片以**目录别名**（@scope__id）请求资产、登记键是 manifest.id：
/// fallback 按 `__<id>` 后缀对回（真机 2026-11-03 卡片图标空白根因）。
#[test]
fn plugin_asset_resolves_dir_alias_to_registered_manifest_id() {
    let tmp = std::env::temp_dir().join(format!("pf-asset-alias-{}", std::process::id()));
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(tmp.join("wx.svg"), "<svg/>").unwrap();
    register_plugin_dir("pf-wxbot", &tmp);
    // 先清缓存（同进程其他测试可能登记过同键）
    if let Ok(mut c) = PLUGIN_ASSETS
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    {
        c.clear();
    }
    let hit = plugin_asset("@pi-flash-plugins__pf-wxbot/wx.svg");
    assert!(hit.is_some(), "目录别名须对回 manifest.id 登记的目录");
    assert_eq!(hit.unwrap(), b"<svg/>");
    assert!(plugin_asset("@pi-flash-plugins__pf-wxbot/missing.svg").is_none());
    assert!(plugin_asset("@x__unrelated/wx.svg").is_none());
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 宿主协议文件（005 §③）：seq 单调递增 + enabled/reason 逐次覆写；
/// 原子写无 tmp 残留。
#[test]
fn pf_runtime_write_seq_and_flags() {
    let root = std::env::temp_dir().join(format!("pf-runtime-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let tmp = root.join("pf-wxbot"); // parse_manifest 校验 id == 目录名
    std::fs::create_dir_all(&tmp).unwrap();
    std::fs::write(
        tmp.join("pf-plugin.toml"),
        "schema = 5\nid = \"pf-wxbot\"\ntitle = { zh = \"微信通信\" }\nversion = \"0.2.0\"\n\n[[contributes.fn_bar]]\ncommand = \"bind\"\n\n[[contributes.pi_extensions]]\npath = \"pi/index.mjs\"\n",
    )
    .unwrap();
    let m = parse_manifest(&tmp).expect("清单合法");
    crate::pf_plugin::pf_runtime_write(&m, true, "enable");
    crate::pf_plugin::pf_runtime_write(&m, false, "disable");
    crate::pf_plugin::pf_runtime_write(&m, false, "quit");
    let v: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(tmp.join("pf-runtime.json")).unwrap())
            .unwrap();
    assert_eq!(v["pf_enabled"], serde_json::Value::Bool(false));
    assert_eq!(v["pf_seq"], serde_json::json!(3), "跨调用单调递增");
    assert_eq!(v["pf_reason"], serde_json::json!("quit"));
    assert!(
        !tmp.join("pf-runtime.json.tmp").exists(),
        "原子写无残留 tmp"
    );
    let _ = std::fs::remove_dir_all(&root);
}
