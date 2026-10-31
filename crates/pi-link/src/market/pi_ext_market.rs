//! Pi 扩展市场数据面（040-1-Pi的扩展市场.md）：数据源 = npm registry。
//!
//! 为什么不抓 https://pi.dev/packages：站点没有数据接口
//! （`/api/packages` 返回 `API routes are reserved for future features.`，
//! `/packages.json` 404），只有服务端渲染 HTML。npm 搜索接口是站点自身的数据源，
//! 字段够用：name / description / version / date / 月下载量 / repo。
//!
//! 两段式：①搜索（1 次，50 条）②每个包的 `latest` 清单（`pi` 键 → 资源类型）。
//! 类型是「只展示扩展」过滤的唯一依据，所以清单必须补；但它是延迟型请求
//! （单包 ~0.4s），故按 10 条一批并发 8 补全（见 [`fill_kinds`]），调用方
//! 每批判完就上屏，不攒满一页。
//!
//! 缓存只有一份首页快照（[`HomeSnapshot`]）：单文件、≤50 条、<100KB，
//! 启动延迟加载（app 侧 `market::prefetch`）只预取首页写它。

use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// 一页条数（npm 搜索 size 上限内的常用档）。
pub const PAGE_SIZE: usize = 50;
/// 搜索必须带的关键词：npm 上是 `keywords` 字段而非全文。
pub const KIND_KEYWORD: &str = "keywords:pi-package";
/// 补全清单时的并发（8→16 实测只省 0.4s，不值）。
const FILL_CONCURRENCY: usize = 8;

/// 包内资源类型（`package.json` 的 `pi` 键）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarketKind {
    Extension,
    Skill,
    Prompt,
    Theme,
    Subagent,
}

impl MarketKind {
    /// pi 清单里的键名（同时也是规范顺序）。
    const ALL: [(&'static str, MarketKind); 5] = [
        ("extensions", MarketKind::Extension),
        ("skills", MarketKind::Skill),
        ("prompts", MarketKind::Prompt),
        ("themes", MarketKind::Theme),
        ("subagents", MarketKind::Subagent),
    ];

    /// 清单键名（单数形式，`extensions` → `extension`）：040-2 pi.dev 目录页的
    /// 类型徽标 / 类型筛选按这个口径对齐 npm 侧 `kinds`。
    pub fn key(self) -> &'static str {
        match self {
            MarketKind::Extension => "extension",
            MarketKind::Skill => "skill",
            MarketKind::Prompt => "prompt",
            MarketKind::Theme => "theme",
            MarketKind::Subagent => "subagent",
        }
    }

    /// i18n 键（app 侧 `tr()` 直接吃）。
    pub fn i18n_key(self) -> &'static str {
        match self {
            MarketKind::Extension => "market.type.extension",
            MarketKind::Skill => "market.type.skill",
            MarketKind::Prompt => "market.type.prompt",
            MarketKind::Theme => "market.type.theme",
            MarketKind::Subagent => "market.type.subagent",
        }
    }
}

/// 目录条目。`kinds` 空 = 清单未补全/包内没有 `pi` 键（渲染放行并标 `—`，
/// 不因为网络抖动把整页清空）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MarketEntry {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub monthly_downloads: u64,
    #[serde(default)]
    pub updated: String,
    #[serde(default)]
    pub kinds: Vec<MarketKind>,
    #[serde(default)]
    pub repo: Option<String>,
    /// 发布者（npm `publisher.username`）；未知 = None
    #[serde(default)]
    pub author: Option<String>,
}

impl MarketEntry {
    /// 「只展示扩展」的唯一口径（040-1 需求）：清单明确不是扩展才隐藏。
    pub fn is_extension(&self) -> bool {
        self.kinds.is_empty() || self.kinds.contains(&MarketKind::Extension)
    }

    /// 行内展示的安装命令（与 pi.dev 站点文案一致，可直接复制粘贴）。
    pub fn install_command(&self) -> String {
        format!("pi install npm:{}", self.name)
    }

    /// settings.json 里的来源写法（安装/卸载/更新的入参）。
    pub fn source(&self) -> String {
        format!("npm:{}", self.name)
    }
}

/// 一页搜索结果。
#[derive(Debug, Clone, Default)]
pub struct MarketPage {
    pub entries: Vec<MarketEntry>,
    pub total: usize,
}

