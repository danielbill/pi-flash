//! iLink HTTP 传输层（P0 探针）。
//!
//! 出站行为逐条对齐 ZCode `packages/services/src/bots/providers/`：
//! - 扫码接口：GET + `iLink-App-ClientVersion: 1`（`weixinRegistration.ts:86-101`）
//! - 业务接口：POST + `AuthorizationType: ilink_bot_token` + `Bearer` +
//!   `X-WECHAT-UIN` + `base_info.channel_version`（`weixinProvider.ts:104-155`）
//! - 三档超时 30s / 90s / 15s，分别对齐 ZCode 的三个常量
//!
//! **每个请求的 request 与 response 原样落盘**到 `~/.pi-flash/wxprobe-dump/`，
//! 这是 P0 的主要产出：用真实字段名对拍 ZCode 的 6 候选嗅探。

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde_json::{json, Map, Value};
use ureq::Agent;

pub const ILINK_BASE: &str = "https://ilinkai.weixin.qq.com";
pub const API_PREFIX: &str = "/ilink/bot";
pub const CHANNEL_VERSION: &str = "2.0.0";

/// 扫码接口超时（ZCode `WEIXIN_LOGIN_REQUEST_TIMEOUT_MS = 30_000`）。
pub const QR_TIMEOUT: Duration = Duration::from_secs(30);
/// getupdates 长轮询超时（ZCode `WEIXIN_GET_UPDATES_TIMEOUT_MS = 90_000`）。
pub const POLL_TIMEOUT: Duration = Duration::from_secs(90);
/// 普通业务接口超时（ZCode `BOT_PROVIDER_REQUEST_TIMEOUT_MS = 15_000`）。
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15);

/// dump 目录只留最近这么多对文件，探针反复跑不会把磁盘写爆。
const DUMP_KEEP: usize = 120;

pub struct Wire {
    qr: Agent,
    default: Agent,
    poll: Agent,
    dump_dir: PathBuf,
    seq: u32,
}

impl Wire {
    pub fn new(dump_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dump_dir);
        prune_dump(&dump_dir);
        let seq = next_seq(&dump_dir);
        Self {
            qr: agent(QR_TIMEOUT),
            default: agent(DEFAULT_TIMEOUT),
            poll: agent(POLL_TIMEOUT),
            dump_dir,
            seq,
        }
    }

    /// 扫码：`GET /ilink/bot/<path>?<query>`，无鉴权。
    pub fn get_qr(&mut self, tag: &str, path: &str, query: &[(&str, &str)]) -> Result<Value, String> {
        let url = format!("{ILINK_BASE}{API_PREFIX}{path}");
        let mut req = self.qr.get(&url).header("iLink-App-ClientVersion", "1");
        for (k, v) in query {
            req = req.query(*k, *v);
        }
        let seq = self.seq;
        self.seq += 1;
        let res = req.call().map_err(|e| format!("GET {path}: {e}"))?;
        self.consume(tag, seq, res.status().as_u16(), res, None)
    }

    /// 业务：`POST /ilink/bot/<path>`，带 bot_token（15s 超时）。
    pub fn post(
        &mut self,
        tag: &str,
        path: &str,
        token: Option<&str>,
        body: Value,
    ) -> Result<Value, String> {
        self.post_inner(&self.default.clone(), tag, path, token, body)
    }

    /// getupdates 长轮询（90s 超时）。
    pub fn post_poll(
        &mut self,
        tag: &str,
        path: &str,
        token: Option<&str>,
        body: Value,
    ) -> Result<Value, String> {
        self.post_inner(&self.poll.clone(), tag, path, token, body)
    }

    fn post_inner(
        &mut self,
        agent: &Agent,
        tag: &str,
        path: &str,
        token: Option<&str>,
        body: Value,
    ) -> Result<Value, String> {
        let url = format!("{ILINK_BASE}{API_PREFIX}{path}");
        let mut req = agent.post(&url);
        if let Some(t) = token {
            req = req
                .header("content-type", "application/json")
                .header("AuthorizationType", "ilink_bot_token")
                .header("Authorization", format!("Bearer {t}"))
                .header("X-WECHAT-UIN", wechat_uin());
        }
        let payload = with_base_info(body);
        let seq = self.seq;
        self.seq += 1;
        let res = req
            .send_json(&payload)
            .map_err(|e| format!("POST {path}: {e}"))?;
        self.consume(tag, seq, res.status().as_u16(), res, Some(payload))
    }

    fn consume(
        &mut self,
        tag: &str,
        seq: u32,
        status: u16,
        mut res: ureq::http::Response<ureq::Body>,
        req_body: Option<Value>,
    ) -> Result<Value, String> {
        let text = res
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("{tag}: 读响应体失败 {e}"))?;
        self.dump(seq, tag, status, req_body, &text);
        if !(200..300).contains(&status) {
            return Err(format!("{tag}: HTTP {status}: {}", truncate(&text, 400)));
        }
        let value: Value =
            serde_json::from_str(&text).map_err(|e| format!("{tag}: 非 JSON 响应 {e}: {}", truncate(&text, 400)))?;
        check_ret(&value)?;
        Ok(unwrap_data(value))
    }

    fn dump(&self, seq: u32, tag: &str, status: u16, req: Option<Value>, res_text: &str) {
        let base = self.dump_dir.join(format!("{seq:03}-{tag}"));
        if let Some(r) = req {
            let _ = fs::write(base.with_extension("req.json"), to_pretty(&r));
        }
        let head = format!("HTTP {status}\n");
        let _ = fs::write(base.with_extension("res.txt"), format!("{head}{res_text}"));
    }
}

