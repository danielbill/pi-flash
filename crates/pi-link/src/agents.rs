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

/// 工具档位四选一（034 配方复用；无 `full+`，那是委派者专属）。
pub const TOOL_TIERS: [&str; 4] = ["inherit", "read-only", "default", "full"];

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

/// 最大轮次档（UI 四档，044-1 §2.2）。`Custom` 携带用户指定的 N。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaxTurnsTier {
    Fifty,
    Hundred,
    Unlimited,
    Custom(u32),
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
            None => MaxTurnsTier::Hundred,
            Some(0) => MaxTurnsTier::Unlimited,
            Some(50) => MaxTurnsTier::Fifty,
            Some(n) => MaxTurnsTier::Custom(n),
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

/// 内置 Agent 定义（随扩展出厂，只读）。id 与内容须与扩展侧内置清单
/// （`extensions/pf-subagents/src/profile.ts` `BUILTIN_PROFILE_DOCS`）逐字
/// 一致——两侧各存一份，改动时同步。
/// 2026-10-19 拍板：dsh 无出厂内置，对齐 ZCode 两内置（general-purpose +
/// Explore，explorer 即 Explore 位）。
pub fn builtin_agents() -> Vec<AgentProfile> {
    vec![
        AgentProfile {
            id: "explorer".into(),
            description: "只读探索 · 调研与定位".into(),
            model: None,
            tools: Some("read-only".into()),
            max_turns: Some(0),
            inject_agents_md: Some(true),
            system_prompt: None,
            append_system_prompt: Some(
                "你是只读探索员：调研代码、定位实现、回答问题。\n不执行任何写操作。".into(),
            ),
            unknown: Vec::new(),
            origin: AgentOrigin::Builtin,
            path: None,
            error: None,
        },
        // 不写 `tools:` = inherit = 继承委派者全部工具（ZCode「全部工具」位）
        AgentProfile {
            id: "general-purpose".into(),
            description: "通用子代理 · 复杂调研、代码检索、多步任务".into(),
            model: None,
            tools: None,
            max_turns: Some(0),
            inject_agents_md: Some(true),
            system_prompt: None,
            append_system_prompt: Some(
                "你是通用子代理：独立完成委派的任务——调研复杂问题、检索代码、执行多步操作。\n可使用会话的全部工具，按需读写文件与执行命令。".into(),
            ),
            unknown: Vec::new(),
            origin: AgentOrigin::Builtin,
            path: None,
            error: None,
        },
    ]
}

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
            max_turns: None,
            inject_agents_md: None,
            system_prompt: None,
            append_system_prompt: None,
            unknown: Vec::new(),
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
        FIELD_MAX_TURNS,
        FIELD_INJECT_AGENTS_MD,
        "systemPrompt",
        "inheritContextLimit",
        "isolation",
    ];
    let unknown = data
        .iter()
        .filter(|(k, _)| !known.contains(&k.as_str()))
        .cloned()
        .collect();

    Ok(AgentProfile {
        id: id.to_string(),
        description: get("description").unwrap_or_default(),
        model: get("model").filter(|m| !m.is_empty()),
        tools,
        max_turns,
        inject_agents_md: inject,
        system_prompt: get("systemPrompt").filter(|s| !s.is_empty()),
        append_system_prompt: if body.is_empty() { None } else { Some(body) },
        unknown,
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
    if let Some(n) = a.max_turns {
        out.push_str(&format!("{FIELD_MAX_TURNS}: {n}\n"));
    }
    if let Some(i) = a.inject_agents_md {
        out.push_str(&format!("{FIELD_INJECT_AGENTS_MD}: {i}\n"));
    }
    if let Some(sp) = &a.system_prompt {
        out.push_str(&scalar("systemPrompt", sp));
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

/// 全局 agents 目录（`~/.pi/agent/agents`）。
pub fn agents_dir() -> PathBuf {
    crate::config::agent_dir().join("agents")
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
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("agents-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn parses_v1_fields_and_ignores_unknown() {
        let doc = "---\ndescription: lab\nmodel: deepseek/deepseek-chat\ntools: read-only\nmaxTurns: 42\ninjectAgentsMd: false\nfutureField: whatever\n---\nappend body\n";
        let a = parse_agent("reviewer", doc, AgentOrigin::Custom).unwrap();
        assert_eq!(a.description, "lab");
        assert_eq!(a.model.as_deref(), Some("deepseek/deepseek-chat"));
        assert_eq!(a.tools_tier(), "read-only");
        assert_eq!(a.max_turns, Some(42));
        assert_eq!(a.max_turns_tier(), MaxTurnsTier::Custom(42));
        assert!(!a.inject_enabled());
        assert_eq!(a.append_system_prompt.as_deref(), Some("append body"));
        // 未知字段透传
        assert!(
            a.unknown
                .iter()
                .any(|(k, v)| k == "futureField" && v == "whatever")
        );
    }

    #[test]
    fn reads_systemprompt_block_scalar() {
        let doc = "---\ndescription: strict\nsystemPrompt: |\n  You are strict.\n  Never edit.\n---\nextra notes\n";
        let a = parse_agent("strict", doc, AgentOrigin::Custom).unwrap();
        assert_eq!(
            a.system_prompt.as_deref(),
            Some("You are strict.\nNever edit.")
        );
        assert_eq!(a.append_system_prompt.as_deref(), Some("extra notes"));
    }

    #[test]
    fn max_turns_tiers_and_validation() {
        let u = parse_agent("u", "---\nmaxTurns: 0\n---\nx\n", AgentOrigin::Custom).unwrap();
        assert_eq!(u.max_turns_tier(), MaxTurnsTier::Unlimited);
        let d = parse_agent("d", "---\ndescription: d\n---\n", AgentOrigin::Custom).unwrap();
        assert_eq!(d.max_turns_tier(), MaxTurnsTier::Hundred);
        assert!(parse_agent("n", "---\nmaxTurns: -1\n---\nx\n", AgentOrigin::Custom).is_err());
        assert!(parse_agent("i", "---\nmaxTurns: abc\n---\nx\n", AgentOrigin::Custom).is_err());
    }

    #[test]
    fn rejects_bad_tools_and_inject() {
        assert!(parse_agent("p", "---\ntools: read\n---\nx\n", AgentOrigin::Custom).is_err());
        assert!(parse_agent("p", "---\ntools:\n  - read\n---\nx\n", AgentOrigin::Custom).is_err());
        assert!(
            parse_agent(
                "p",
                "---\ninjectAgentsMd: yes\n---\nx\n",
                AgentOrigin::Custom
            )
            .is_err()
        );
        assert!(parse_agent("p", "---\nisolation: docker\n---\nx\n", AgentOrigin::Custom).is_err());
    }

    #[test]
    fn roundtrip_preserves_fields_and_unknown() {
        let doc = "---\ndescription: 评审\nmodel: a/b\ntools: full\nmaxTurns: 50\ninjectAgentsMd: true\nsystemPrompt: |\n  line1\n  line2\nextra: keep\n---\n\nbody line\n";
        let a = parse_agent("rt", doc, AgentOrigin::Custom).unwrap();
        let out = serialize_agent(&a);
        let b = parse_agent("rt", &out, AgentOrigin::Custom).unwrap();
        assert_eq!(b.description, "评审");
        assert_eq!(b.model.as_deref(), Some("a/b"));
        assert_eq!(b.tools_tier(), "full");
        assert_eq!(b.max_turns, Some(50));
        assert!(b.inject_enabled());
        assert_eq!(b.system_prompt.as_deref(), Some("line1\nline2"));
        assert_eq!(b.append_system_prompt.as_deref(), Some("body line"));
        assert!(b.unknown.iter().any(|(k, v)| k == "extra" && v == "keep"));
    }

    #[test]
    fn scan_marks_corrupt_and_builtin_override() {
        let d = tmp_dir("scan");
        std::fs::write(d.join("good.md"), "---\ndescription: ok\n---\n").unwrap();
        // 无闭合 --- 的损坏文件：不丢弃，记 error
        std::fs::write(d.join("broken.md"), "---\ndescription: oops\n").unwrap();
        let list = scan_agents_dir(&d);
        assert_eq!(list.len(), 2);
        assert!(
            list.iter()
                .find(|a| a.id == "good.md" || a.id == "good")
                .map(|a| a.error.is_none())
                .unwrap_or(false)
        );
        let broken = list.iter().find(|a| a.id.starts_with("broken")).unwrap();
        assert!(broken.error.is_some(), "corrupt file must carry an error");
        assert_eq!(broken.origin, AgentOrigin::Custom);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn builtin_explorer_and_custom_override() {
        let builtins = builtin_agents();
        assert_eq!(
            builtins.len(),
            2,
            "explorer + general-purpose（2026-10-19 拍板）"
        );
        let ex = builtins.iter().find(|b| b.id == "explorer").unwrap();
        assert_eq!(ex.tools_tier(), "read-only");
        assert_eq!(ex.max_turns_tier(), MaxTurnsTier::Unlimited);
        let gp = builtins.iter().find(|b| b.id == "general-purpose").unwrap();
        assert_eq!(gp.tools_tier(), "inherit", "general-purpose 继承全部工具");
        assert_eq!(gp.max_turns_tier(), MaxTurnsTier::Unlimited);
        let custom = AgentProfile {
            id: "explorer".into(),
            origin: AgentOrigin::Custom,
            ..parse_agent(
                "explorer",
                "---\ndescription: mine\n---\n",
                AgentOrigin::Custom,
            )
            .unwrap()
        };
        assert!(custom.overrides_builtin(&builtins));
        let other = AgentProfile {
            id: "nope".into(),
            ..custom.clone()
        };
        assert!(!other.overrides_builtin(&builtins));
    }

    /// 两侧内置清单逐字一致（profile.ts `BUILTIN_PROFILE_DOCS` ↔ 本模块
    /// `builtin_agents`）：id 集、description、tools、maxTurns、正文。
    /// 防止「两处各存一份」的同步契约静默烂掉——扩展侧改了这边忘改就红。
    #[test]
    fn builtins_match_extension_docs() {
        let docs = [
            (
                "explorer",
                "只读探索 · 调研与定位",
                Some("read-only"),
                "你是只读探索员：调研代码、定位实现、回答问题。\n不执行任何写操作。",
            ),
            (
                "general-purpose",
                "通用子代理 · 复杂调研、代码检索、多步任务",
                None,
                "你是通用子代理：独立完成委派的任务——调研复杂问题、检索代码、执行多步操作。\n可使用会话的全部工具，按需读写文件与执行命令。",
            ),
        ];
        let builtins = builtin_agents();
        assert_eq!(builtins.len(), docs.len());
        for (id, description, tools, body) in docs {
            let a = builtins
                .iter()
                .find(|b| b.id == id)
                .unwrap_or_else(|| panic!("builtin {id} missing"));
            assert_eq!(
                a.description, description,
                "{id} description 与扩展侧不一致"
            );
            assert_eq!(a.tools.as_deref(), tools, "{id} tools 与扩展侧不一致");
            assert_eq!(a.max_turns, Some(0), "{id} maxTurns 与扩展侧不一致");
            assert_eq!(
                a.append_system_prompt.as_deref(),
                Some(body),
                "{id} 正文与扩展侧不一致"
            );
        }
    }
}