// ---------------------------------------------------------------------------
// URL 拼装（纯函数，单测主战场）
// ---------------------------------------------------------------------------

/// percent-encode：保留 RFC3986 unreserved + `@`（npm scope 前缀），其余全编。
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'@' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// npm 搜索端点。空 query = 纯关键词（按下载量权重排序）。
pub fn search_url(query: &str, from: usize, size: usize) -> String {
    let q = query.trim();
    let text = if q.is_empty() {
        KIND_KEYWORD.to_string()
    } else {
        format!("{KIND_KEYWORD} {q}")
    };
    format!(
        "{}/-/v1/search?text={}&size={size}&from={from}&popularity=1&quality=0&maintenance=0",
        crate::market::registry_base(),
        encode(&text)
    )
}

/// 单包 latest 清单端点（scoped 名的 `/` 编成 `%2F`）。
pub fn latest_url(name: &str) -> String {
    format!("{}/{}/latest", crate::market::registry_base(), encode(name))
}

/// 下载量展示：1.8M / 792.8K / 3.4K / 999。
pub fn format_downloads(n: u64) -> String {
    fn trim(s: String) -> String {
        s.replace(".0M", "M").replace(".0K", "K")
    }
    if n >= 1_000_000 {
        trim(format!("{:.1}M", n as f64 / 1_000_000.))
    } else if n >= 1_000 {
        trim(format!("{:.1}K", n as f64 / 1_000.))
    } else {
        n.to_string()
    }
}

/// 某个 npm 时间戳（ISO 8601）距 `now_unix` 的天数（0 = 今天，负数 = 未来）。
/// 只取日期部分；解析失败 → None。
pub fn days_since(iso: &str, now_unix: i64) -> Option<i64> {
    let date = iso.get(0..10)?;
    let y = date.get(0..4)?.parse::<i64>().ok()?;
    let m = date.get(5..7)?.parse::<i64>().ok()?;
    let d = date.get(8..10)?.parse::<i64>().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(now_unix.div_euclid(86_400) - days_from_civil(y, m, d))
}

/// 公历 → 自 1970-01-01 起的天数（Howard Hinnant 算法，省一个 chrono 依赖）。
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

// ---------------------------------------------------------------------------
// 解析（纯函数）
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SearchResp {
    #[serde(default)]
    objects: Vec<SearchObj>,
    #[serde(default)]
    total: u64,
}

#[derive(Deserialize)]
struct SearchObj {
    #[serde(default)]
    package: SearchPkg,
    #[serde(default)]
    downloads: SearchDownloads,
}

#[derive(Default, Deserialize)]
struct SearchPkg {
    #[serde(default)]
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    version: String,
    #[serde(default)]
    date: String,
    #[serde(default)]
    links: SearchLinks,
    #[serde(default)]
    publisher: Option<SearchPublisher>,
}

#[derive(Default, Deserialize)]
struct SearchDownloads {
    #[serde(default)]
    monthly: u64,
}

#[derive(Default, Deserialize)]
struct SearchPublisher {
    #[serde(default)]
    username: Option<String>,
}

#[derive(Default, Deserialize)]
struct SearchLinks {
    #[serde(default)]
    repository: Option<String>,
}

/// 解析 npm 搜索响应。缺字段容错（description/downloads 都可能没有）。
pub fn parse_search_page(body: &str) -> Result<MarketPage, String> {
    let resp: SearchResp = serde_json::from_str(body).map_err(|e| format!("parse: {e}"))?;
    let entries = resp
        .objects
        .into_iter()
        .filter(|o| !o.package.name.trim().is_empty())
        .map(|o| MarketEntry {
            name: o.package.name,
            description: o.package.description.unwrap_or_default().trim().to_string(),
            version: o.package.version,
            monthly_downloads: o.downloads.monthly,
            updated: o.package.date,
            kinds: Vec::new(),
            repo: o.package.links.repository.map(|r| {
                r.trim_start_matches("git+")
                    .trim_end_matches(".git")
                    .to_string()
            }),
            author: o
                .package
                .publisher
                .and_then(|p| p.username)
                .map(|u| u.trim().to_string())
                .filter(|u| !u.is_empty()),
        })
        .collect();
    Ok(MarketPage {
        entries,
        total: resp.total as usize,
    })
}

