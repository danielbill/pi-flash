//! 通路探查（042 决策 3，PF 超出 pi-web 的优化）。2026-10-31 定稿：测速
//! 按钮拆成三个独立功能模块，本文件按模块分节，**代码里不得存在任何写死
//! 的模型名**（历史教训：messages 兜底里硬编码过 `deepseek-chat`——那是
//! 「模块②缺失被业务请求顶替」的结构必然，见 [`probe_connect`] 文档）。
//!
//! - 模块① [`infer_api`]：baseUrl → api type（pi 注册与真实业务请求需要），
//!   **不参与连通探测**。
//! - 模块② [`probe_connect`]：连通性测试，输入只有 baseUrl + key，与 api
//!   type 解耦。
//! - 模块③ [`probe`]：拉取 Model List（GET /models + best-effort 解析），
//!   供模块②快路径与内置商端点表（[`catalog_target`]，vendor pin 的已知
//!   端点表，非模型名硬编码）使用。
//!
//! 结果只驱动 UI 状态点，不回滚保存——可能是网络问题，key 留着。

use std::time::Duration;

/// 列模型请求的鉴权样式（决定头形状）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth {
    /// `Authorization: Bearer <key>`（openai 系）
    Bearer,
    /// `x-api-key` + `anthropic-version`（anthropic 系）
    Anthropic,
    /// `x-goog-api-key`（google-generative-ai 系）
    Google,
}

/// 探查结果三态。
#[derive(Debug, Clone, PartialEq)]
pub enum ProbeOutcome {
    /// 2xx：通路与 key 均有效；`catalog` = 响应体里 best-effort 解出的模型
    /// 目录（字段缺失尽量回填，解不出为空）。
    Ok {
        models: usize,
        latency_ms: u64,
        catalog: Vec<crate::protocol::ModelInfo>,
    },
    /// 401/403：key 被拒（网络通）。
    AuthRejected { status: u16 },
    /// 超时/DNS/其他 HTTP 状态：不可达或端点不符。
    Unreachable { error: String },
}

// ---------------------------------------------------------------------------
// 模块①：API Type 判定（服务 pi 注册与真实业务请求，不做连通探测）
// ---------------------------------------------------------------------------

/// baseUrl → pi wire 协议形状（042-1 改版：Provider = baseUrl + apiKey，
/// API 形状用户不选，PF 推断后随条目落盘——pi 侧 modelFromJson /
/// extensionModelFromDefinition 对缺 api 硬 throw，绝不留空）。
/// `*/anthropic` / 含 `/anthropic/` → anthropic-messages（DeepSeek /anthropic、
/// GLM、Kimi 等兼容端点）；`*/v1beta` → google-generative-ai；其余 →
/// openai-completions（最大公约数）。openai-responses 与 completions 在 URL
/// 上不可区分，一律默认 completions。**只服务注册与业务请求（[`crate::model_test`]），
/// 连通探测走 [`probe_connect`]（不依赖本函数）。**
pub fn infer_api(base_url: &str) -> &'static str {
    let base = base_url.trim().trim_end_matches('/');
    if base.ends_with("/anthropic") || base.contains("/anthropic/") {
        "anthropic-messages"
    } else if base.ends_with("/v1beta") || base.contains("/v1beta/") {
        "google-generative-ai"
    } else {
        "openai-completions"
    }
}

// ---------------------------------------------------------------------------
// 模块③：拉取 Model List（GET /models + best-effort 解析）
// ---------------------------------------------------------------------------

