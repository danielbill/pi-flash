//! Agent 定义（profile）目录读写 —— Settings-子代理页（044-1 §2/§3）的
//! pi-link 侧设施。
//!
//! 事实源 = `~/.pi/agent/agents/*.md`（用户资产惯例，非扩展私有数据）。本模块
//! 的 frontmatter 语义必须与扩展侧 `extensions/pf-subagents/src/profile.ts`
//! 的 `parseFrontmatter` **逐字对齐**——同一个 .md 两边读必须得到同一份字段，
//! 否则 Settings 页存的值与运行期扩展读到的值会静默分叉。
//!
//! 未知字段原样透传（防小白 + 不写死 schema 契约，044-1 §2.1）：解析时把
//! v1 已知字段之外的键值原序留在 [`AgentProfile::unknown`]，写回时拼回
//! frontmatter。

use std::path::{Path, PathBuf};

/// v1 已知字段（044-1 §3）。新增字段时同步扩展侧 `Profile` 与本 struct。
pub const FIELD_TOOLS: &str = "tools";
pub const FIELD_MAX_TURNS: &str = "maxTurns";
pub const FIELD_INJECT_AGENTS_MD: &str = "injectAgentsMd";
/// 思考强度（2026-11-07 子代理表单加）：键省略 = auto = 不覆盖（子会话落
/// pi 默认档）。值集 = pi `ThinkingLevel` 七档。
pub const FIELD_THINKING: &str = "thinking";
/// 工作目录（2026-11-08 子代理表单加）：键省略 = 继承委派者 cwd；
/// `pf-data` = PF 数据目录（扩展侧从 PI_FLASH_DIR 取；001-2 §5 env 白名单）。
pub const FIELD_CWD: &str = "cwd";
/// [`FIELD_CWD`] 唯一合法显式值。
pub const CWD_PF_DATA: &str = "pf-data";

/// 工具档位四选一（034 配方复用；无 `full+`，那是委派者专属）。
pub const TOOL_TIERS: [&str; 6] = [
    "inherit",
    "chat-only",
    "read-only",
    "default",
    "full",
    "full+",
];

/// 思考强度七档（pi `ThinkingLevel`，pi-agent-core types.d.ts）。
pub const THINKING_LEVELS: [&str; 7] = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];

/// 默认轮次预算：既非调用级 `maxloop` 也非 profile `maxTurns` 时用它。
pub const DEFAULT_MAX_TURNS: u32 = 100;

/// Agent 定义的来源分组。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentOrigin {
    /// 扩展随包出厂：只读，可「复制为自定义」。
    Builtin,
    /// 用户在 `~/.pi/agent/agents/` 自建：可建/改/删。
    Custom,
}

impl AgentOrigin {
    pub fn is_custom(self) -> bool {
        matches!(self, AgentOrigin::Custom)
    }
}

/// 最大轮次档（2026-11-08 定稿四档 + 2026-11-10 加 5 档 = 五档：
/// 5 | 30 | 100 | 不限 | 指定）。
///
/// 5 档是内置摘要机（summarizer）的固定预算——不给它单独造一套语义，
/// 面板上就是一个普通档位，用户选它写 `maxTurns: 5`，内置条目读出来也是
/// 这一档，不存在「文档说 5、面板显示指定」的双重语义。
///
/// `Custom` = 「指定」= 由主代理委派时给 `maxloop`，profile **不写**
/// maxTurns；档外显式数值（手写档）也归到 `Custom` 视图，原始值留在
/// [`AgentProfile::max_turns`] 透传保真，只有用户换档才归一。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxTurnsTier {
    Five,
    Thirty,
    Hundred,
    Unlimited,
    Custom,
}