/// 解析单包 latest 清单 → 资源类型（规范顺序）。无 `pi` 键 = 空表。
pub fn parse_latest_manifest(body: &str) -> Result<Vec<MarketKind>, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| format!("parse: {e}"))?;
    let Some(pi) = v.get("pi").and_then(|p| p.as_object()) else {
        return Ok(Vec::new());
    };
    let mut kinds = Vec::new();
    for (key, kind) in MarketKind::ALL {
        // 键存在且非 null 才算声明（`"skills": []` 也算「不加载技能」之外的空声明，
        // 但 `null`/缺失一律不算）
        if pi.get(key).is_some_and(|x| !x.is_null()) {
            kinds.push(kind);
        }
    }
    Ok(kinds)
}

// ---------------------------------------------------------------------------
// 网络（阻塞，调用方在后台线程）
// ---------------------------------------------------------------------------

/// 搜索一页（空 query = 按下载量取首页）。
pub fn search(query: &str, from: usize, size: usize) -> Result<MarketPage, String> {
    let body = crate::market::http_get_json(&search_url(query, from, size))?;
    parse_search_page(&body)
}

/// npm 搜索的 size 上限（服务端钳制，超了也只会回 250）。
pub const SEARCH_SIZE_MAX: usize = 250;

/// 「最近发布」一栏的数据面（040-2）：npm 搜索**没有排序参数**（`sort=` 被忽略，
/// 实测 `sort=updated` 与默认同序），所以只能取一页上限 250 条、本地按发布时间
/// 倒序后截前 N 条。语义 = 「按相关度取到的这一批里最近发布的」，不是全库最新
/// ——页面文案按这个口径写，不装懂。
pub fn search_recent(take: usize) -> Result<Vec<MarketEntry>, String> {
    let mut page = search("", 0, SEARCH_SIZE_MAX)?;
    page.entries.sort_by(|a, b| b.updated.cmp(&a.updated));
    page.entries.truncate(take);
    Ok(page.entries)
}

/// 单包资源类型。失败 = `Err`（调用方**不写缓存**，下次自然重试）。
pub fn fetch_kinds(name: &str) -> Result<Vec<MarketKind>, String> {
    let body = crate::market::http_get_json(&latest_url(name))?;
    parse_latest_manifest(&body)
}

/// 给一批条目补 `kinds`：命中 `kinds` 表则跳过网络；批内并发 8。
/// 失败（网络/解析）不写表、条目保留空 kinds（→ 渲染放行）。
pub fn fill_kinds(entries: &mut [MarketEntry], kinds: &mut HashMap<String, Vec<MarketKind>>) {
    let need: Vec<String> = entries
        .iter()
        .filter(|e| !kinds.contains_key(&e.name))
        .map(|e| e.name.clone())
        .collect();
    for chunk in need.chunks(FILL_CONCURRENCY) {
        let got: Vec<(String, Option<Vec<MarketKind>>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|name| {
                    let name = name.clone();
                    scope.spawn(move || {
                        let k = fetch_kinds(&name).ok();
                        (name, k)
                    })
                })
                .collect();
            handles.into_iter().filter_map(|h| h.join().ok()).collect()
        });
        for (name, k) in got {
            if let Some(k) = k {
                kinds.insert(name, k);
            }
        }
    }
    for e in entries.iter_mut() {
        if let Some(k) = kinds.get(&e.name) {
            e.kinds = k.clone();
        }
    }
}

// ---------------------------------------------------------------------------
// 首页快照（唯一的磁盘缓存）
// ---------------------------------------------------------------------------

/// 快照最多存多少条（= 一页）。
pub const SNAPSHOT_MAX: usize = 50;

/// 首页快照：`{query, entries, fetched_at}`。进页面先渲染它（0 网络），
/// 后台刷新完就地替换。单文件 <100KB。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HomeSnapshot {
    pub query: String,
    pub entries: Vec<MarketEntry>,
    pub fetched_at: i64,
}

impl HomeSnapshot {
    /// 从 `<PF数据目录>/pi-ext-market-cache.json` 读（坏文件/缺字段 → None）。
    pub fn load() -> Option<Self> {
        Self::load_at(&crate::paths::pi_ext_market_cache_file()?)
    }