/// 内置商探查端点表（vendor pin 1.1.0）。只收高置信条目——表外商跳过
/// 探查（UI 直接呈现已配置态），宁缺勿错报。**这是已知端点表，不是模型名
/// 硬编码**——模型名只该出现在真实业务请求（`model_test`，调用方提供）。
pub fn catalog_target(provider: &str) -> Option<(&'static str, Auth)> {
    const TABLE: &[(&str, &str, Auth)] = &[
        ("openai", "https://api.openai.com/v1/models", Auth::Bearer),
        (
            "deepseek",
            "https://api.deepseek.com/v1/models",
            Auth::Bearer,
        ),
        (
            "anthropic",
            "https://api.anthropic.com/v1/models",
            Auth::Anthropic,
        ),
        (
            "google",
            "https://generativelanguage.googleapis.com/v1beta/models",
            Auth::Google,
        ),
        (
            "openrouter",
            "https://openrouter.ai/api/v1/models",
            Auth::Bearer,
        ),
        (
            "groq",
            "https://api.groq.com/openai/v1/models",
            Auth::Bearer,
        ),
        (
            "cerebras",
            "https://api.cerebras.ai/v1/models",
            Auth::Bearer,
        ),
        ("mistral", "https://api.mistral.ai/v1/models", Auth::Bearer),
        ("xai", "https://api.x.ai/v1/models", Auth::Bearer),
        (
            "moonshotai",
            "https://api.moonshot.ai/v1/models",
            Auth::Bearer,
        ),
        (
            "moonshotai-cn",
            "https://api.moonshot.cn/v1/models",
            Auth::Bearer,
        ),
        (
            "together",
            "https://api.together.xyz/v1/models",
            Auth::Bearer,
        ),
        (
            "fireworks",
            "https://api.fireworks.ai/inference/v1/models",
            Auth::Bearer,
        ),
    ];
    TABLE
        .iter()
        .find(|(p, _, _)| *p == provider)
        .map(|(_, url, auth)| (*url, *auth))
}

/// 发一次列模型 GET（阻塞，20s 超时）并按三态分类。2xx 响应体交
/// [`parse_catalog`] 解析目录。
pub fn probe(url: &str, auth: Auth, key: &str) -> ProbeOutcome {
    match http(url, auth, key, None) {
        Ok((latency_ms, body)) => {
            let (models, catalog) = parse_catalog(&body);
            ProbeOutcome::Ok {
                models,
                latency_ms,
                catalog,
            }
        }
        Err(HttpFail::Auth(status)) => ProbeOutcome::AuthRejected { status },
        Err(HttpFail::Status(status, _)) => ProbeOutcome::Unreachable {
            error: format!("HTTP {status}"),
        },
        Err(HttpFail::Net(e)) => ProbeOutcome::Unreachable { error: e },
    }
}

// ---------------------------------------------------------------------------
// 模块②：连通性测试（baseUrl + key，与 api type 解耦）
// ---------------------------------------------------------------------------

