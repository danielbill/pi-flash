use super::*;

/// 测试宿主：私有目录内存态 + 动作入队（同 app 侧 ChatHost 形状）。
struct TestHost {
    files: std::collections::HashMap<String, String>,
    actions: Vec<JsAction>,
}
impl JsHost for TestHost {
    fn is_executive(&mut self, path: &str) -> bool {
        path.ends_with(".ps1") || path.ends_with(".bat")
    }
    fn file_icon(&mut self, path: &str) -> String {
        if path.ends_with(".md") {
            "file-text".into()
        } else {
            "terminal".into()
        }
    }
    fn dir(&mut self) -> String {
        "/tmp/pf".into()
    }
    fn fs_read(&mut self, name: &str) -> String {
        self.files.get(name).cloned().unwrap_or_default()
    }
    fn fs_write(&mut self, name: &str, data: &str) {
        self.files.insert(name.to_string(), data.to_string());
    }
    fn fs_list(&mut self) -> Vec<String> {
        self.files.keys().cloned().collect()
    }
    fn queue(&mut self, action: JsAction) {
        self.actions.push(action);
    }
}

/// 与插件仓 packages/quick-panel/main.js 同契约的最小插件源码（v2：内联树）。
const PLUGIN: &str = r#"
const KEY = "pins.json";
const load = (ctx) => { try { const a = JSON.parse(ctx.fs.read(KEY) || "[]"); return Array.isArray(a) ? a : []; } catch { return []; } };
const save = (ctx, pins) => ctx.fs.write(KEY, JSON.stringify(pins));
export function activate(ctx) {
  ctx.onMenu("set-quick", ({ path }) => {
const pins = load(ctx);
if (pins.includes(path)) { ctx.toast("dup"); return; }
save(ctx, [...pins, path]);
ctx.toast("pinned");
  });
  ctx.onCommand("unpin", ({ path }) => {
save(ctx, load(ctx).filter((p) => p !== path));
ctx.toast("unpinned");
  });
}
export function render(ctx) {
  const pins = load(ctx);
  if (pins.length === 0)
return { type: "text", value: { zh: "空" }, tone: "text.faint" };
  return { type: "column", pad: "SP4", gap: "SP2", children: pins.map((p) => ({
type: "button", icon: ctx.fileIcon(p), label: p.split(/[\\/]/).pop(), tip: p,
action: ctx.isExecutive(p) ? "run" : "edit", arg: p,
menu: [ { id: "unpin", title: { zh: "移除快捷" } } ],
  })) };
}
"#;

