//! 清单解析（02 §2）——单测（自 manifest.rs 外移；fixture 落 temp 目录按
//! 进程号隔离）。启动扫描 / 资产注册表 / 徽标 / 宿主协议文件的单测在
//! [`super::tests_scan`]。

use std::path::{Path, PathBuf};

use super::PluginDock;
use super::parse::parse_manifest;
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

#[test]
fn manifest_parse_and_validate() {
    let tmp = std::env::temp_dir().join(format!("pf-manifest-test-{}", std::process::id()));
    let dir = tmp.join("quick-panel");
    write_manifest(&dir, GOOD);
    std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();

    let m = parse_manifest(&dir).expect("合法清单应通过");
    assert_eq!(m.id, "quick-panel");
    assert_eq!(m.title_zh, "快捷面板");
    assert_eq!(m.title_en.as_deref(), Some("Quick Panel"));
    assert_eq!(m.status_bar.as_ref().unwrap().icon, "assets/bolt.svg");
    assert_eq!(m.file_menu.len(), 1);
    assert_eq!(m.file_menu[0].id, "set-quick");
    assert_eq!(m.panels[0].entry, "main.js");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 契约 v2.5 topbar_menu 解析（eb6）：条目入清单；重复声明拒绝
#[test]
fn manifest_topbar_menu_parse() {
    let tmp = std::env::temp_dir().join(format!("pf-topbar-test-{}", std::process::id()));
    let body = format!(
        "{GOOD}\n[[contributes.topbar_menu]]\ntitle = {{ zh = \"面板标题\" }}\nicon = \"assets/top.svg\"\n"
    );
    let dir = tmp.join("quick-panel");
    write_manifest(&dir, &body);
    std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/top.svg"), "<svg/>").unwrap();
    std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
    let m = parse_manifest(&dir).expect("topbar_menu 清单应通过");
    let tb = m.topbar_menu.as_ref().expect("条目应入清单");
    assert_eq!(tb.title_zh, "面板标题");
    assert_eq!(tb.icon, "assets/top.svg");
    // 图标三来源映射（与 status_bar 共用 contrib_icon_path）
    assert_eq!(
        topbar_menu_icon_path(&plugin_entries(vec![m]).remove(0)).map(|s| s.to_string()),
        Some("plugins/quick-panel/assets/top.svg".to_string())
    );
    // 重复段 = 拒绝
    let dir2 = tmp.join("dup");
    write_manifest(
        &dir2,
        &format!(
            "{body}\n[[contributes.topbar_menu]]\ntitle = {{ zh = \"第二条\" }}\nicon = \"bolt\"\n"
        ),
    );
    std::fs::write(dir2.join("main.js"), "export function render(){{}}").unwrap();
    assert!(parse_manifest(&dir2).is_err(), "topbar_menu 只能声明一个");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 契约 v2.4 sessions 解析：认领前缀声明段必须入清单（漏段曾把整份
/// 清单拒在扫描关——插件包「未知段 [[contributes.sessions]]」事故）
#[test]
fn manifest_sessions_parse() {
    let tmp = std::env::temp_dir().join(format!("pf-sessions-test-{}", std::process::id()));
    let body = format!("{GOOD}\n[[contributes.sessions]]\nprefix = \"pf-demo-\"\n");
    let dir = tmp.join("quick-panel");
    write_manifest(&dir, &body);
    std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
    let m = parse_manifest(&dir).expect("含 sessions 段的清单必须通过");
    assert_eq!(m.sessions.len(), 1);
    assert_eq!(m.sessions[0].prefix, "pf-demo-");
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 契约 v2.4 dock 解析：tab/panel/缺省/非法值四路
/// 认领会话名：前缀 + 15 位时间戳（§5 口径，各插件认领线同构）
#[test]
fn claim_session_name_format() {
    let n = claim_session_name("mybot-");
    assert!(n.starts_with("mybot-"), "{n}");
    let ts = n.trim_start_matches("mybot-");
    assert_eq!(ts.len(), 15, "YYMMDDHHMMSS + 3位毫秒 = 15 位：{n}");
    assert!(ts.chars().all(|c| c.is_ascii_digit()), "{n}");
    // 唯一性由「前缀 + 毫秒」联合保证；同毫秒重名由认领槽替换语义吸收
    //（新建即换绑，旧名随换绑退役），不做 flaky 的跨调用断言
}

#[test]
fn manifest_panel_dock_parse() {
    let tmp = std::env::temp_dir().join(format!("pf-dock-test-{}", std::process::id()));
    // tab 形态
    let dir = tmp.join("quick-panel");
    write_manifest(
        &dir,
        &GOOD.replace("entry = \"main.js\"", "entry = \"main.js\"\ndock = \"tab\""),
    );
    std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
    let mut m = parse_manifest(&dir).expect("tab 清单应通过");
    assert_eq!(m.panels[0].dock, PluginDock::Tab);
    m.pkg = "quick-panel".into(); // pkg 是扫描期回填字段，单测手工补
    assert_eq!(m.tab_page(), Some(("quick-panel", "quick")));

    // 缺省 = panel（老插件零迁移）
    let dir = tmp.join("quick-panel");
    write_manifest(&dir, GOOD);
    std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
    let m = parse_manifest(&dir).expect("缺省清单应通过");
    assert_eq!(m.panels[0].dock, PluginDock::Panel);
    assert_eq!(m.tab_page(), None);

    // 非法值拒装（封闭词表）
    let dir = tmp.join("quick-panel");
    write_manifest(
        &dir,
        &GOOD.replace(
            "entry = \"main.js\"",
            "entry = \"main.js\"\ndock = \"sidebar\"",
        ),
    );
    let e = parse_manifest(&dir).unwrap_err();
    assert!(e.contains("dock"), "错误应提及 dock：{e}");

    let _ = std::fs::remove_dir_all(&tmp);
}

/// 契约 v4 service 解析：入清单 + scope 封闭；pi_extensions 已移除（声明即错）
#[test]
fn manifest_service_parse() {
    let tmp = std::env::temp_dir().join(format!("pf-svc-test-{}", std::process::id()));
    let body = GOOD.replace(
        "entry = \"main.js\"",
        "entry = \"main.js\"

[[contributes.service]]
id = \"demo-svc\"
scope = \"session\"
entry = \"pi/index.mjs\"",
    );
    let dir = tmp.join("quick-panel");
    write_manifest(&dir, &body);
    std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
    let m = parse_manifest(&dir).expect("service 清单应通过");
    assert_eq!(m.service.len(), 1);
    assert_eq!(m.service[0].id, "demo-svc");
    assert_eq!(m.service[0].scope, "session");
    assert_eq!(m.service[0].entry, "pi/index.mjs");
    // scope 封闭（本版仅 session）
    let bad = GOOD.replace(
        "entry = \"main.js\"",
        "entry = \"main.js\"

[[contributes.service]]
id = \"demo-svc\"
scope = \"global\"
entry = \"pi/index.mjs\"",
    );
    write_manifest(&dir, &bad);
    let e = parse_manifest(&dir).expect_err("scope=global 应拒");
    assert!(e.contains("session"), "错误应提及 session：{e}");
    // pi_extensions 段已恢复（90-6 revert：schema 3/4 双收）→ 合法且入清单
    let legacy = GOOD.replace(
        "entry = \"main.js\"",
        "entry = \"main.js\"

[[contributes.pi_extensions]]
path = \"pi/index.mjs\"",
    );
    write_manifest(&dir, &legacy);
    let m = parse_manifest(&dir).expect("pi_extensions 段已恢复，应通过");
    assert_eq!(m.pi_extensions, vec!["pi/index.mjs".to_string()]);
    let _ = std::fs::remove_dir_all(&tmp);
}

/// 契约 v3 fn_bar / popups 解析：入清单 + command 必填 + size 封闭词表
#[test]
fn manifest_fn_bar_and_popups_parse() {
    let tmp = std::env::temp_dir().join(format!("pf-v3-test-{}", std::process::id()));
    let body = format!(
        "{GOOD}
[[contributes.fn_bar]]
command = \"bind\"

[[contributes.popups]]
id = \"qr\"
title = {{ zh = \"绑定面板\" }}
size = \"MD\"
entry = \"main.js\"
"
    );
    let dir = tmp.join("quick-panel");
    write_manifest(&dir, &body);
    std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("assets/bolt.svg"), "<svg/>").unwrap();
    let m = parse_manifest(&dir).expect("v3 清单应通过");
    assert_eq!(m.fn_bar.as_ref().unwrap().command, "bind");
    assert_eq!(m.popups.len(), 1);
    assert_eq!(m.popups[0].id, "qr");
    assert_eq!(m.popups[0].size, "MD");
    // fn_bar 缺 command = 拒
    let dir = tmp.join("bad-cmd");
    write_manifest(
        &dir,
        &body.replace(
            "command = \"bind\"
",
            "",
        ),
    );
    assert!(parse_manifest(&dir).is_err());
    // popup size 封闭词表
    let dir = tmp.join("bad-size");
    write_manifest(&dir, &body.replace("size = \"MD\"", "size = \"XL\""));
    assert!(parse_manifest(&dir).is_err());
    // fn_bar / popups 也是进驻面：仅这两段的包不是纯库包
    let dir = tmp.join("pf-demo");
    write_manifest(
        &dir,
        "schema = 4
id = \"pf-demo\"
title = { zh = \"x\" }
version = \"0.1.0\"

[[contributes.fn_bar]]
command = \"bind\"

[[contributes.popups]]
id = \"qr\"
title = { zh = \"弹窗\" }
entry = \"main.js\"
",
    );
    std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
    let m = parse_manifest(&dir).expect("仅 fn_bar+popups 的包应通过");
    assert!(m.fn_bar.is_some() && m.popups.len() == 1);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn manifest_rejects() {
    let tmp = std::env::temp_dir().join(format!("pf-manifest-bad-{}", std::process::id()));
    // id ≠ 目录名
    let dir = tmp.join("other-name");
    write_manifest(
        &dir,
        &GOOD.replace("id = \"quick-panel\"", "id = \"quick-panel-x\""),
    );
    assert!(parse_manifest(&dir).is_err());
    // schema ≠ 支持的契约大版本（v3 起拒旧版；不降级运行）
    let dir = tmp.join("quick-panel");
    write_manifest(&dir, &GOOD.replace("schema = 4", "schema = 2"));
    assert!(parse_manifest(&dir).is_err());
    // 纯库包（无 contributes）
    let dir = tmp.join("quick-panel");
    write_manifest(
        &dir,
        "schema = 4\nid = \"quick-panel\"\ntitle = { zh = \"x\" }\nversion = \"0.1.0\"\n",
    );
    let e = parse_manifest(&dir).unwrap_err();
    assert!(e.contains("纯库"));
    // entry 不存在
    let dir = tmp.join("quick-panel");
    write_manifest(&dir, GOOD);
    assert!(parse_manifest(&dir).is_err());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn manifest_tolerates_inline_comments() {
    // 02 §2 示例清单自带行内注释；引号内的 # 是文案不是注释
    let tmp = std::env::temp_dir().join(format!("pf-manifest-cmt-{}", std::process::id()));
    let dir = tmp.join("quick-panel");
    write_manifest(
        &dir,
        "schema = 4  # 顶部注释
id = \"quick-panel\"   
title = { zh = \"快捷#面板\" } # 行内
version = \"0.1.0\"

[[contributes.panels]] # 段后注释
id = \"quick\"
title = { zh = \"快捷\" }
entry = \"main.js\" # 入口
",
    );
    std::fs::write(dir.join("main.js"), "export function render(){{}}").unwrap();
    let m = parse_manifest(&dir).expect("带行内注释的清单应通过");
    assert_eq!(m.title_zh, "快捷#面板"); // 引号内 # 保留
    assert_eq!(m.panels[0].entry, "main.js");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn manifest_inner_package_parse() {
    // 加密包（inner_package）：无 entry 面板声明跳过文件校验；tab icon
    // 走宿主内置名（无 assets 文件也可过）——发版脚本 dist.mjs 的产物形态
    let tmp = std::env::temp_dir().join(format!("pf-inner-test-{}", std::process::id()));
    let dir = tmp.join("quick-panel");
    write_manifest(
        &dir,
        r#"
schema = 4
id = "quick-panel"
title = { zh = "快捷面板", en = "Quick Panel" }
version = "0.1.3"

wasm = "plugin.wasm"

[[contributes.status_bar]]
icon = "plug"
title = { zh = "快捷面板" }

[[contributes.panels]]
id = "quick-panel"
title = { zh = "快捷面板" }
"#,
    );
    std::fs::write(dir.join("plugin.wasm"), b"\0wasm-fake").unwrap();

    let m = parse_manifest(&dir).expect("加密包清单应通过（无 entry 文件）");
    assert_eq!(m.wasm.as_deref(), Some("plugin.wasm"));
    assert_eq!(m.status_bar.as_ref().unwrap().icon, "plug");
    assert_eq!(m.panels.len(), 1);
    let _ = std::fs::remove_dir_all(&tmp);
}