/// 连通性测试（2026-10-31 三模块定稿）。输入只有 baseUrl + key，内部按
/// 候选阶梯逐发请求，**全部无厂商知识、无写死模型名**：
///
/// ```text
/// GET  {base}/models              Bearer      ← 命中即连通 + 目录（快路径）
/// GET  {base}/v1/models           Bearer
/// GET  {base}/v1/models           Anthropic
/// GET  {base}/v1beta/models       Google
/// POST {base}/chat/completions {} Bearer      ← 协议握手：体 {} 故意缺
/// POST {base}/v1/messages      {} Anthropic  │  model 字段，协议正确的服务
/// POST {base}/v1/messages      {} Bearer     │  端必须回请求校验类 4xx
///                                               （缺字段 validation——官方
///                                               API 是 400，DeepSeek 实测是
///                                               422），即「路由可达且讲协议」
/// ```
///
/// 单步结论：2xx = 连通成立；**握手 POST 的校验类 4xx（非 401/403/404/405）
/// = 协议可达**（服务端已解析并校验请求体 → 连通成立、目录为空）；401/403
/// 记为 key 被拒证据但**不定论**（换下一种鉴权样式可能就通）；404/405 =
/// 路由不存在；其余换下一候选。终局：连通 → [`ProbeOutcome::Ok`]（空目录 = UI
/// 的 PassedEmpty，提示手动 +模型）；未连通但见过 401/403 →
/// [`ProbeOutcome::AuthRejected`]；否则 [`ProbeOutcome::Unreachable`]。
///
/// 历史（P3-7 根因）：这里曾用 `POST /v1/messages` 发真实业务请求判连通，
/// 请求体必须带 model 而探测层恰恰没有合法 model id——只能硬编码
/// `deepseek-chat`，等于赌某个厂商。改用「缺 model 的协议握手」后，400 的
/// 语义恰好是「讲协议且可达」，不绑模型、不消耗 token。
pub fn probe_connect(base_url: &str, key: &str) -> ProbeOutcome {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return ProbeOutcome::Unreachable {
            error: "empty base url".into(),
        };
    }
    // 候选阶梯（url, auth, is_handshake）；base 已含版本段时跳过会拼重的
    // GET 形态（/v1 + /v1/models → /v1/v1/models）
    let mut ladder: Vec<(String, Auth, bool)> = Vec::new();
    let mut add = |path: &str, auth: Auth, hs: bool| {
        let url = format!("{base}{path}");
        // 去重键 = (url, auth)：同一 URL 换鉴权样式是不同候选
        if !ladder.iter().any(|(u, a, _)| *u == url && *a == auth) {
            ladder.push((url, auth, hs));
        }
    };
    add("/models", Auth::Bearer, false);
    if !base.ends_with("/v1") {
        add("/v1/models", Auth::Bearer, false);
    }
    if !base.ends_with("/v1") {
        add("/v1/models", Auth::Anthropic, false);
    }
    if base.ends_with("/v1beta") {
        add("/models", Auth::Google, false);
    } else {
        add("/v1beta/models", Auth::Google, false);
    }
    add("/chat/completions", Auth::Bearer, true);
    add("/v1/messages", Auth::Anthropic, true);
    add("/v1/messages", Auth::Bearer, true);

    let mut auth_rejected: Option<u16> = None;
    let mut last_error = String::new();
    for (url, auth, is_handshake) in ladder {
        let body = is_handshake.then_some("{}");
        match http(&url, auth, key, body) {
            Ok((latency_ms, resp)) => {
                // 2xx：连通成立（握手体 {} 若真被接受也按空目录连通计）
                let (models, catalog) = if is_handshake {
                    (0, Vec::new())
                } else {
                    parse_catalog(&resp)
                };
                return ProbeOutcome::Ok {
                    models,
                    latency_ms,
                    catalog,
                };
            }
            Err(HttpFail::Auth(status)) => {
                // key 被拒证据：换下一种鉴权样式可能就通，先不定论
                auth_rejected.get_or_insert(status);
            }
            Err(HttpFail::Status(status, latency_ms))
                if is_handshake
                    && (400..500).contains(&status)
                    && !matches!(status, 401 | 403 | 404 | 405) =>
            {
                // 协议握手成立：服务端已解析并校验请求体（缺 model/messages
                // 等字段的 validation 错，400/422 随各家实现）→ 路由可达且
                // 讲该协议；404/405 = 路由不存在，换下一候选
                return ProbeOutcome::Ok {
                    models: 0,
                    latency_ms,
                    catalog: Vec::new(),
                };
            }
            Err(HttpFail::Status(status, _)) => {
                last_error = format!("HTTP {status}");
            }
            Err(HttpFail::Net(e)) => {
                last_error = e;
            }
        }
    }
    match auth_rejected {
        Some(status) => ProbeOutcome::AuthRejected { status },
        None => ProbeOutcome::Unreachable { error: last_error },
    }
}

// ---------------------------------------------------------------------------
// HTTP 原语（模块②③共用）
// ---------------------------------------------------------------------------

