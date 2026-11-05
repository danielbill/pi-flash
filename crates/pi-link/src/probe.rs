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
//!   供模块②快路径与内置商「获取模型」使用——端点由 [`crate::registry`]
//!   dump 携带的 SDK baseUrl/api 经 [`models_list_url`] 机械派生（2026-11-01
//!   起单一来源；13 条手写白名单表已废）。
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

/// 列模型 URL 机械拼装（模块③入口，对齐 pi-web `buildModelsListUrl`，
/// **零厂商/模型名硬编码**）：输入 baseUrl + api 形状 → (url, auth)。
/// - 路径已以 /models 结尾：只按形状定鉴权头
/// - anthropic-messages：路径非 /vN(beta) 结尾补 /v1，加 `limit=1000`
/// - google-generative-ai：路径非版本段结尾补 /v1beta，加 `pageSize=1000`
/// - 其余（openai 系，含 /v1 /v4 等版本段）：直接接 /models
pub fn models_list_url(base_url: &str, api: &str) -> Option<(String, Auth)> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return None;
    }
    let auth = match api {
        "anthropic-messages" => Auth::Anthropic,
        "google-generative-ai" => Auth::Google,
        _ => Auth::Bearer,
    };
    // 路径段（host 之后的全部）；无路径 = 空
    let path = base
        .split_once("://")
        .and_then(|(_, r)| r.split_once('/'))
        .map(|(_, p)| format!("/{}", p.trim_end_matches('/')))
        .unwrap_or_default();
    // 末段是版本段：/v<数字> 或 /v<数字>beta（v1 / v4 / v1beta）
    let ends_versioned = path
        .rsplit('/')
        .next()
        .and_then(|tail| tail.strip_prefix('v'))
        .is_some_and(|t| {
            let digits = t.split('b').next().unwrap_or("");
            !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
        });
    let suffix = if path.ends_with("/models") {
        String::new()
    } else {
        match api {
            "anthropic-messages" if !ends_versioned => "/v1/models",
            "google-generative-ai" if !ends_versioned => "/v1beta/models",
            _ => "/models",
        }
        .to_string()
    };
    let mut url = format!("{base}{suffix}");
    if api == "anthropic-messages" && !url.contains("limit=") {
        url.push_str("?limit=1000");
    }
    if api == "google-generative-ai" && !url.contains("pageSize=") {
        url.push_str("?pageSize=1000");
    }
    Some((url, auth))
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
    // 一次性确认各家 /models 到底回了哪些字段（0.3.2 争议：DeepSeek 是否给
    // 最大输出）——只打首条的键集，不打值；debug 构建专属，release 零输出
    #[cfg(debug_assertions)]
    if let Some(first) = entries.first().and_then(|e| e.as_object()) {
        let keys: Vec<&str> = first.keys().map(String::as_str).collect();
        eprintln!("[probe] /models first entry keys: {keys:?}");
    }
    let catalog = entries.iter().filter_map(parse_entry).collect();
    (models, catalog)
}

/// 单条模型目录项的 best-effort 解析（各家 /models 字段形状不一）。
/// 最大输出按常见键名顺序取（openrouter 嵌在 `top_provider` 里）；没给
/// 就 None——pi `modelFromJson` 缺省 16384，不算错。
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
    let max_tokens = [
        "max_tokens",
        "max_output_tokens",
        "max_completion_tokens",
        "outputTokenLimit",
    ]
    .iter()
    .find_map(|k| e.get(*k).and_then(|v| v.as_u64()))
    .or_else(|| {
        e.get("top_provider")
            .and_then(|t| t.get("max_completion_tokens"))
            .and_then(|v| v.as_u64())
    })
    .filter(|n| *n > 0);
    // 输入模态（DeepSeek `input_modalities`；pi 只认 text/image，其余丢弃）
    let input = e
        .get("input_modalities")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str())
                .filter(|s| *s == "text" || *s == "image")
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty());
    // 推理能力：DeepSeek `effort.supported_levels` 非空 = 支持；没给就 None
    //（交给落盘缺省，不猜）
    let reasoning = e
        .get("effort")
        .and_then(|v| v.get("supported_levels"))
        .and_then(|v| v.as_array())
        .map(|a| !a.is_empty());
    Some(crate::protocol::ModelInfo {
        id: id.to_string(),
        name,
        provider: String::new(),
        context_window,
        // 探查取 id/name/上下文窗口/最大输出/模态/推理（各家给多少取多少）
        max_tokens,
        reasoning,
        input,
        cost: None,
    })
}

#[cfg(test)]
mod tests;