/// 一份 Agent 定义（已解析 + 定位）。损坏的文件不丢弃：解析失败的错误
/// 落在 `error`，UI 据此渲染红条 + 「打开文件修复」（fail-closed 是运行时
/// 职责，PF 保证可修，044-1 §2.2）。
#[derive(Debug, Clone)]
pub struct AgentProfile {
    pub id: String,
    /// frontmatter 的 `description`（列表展示）。
    pub description: String,
    /// `model`：None = 键省略 = 继承会话模型。
    pub model: Option<String>,
    /// `tools` 档位：None = inherit。
    pub tools: Option<String>,
    /// `thinking` 思考强度：None = auto = 不覆盖（子会话落 pi 默认档）。
    pub thinking_level: Option<String>,
    /// `cwd` 工作目录：None = 继承委派者；`pf-data` = PF 数据目录。
    pub cwd: Option<String>,
    /// `maxTurns`：None = 键省略（默认 100 档）。
    pub max_turns: Option<u32>,
    /// `injectAgentsMd`：None = 键省略（默认 true）。
    pub inject_agents_md: Option<bool>,
    /// `systemPrompt`：替换式人格（schema 保留，UI 不暴露，044-1 §2.2）。
    pub system_prompt: Option<String>,
    /// `appendSystemPrompt`：追加式人格 = 文档正文（UI 系统提示框绑它）。
    pub append_system_prompt: Option<String>,
    /// v1 已知字段之外的原序键值（写回时透传）。
    pub unknown: Vec<(String, String)>,
    /// `extensions` 清单（full+ 档的 pi-ext 选择；空 = 不额外挂，044-0）。
    pub extensions: Vec<String>,
    /// `mcp` 清单（full+ 档的 mcp 服务选择；空 = 默认装配，044-0）。
    pub mcp_servers: Vec<String>,
    /// `skills` 清单（044-0 技能设置三态）：`None` = 键缺失 = 注入系统开启
    /// 集（继承）；`Some(空)` = **键存在但无条目 = 关闭**；`Some(清单)` =
    /// 自定义。键的存在性是语义的一部分，别用空 Vec 表示继承。
    pub skills: Option<Vec<String>>,
    pub origin: AgentOrigin,
    /// 文件路径；内置常量无文件时为 None。
    pub path: Option<PathBuf>,
    /// 解析失败的错误消息；Some = 损坏态。
    pub error: Option<String>,
}

impl AgentProfile {
    /// UI 最大轮次档位视图。
    pub fn max_turns_tier(&self) -> MaxTurnsTier {
        match self.max_turns {
            // 缺省 = 「指定」= 主代理委派时给 maxloop（2026-11-08 定稿）
            None => MaxTurnsTier::Custom,
            Some(0) => MaxTurnsTier::Unlimited,
            Some(5) => MaxTurnsTier::Five,
            Some(30) => MaxTurnsTier::Thirty,
            Some(100) => MaxTurnsTier::Hundred,
            Some(_) => MaxTurnsTier::Custom,
        }
    }

    /// UI 工具档位视图（None 归一到 inherit）。
    pub fn tools_tier(&self) -> &str {
        self.tools.as_deref().unwrap_or("inherit")
    }

    /// `injectAgentsMd` 的有效值（键省略即 true）。
    pub fn inject_enabled(&self) -> bool {
        self.inject_agents_md.unwrap_or(true)
    }

    /// 系统提示框的可编辑文本 = 追加式人格（正文）。
    pub fn prompt_text(&self) -> String {
        self.append_system_prompt.clone().unwrap_or_default()
    }

    /// 内置清单里同名条目是否被本自定义条目覆盖（044-1 §0 同名覆盖）。
    pub fn overrides_builtin(&self, builtins: &[AgentProfile]) -> bool {
        self.origin.is_custom() && builtins.iter().any(|b| b.id == self.id)
    }
}

mod builtin;
pub use builtin::builtin_agents;
mod module_config;
pub use module_config::{
    CAPACITY_UI_MAX, CAPACITY_UI_MIN, MODULE_CONFIG_FILENAME, capacity, ensure_module_config,
    module_config_path, read_capacity_at, set_capacity, write_capacity_at,
};

/// 扫一个目录里的 `*.md`，逐个解析（损坏的保留 + 记 error）。
/// 不含内置——内置由 [`builtin_agents`] 单独给，避免与自定义目录重复。
pub fn scan_agents_dir(dir: &Path) -> Vec<AgentProfile> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
        .collect();
    files.sort();
    files
        .iter()
        .filter_map(|p| {
            let id = p.file_stem()?.to_string_lossy().to_string();
            Some(read_agent_file(id, p))
        })
        .collect()
}

