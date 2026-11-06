//! 子代理 catalog 流式读取（044-1 §1.4）——子代理面板的数据源。
//!
//! 事实源 = 父会话 JSONL 里的 `subagent/descriptor` custom entry（扩展每次
//! activation 状态变化实时 `appendEntry`）。pi 落盘形状：
//!
//! ```json
//! {"type":"custom","customType":"subagent/descriptor","data":{…descriptor…},
//!  "id":"…","parentId":"…","timestamp":"…"}
//! ```
//!
//! 增量续读：`from_offset` 起只读新增字节（面板打开期 1s 轮询）。同 childId
//! **末条覆盖**——JSONL 只追加，按序读到的后条覆盖前条。宿主持有
//! `rows + offset`，本函数只返回本块解析出的行，由宿主按 childId 合并
//! （[`merge_rows`]），这样首读（offset=0）与增量读走同一条路径。
//!
//! 半行保护：文件尾部未写完的行**不推进 offset**，留到下一轮重读——扩展
//! 正在 append 时面板可能读到被截断的半行，按它推进 offset 会永久丢这条
//! descriptor。

use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use serde_json::Value;

/// 面板要展示的一个子代理（descriptor 的宿主侧投影）。
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogRow {
    pub child_id: String,
    pub provider: String,
    /// 无 label 时 UI 退回 child_id。
    pub label: Option<String>,
    pub profile: Option<String>,
    pub depth: u32,
    /// provisioning / active / completed / interrupted / failed
    pub phase: String,
    /// completed / aborted / error / max_turns（终态才有）
    pub stop_reason: Option<String>,
    /// 终态错误分码（UI 渲染 ABORTED 等）
    pub error_code: Option<String>,
    /// 子会话文件（整行点击 = 以正式 tab 打开）
    pub session_file: Option<String>,
    /// 父会话派发工具调用 id（`<parent>/<n>` for nested）：导航面板按轮
    /// 归属的钥匙（044-0 验收⑥：轮卡第三行状态点 + hover 扩展菜单）
    pub parent_tool_call_id: Option<String>,
    /// 子代理 token 合计（descriptor.usage，终态才落盘；老 descriptor 无此字段）
    pub usage_total: Option<u64>,
    /// 活动计数（descriptor.stats，044-0 面板行二：轮次/发言数/工具调用数；
    /// 老 descriptor 无此字段 → None，面板显 "—"）
    pub turns: Option<u64>,
    pub agent_messages: Option<u64>,
    pub tool_calls: Option<u64>,
    pub started_at: String,
    pub updated_at: String,
}

impl CatalogRow {
    /// 仍在跑（provisioning / active）——运行中计数徽标数它。
    pub fn in_flight(&self) -> bool {
        self.phase == "provisioning" || self.phase == "active"
    }

    /// 面板行的展示名：label 优先，退回 child_id。
    pub fn display_label(&self) -> &str {
        self.label
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.child_id)
    }

    /// 运行中徽标色（⟳ 黄）/ 终态按 stopReason 分色。返回主题语义色名。
    pub fn status_kind(&self) -> StatusKind {
        if self.in_flight() {
            return StatusKind::Running;
        }
        match self.stop_reason.as_deref() {
            Some("completed") => StatusKind::Done,
            Some("aborted") => StatusKind::Aborted,
            Some("max_turns") => StatusKind::MaxTurns,
            Some("error") => StatusKind::Error,
            _ => match self.phase.as_str() {
                "interrupted" => StatusKind::Aborted,
                "failed" => StatusKind::Error,
                _ => StatusKind::Done,
            },
        }
    }
}

/// 行状态的视觉分档（对齐 044-1 §1.3 交互稿的徽标配色）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    /// ⟳ 运行中（provisioning/active）
    Running,
    /// ✓ 完成
    Done,
    /// ⊘ 中止（aborted / interrupted）
    Aborted,
    /// ⌛ 轮次用尽（max_turns）——软限止损，橙
    MaxTurns,
    /// ✕ 失败（error / failed）
    Error,
}