fn agent(timeout: Duration) -> Agent {
    Agent::new_with_config(
        Agent::config_builder()
            .timeout_global(Some(timeout))
            .build(),
    )
}

/// ZCode `appendBaseInfo`：`{ base_info, ...body }`。
fn with_base_info(body: Value) -> Value {
    let mut map = Map::new();
    map.insert(
        "base_info".into(),
        json!({ "channel_version": CHANNEL_VERSION }),
    );
    match body {
        Value::Object(m) => map.extend(m),
        other => {
            map.insert("payload".into(), other);
        }
    }
    Value::Object(map)
}

/// ZCode `unwrapData`：`payload.data ?? payload`（扫码接口把字段包在 `data` 里）。
pub fn unwrap_data(value: Value) -> Value {
    match value {
        Value::Object(ref m) if m.contains_key("data") && m["data"].is_object() => {
            let mut m = m.clone();
            let inner = m.remove("data").unwrap_or(Value::Null);
            match inner {
                Value::Object(im) => {
                    for (k, v) in im {
                        m.entry(k).or_insert(v);
                    }
                    Value::Object(m)
                }
                other => other,
            }
        }
        other => other,
    }
}

/// ZCode `requestWeixinJson` 的 ret/errcode 校验（`weixinProvider.ts:141-150`）。
pub fn check_ret(payload: &Value) -> Result<(), String> {
    let ret = num(payload, "ret");
    let errcode = num(payload, "errcode");
    if ret.is_some_and(|v| v != 0.0) || errcode.is_some_and(|v| v != 0.0) {
        let msg = str_of(payload, "errmsg")
            .or_else(|| str_of(payload, "message"))
            .unwrap_or_default();
        return Err(format!(
            "iLink 业务错误 ret={:?} errcode={:?} {msg}",
            ret, errcode
        ));
    }
    Ok(())
}

/// `X-WECHAT-UIN` = base64(十进制 u32 的 UTF-8 字节)，ZCode `buildRandomWechatUin`。
fn wechat_uin() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0x5EED);
    let mixed = nanos ^ std::process::id().wrapping_mul(2_654_435_761);
    base64::engine::general_purpose::STANDARD.encode((mixed & 0xFFFF_FFFF).to_string())
}

pub fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64)
}

pub fn str_of(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(|x| match x {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    })
}

/// 依次取第一个非空候选键——ZCode 的防御式嗅探风格。
/// 泛型同时收数组与切片，避免 12 处调用点反复写 `&`。
pub fn first_str<K>(v: &Value, keys: K) -> Option<String>
where
    K: AsRef<[&'static str]>,
{
    keys.as_ref()
        .iter()
        .find_map(|k| str_of(v, k).filter(|s| !s.is_empty()))
}

pub fn to_pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_else(|_| v.to_string())
}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let cut: String = s.chars().take(n).collect();
        format!("{cut}…")
    }
}

fn next_seq(dir: &PathBuf) -> u32 {
    fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().is_some_and(|x| x == "res.txt"))
                .filter_map(|e| {
                    e.file_name()
                        .to_str()?
                        .split('-')
                        .next()?
                        .parse::<u32>()
                        .ok()
                })
                .max()
                .map(|m| m + 1)
                .unwrap_or(1)
        })
        .unwrap_or(1)
}

fn prune_dump(dir: &PathBuf) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().is_some_and(|x| x == "res.txt"))
        .collect();
    if entries.len() <= DUMP_KEEP {
        return;
    }
    entries.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    for e in entries.iter().take(entries.len() - DUMP_KEEP) {
        let p = e.path();
        let _ = fs::remove_file(&p);
        let _ = fs::remove_file(p.with_extension("req.json"));
    }
}
