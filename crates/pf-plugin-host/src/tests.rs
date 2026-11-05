use super::*;


fn proof_official() -> String {
    std::env::var("PF_TEST_PROOF_OFFICIAL").unwrap_or_default()
}
fn proof_placeholder() -> String {
    r#"{"kid":"","proof":"PROOF_PLACEHOLDER"}"#.into()
}
fn proof_forged() -> String {
    std::env::var("PF_TEST_PROOF_FORGED").unwrap_or_default()
}

/// 内层插件证明门（cq9）：官方 proof → init Ok；占位符/伪造 → Err 带可读原因。
/// 由 inner/scripts/e2e.mjs 注入 PF_TEST_INNER_WASM 等环境后运行；缺环境直接跳过。
#[test]
fn inner_plugin_proof_gate() {
    let Ok(wasm) = std::env::var("PF_TEST_INNER_WASM") else {
        return;
    };
    let Ok(_official) = std::env::var("PF_TEST_PROOF_OFFICIAL") else {
        return;
    };
    let forged = std::env::var("PF_TEST_PROOF_FORGED").unwrap_or_default();
    install_proof_hook(proof_official);
    let path = std::path::PathBuf::from(wasm);
    let name = "demo-inner";

    // ① 官方 proof → 正常激活

    let mut inst = PluginInstance::load(&path).expect("load");
    inst.load_ctx(&test_ctx(name))
        .expect("官方 proof 必须激活成功");
    // 激活后渲染应有产出（多点校验过的路径；090-5 起 Paint 直绘面退役，回归组件树契约）
    assert!(
        !inst.render_json().expect("render").is_empty(),
        "激活后应有渲染输出"
    );

    // ② 占位符（fork 自建/无 key 构建）→ 拒载 + 原因
    install_proof_hook(proof_placeholder);
    let mut inst = PluginInstance::load(&path).expect("load");
    let err = inst.load_ctx(&test_ctx(name)).expect_err("占位符必须拒载");
    assert!(err.to_string().contains("官方构建"), "原因要可读: {err}");

    // ③ 伪造（换钥签名）→ 拒载 + 原因
    if !forged.is_empty() {
        install_proof_hook(proof_forged);
        let mut inst = PluginInstance::load(&path).expect("load");
        let err = inst.load_ctx(&test_ctx(name)).expect_err("伪造必须拒载");
        assert!(err.to_string().contains("验签失败"), "原因要可读: {err}");
    }
}

/// 契约 v3 能力面（8tb.2）：v3 包探测命中三钩子且类型吻合（类型不符 =
/// 探测 None，所以 has=true + 调用 Ok 即签名验证过）；v2 旧包探不到 =
/// 功能关、装载不受影响（向后兼容的核心承诺）。缺环境跳过；由 W4
/// 部署链注入 PF_TEST_V3_WASM / PF_TEST_V2_WASM 后运行。
#[test]
fn wasm_v3_capability_face() {
    let Ok(v3) = std::env::var("PF_TEST_V3_WASM") else {
        return;
    };
    install_proof_hook(proof_placeholder); // 不 init，纯装载 + 探测
    // 拷进独立临时目录：测试各自的 data/ 互不串扰（插件 storage = 包目录/data）
    let dir = std::env::temp_dir().join(format!("pf-v3-cap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let wasm = dir.join("plugin.wasm");
    std::fs::copy(&v3, &wasm).unwrap();
    let mut p = PluginInstance::load(&wasm).expect("load v3");
    assert!(p.has_fn_bar(), "v3 包应探到 fn-bar");
    assert!(p.has_session_row(), "v3 包应探到 session-row");
    assert!(p.has_render_popup(), "v3 包应探到 render-popup");
    // 签名吻合：调用不 trap、lift/lower 走通（桩返回 "null" → None）
    assert_eq!(p.fn_bar_json("{}").unwrap(), None);
    assert_eq!(p.session_row_json("{}").unwrap(), None);
    let _ = p.render_popup_json().expect("render-popup 调用应成功");

    // 官方 proof（可选注入）→ init 验签激活后，钩子应回真值：插件 id 由
    // PF_TEST_PLUGIN_ID 注入（PF 不内置任何插件身份）；只验**宿主契约**
    // 形状（chrome 三键、未绑无徽标、弹窗是组件树），不验插件自己的文案/图标
    if let (Ok(_), Ok(plugin_id)) = (
        std::env::var("PF_TEST_PROOF_OFFICIAL"),
        std::env::var("PF_TEST_PLUGIN_ID"),
    ) {
        install_proof_hook(proof_official);
        let mut p = PluginInstance::load(&wasm).expect("load v3");
        p.load_ctx(&test_ctx(&plugin_id))
            .expect("官方 proof 必须激活");
        let chrome = p.fn_bar_json("{}").unwrap().expect("chrome 应有值");
        let v: serde_json::Value = serde_json::from_str(&chrome).unwrap();
        assert_eq!(v["tone"], "text.faint", "未绑 = 灰");
        assert!(
            v["tip"]["zh"].as_str().is_some_and(|s| !s.is_empty()),
            "未绑态应有引导 tip"
        );
        assert!(
            v["icon"].as_str().is_some_and(|s| !s.is_empty()),
            "chrome 应带图标"
        );
        assert_eq!(p.session_row_json("{}").unwrap(), None, "未绑 = 无徽标");
        let popup = p.render_popup_json().unwrap();
        let tree: serde_json::Value = serde_json::from_str(&popup).unwrap();
        assert_eq!(tree["type"], "column", "弹窗体应是组件树");
    }

    // v2 旧包：探不到新钩子，装载与 v2 render 面不受影响
    if let Ok(v2) = std::env::var("PF_TEST_V2_WASM") {
        let mut p = PluginInstance::load(std::path::Path::new(&v2)).expect("load v2");
        assert!(!p.has_fn_bar() && !p.has_session_row() && !p.has_render_popup());
        p.render_json().expect("v2 render 面应可用");
    }
}
