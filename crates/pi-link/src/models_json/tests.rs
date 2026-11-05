use super::*;
use serde_json::json;

#[test]
fn normalize_strips_pf_private_source_key() {
    let mut doc = json!({"providers": {"p": {"models": [
        {"id": "m", "cost": {"input": 1, "output": 2, "cacheRead": 0, "cacheWrite": 0},
         "priceSource": "models-dev"}]}}});
    assert!(repair(&mut doc));
    assert!(
        doc["providers"]["p"]["models"][0]
            .get("priceSource")
            .is_none(),
        "PF 私有元数据不进 pi 账本"
    );
    assert_eq!(
        doc["providers"]["p"]["models"][0]["cost"]["input"],
        json!(1)
    );
}

#[test]
fn ensure_model_defaults_fills_missing_keeps_existing() {
    let mut m = json!({"id": "x"});
    ensure_model_defaults(&mut m);
    assert_eq!(m["input"], json!(["text"]));
    assert_eq!(
        m["reasoning"],
        json!(true),
        "缺省可推理：否则档位菜单锁死 off"
    );
    assert_eq!(m["cost"]["input"], json!(0));
    // 已有值保留
    let mut m = json!({"id": "x", "input": ["text", "image"], "reasoning": true,
        "cost": {"input": 1.5, "output": 6.0, "cacheRead": 0.1, "cacheWrite": 0.2}});
    ensure_model_defaults(&mut m);
    assert_eq!(m["input"], json!(["text", "image"]));
    assert_eq!(m["reasoning"], json!(true));
    assert_eq!(m["cost"]["input"], json!(1.5));
    // 非对象条目无害
    let mut junk = json!("str");
    ensure_model_defaults(&mut junk);
    assert_eq!(junk, json!("str"));
}

/// 2026-11-02 真机：自动填参只拿到 input/output → pi ModelCostSchema 四键
/// 必填 → 整份 models.json 拒载 → my-glm 消失、--model 退出、对话空转。
#[test]
fn partial_cost_is_completed_and_rejected_until_repaired() {
    let mut m = json!({"id": "x", "cost": {"input": 0.0285, "output": 0.0923}});
    ensure_model_defaults(&mut m);
    assert_eq!(m["cost"]["input"], json!(0.0285), "已有值不动");
    assert_eq!(m["cost"]["cacheRead"], json!(0));
    assert_eq!(m["cost"]["cacheWrite"], json!(0));
    // 非对象 cost 重置为完整四键
    let mut m = json!({"id": "x", "cost": 3});
    ensure_model_defaults(&mut m);
    assert_eq!(m["cost"]["output"], json!(0));
    // validate 是最后一道闸：残缺 cost 拒绝；repair 后通过且报告有改动
    let mut doc = json!({"providers": {"my-glm": {"baseUrl": "https://x", "api": "a",
        "models": [{"id": "glm-5.3-flash", "cost": {"input": 1, "output": 2}}]}}});
    assert!(validate(&doc).unwrap_err().contains("cacheRead"));
    assert!(repair(&mut doc));
    assert!(validate(&doc).is_ok());
    assert_eq!(
        doc["providers"]["my-glm"]["models"][0]["cost"]["cacheWrite"],
        json!(0)
    );
    // 幂等：再修一次无改动
    assert!(!repair(&mut doc));
}

/// 0.3.2 回归锁：仓命中分支（探测拉到的模型 = 仓里必有定义）勾选后
/// doc 里**必须**出现该模型——此前该分支 push 完直接 return 没落盘。
#[test]
fn materialize_from_store_def_lands_in_doc() {
    let mut doc = json!({ "providers": { "myds": {
        "api": "openai-completions", "name": "myds",
        "baseUrl": "https://api.deepseek.com" } } });
    let def = json!({"id": "deepseek-flash", "name": "DeepSeek-V4.1-Flash",
        "contextWindow": 1048576});
    assert!(materialize_model(
        &mut doc,
        "myds",
        "deepseek-flash",
        true,
        Some(&def),
        None,
        None
    ));
    let m = &doc["providers"]["myds"]["models"][0];
    assert_eq!(m["id"], "deepseek-flash");
    assert_eq!(m["name"], "DeepSeek-V4.1-Flash");
    assert_eq!(m["contextWindow"], 1048576);
    // 骨架兜底 + 缺省形状（pi modelFromJson / 聊天路径硬要求）
    assert_eq!(m["api"], "openai-completions");
    assert_eq!(m["baseUrl"], "https://api.deepseek.com");
    assert_eq!(m["input"], json!(["text"]));
    assert_eq!(m["reasoning"], json!(true));
    assert!(m["cost"].is_object());
    // 幂等：再勾一次不重复
    assert!(!materialize_model(
        &mut doc,
        "myds",
        "deepseek-flash",
        true,
        Some(&def),
        None,
        None
    ));
    assert_eq!(
        doc["providers"]["myds"]["models"].as_array().unwrap().len(),
        1
    );
}

