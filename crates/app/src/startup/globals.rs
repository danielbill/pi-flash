//! 启动全局态一次性装载（010-启动.md §1-§8）：模型清单 / 命令 / 插件 / mcp /
//! 默认项 / 项目上下文，启动时从硬盘装进内存，渲染路径只读内存（不扫盘、不发
//! RPC）；会话进程答到的 RPC 只做覆盖（并回写 `catalog-cache.json`，§4.1）。

use std::path::Path;

use pi_link::protocol::{ModelInfo, SlashCommand};

// ---------------------------------------------------------------------------
// 全局态（一次装载，渲染只读）：模型清单 / 命令 / 默认项 / 插件 / mcp
// ---------------------------------------------------------------------------

/// 跨项目共享的全局态（010-启动.md §1）。
#[derive(Default)]
pub(crate) struct Globals {
    /// 模型清单 = `catalog-cache.json` ∪ 磁盘（`models.json` + `models-store.json`）。
    /// 会话进程的 `get_available_models` 答到后**合并覆盖**（见 `merge_models`）。
    pub models: Vec<ModelInfo>,
    /// 扩展命令缓存 + pi 内置表（pi 1.0.0 = 空）。skill 命令按项目走 `ProjectCtx`。
    pub commands: Vec<SlashCommand>,
    /// `settings.json defaultTools`（会话预设 `configured` 由 pi 按它解析）。
    pub default_tools: Option<Vec<String>>,
    /// 全局插件声明（`settings.json packages`）。
    pub packages: Vec<serde_json::Value>,
    /// 模型清单仍是缓存种子、尚未经任何会话进程答复校验。首个
    /// `SessionEvent::Models` 到达时走一次 [`purge_ghosts`] 后清零——缓存里
    /// 已从 models.json 删除的商（051 测试商 ds/glm 残留）否则永不淘汰：
    /// `merge_models` 是并集，且它们"不在静态注册表 ∧ 不在 models.json"正好
    /// 落进设置页「扩展 provider」兜底桶、上组恒入列。
    pub catalog_provisional: bool,
}

/// 一个项目的上下文（010-启动.md §5）：启动集合内逐项目装载一次，切项目直接命中。
#[derive(Default, Clone)]
pub(crate) struct ProjectCtx {
    /// 项目 `.pi/settings.json` 覆盖了 enabledModels（设置页只读）。
    pub project_scope: bool,
    pub packages: Vec<serde_json::Value>,
    pub skills: Vec<pi_link::skills::SkillEntry>,
    /// `skill:<name>` 命令（由 `skills` 派生，草稿态 `/` 菜单用）。
    pub skill_commands: Vec<SlashCommand>,
    // 043：MCP 不进 ProjectCtx——mcp.json 会被设置页/勾选菜单随时改盘，
    // 缓存副本只会拿到旧列表；消费方（reload_settings_panel）一律盘上现读
}

/// 装载全局态：模型清单（缓存 ∪ 磁盘）、命令（内置 + 扩展缓存）、默认项、插件、全局 mcp。
pub(crate) fn load_globals() -> Globals {
    // 模型清单 = 自有缓存 ∪ 磁盘（010-启动.md §4.1，2026-11-01 回归：缓存含
    // 包内联 provider 与拉取过的内置商目录，磁盘拿不到的那部分）；包内联
    // provider 与扩展命令等会话进程答到后经 `merge_models` / `merge_commands`
    // 并进本进程内存并回写缓存。
    let cache = read_cache();
    let mut models = cache.models;
    models.extend(pi_link::catalog::disk_models());
    let models = pi_link::catalog::dedupe(models);

    let mut commands = pi_link::catalog::builtin_commands();
    commands.extend(cache.commands);

    let settings_path = pi_link::config::settings_path();
    Globals {
        models,
        commands,
        default_tools: pi_link::config::read_default_tools(&settings_path).unwrap_or(None),
        packages: pi_link::config::read_packages(&settings_path).unwrap_or_default(),
        catalog_provisional: true,
    }
}

// ---------------------------------------------------------------------------
// 自有缓存 `~/.pi-flash/catalog-cache.json`（010-启动.md §4.1；042-1 曾删除、
// 2026-11-01 回归——表外内置商拉取结果没有它就是进程死即失）
// ---------------------------------------------------------------------------

/// 缓存内容：模型目录 + 扩展命令（磁盘上拿不到的那部分）。
#[derive(Default, Clone)]
pub(crate) struct CatalogCache {
    pub models: Vec<ModelInfo>,
    pub commands: Vec<SlashCommand>,
}