/// 读单个 Agent 文件并解析。文件读不到也算损坏（红条可修）。
pub fn read_agent_file(id: String, path: &Path) -> AgentProfile {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_agent(&id, &text, AgentOrigin::Custom).map_err(|e| (id.clone(), e)),
        Err(e) => Err((id.clone(), e.to_string())),
    }
    .map_or_else(
        |(_, error)| AgentProfile {
            id,
            description: String::new(),
            model: None,
            tools: None,
            thinking_level: None,
            cwd: None,
            max_turns: None,
            inject_agents_md: None,
            system_prompt: None,
            append_system_prompt: None,
            unknown: Vec::new(),
            extensions: Vec::new(),
            mcp_servers: Vec::new(),
            skills: None,
            origin: AgentOrigin::Custom,
            path: Some(path.to_path_buf()),
            error: Some(error),
        },
        |mut a| {
            a.path = Some(path.to_path_buf());
            a
        },
    )
}

/// 解析 Agent 文档（frontmatter + 正文）。语义对齐扩展侧
/// `profileFromDocument`。
pub fn parse_agent(id: &str, text: &str, origin: AgentOrigin) -> Result<AgentProfile, String> {
    let (data, body) = parse_frontmatter(text).ok_or("unterminated frontmatter")?;
    let get = |k: &str| {
        data.iter()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
    };

    let tools = get(FIELD_TOOLS);
    if let Some(t) = &tools {
        if !TOOL_TIERS.contains(&t.as_str()) {
            return Err(format!(
                "{FIELD_TOOLS} must be one of {} (got {t})",
                TOOL_TIERS.join("/")
            ));
        }
    }
    let cwd = get(FIELD_CWD);
    if let Some(v) = &cwd {
        if v != CWD_PF_DATA {
            return Err(format!("{FIELD_CWD} must be \"{CWD_PF_DATA}\" (got {v})"));
        }
    }
    let thinking = get(FIELD_THINKING);
    if let Some(lv) = &thinking {
        if !THINKING_LEVELS.contains(&lv.as_str()) {
            return Err(format!(
                "{FIELD_THINKING} must be one of {} (got {lv})",
                THINKING_LEVELS.join("/")
            ));
        }
    }
    if let Some(iso) = get("isolation") {
        if iso != "worktree" {
            return Err(format!("unsupported isolation {iso}"));
        }
    }
    let max_turns = match get(FIELD_MAX_TURNS) {
        None => None,
        Some(v) => v.parse::<u32>().map(Some).map_err(|_| {
            format!("{FIELD_MAX_TURNS} must be a non-negative integer (0 = unlimited)")
        })?,
    };
    let inject = match get(FIELD_INJECT_AGENTS_MD) {
        None => None,
        Some(v) if v == "true" => Some(true),
        Some(v) if v == "false" => Some(false),
        Some(_) => return Err(format!("{FIELD_INJECT_AGENTS_MD} must be true or false")),
    };

    // 未知字段原序透传（已知键剔除，正文单独走 append_system_prompt）
    let known = [
        "description",
        "model",
        FIELD_TOOLS,
        FIELD_THINKING,
        FIELD_MAX_TURNS,
        FIELD_INJECT_AGENTS_MD,
        "systemPrompt",
        FIELD_CWD,
        "inheritContextLimit",
        "isolation",
        "extensions",
        "mcp",
        "skills",
    ];
    let unknown = data
        .iter()
        .filter(|(k, _)| !known.contains(&k.as_str()))
        .cloned()
        .collect();
    // `- item` 清单字段（parse_frontmatter 把清单项拼进一个值，`\n-item` 分隔）
    let parse_list = |key: &str| -> Vec<String> {
        get(key)
            .map(|v| {
                v.split("\n-")
                    .map(|s| s.trim_start_matches('-').trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_default()
    };

    Ok(AgentProfile {
        id: id.to_string(),
        description: get("description").unwrap_or_default(),
        model: get("model").filter(|m| !m.is_empty()),
        tools,
        thinking_level: thinking.filter(|l| !l.is_empty()),
        cwd: cwd.filter(|c| !c.is_empty()),
        max_turns,
        inject_agents_md: inject,
        system_prompt: get("systemPrompt").filter(|s| !s.is_empty()),
        append_system_prompt: if body.is_empty() { None } else { Some(body) },
        unknown,
        extensions: parse_list("extensions"),
        mcp_servers: parse_list("mcp"),
        // 三态（044-0）：键缺失 = None = 继承；键存在空清单 = Some([]) = 关闭
        skills: get("skills").map(|_| parse_list("skills")),
        origin,
        path: None,
        error: None,
    })
}

/// 写回一个 Agent 定义为 `*.md`（未知字段透传 + 多行值走 `|` 块标量）。
pub fn serialize_agent(a: &AgentProfile) -> String {
    let mut out = String::from("---\n");
    let scalar = |k: &str, v: &str| scalar_line(k, v);
    if !a.description.is_empty() {
        out.push_str(&scalar("description", &a.description));
    }
    if let Some(m) = &a.model {
        out.push_str(&scalar("model", m));
    }
    if let Some(t) = &a.tools {
        out.push_str(&scalar(FIELD_TOOLS, t));
    }
    if let Some(lv) = &a.thinking_level {
        out.push_str(&scalar(FIELD_THINKING, lv));
    }
    if let Some(c) = &a.cwd {
        out.push_str(&scalar(FIELD_CWD, c));
    }
    if let Some(n) = a.max_turns {
        out.push_str(&format!("{FIELD_MAX_TURNS}: {n}\n"));
    }
    if let Some(i) = a.inject_agents_md {
        out.push_str(&format!("{FIELD_INJECT_AGENTS_MD}: {i}\n"));
    }
    if let Some(sp) = &a.system_prompt {
        out.push_str(&scalar("systemPrompt", sp));
    }
    // `- item` 清单（044-0：full+ 的 extensions/mcp + skills 定制）
    let mut list = |k: &str, items: &[String]| {
        if items.is_empty() {
            return;
        }
        out.push_str(&format!("{k}:\n"));
        for item in items {
            out.push_str(&format!("- {item}\n"));
        }
    };
    list("extensions", &a.extensions);
    list("mcp", &a.mcp_servers);
    // skills 三态（044-0）：None 不写键（继承）；Some(空) 写裸键（关闭）
    match &a.skills {
        None => {}
        Some(items) => {
            out.push_str("skills:\n");
            for item in items {
                out.push_str(&format!("- {item}\n"));
            }
        }
    }
    for (k, v) in &a.unknown {
        out.push_str(&scalar(k, v));
    }
    out.push_str("---\n");
    if let Some(body) = &a.append_system_prompt {
        out.push('\n');
        out.push_str(body);
        out.push('\n');
    }
    out
}

/// 单条 frontmatter 行：多行值走 `|` 块标量（每行缩进 2），单行值原样。
fn scalar_line(key: &str, value: &str) -> String {
    let mut s = String::new();
    if value.contains('\n') {
        s.push_str(&format!("{key}: |\n"));
        for line in value.lines() {
            s.push_str("  ");
            s.push_str(line);
            s.push('\n');
        }
    } else {
        s.push_str(&format!("{key}: {value}\n"));
    }
    s
}

/// 平铺 YAML 子集解析（对齐扩展侧）：标量、`|` 块标量、引号标量、`- item` 列表。
/// 返回（键值对, 正文）；正文 = 闭合 `---` 之后去掉前导空行并 trim。
pub fn parse_frontmatter(text: &str) -> Option<(Vec<(String, String)>, String)> {
    let normalized = text.replace("\r\n", "\n");
    if !normalized.starts_with("---\n") {
        return Some((Vec::new(), normalized.trim().to_string()));
    }
    let end = normalized[4..].find("\n---")? + 4;
    let block = &normalized[4..end];
    let body = normalized[end + 4..]
        .strip_prefix('\n')
        .unwrap_or(&normalized[end + 4..]);
    let body = body.trim().to_string();

    let mut out: Vec<(String, String)> = Vec::new();
    let mut list_key: Option<String> = None;
    let lines: Vec<&str> = block.split('\n').collect();
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        if raw.trim().is_empty() || raw.trim().starts_with('#') {
            i += 1;
            continue;
        }
        // `- item` 列表项
        if let Some(item) = list_item(raw) {
            if let Some(k) = &list_key {
                if let Some(slot) = out.iter_mut().find(|(key, _)| key == k) {
                    slot.1.push_str(&format!("\n-{}", item));
                } else {
                    out.push((k.clone(), format!("-{item}")));
                }
                i += 1;
                continue;
            }
        }
        let Some((key, value)) = split_pair(raw) else {
            i += 1;
            continue;
        };
        list_key = None;
        if matches!(value, "|" | "|-" | "|+") {
            // 块标量：吃掉后续所有缩进行，按首行缩进反缩进
            let mut collected: Vec<String> = Vec::new();
            let mut indent: Option<usize> = None;
            while i + 1 < lines.len() {
                let next = lines[i + 1];
                if next.trim().is_empty() {
                    collected.push(String::new());
                    i += 1;
                    continue;
                }
                let leading = next.len() - next.trim_start().len();
                if leading == 0 {
                    break;
                }
                let base = *indent.get_or_insert(leading);
                collected.push(next[leading.min(base)..].to_string());
                i += 1;
            }
            let mut joined = collected.join("\n");
            while joined.ends_with('\n') {
                joined.pop();
            }
            out.push((key, joined));
            i += 1;
            continue;
        }
        if value.is_empty() {
            list_key = Some(key.clone());
            out.push((key, String::new()));
            i += 1;
            continue;
        }
        out.push((key, unquote(value)));
        i += 1;
    }
    Some((out, body))
}

fn list_item(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    let rest = trimmed.strip_prefix("- ")?;
    Some(rest.trim().to_string())
}

fn split_pair(line: &str) -> Option<(String, &str)> {
    let colon = line.find(':')?;
    let key = line[..colon].trim();
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return None;
    }
    Some((key.to_string(), line[colon + 1..].trim()))
}

fn unquote(value: &str) -> String {
    let bytes = value.as_bytes();
    if value.len() >= 2
        && ((bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\''))
    {
        return value[1..value.len() - 1].to_string();
    }
    value.to_string()
}

/// 全局 agents 目录：**PF 自有数据目录**（`<PF数据目录>/agents`，dev =
/// `./pi-flash-dev/agents`、release = `~/.pi-flash/agents`）。
///
/// 2026-11-06 迁出 `~/.pi/agent/agents`：pi 扩展形态曾跟随 pi 生态约定放
/// pi 全局配置根，但 profile frontmatter schema 是 pf-subagents 私有的，
/// 与 pi 市场社区 subagent 扩展共用目录 = 同一批 `.md` 被两套 schema 各自
/// 解释（真机实况：explorer.md 被双向消费、目录里还躺着别家的
/// settings.json）——命名空间污染，迁回 PF 自治。
pub fn agents_dir() -> PathBuf {
    match crate::paths::dir() {
        Some(d) => d.join("agents"),
        None => {
            let home = std::env::var("USERPROFILE")
                .or_else(|_| std::env::var("HOME"))
                .unwrap_or_else(|_| ".".into());
            Path::new(&home).join(".pi-flash").join("agents")
        }
    }
}

/// 旧全局目录（pi 生态约定，2026-11-06 起弃用）。
pub fn legacy_agents_dir() -> PathBuf {
    crate::config::agent_dir().join("agents")
}

/// 一次性迁移：旧目录的 `*.md` 复制进新目录（**不覆盖已存在**——同名冲突
/// 以新目录为准；别家工具还要用旧目录，copy 而非 move）。幂等：无 .md 或
/// 已迁过 = no-op。
pub fn migrate_legacy_agents() {
    migrate_agents_from(&legacy_agents_dir(), &agents_dir());
}

/// 迁移核心（路径解耦，纯函数可测）：from 目录的 `*.md` copy 进 to（不覆盖）。
pub fn migrate_agents_from(from: &Path, to: &Path) {
    let Ok(entries) = std::fs::read_dir(from) else {
        return;
    };
    let _ = std::fs::create_dir_all(to);
    for entry in entries.flatten() {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Some(name) = p.file_name() else { continue };
        let dst = to.join(&name);
        if !dst.exists() {
            let _ = std::fs::copy(&p, &dst);
        }
    }
}

/// 全局目录 + 内置（自定义 > 内置同名覆盖；内置只留未被覆盖的）。
pub fn list_all_agents() -> Vec<AgentProfile> {
    let custom = scan_agents_dir(&agents_dir());
    let mut builtins: Vec<AgentProfile> = builtin_agents()
        .into_iter()
        .filter(|b| !custom.iter().any(|c| c.id == b.id))
        .collect();
    builtins.extend(custom);
    builtins
}

#[cfg(test)]
mod tests;