    pub fn load_at(path: &Path) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        let snap: HomeSnapshot = serde_json::from_str(&text).ok()?;
        (!snap.entries.is_empty()).then_some(snap)
    }

    /// 覆盖写（原子：tmp + rename；失败静默——缓存不值得打扰用户）。
    pub fn save(&self) {
        if let Some(path) = crate::paths::pi_ext_market_cache_file() {
            let _ = self.save_at(&path);
        }
    }

    pub fn save_at(&self, path: &Path) -> std::io::Result<()> {
        let mut snap = self.clone();
        snap.entries.truncate(SNAPSHOT_MAX);
        let text = serde_json::to_string(&snap)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }

    /// 快照年龄（秒）。
    pub fn age_secs(&self) -> i64 {
        now_secs() - self.fetched_at
    }

    /// 用给定条目建快照（`fetched_at` = 现在）。
    pub fn new_now(query: &str, entries: Vec<MarketEntry>) -> Self {
        Self {
            query: query.to_string(),
            entries,
            fetched_at: now_secs(),
        }
    }
}

/// 会话外 kinds 缓存（090 重构补）：`<数据目录>/pi-native-kinds-cache.json`，
/// name → 包内资源类型。原生市场每次重查都要重补 50 个 `latest` 清单
/// （~15-20s），有这份缓存后命中即零网络——选类型立即出结果。
/// 单条 ~100B；超 [`KINDS_CACHE_MAX`] 整体清空重学（兜底，不设 LRU）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KindsCache {
    pub kinds: HashMap<String, Vec<MarketKind>>,
}

/// 缓存条目上限（13,084 包全量也才 ~2MB；正常使用到不了）。
const KINDS_CACHE_MAX: usize = 5000;

impl KindsCache {
    /// 读（坏文件/缺失 → 空表）。
    pub fn load() -> HashMap<String, Vec<MarketKind>> {
        match crate::paths::pi_native_kinds_cache_file() {
            Some(path) => Self::load_at(&path).unwrap_or_default(),
            None => HashMap::new(),
        }
    }

    fn load_at(path: &Path) -> Option<HashMap<String, Vec<MarketKind>>> {
        let text = std::fs::read_to_string(path).ok()?;
        let c: KindsCache = serde_json::from_str(&text).ok()?;
        Some(c.kinds)
    }

    /// 覆盖写（原子：tmp + rename；失败静默——缓存不值得打扰用户）。
    pub fn save(kinds: &HashMap<String, Vec<MarketKind>>) {
        if let Some(path) = crate::paths::pi_native_kinds_cache_file() {
            let _ = Self::save_at(kinds, &path);
        }
    }