/// 非 2xx 失败分类。Auth = 401/403（key 被拒证据）；Status = 其他 HTTP 状态
/// （连带耗时，握手 400 的「测速」语义要用）；Net = 网络/超时。
enum HttpFail {
    Auth(u16),
    Status(u16, u64),
    Net(String),
}

/// 发一次请求（阻塞，20s 超时）。`body` = Some → POST JSON，None → GET。
/// 2xx → (耗时, 响应体截断 1MB)。
fn http(url: &str, auth: Auth, key: &str, body: Option<&str>) -> Result<(u64, String), HttpFail> {
    let started = std::time::Instant::now();
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();
    let req = if body.is_some() {
        agent.post(url)
    } else {
        agent.get(url)
    };
    let req = match auth {
        Auth::Bearer => req.set("Authorization", &format!("Bearer {key}")),
        Auth::Anthropic => req
            .set("x-api-key", key)
            .set("anthropic-version", "2023-06-01"),
        Auth::Google => req.set("x-goog-api-key", key),
    };
    let req = if body.is_some() {
        req.set("Content-Type", "application/json")
    } else {
        req
    };
    let resp_result = match body {
        Some(b) => req.send_string(b),
        None => req.call(),
    };
    let resp = match resp_result {
        Ok(r) => r,
        Err(ureq::Error::Status(status, _)) => {
            let latency_ms = started.elapsed().as_millis() as u64;
            return Err(if status == 401 || status == 403 {
                HttpFail::Auth(status)
            } else {
                HttpFail::Status(status, latency_ms)
            });
        }
        Err(e) => return Err(HttpFail::Net(e.to_string())),
    };
    let latency_ms = started.elapsed().as_millis() as u64;
    // 响应体截断到 1MB——列表端点可能带全文档，计数不需要
    let text = resp
        .into_string()
        .map(|s| s.chars().take(1024 * 1024).collect::<String>())
        .unwrap_or_default();
    Ok((latency_ms, text))
}

/// 目录响应解析（模块③）：`data[]`（openai/anthropic 形状）或 `models[]`
/// （google 形状）；解不出按空目录计。返回 (条目数, 解析出的目录)。
fn parse_catalog(body: &str) -> (usize, Vec<crate::protocol::ModelInfo>) {
    let entries = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("data")
                .or_else(|| v.get("models"))
                .and_then(|a| a.as_array())
                .cloned()
        })
        .unwrap_or_default();
    let models = entries.len();
    let catalog = entries.iter().filter_map(parse_entry).collect();
    (models, catalog)
}