/// 从显式路径读缓存（测试用同一实现，不碰进程环境）。
fn cache_load_from(path: &Path) -> CatalogCache {
    let Ok(value) = pi_link::config::read_json(path) else {
        return CatalogCache::default();
    };
    let models = value
        .get("models")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let id = m.get("id")?.as_str()?.to_string();
                    let provider = m.get("provider")?.as_str()?.to_string();
                    Some(ModelInfo {
                        id,
                        name: m
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        provider,
                        context_window: m.get("contextWindow").and_then(|v| v.as_u64()),
                        max_tokens: None,
                        reasoning: None,
                        input: None,
                        cost: None,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let commands = value
        .get("commands")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|c| {
                    Some(SlashCommand {
                        name: c.get("name")?.as_str()?.to_string(),
                        description: c
                            .get("description")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    CatalogCache { models, commands }
}

/// 写缓存（原子写）；返回是否写入。
fn cache_save_to(path: &Path, cache: &CatalogCache) -> bool {
    let models: Vec<serde_json::Value> = cache
        .models
        .iter()
        .map(|m| {
            serde_json::json!({
                "provider": m.provider,
                "id": m.id,
                "name": m.name,
                "contextWindow": m.context_window,
            })
        })
        .collect();
    let commands: Vec<serde_json::Value> = cache
        .commands
        .iter()
        .map(|c| serde_json::json!({ "name": c.name, "description": c.description }))
        .collect();
    let value = serde_json::json!({
        "version": 1,
        "fetchedAt": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        "models": models,
        "commands": commands,
    });
    let _ = pi_link::paths::ensure_dir();
    pi_link::config::write_json(path, &value).is_ok()
}

/// 读自有缓存（无文件/坏文件 → 空）。
pub(crate) fn read_cache() -> CatalogCache {
    match pi_link::paths::catalog_cache_file() {
        Some(p) => cache_load_from(&p),
        None => CatalogCache::default(),
    }
}

/// 回写自有缓存（会话进程的 RPC 答复 / 启动 sweep 拉取到位时调用）。
pub(crate) fn write_cache(models: &[ModelInfo], commands: &[SlashCommand]) -> bool {
    let Some(path) = pi_link::paths::catalog_cache_file() else {
        return false;
    };
    cache_save_to(
        &path,
        &CatalogCache {
            models: models.to_vec(),
            commands: commands.to_vec(),
        },
    )
}

/// 装载一个项目的技能表（纯磁盘读）。设置页开关写盘后也要现读一遍——
/// `ProjectCtx` 是启动缓存，拿它当写后回读源就是「开关点了没反应」。
pub(crate) fn load_skills(cwd: &Path) -> Vec<pi_link::skills::SkillEntry> {
    let agent_dir = pi_link::config::agent_dir();
    let settings_value = pi_link::config::read_json(&pi_link::config::settings_path())
        .unwrap_or(serde_json::json!({}));
    let home_agents = agent_dir
        .parent()
        .map(|p| p.join("..").join(".agents").join("skills"))
        // dunce：剥掉 std canonicalize 在 Windows 必带的 \\?\ verbatim 前缀，
        // 否则会顺着 SkillEntry.path 漏进技能面板显示（Zed 同款处理）。
        .map(|p| dunce::canonicalize(&p).unwrap_or(p))
        .unwrap_or_else(|| agent_dir.clone());
    pi_link::skills::discover_skills(cwd, &agent_dir, &home_agents, &settings_value)
}

/// 装载一个项目的上下文（纯磁盘读；启动集合内逐项目跑，切项目未命中时现算）。
pub(crate) fn load_project(cwd: &Path) -> ProjectCtx {
    let project_settings = pi_link::config::project_settings_path(cwd);
    let skills = load_skills(cwd);
    let skill_commands = pi_link::catalog::skill_commands(&skills);
    ProjectCtx {
        project_scope: pi_link::config::read_enabled_models(&project_settings)
            .unwrap_or_else(|_| None)
            .is_some(),
        packages: pi_link::config::read_packages(&project_settings).unwrap_or_default(),
        skills,
        skill_commands,
    }
}

/// 把 RPC 答到的清单并进全局态（进程答案只做覆盖）：返回是否有变化。
pub(crate) fn merge_models(globals: &mut Globals, rpc: &[ModelInfo]) -> bool {
    let mut merged = rpc.to_vec();
    merged.extend(globals.models.iter().cloned());
    let deduped = pi_link::catalog::dedupe(merged);
    let changed = deduped.len() != globals.models.len()
        || deduped.iter().zip(globals.models.iter()).any(|(a, b)| {
            a.provider != b.provider
                || a.id != b.id
                || a.name != b.name
                || a.context_window != b.context_window
        });
    globals.models = deduped;
    changed
}

/// 首个会话进程答复到达时淘汰缓存幽灵：`globals.models` 里 provider **既不在
/// 实时答复、也不在 `keep` 集**的条目删掉。`keep` = models.json 账本商 ∪
/// registry 静态商（有磁盘佐证，不是幽灵）；扩展注册商（freeflow）由实时答复
/// 佐证。返回是否有删除。只在 `catalog_provisional` 期调一次——之后多 runtime
/// 答复照旧并集（多 cwd 语义）。
pub(crate) fn purge_ghosts(
    globals: &mut Globals,
    live: &[ModelInfo],
    keep: &std::collections::HashSet<String>,
) -> bool {
    let before = globals.models.len();
    globals
        .models
        .retain(|m| keep.contains(&m.provider) || live.iter().any(|l| l.provider == m.provider));
    globals.models.len() != before
}

/// 把 RPC 答到的扩展命令并进全局命令清单（`skill:` 前缀不入全局，skill 走磁盘
/// 派生）：返回是否有变化。
pub(crate) fn merge_commands(globals: &mut Globals, rpc: &[SlashCommand]) -> bool {
    let mut changed = false;
    for c in rpc.iter().filter(|c| !c.name.starts_with("skill:")) {
        if !globals.commands.iter().any(|g| g.name == c.name) {
            globals.commands.push(c.clone());
            changed = true;
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_models_keeps_union_with_priority_to_rpc() {
        let mut g = Globals {
            models: vec![ModelInfo {
                id: "x".into(),
                name: "disk".into(),
                provider: "p".into(),
                context_window: None,
                max_tokens: None,
                reasoning: None,
                input: None,
                cost: None,
            }],
            ..Default::default()
        };
        let rpc = vec![
            ModelInfo {
                id: "x".into(),
                name: "rpc".into(),
                provider: "p".into(),
                context_window: Some(9),
                max_tokens: None,
                reasoning: None,
                input: None,
                cost: None,
            },
            ModelInfo {
                id: "y".into(),
                name: "rpc2".into(),
                provider: "p".into(),
                context_window: None,
                max_tokens: None,
                reasoning: None,
                input: None,
                cost: None,
            },
        ];
        assert!(merge_models(&mut g, &rpc));
        assert_eq!(g.models.len(), 2);
        assert_eq!(g.models[0].name, "rpc", "RPC 答案优先");
        assert_eq!(g.models[1].id, "y");
    }

    #[test]
    fn purge_ghosts_drops_only_unbacked_providers() {
        let mk = |provider: &str, id: &str| ModelInfo {
            id: id.into(),
            name: id.into(),
            provider: provider.into(),
            context_window: None,
            max_tokens: None,
            reasoning: None,
            input: None,
            cost: None,
        };
        // 缓存种子：ds/glm = 已从 models.json 删掉的测试商（幽灵）；dds = 账本商；
        // deepseek = 静态注册表商；freeflow = 扩展商（只有实时答复佐证）
        let mut g = Globals {
            models: vec![
                mk("ds", "deepseek-flash"),
                mk("glm", "glm-5.3"),
                mk("dds", "glm-5.3-flash"),
                mk("deepseek", "deepseek-chat"),
                mk("freeflow", "free-pro"),
            ],
            catalog_provisional: true,
            ..Default::default()
        };
        let live = vec![mk("freeflow", "free-pro"), mk("deepseek", "deepseek-chat")];
        let keep: std::collections::HashSet<String> = ["dds".to_string(), "deepseek".to_string()]
            .into_iter()
            .collect();

        assert!(purge_ghosts(&mut g, &live, &keep));
        let provs: Vec<&str> = g.models.iter().map(|m| m.provider.as_str()).collect();
        assert_eq!(provs, vec!["dds", "deepseek", "freeflow"], "只删 ds/glm");
        // 幽灵已清，再跑一次无删除
        assert!(!purge_ghosts(&mut g, &live, &keep));
    }

    #[test]
    fn merge_commands_skips_skill_prefix() {
        let mut g = Globals::default();
        let rpc = vec![
            SlashCommand {
                name: "skill:image-gen".into(),
                description: "s".into(),
            },
            SlashCommand {
                name: "notify".into(),
                description: "n".into(),
            },
        ];
        assert!(merge_commands(&mut g, &rpc));
        assert_eq!(g.commands.len(), 1);
        assert_eq!(g.commands[0].name, "notify");
        // 幂等
        assert!(!merge_commands(&mut g, &rpc));
    }

    fn tmp_cache_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir()
            .join(format!(
                "pi-flash-cache-{tag}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .subsec_nanos()
            ))
            .join("catalog-cache.json")
    }

    #[test]
    fn cache_roundtrip_keeps_models_and_commands() {
        let path = tmp_cache_path("roundtrip");
        let cache = CatalogCache {
            models: vec![ModelInfo {
                id: "m1".into(),
                name: "Model One".into(),
                provider: "p".into(),
                context_window: Some(1234),
                max_tokens: None,
                reasoning: None,
                input: None,
                // 042-2：ModelInfo 增带价格四键（Option，测试用 None）
                cost: None,
            }],
            commands: vec![SlashCommand {
                name: "notify".into(),
                description: "d".into(),
            }],
        };
        assert!(cache_save_to(&path, &cache));
        let back = cache_load_from(&path);
        assert_eq!(back.models.len(), 1);
        assert_eq!(back.models[0].name, "Model One");
        assert_eq!(back.models[0].context_window, Some(1234));
        assert_eq!(back.commands.len(), 1);
        assert_eq!(back.commands[0].name, "notify");
        // 缺文件 → 空缓存（不 panic）
        let missing = cache_load_from(&path.with_file_name("nope.json"));
        assert!(missing.models.is_empty() && missing.commands.is_empty());
        let _ = std::fs::remove_dir_all(path.parent().unwrap());
    }
}