/// 契约 v3 面：fn_bar chrome / fire_command_env / act.openPopup /
/// render_popup（无导出退回 render）。
#[test]
fn v3_fn_bar_command_env_popup() {
    let dir = std::env::temp_dir().join("pf-js-v3-test");
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.js");
    std::fs::write(
        &entry,
        r#"
export function activate(ctx) {
  ctx.onCommand("bind", (env) => {
ctx.fs.write("env.json", JSON.stringify(env));
  });
  ctx.onCommand("toggle-enabled", (env) => {
ctx.fs.write("switch.json", JSON.stringify(env));
  });
  ctx.act.openPopup("qr");
}
export function fn_bar(ctx, env) {
  return {
icon: "assets:demo.svg",
tone: env.session && env.session.key === "s1" ? "accent" : "text.faint",
tip: { zh: "示例插件" },
  };
}
export function render_popup(ctx) {
  return { type: "column", children: [
{ type: "switch", value: true, command: "toggle-enabled" },
  ] };
}
export function render(ctx) {
  return { type: "text", value: { zh: "旧面" } };
}
"#,
    )
    .unwrap();
    let host = Rc::new(RefCell::new(TestHost {
        files: Default::default(),
        actions: Vec::new(),
    }));
    let plugin = JsPlugin::load(&entry, "/tmp/pf", host.clone()).expect("load");
    plugin.activate().expect("activate");
    // act.openPopup → 动作入队
    assert_eq!(
        host.borrow().actions.clone(),
        vec![JsAction::OpenPopup("qr".into())]
    );
    // fn_bar chrome：env 带会话键 → 绿；换键 → 灰
    let green = plugin
        .fn_bar_json(r#"{"session":{"key":"s1"}}"#)
        .unwrap()
        .unwrap();
    assert!(
        green.contains("assets:demo.svg") && green.contains("accent"),
        "{green}"
    );
    let grey = plugin
        .fn_bar_json(r#"{"session":{"key":"s2"}}"#)
        .unwrap()
        .unwrap();
    assert!(grey.contains("text.faint"), "{grey}");
    // 命令 env 负载形态：switch 翻转 {value:bool} / bind {session:{key}}
    plugin
        .fire_command_env("toggle-enabled", r#"{"value":false}"#)
        .unwrap();
    assert_eq!(
        host.borrow_mut()
            .files
            .get("switch.json")
            .map(String::as_str),
        Some(r#"{"value":false}"#)
    );
    plugin
        .fire_command_env("bind", r#"{"session":{"key":"sA"}}"#)
        .unwrap();
    assert_eq!(
        host.borrow_mut().files.get("env.json").map(String::as_str),
        Some(r#"{"session":{"key":"sA"}}"#)
    );
    // 弹窗内容：render_popup 优先（不含旧面文案）
    let popup = plugin.render_popup_json().unwrap();
    assert!(
        popup.contains("switch") && !popup.contains("旧面"),
        "{popup}"
    );
    // 未导出 fn_bar 的插件 → None（负缓存语义由 app 侧管）
    let dir2 = std::env::temp_dir().join("pf-js-v3-nofnbar");
    std::fs::create_dir_all(&dir2).unwrap();
    let entry2 = dir2.join("main.js");
    std::fs::write(
        &entry2,
        "export function render(ctx) { return { type: \"text\", value: { zh: \"x\" } }; }",
    )
    .unwrap();
    let host2 = Rc::new(RefCell::new(TestHost {
        files: Default::default(),
        actions: Vec::new(),
    }));
    let p2 = JsPlugin::load(&entry2, "/tmp/pf", host2).expect("load");
    assert!(p2.fn_bar_json("{}").unwrap().is_none());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&dir2);
}

#[test]
fn host_proof_placeholder_and_nonce_tag() {
    // 未覆写 host_proof 的宿主（TestHost 同 fork 自建款）：占位符 fail-closed
    let dir = std::env::temp_dir().join("pf-js-proof-test");
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.js");
    std::fs::write(
        &entry,
        r#"export function render(ctx) {
  const p = ctx.hostProof("n1");
  return { type: "text", value: p.kid + "|" + p.proof + "|" + p.nonceTag };
}"#,
    )
    .unwrap();
    let host = Rc::new(RefCell::new(TestHost {
        files: Default::default(),
        actions: Vec::new(),
    }));
    let plugin = JsPlugin::load(&entry, "/tmp/pf", host).expect("load");
    let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
    let parts: Vec<&str> = v["value"].as_str().unwrap().split('|').collect();
    assert_eq!(parts[0], ""); // 未覆写 → kid 空
    assert_eq!(parts[1], "PROOF_PLACEHOLDER"); // fail-closed：验签必失败
    assert_eq!(parts[2], nonce_tag("PROOF_PLACEHOLDER", "n1")); // 派生一致
    assert_ne!(nonce_tag("P", "n1"), nonce_tag("P", "n2")); // 不同 nonce 不同 tag
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn broadcast_reaches_registered_handler() {
    // 契约 v2.4 broadcast_events：onBroadcast 注册 → fire_broadcast 负载直达
    // → 插件自存状态 → render 可读（差分/派发时序在宿主 broadcast.rs 测）
    let dir = std::env::temp_dir().join("pf-js-broadcast-test");
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.js");
    std::fs::write(
        &entry,
        r#"
let last = null;
export function activate(ctx) {
  ctx.onBroadcast("inputpanel", (payload) => { last = payload; });
}
export function render(ctx) {
  if (!last) return { type: "text", value: "no-broadcast" };
  return { type: "text",
       value: (last.project || "-") + "|" + (last.model ? last.model.provider + "/" + last.model.id : "-") };
}
"#,
    )
    .unwrap();
    let host = Rc::new(RefCell::new(TestHost {
        files: Default::default(),
        actions: Vec::new(),
    }));
    let plugin = JsPlugin::load(&entry, "/tmp/pf", host).expect("load");
    plugin.activate().expect("activate");

    // 未广播 → 无负载
    let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
    assert_eq!(v["value"], "no-broadcast");

    // 广播带项目无模型 → 负载直达（可空字段走 null）
    plugin
        .fire_broadcast("inputpanel", r#"{"project":"D:/work","model":null}"#)
        .expect("fire");
    let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
    assert_eq!(v["value"], "D:/work|-");

    // 广播项目+模型齐 → 负载更新
    plugin
        .fire_broadcast(
            "inputpanel",
            r#"{"project":"D:/work","model":{"provider":"prov","id":"m1"}}"#,
        )
        .expect("fire");
    let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
    assert_eq!(v["value"], "D:/work|prov/m1");

    // 未注册的事件名 → no-op 不报错
    plugin.fire_broadcast("other", r#"{}"#).expect("noop");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sessions_newdraft_contract_namespace() {
    // 契约 v2.4 会话槽：ctx.sessions.newDraft（嵌套面）。扁平 ctx.newDraft
    // 仅旧兼容——插件按契约写法必须可达。
    let dir = std::env::temp_dir().join("pf-js-sessions-test");
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.js");
    std::fs::write(
        &entry,
        r#"
let got = null;
export function activate(ctx) {
  ctx.sessions.newDraft("pf-demo-", "D:/proj/demo");
  got = typeof ctx.sessions.newDraft;
}
export function render(ctx) { return { type: "text", value: got }; }
"#,
    )
    .unwrap();
    let host = Rc::new(RefCell::new(TestHost {
        files: Default::default(),
        actions: Vec::new(),
    }));
    let plugin = JsPlugin::load(&entry, "/tmp/pf", host.clone()).expect("load");
    plugin.activate().expect("activate");
    let drafts: Vec<_> = host
        .borrow()
        .actions
        .iter()
        .filter_map(|a| match a {
            JsAction::NewDraft { prefix, cwd } => Some((prefix.clone(), cwd.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        drafts,
        vec![("pf-demo-".into(), "D:/proj/demo".into())],
        "ctx.sessions.newDraft 必须到达动作队列"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn activate_menu_render_and_command() {
    let dir = std::env::temp_dir().join("pf-js-v2-test");
    std::fs::create_dir_all(&dir).unwrap();
    let entry = dir.join("main.js");
    std::fs::write(&entry, PLUGIN).unwrap();
    let host = Rc::new(RefCell::new(TestHost {
        files: Default::default(),
        actions: Vec::new(),
    }));
    let plugin = JsPlugin::load(&entry, "/tmp/pf", host.clone()).expect("load");
    plugin.activate().expect("activate");

    // 空面板
    let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
    assert_eq!(v["type"], "text");

    // 右键设为快捷 ×3（含重复）
    plugin
        .fire_menu("set-quick", r"D:\p\build.ps1", "terminal")
        .expect("menu1");
    plugin
        .fire_menu("set-quick", r"D:\p\README.md", "file-text")
        .expect("menu2");
    plugin
        .fire_menu("set-quick", r"D:\p\build.ps1", "terminal")
        .expect("menu-dup");
    let toasts: Vec<_> = host
        .borrow()
        .actions
        .iter()
        .map(|a| match a {
            JsAction::Toast(m) => m.clone(),
            _ => String::new(),
        })
        .collect();
    assert_eq!(toasts, ["pinned", "pinned", "dup"]);
    // 只存路径，派生值不落盘
    let stored = host.borrow().files.get("pins.json").cloned().unwrap();
    assert_eq!(stored, r#"["D:\\p\\build.ps1","D:\\p\\README.md"]"#);

    // 渲染：值内联 + 动作显式
    plugin.fire_panel_open().expect("panel open");
    let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
    assert_eq!(v["type"], "column");
    assert_eq!(v["children"].as_array().unwrap().len(), 2);
    assert_eq!(v["children"][0]["action"], "run");
    assert_eq!(v["children"][0]["label"], "build.ps1");
    assert_eq!(v["children"][1]["action"], "edit");
    assert!(v.get("rows").is_none());

    // onCommand：插件改自己的数据
    plugin
        .fire_command("unpin", r"D:\p\build.ps1")
        .expect("cmd");
    let v: serde_json::Value = serde_json::from_str(&plugin.render_json().unwrap()).unwrap();
    assert_eq!(v["children"].as_array().unwrap().len(), 1);
    assert_eq!(v["children"][0]["label"], "README.md");

    // 异常插件：render 抛错 → Err（app 侧 toast + 会话级禁用）
    let bad = std::env::temp_dir().join("pf-js-v2-bad.js");
    std::fs::write(&bad, "export function render(ctx) { no_such_fn(); }").unwrap();
    let plugin = JsPlugin::load(&bad, "/tmp/pf", host.clone()).expect("load bad");
    assert!(plugin.render_json().is_err());
    let _ = std::fs::remove_file(&bad);
    let _ = std::fs::remove_dir_all(&dir);
    }
