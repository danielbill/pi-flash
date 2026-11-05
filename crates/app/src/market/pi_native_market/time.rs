//! Pi 原生扩展市场——时间戳文案（040-2）：设计稿口径
//! 「10m ago / 3h ago / 2d ago」（分钟→小时→天）。
//!
//! 从 [`super::design`] 拆出：这里不是设计稿字面量（颜色/尺寸），是纯时间
//! 换算；design.rs 保持「外部真值 = 颜色/尺寸/几何」的唯一落点。

/// ISO 8601 → unix 秒（只吃 `YYYY-MM-DDTHH:MM:SS` 前缀；解析失败 None）。
fn parse_unix(iso: &str) -> Option<i64> {
    let date = iso.get(0..10)?;
    let time = iso.get(11..19).unwrap_or("00:00:00");
    let y = date.get(0..4)?.parse::<i64>().ok()?;
    let m = date.get(5..7)?.parse::<i64>().ok()?;
    let d = date.get(8..10)?.parse::<i64>().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let hh = time.get(0..2)?.parse::<i64>().ok()?;
    let mm = time.get(3..5)?.parse::<i64>().ok()?;
    let ss = time.get(6..8)?.parse::<i64>().ok()?;
    Some(days_from_civil(y, m, d) * 86_400 + hh * 3600 + mm * 60 + ss)
}

/// 公历 → 自 1970-01-01 起的天数（Howard Hinnant；与 pi-link 同款，避免为一个
/// 展示函数把纯函数提到协议层）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 「10m ago / 3h ago / 2d ago」（设计稿时间戳口径：分钟→小时→天）。
pub(super) fn ago_label(iso: &str) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let Some(t) = parse_unix(iso) else {
        return String::new();
    };
    let secs = (now - t).max(0);
    if secs < 3_600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3_600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}