    fn save_at(kinds: &HashMap<String, Vec<MarketKind>>, path: &Path) -> std::io::Result<()> {
        let mut map = kinds.clone();
        if map.len() > KINDS_CACHE_MAX {
            map.clear();
        }
        let text = serde_json::to_string(&KindsCache { kinds: map })?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, path)
    }
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH_FIXTURE: &str = r#"{
      "objects": [
        {"downloads":{"monthly":1802478},"updated":"2026-10-19T22:33:01.605Z",
         "package":{"name":"billion-context","description":"ctx compression","version":"0.1.202",
                    "date":"2026-10-19T09:50:11.928Z",
                    "links":{"repository":"git+https://github.com/ranxianglei/billion-context.git"}}},
        {"package":{"name":"@scope/pi-tool","version":"2.0.0"}}
      ],
      "total": 12364
    }"#;

    #[test]
    fn search_url_encodes_and_weights_popularity() {
        let u = search_url("web search", 50, 50);
        assert!(u.contains("/-/v1/search?text="), "{u}");
        assert!(u.contains("keywords%3Api-package%20web%20search"), "{u}");
        assert!(u.contains("size=50") && u.contains("from=50"), "{u}");
        assert!(u.contains("popularity=1&quality=0&maintenance=0"), "{u}");
        // 空词只带关键词
        let u = search_url("   ", 0, 50);
        assert!(u.contains("text=keywords%3Api-package&"), "{u}");
    }

    #[test]
    fn latest_url_escapes_scope_slash() {
        assert_eq!(
            latest_url("@scope/pkg"),
            format!("{}/@scope%2Fpkg/latest", crate::market::registry_base())
        );
        assert!(latest_url("plain").ends_with("/plain/latest"));
    }

    #[test]
    fn parse_search_page_tolerates_missing_fields() {
        let page = parse_search_page(SEARCH_FIXTURE).unwrap();
        assert_eq!(page.total, 12364);
        assert_eq!(page.entries.len(), 2);
        assert_eq!(page.entries[0].monthly_downloads, 1_802_478);
        assert_eq!(
            page.entries[0].repo.as_deref(),
            Some("https://github.com/ranxianglei/billion-context")
        );
        assert!(page.entries[1].description.is_empty());
        assert_eq!(page.entries[1].monthly_downloads, 0);
        assert_eq!(page.entries[1].name, "@scope/pi-tool");
        assert!(parse_search_page("not json").is_err());
    }

    #[test]
    fn parse_latest_manifest_maps_pi_keys() {
        let body = r#"{"name":"x","pi":{"extensions":["./dist"],"skills":["./s"],
                        "prompts":null,"themes":[]}}"#;
        assert_eq!(
            parse_latest_manifest(body).unwrap(),
            vec![MarketKind::Extension, MarketKind::Skill, MarketKind::Theme]
        );
        // 无 pi 键 = 空表（不是错误）
        assert!(parse_latest_manifest(r#"{"name":"x"}"#).unwrap().is_empty());
        assert!(parse_latest_manifest("{bad").is_err());
    }

    #[test]
    fn is_extension_only_hides_known_non_extensions() {
        let mut e = MarketEntry {
            name: "a".into(),
            ..Default::default()
        };
        // 清单未补全 → 放行
        assert!(e.is_extension());
        e.kinds = vec![MarketKind::Skill];
        assert!(!e.is_extension());
        e.kinds = vec![MarketKind::Extension, MarketKind::Skill];
        assert!(e.is_extension());
    }

    #[test]
    fn parse_author_and_days_since() {
        let body = r#"{"objects":[{"package":{"name":"x","publisher":{"username":"nicopreme"}}}]}"#;
        let page = parse_search_page(body).unwrap();
        assert_eq!(page.entries[0].author.as_deref(), Some("nicopreme"));
        // 2026-10-20T12:00:00Z 的秒数 = 1792497600
        assert_eq!(
            days_since("2026-10-19T22:33:01.605Z", 1_792_497_600),
            Some(1)
        );
        assert_eq!(days_since("2026-10-20T00:10:00Z", 1_792_497_600), Some(0));
        assert_eq!(days_since("1970-01-01", 0), Some(0));
        assert_eq!(days_since("", 0), None);
        assert_eq!(days_since("garbage", 0), None);
    }

    #[test]
    fn install_command_and_source() {
        let e = MarketEntry {
            name: "@s/p".into(),
            ..Default::default()
        };
        assert_eq!(e.install_command(), "pi install npm:@s/p");
        assert_eq!(e.source(), "npm:@s/p");
    }

    #[test]
    fn format_downloads_scales() {
        assert_eq!(format_downloads(999), "999");
        assert_eq!(format_downloads(3_400), "3.4K");
        assert_eq!(format_downloads(792_800), "792.8K");
        assert_eq!(format_downloads(1_800_000), "1.8M");
        assert_eq!(format_downloads(1_000_000), "1M");
    }

    #[test]
    fn home_snapshot_roundtrip_and_bad_file() {
        let dir = std::env::temp_dir().join(format!("pf-mkt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("snap.json");

        // 坏文件 → None（不 panic）
        std::fs::write(&path, "{oops").unwrap();
        assert!(HomeSnapshot::load_at(&path).is_none());

        let snap = HomeSnapshot::new_now(
            "web",
            vec![MarketEntry {
                name: "pi-web-access".into(),
                kinds: vec![MarketKind::Extension],
                ..Default::default()
            }],
        );
        snap.save_at(&path).unwrap();
        let back = HomeSnapshot::load_at(&path).unwrap();
        assert_eq!(back.query, "web");
        assert_eq!(back.entries.len(), 1);
        assert_eq!(back.entries[0].kinds, vec![MarketKind::Extension]);
        assert!(back.age_secs().abs() < 5);

        // 上限截断
        let many = HomeSnapshot::new_now("q", vec![MarketEntry::default(); 80]);
        many.save_at(&path).unwrap();
        assert_eq!(
            HomeSnapshot::load_at(&path).unwrap().entries.len(),
            SNAPSHOT_MAX
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