/// 一次增量读的结果。
#[derive(Debug, Default, Clone)]
pub struct CatalogScan {
    /// 本块解析出的 descriptor（同 childId 已在块内末条覆盖）。
    pub rows: Vec<CatalogRow>,
    /// 下一轮续读的起始 offset（半行不推进）。
    pub next_offset: u64,
}

/// 把本块的行按 childId 合并进宿主既有 rows（末条覆盖；无则追加）。
/// 保持首次出现的顺序，让运行中的排前面由调用方另行排序。
pub fn merge_rows(host: &mut Vec<CatalogRow>, fresh: Vec<CatalogRow>) {
    for row in fresh {
        if let Some(slot) = host.iter_mut().find(|r| r.child_id == row.child_id) {
            *slot = row;
        } else {
            host.push(row);
        }
    }
}

/// 从 `from_offset` 起流式扫父会话文件，抽出 `subagent/descriptor`。
///
/// 文件不存在 / 打不开按空块处理（返回 offset 0，下一轮重试）。文件变小
/// （换会话/截断）也归零重扫。
pub fn read_catalog(file: &Path, from_offset: u64) -> CatalogScan {
    let mut f = match std::fs::File::open(file) {
        Ok(f) => f,
        Err(_) => return CatalogScan::default(),
    };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len < from_offset {
        // 文件变小：从头重扫
        return read_catalog(file, 0);
    }
    if len == from_offset {
        return CatalogScan {
            rows: Vec::new(),
            next_offset: from_offset,
        };
    }
    if f.seek(SeekFrom::Start(from_offset)).is_err() {
        return CatalogScan::default();
    }
    let mut buf = String::new();
    if f.read_to_string(&mut buf).is_err() {
        // 尾部有非法 UTF-8（正在写）：本轮跳过，下一轮重试同一 offset
        return CatalogScan {
            rows: Vec::new(),
            next_offset: from_offset,
        };
    }

    let mut by_id: HashMap<String, CatalogRow> = HashMap::new();
    let mut consumed: u64 = 0;
    for line in buf.split_inclusive('\n') {
        // 最后一段若没以 \n 结尾 = 半行：不消费、不解析
        if !line.ends_with('\n') {
            break;
        }
        consumed += line.len() as u64;
        if let Some(row) = parse_line(line) {
            by_id.insert(row.child_id.clone(), row);
        }
    }
    // HashMap 无序：按文件出现顺序输出（保留首次出现序）
    let mut seen: Vec<String> = Vec::new();
    let mut rows: Vec<CatalogRow> = Vec::new();
    for line in buf.split_inclusive('\n').filter(|l| l.ends_with('\n')) {
        if let Some(row) = parse_line(line) {
            if seen.iter().any(|id| *id == row.child_id) {
                continue;
            }
            seen.push(row.child_id.clone());
            rows.push(by_id.remove(&row.child_id).unwrap_or(row));
        }
    }
    CatalogScan {
        rows,
        next_offset: from_offset + consumed,
    }
}