#[test]
fn materialize_off_on_roundtrip_keeps_store_params() {
    let mut doc = json!({ "providers": { "p": {
        "api": "openai-completions", "baseUrl": "https://x" } } });
    // 手调过的仓定义（maxTokens/reasoning=false）
    let def = json!({"id": "m1", "maxTokens": 65536, "reasoning": false,
        "input": ["text", "image"]});
    assert!(materialize_model(
        &mut doc,
        "p",
        "m1",
        true,
        Some(&def),
        None,
        None
    ));
    assert!(materialize_model(
        &mut doc,
        "p",
        "m1",
        false,
        Some(&def),
        None,
        None
    ));
    // 空 models[] 删键，骨架保留
    assert!(doc["providers"]["p"].get("models").is_none());
    assert_eq!(doc["providers"]["p"]["baseUrl"], "https://x");
    // 取消再取消 = no-op
    assert!(!materialize_model(
        &mut doc,
        "p",
        "m1",
        false,
        Some(&def),
        None,
        None
    ));
    assert!(materialize_model(
        &mut doc,
        "p",
        "m1",
        true,
        Some(&def),
        None,
        None
    ));
    let m = &doc["providers"]["p"]["models"][0];
    assert_eq!(m["maxTokens"], 65536);
    assert_eq!(
        m["reasoning"],
        json!(false),
        "仓里显式 false 不被缺省改回 true"
    );
    assert_eq!(m["input"], json!(["text", "image"]));
}

#[test]
fn materialize_without_store_def_uses_shown_entry() {
    let mut doc = json!({ "providers": { "p": {
        "api": "anthropic-messages", "baseUrl": "https://x" } } });
    assert!(materialize_model(
        &mut doc,
        "p",
        "m2",
        true,
        None,
        Some("Model Two"),
        Some(200_000)
    ));
    let m = &doc["providers"]["p"]["models"][0];
    assert_eq!(m["id"], "m2");
    assert_eq!(m["name"], "Model Two");
    assert_eq!(m["contextWindow"], 200_000);
    assert_eq!(m["api"], "anthropic-messages");
    // 商不在账本（内置/扩展）→ 不物化
    assert!(!materialize_model(
        &mut doc, "nope", "m", true, None, None, None
    ));
    assert!(doc["providers"].get("nope").is_none());
}

#[test]
fn upsert_add_and_update_models() {
    let mut doc = json!({ "providers": {} });
    assert!(!upsert_provider(
        &mut doc,
        "glm",
        json!({ "baseUrl": "https://x/v1", "api": "openai-completions" })
    ));
    assert_eq!(push_model(&mut doc, "glm", json!({ "id": "m0" })), 0);
    assert_eq!(push_model(&mut doc, "glm", json!({ "id": "m1" })), 1);
    update_model(
        &mut doc,
        "glm",
        1,
        json!({ "id": "glm-5.3", "reasoning": true }),
    );
    let entry = provider_entry(&doc, "glm").unwrap();
    assert_eq!(provider_models(entry).len(), 2);
    assert_eq!(entry["models"][1]["reasoning"], true);
    // replace flags
    assert!(upsert_provider(
        &mut doc,
        "glm",
        json!({ "baseUrl": "https://y" })
    ));
    assert!(provider_entry(&doc, "glm").unwrap().get("models").is_none());
}

#[test]
fn rename_keeps_order_and_rejects_collision() {
    let mut doc = json!({ "providers": {
        "aaa": { "baseUrl": "a" }, "glm": { "baseUrl": "g" }, "zzz": { "baseUrl": "z" }
    }});
    rename_provider(&mut doc, "glm", "deepseek").unwrap();
    let names: Vec<String> = providers(&doc).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, vec!["aaa", "deepseek", "zzz"]);
    let err = rename_provider(&mut doc, "aaa", "zzz").unwrap_err();
    assert!(err.contains("already exists"));
    assert!(remove_provider(&mut doc, "deepseek"));
    assert!(!remove_provider(&mut doc, "deepseek"));
}

#[test]
fn remove_model_and_missing_provider_are_safe() {
    let mut doc = json!({ "providers": { "p": { "models": [ { "id": "a" } ] } } });
    assert!(remove_model(&mut doc, "p", 0));
    assert!(!remove_model(&mut doc, "p", 0));
    assert!(!remove_model(&mut doc, "nope", 0));
    push_model(&mut doc, "fresh", json!({ "id": "a" }));
    assert_eq!(
        provider_entry(&doc, "fresh").unwrap()["models"][0]["id"],
        "a"
    );
}

#[test]
fn push_model_appends_and_creates_missing_array() {
    let mut doc = json!({ "providers": {} });
    // 无 models 数组的空壳商也能追加（+模型草稿行保存路径）
    assert_eq!(push_model(&mut doc, "p", json!({ "id": "m1" })), 0);
    assert_eq!(push_model(&mut doc, "p", json!({ "id": "m2" })), 1);
    let models = provider_models(provider_entry(&doc, "p").unwrap());
    assert_eq!(models.len(), 2);
    assert_eq!(models[1]["id"], "m2");
    // 整份校验通过（非空 id）——save 语义闭环
    assert!(validate(&doc).is_ok());
}