/// 单条模型目录项的 best-effort 解析（各家 /models 字段形状不一）。
fn parse_entry(e: &serde_json::Value) -> Option<crate::protocol::ModelInfo> {
    let raw_id = e
        .get("id")
        .and_then(|v| v.as_str())
        .or_else(|| e.get("name").and_then(|v| v.as_str()))?;
    let id = raw_id.strip_prefix("models/").unwrap_or(raw_id);
    if id.is_empty() {
        return None;
    }
    let display = e
        .get("displayName")
        .and_then(|v| v.as_str())
        .or_else(|| e.get("display_name").and_then(|v| v.as_str()))
        .or_else(|| e.get("name").and_then(|v| v.as_str()))
        .unwrap_or("");
    let name = if display.is_empty() || display == raw_id {
        String::new()
    } else {
        display.to_string()
    };
    let context_window = e
        .get("context_length")
        .and_then(|v| v.as_u64())
        .or_else(|| e.get("context_window").and_then(|v| v.as_u64()))
        .or_else(|| e.get("inputTokenLimit").and_then(|v| v.as_u64()));
    Some(crate::protocol::ModelInfo {
        id: id.to_string(),
        name,
        provider: String::new(),
        context_window,
        // 探查只关心 id/name/上下文窗口（保存时由 catalog 补全其余参数）
        max_tokens: None,
        reasoning: None,
        input: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    #[test]
    fn catalog_table_covers_the_common_api_key_providers() {
        assert_eq!(
            catalog_target("openai"),
            Some(("https://api.openai.com/v1/models", Auth::Bearer))
        );
        assert_eq!(
            catalog_target("anthropic"),
            Some(("https://api.anthropic.com/v1/models", Auth::Anthropic))
        );
        assert!(catalog_target("zai-coding-cn").is_none(), "表外商跳过探查");
    }

    #[test]
    fn infer_api_routes_by_base_shape() {
        assert_eq!(
            infer_api("https://api.deepseek.com/anthropic"),
            "anthropic-messages"
        );
        assert_eq!(
            infer_api("https://gw.example.com/api/anthropic/"),
            "anthropic-messages"
        );
        assert_eq!(
            infer_api("https://generativelanguage.googleapis.com/v1beta"),
            "google-generative-ai"
        );
        assert_eq!(infer_api("https://api.openai.com/v1"), "openai-completions");
        assert_eq!(infer_api("https://api.deepseek.com"), "openai-completions");
        assert_eq!(infer_api(""), "openai-completions");
    }

    #[test]
    fn entry_parse_maps_each_provider_shape() {
        // openrouter：name/id/context_length
        let m = parse_entry(&serde_json::json!({
            "id": "deepseek/deepseek-r1", "name": "DeepSeek R1", "context_length": 128000
        }))
        .unwrap();
        assert_eq!(m.id, "deepseek/deepseek-r1");
        assert_eq!(m.name, "DeepSeek R1");
        assert_eq!(m.context_window, Some(128000));
        // google：name 带 models/ 前缀 + displayName + inputTokenLimit
        let m = parse_entry(&serde_json::json!({
            "name": "models/gemini-2.0-flash", "displayName": "Gemini 2.0 Flash",
            "inputTokenLimit": 1048576
        }))
        .unwrap();
        assert_eq!(m.id, "gemini-2.0-flash");
        assert_eq!(m.name, "Gemini 2.0 Flash");
        assert_eq!(m.context_window, Some(1048576));
        // openai：只有 id（name=owned_by 字符串时回落空显示名）
        let m = parse_entry(&serde_json::json!({ "id": "gpt-4o", "object": "model" })).unwrap();
        assert_eq!(m.id, "gpt-4o");
        assert_eq!(m.name, "");
        assert_eq!(m.context_window, None);
        assert!(parse_entry(&serde_json::json!({ "object": "model" })).is_none());
        assert_eq!(
            parse_entry(&serde_json::json!({ "id": "x", "name": "x" }))
                .unwrap()
                .name,
            ""
        );
    }

    // ---- 模块②阶梯：本地 mock server 离线覆盖 -----------------------------

    /// 极简 HTTP mock：按 (path, 鉴权样式, 请求体) 决定状态码；200 带 openai
    /// 形状目录体、400 带「缺 model」校验错体。返回 base url + 记录到的请求体。
    fn spawn_mock(
        rules: Arc<dyn Fn(&str, &str, &str) -> u16 + Send + Sync>,
        seen: Arc<std::sync::Mutex<Vec<(String, String, String)>>>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let rules = rules.clone();
                let seen = seen.clone();
                std::thread::spawn(move || {
                    let mut stream = stream;
                    let mut buf = Vec::new();
                    let mut byte = [0u8; 1];
                    loop {
                        match stream.read(&mut byte) {
                            Ok(1) => {
                                buf.push(byte[0]);
                                if buf.ends_with(b"\r\n\r\n") {
                                    break;
                                }
                            }
                            _ => return,
                        }
                    }
                    let head = String::from_utf8_lossy(&buf);
                    let mut lines = head.lines();
                    let req_line = lines.next().unwrap_or("");
                    let path = req_line
                        .split_whitespace()
                        .nth(1)
                        .unwrap_or("/")
                        .to_string();
                    let mut auth_kind = "none";
                    let mut content_len = 0usize;
                    for l in lines {
                        let lower = l.to_lowercase();
                        if lower.starts_with("authorization: bearer") {
                            auth_kind = "bearer";
                        }
                        if lower.starts_with("x-api-key:") {
                            auth_kind = "anthropic";
                        }
                        if lower.starts_with("x-goog-api-key:") {
                            auth_kind = "google";
                        }
                        if lower.starts_with("content-length:") {
                            content_len = l
                                .split(':')
                                .nth(1)
                                .and_then(|v| v.trim().parse().ok())
                                .unwrap_or(0);
                        }
                    }
                    let mut body = vec![0u8; content_len];
                    if content_len > 0 {
                        stream.read_exact(&mut body).ok();
                    }
                    let body_s = String::from_utf8_lossy(&body).to_string();
                    seen.lock()
                        .unwrap()
                        .push((path.clone(), auth_kind.into(), body_s.clone()));
                    let status = rules(&path, auth_kind, &body_s);
                    let (resp_body, reason) = match status {
                        200 => (r#"{"data":[{"id":"m-1","name":"M1"}]}"#, "OK"),
                        400 => (
                            r#"{"error":{"message":"model: Field required"}}"#,
                            "Bad Request",
                        ),
                        401 => ("{}", "Unauthorized"),
                        403 => ("{}", "Forbidden"),
                        _ => ("{}", "Not Found"),
                    };
                    let resp = format!(
                        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{resp_body}",
                        resp_body.len()
                    );
                    stream.write_all(resp.as_bytes()).ok();
                });
            }
        });
        format!("http://{addr}")
    }

    #[test]
    fn connect_direct_catalog_hit() {
        // 候选 1（GET {base}/models Bearer）直接 200：连通 + 有目录
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let base = spawn_mock(
            Arc::new(|path, kind, _body| {
                if path.ends_with("/models") && kind == "bearer" {
                    200
                } else {
                    404
                }
            }),
            seen.clone(),
        );
        match probe_connect(&base, "k") {
            ProbeOutcome::Ok {
                models, catalog, ..
            } => {
                assert_eq!(models, 1);
                assert_eq!(catalog[0].id, "m-1");
            }
            other => panic!("expected Ok, got {other:?}"),
        }
        // 首个请求就是 GET /models，且 GET 不带请求体
        let first = &seen.lock().unwrap()[0];
        assert_eq!(first.0, "/models");
        assert_eq!(first.1, "bearer");
        assert_eq!(first.2, "");
    }

    #[test]
    fn connect_handshake_400_is_protocol_reachable() {
        // 全部 GET 404（Messages-only 网关）：messages 握手 400 = 协议可达 →
        // Ok{空目录}（UI 的 PassedEmpty 源头）；握手体必须是 `{}`——无任何
        // 写死模型名
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let base = spawn_mock(
            Arc::new(|path, _kind, body| {
                if path.ends_with("/v1/messages") && body == "{}" {
                    400
                } else {
                    404
                }
            }),
            seen.clone(),
        );
        match probe_connect(&base, "k") {
            ProbeOutcome::Ok { models, .. } => assert_eq!(models, 0),
            other => panic!("expected Ok(empty), got {other:?}"),
        }
        let seen = seen.lock().unwrap();
        let handshake = seen.iter().find(|(p, ..)| p.ends_with("/v1/messages"));
        assert_eq!(handshake.unwrap().2, "{}", "握手体 = {{}}，无模型名");
        assert!(
            seen.iter().all(|(_, _, b)| b != "deepseek-chat"),
            "任何请求都不得携带写死模型名"
        );
    }

    #[test]
    fn connect_handshake_422_validation_class_counts() {
        // DeepSeek 实测：协议校验错回 422（非 400）——校验类 4xx 整类都算
        // 协议可达
        let base = spawn_mock(
            Arc::new(|path, _kind, body| {
                if path.ends_with("/v1/messages") && body == "{}" {
                    422
                } else {
                    404
                }
            }),
            Arc::new(std::sync::Mutex::new(Vec::new())),
        );
        assert!(matches!(
            probe_connect(&base, "k"),
            ProbeOutcome::Ok { models: 0, .. }
        ));
    }

    #[test]
    fn connect_handshake_404_route_missing_falls_through() {
        // 404 = 路由不存在，不算协议可达：全 404 → Unreachable
        let base = spawn_mock(
            Arc::new(|_path, _kind, _body| 404),
            Arc::new(std::sync::Mutex::new(Vec::new())),
        );
        assert!(matches!(
            probe_connect(&base, "k"),
            ProbeOutcome::Unreachable { .. }
        ));
    }

    #[test]
    fn connect_auth_style_fallback() {
        // Bearer 全 401、Anthropic 样式 GET /v1/models 200：key 被拒证据
        // 不定论，换鉴权样式命中 → Ok{有目录}
        let base = spawn_mock(
            Arc::new(
                |_path, kind, _body| {
                    if kind == "anthropic" { 200 } else { 401 }
                },
            ),
            Arc::new(std::sync::Mutex::new(Vec::new())),
        );
        match probe_connect(&base, "k") {
            ProbeOutcome::Ok { models, .. } => assert_eq!(models, 1),
            other => panic!("expected Ok, got {other:?}"),
        }
    }

    #[test]
    fn connect_all_401_is_auth_rejected() {
        let base = spawn_mock(
            Arc::new(|_path, _kind, _body| 401),
            Arc::new(std::sync::Mutex::new(Vec::new())),
        );
        assert_eq!(
            probe_connect(&base, "bad"),
            ProbeOutcome::AuthRejected { status: 401 }
        );
    }

    #[test]
    fn connect_dead_port_is_unreachable() {
        // 占住端口再释放：连接必然被拒
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        match probe_connect(&format!("http://{addr}"), "k") {
            ProbeOutcome::Unreachable { .. } => {}
            other => panic!("expected Unreachable, got {other:?}"),
        }
    }

    #[test]
    fn connect_empty_base_is_unreachable() {
        assert!(matches!(
            probe_connect("", "k"),
            ProbeOutcome::Unreachable { .. }
        ));
    }

    #[test]
    fn connect_version_segment_base_skips_dupe_gets() {
        // base 末段已是 /vN（智谱 coding /v4 实测样本）：{base}/models 直接
        // 命中候选 1，不产生 /v1/v1/models 之类的拼重请求
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let base = spawn_mock(
            Arc::new(|path, kind, _body| {
                if path.ends_with("/models") && kind == "bearer" {
                    200
                } else {
                    404
                }
            }),
            seen.clone(),
        );
        let versioned = format!("{base}/v4");
        match probe_connect(&versioned, "k") {
            ProbeOutcome::Ok { models, .. } => assert_eq!(models, 1),
            other => panic!("expected Ok, got {other:?}"),
        }
        let seen = seen.lock().unwrap();
        assert!(
            !seen.iter().any(|(p, _, _)| p.contains("/v1/")),
            "base 已带版本段不得再拼 /v1/*：{:?}",
            seen.iter().map(|(p, _, _)| p).collect::<Vec<_>>()
        );
    }

    #[test]
    fn v1beta_base_tries_google_auth_on_direct_models() {
        // base 末段 /v1beta：Google 鉴权直连 {base}/models
        let base = spawn_mock(
            Arc::new(|path, kind, _body| {
                if path.ends_with("/models") && kind == "google" {
                    200
                } else {
                    404
                }
            }),
            Arc::new(std::sync::Mutex::new(Vec::new())),
        );
        assert!(matches!(
            probe_connect(&format!("{base}/v1beta"), "k"),
            ProbeOutcome::Ok { .. }
        ));
    }
}