/// 解析一行 JSONL；只认 `type=custom` + `customType=subagent/descriptor`。
/// 非 descriptor 行 / 非法 JSON / 缺必填字段 → None（fail-closed：读不懂
/// 的 descriptor 从不当作活子代理）。
fn parse_line(line: &str) -> Option<CatalogRow> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    if v["type"].as_str()? != "custom" {
        return None;
    }
    if v["customType"].as_str()? != "subagent/descriptor" {
        return None;
    }
    let d = &v["data"];
    let child_id = d["childId"].as_str()?;
    if child_id.is_empty() {
        return None;
    }
    let provider = d["provider"].as_str().unwrap_or("").to_string();
    if provider.is_empty() {
        return None;
    }
    let depth = d["depth"].as_u64().unwrap_or(1) as u32;
    let phase = d["phase"].as_str().unwrap_or("").to_string();
    if phase.is_empty() {
        return None;
    }
    Some(CatalogRow {
        child_id: child_id.to_string(),
        provider,
        label: d["label"]
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        profile: d["profile"]
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        depth,
        phase,
        stop_reason: d["stopReason"].as_str().map(str::to_string),
        error_code: d["errorCode"].as_str().map(str::to_string),
        session_file: d["sessionFile"]
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        parent_tool_call_id: d["parentToolCallId"]
            .as_str()
            .map(str::to_string)
            .filter(|s| !s.is_empty()),
        usage_total: usage_total_of(&d["usage"]),
        turns: stat_of(d, "turns"),
        agent_messages: stat_of(d, "agentMessages"),
        tool_calls: stat_of(d, "toolCalls"),
        started_at: d["startedAt"].as_str().unwrap_or("").to_string(),
        updated_at: d["updatedAt"].as_str().unwrap_or("").to_string(),
    })
}

/// descriptor.usage 的合计 tokens：优先 `total`，缺了把可加字段求和。
/// 无 usage 字段 / 全零 → None（面板显示 "—"，老 descriptor 兼容）。
fn usage_total_of(u: &Value) -> Option<u64> {
    if !u.is_object() {
        return None;
    }
    if let Some(t) = u["total"].as_u64() {
        return Some(t);
    }
    let sum: u64 = ["input", "output", "cacheRead", "cacheWrite"]
        .iter()
        .filter_map(|k| u[*k].as_u64())
        .sum();
    (sum > 0).then_some(sum)
}

/// descriptor.stats 的单顶计数：stats 缺失/非对象/字段缺失 → None。
fn stat_of(d: &Value, key: &str) -> Option<u64> {
    d["stats"][key].as_u64()
}

/// ISO-8601（`2026-10-19T12:34:56.789Z`）→ Unix 秒。解析不出返回 None。
/// 只需要到秒的精度（面板显示耗时），不引时间库。支持 `Z` 与 `±HH:MM`
/// 偏移（偏移会折算进结果）。
pub fn parse_iso_secs(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, time) = s.split_once('T')?;
    let time = time.trim_end_matches('Z');
    // 时间段里可能带 ±HH:MM 偏移：先摘出来，否则会被当成名字的一部分丢掉
    let (time_core, offset_secs) = match time.find(['+', '-']) {
        Some(ix) => {
            let (core, rest) = time.split_at(ix);
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            let mut parts = rest[1..].split(':');
            let oh: i64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
            let om: i64 = parts.next().unwrap_or("0").parse().unwrap_or(0);
            (core, sign * (oh * 3600 + om * 60))
        }
        None => (time, 0),
    };
    // 去掉毫秒尾巴
    let time_core = time_core.split('.').next().unwrap_or(time_core);
    let mut d = date.split('-');
    let year: i64 = d.next()?.parse().ok()?;
    let month: i64 = d.next()?.parse().ok()?;
    let day: i64 = d.next()?.parse().ok()?;
    let mut t = time_core.split(':');
    let hour: i64 = t.next()?.parse().ok()?;
    let min: i64 = t.next()?.parse().ok()?;
    let sec: i64 = t.next().unwrap_or("0").parse().unwrap_or(0);
    Some(days_from_civil(year, month, day) * 86400 + hour * 3600 + min * 60 + sec - offset_secs)
}

/// Howard Hinnant 的 days_from_civil（公历日期 → 距 epoch 天数）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// 行的运行耗时（秒）：updated_at - started_at；缺一个按 None。
pub fn row_duration_secs(row: &CatalogRow) -> Option<i64> {
    let a = parse_iso_secs(&row.started_at)?;
    let b = parse_iso_secs(&row.updated_at)?;
    Some((b - a).max(0))
}

#[cfg(test)]
mod tests;
