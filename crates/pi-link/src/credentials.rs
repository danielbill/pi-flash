//! OS 凭据库（keyring）+ `$PF_KEY_*` 变量引用（051-apikey管理.md）。
//!
//! 明文唯一 rest 归宿是系统凭据库（Windows Credential Manager / macOS
//! Keychain，`keyring` crate）；pf-auth.json / models.json 里只放
//! `$PF_KEY_*` 变量名引用，spawn 时在这里解出并注入 pi 子进程环境。
//! auth.json / models.json 都是 pi 的地盘，PF 零写入（仅启动一次性收编）。
/// 凭据库抽象：生产用 [`KeyringVault`]，测试用 [`MemVault`]。
pub trait SecretVault {
    fn set(&self, entry: &str, secret: &str) -> Result<(), String>;
    fn get(&self, entry: &str) -> Result<Option<String>, String>;
    /// 幂等：条目不存在也算成功。
    fn delete(&self, entry: &str) -> Result<(), String>;
}

/// keyring service 名（凭据管理器里显示的应用名）。
pub const SERVICE: &str = "PiFlash";

/// 真实凭据库（随平台：windows-native / apple-native）。
pub struct KeyringVault;

impl SecretVault for KeyringVault {
    fn set(&self, entry: &str, secret: &str) -> Result<(), String> {
        keyring::Entry::new(SERVICE, entry)
            .and_then(|e| e.set_password(secret))
            .map_err(|e| format!("credential vault: {e}"))
    }

    fn get(&self, entry: &str) -> Result<Option<String>, String> {
        match keyring::Entry::new(SERVICE, entry).and_then(|e| e.get_password()) {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("credential vault: {e}")),
        }
    }

    fn delete(&self, entry: &str) -> Result<(), String> {
        match keyring::Entry::new(SERVICE, entry).and_then(|e| e.delete_credential()) {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("credential vault: {e}")),
        }
    }
}

/// 测试用内存实现。
#[derive(Default)]
pub struct MemVault(pub std::sync::Mutex<std::collections::BTreeMap<String, String>>);

impl SecretVault for MemVault {
    fn set(&self, entry: &str, secret: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(entry.into(), secret.into());
        Ok(())
    }

    fn get(&self, entry: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(entry).cloned())
    }

    fn delete(&self, entry: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(entry);
        Ok(())
    }
}

/// 永远失败的实现：驱动降级路径的测试。
pub struct FailVault;

impl SecretVault for FailVault {
    fn set(&self, _entry: &str, _secret: &str) -> Result<(), String> {
        Err("vault down".into())
    }
    fn get(&self, _entry: &str) -> Result<Option<String>, String> {
        Err("vault down".into())
    }
    fn delete(&self, _entry: &str) -> Result<(), String> {
        Err("vault down".into())
    }
}

/// key 值形态（UI 状态文案 + 降级判断）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StoreMode {
    /// 明文在凭据库，文件里是 `$PF_KEY_*` 引用。
    Vault,
    /// 用户手写的高级引用（`$OTHER` / `!cmd`），PF 不注入。
    Ref,
    /// 凭据库不可用的降级：明文直接在文件里（0600）。
    File,
}

impl StoreMode {
    pub fn as_str(self) -> &'static str {
        match self {
            StoreMode::Vault => "vault",
            StoreMode::Ref => "ref",
            StoreMode::File => "file",
        }
    }

    pub fn from_str(s: &str) -> Option<StoreMode> {
        match s {
            "vault" => Some(StoreMode::Vault),
            "ref" => Some(StoreMode::Ref),
            "file" => Some(StoreMode::File),
            _ => None,
        }
    }
}

/// `$PF_KEY_X` 引用识别 → 变量名（`PF_KEY_X`）。明文 / `$OTHER` / `!cmd` → None。
pub fn pf_ref(value: &str) -> Option<&str> {
    let name = value.strip_prefix('$')?;
    let tail = name.strip_prefix("PF_KEY_")?;
    if tail.is_empty() {
        return None;
    }
    name.chars()
        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        .then_some(name)
}

/// 账本里的 key 值形态分类。
pub fn classify(value: &str) -> StoreMode {
    if pf_ref(value).is_some() {
        StoreMode::Vault
    } else if value.starts_with('$') || value.starts_with('!') {
        StoreMode::Ref
    } else {
        StoreMode::File
    }
}

/// provider id → `PF_KEY_*` 变量名。字母数字直接大写（deepseek →
/// PF_KEY_DEEPSEEK）；含分隔符时净化并追加原始 id 的 6 位 FNV 短哈希——
/// 否则 `a-b` / `a.b` 净化成同名互相覆盖。
pub fn env_var_name(provider: &str) -> String {
    let base: String = provider
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_uppercase() } else { '_' })
        .collect();
    let base = base.trim_matches('_');
    let mut name = format!("PF_KEY_{}", if base.is_empty() { "X" } else { base });
    if provider.chars().any(|c| !c.is_ascii_alphanumeric()) {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for b in provider.as_bytes() {
            hash ^= u64::from(*b);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        name.push_str(&format!("_{:06X}", hash & 0xFFFF_FF));
    }
    name
}

/// 目录 provider 的 pi 官方 env 名。**逐字照抄 vendored pi 的
/// `getApiKeyEnvVars`**（bundle/chunks，`--help` env 段同源），随 vendor pin
/// 固化；不猜、不加。查不到 → None：调用方记 pf-auth 并注入自身变量名，
/// pi 不消费即诚实降级——绝不写 pi 的 models.json 兜底（051 M1.1 铁律）。
pub fn official_env_name(provider: &str) -> Option<&'static str> {
    const TABLE: &[(&str, &str)] = &[
        ("anthropic", "ANTHROPIC_API_KEY"),
        ("ant-ling", "ANT_LING_API_KEY"),
        ("azure-openai-responses", "AZURE_OPENAI_API_KEY"),
        ("baseten", "BASETEN_API_KEY"),
        ("cloudflare-ai-gateway", "CLOUDFLARE_API_KEY"),
        ("cloudflare-workers-ai", "CLOUDFLARE_API_KEY"),
        ("cerebras", "CEREBRAS_API_KEY"),
        ("deepseek", "DEEPSEEK_API_KEY"),
        ("fireworks", "FIREWORKS_API_KEY"),
        ("github-copilot", "COPILOT_GITHUB_TOKEN"),
        ("google", "GEMINI_API_KEY"),
        ("google-vertex", "GOOGLE_CLOUD_API_KEY"),
        ("groq", "GROQ_API_KEY"),
        ("huggingface", "HF_TOKEN"),
        ("kimi-coding", "KIMI_API_KEY"),
        ("meta", "META_API_KEY"),
        ("minimax", "MINIMAX_API_KEY"),
        ("minimax-cn", "MINIMAX_CN_API_KEY"),
        ("mistral", "MISTRAL_API_KEY"),
        ("moonshotai", "MOONSHOT_API_KEY"),
        ("moonshotai-cn", "MOONSHOT_API_KEY"),
        ("nvidia", "NVIDIA_API_KEY"),
        ("openai", "OPENAI_API_KEY"),
        ("opencode", "OPENCODE_API_KEY"),
        ("opencode-go", "OPENCODE_API_KEY"),
        ("openrouter", "OPENROUTER_API_KEY"),
        ("qwen-token-plan", "QWEN_TOKEN_PLAN_API_KEY"),
        ("qwen-token-plan-cn", "QWEN_TOKEN_PLAN_CN_API_KEY"),
        ("qwen-token-plan-individual", "QWEN_TOKEN_PLAN_API_KEY"),
        ("radius", "RADIUS_API_KEY"),
        ("together", "TOGETHER_API_KEY"),
        ("typesafe", "TYPESAFE_API_KEY"),
        ("vercel-ai-gateway", "AI_GATEWAY_API_KEY"),
        ("xiaomi", "XIAOMI_API_KEY"),
        ("xiaomi-token-plan-ams", "XIAOMI_TOKEN_PLAN_AMS_API_KEY"),
        ("xiaomi-token-plan-cn", "XIAOMI_TOKEN_PLAN_CN_API_KEY"),
        ("xiaomi-token-plan-sgp", "XIAOMI_TOKEN_PLAN_SGP_API_KEY"),
        ("xai", "XAI_API_KEY"),
        ("zai", "ZAI_API_KEY"),
        ("zai-coding-cn", "ZAI_CODING_CN_API_KEY"),
    ];
    TABLE.iter().find(|(p, _)| *p == provider).map(|(_, e)| *e)
}

/// spawn 注入表（051 §5）：解 pf-auth.json（目录 provider → `injectAs`
/// 名；降级明文直接注入）与 pf providers.json（自定义 provider 的
/// `$PF_KEY_*` → 同名）。解不出的引用跳过——该 provider 在会话里报鉴权
/// 错，不阻塞 spawn。
pub fn spawn_env_at(
    pf_path: &std::path::Path,
    providers_path: &std::path::Path,
    vault: &dyn SecretVault,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |name: &str, value: &str| {
        if name.is_empty() || out.iter().any(|(n, _)| n == name) {
            return;
        }
        out.push((name.to_string(), value.to_string()));
    };
    if let Ok(entries) = crate::pf_auth::entries_at(pf_path) {
        for (_provider, key, inject_as, mode) in entries {
            match classify(&key) {
                StoreMode::Vault => {
                    let Some(var) = pf_ref(&key) else { continue };
                    if let Ok(Some(secret)) = vault.get(var) {
                        push(&inject_as.unwrap_or_else(|| var.to_string()), &secret);
                    }
                }
                // 降级明文：pf-auth.json 是 PF 私有文件，pi 读不到，必须由
                // PF 注入；用户高级引用（Ref）走继承环境，PF 不掺和。
                StoreMode::File => {
                    if mode == StoreMode::File.as_str() {
                        if let Some(inj) = &inject_as {
                            push(inj, &key);
                        }
                    }
                }
                StoreMode::Ref => {}
            }
        }
    }
    if let Ok(doc) = crate::pf_providers::read_at(providers_path) {
        for (_name, entry) in crate::models_json::providers(&doc) {
            if let Some(key) = entry.get("apiKey").and_then(|v| v.as_str()) {
                if let Some(var) = pf_ref(key) {
                    if let Ok(Some(secret)) = vault.get(var) {
                        push(var, &secret);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pf_ref_shapes() {
        assert_eq!(pf_ref("$PF_KEY_GLM"), Some("PF_KEY_GLM"));
        assert_eq!(pf_ref("$PF_KEY_MY_GW_A1B2C3"), Some("PF_KEY_MY_GW_A1B2C3"));
        assert_eq!(pf_ref("$OTHER_VAR"), None);
        assert_eq!(pf_ref("$PF_KEY_"), None);
        assert_eq!(pf_ref("$PF_KEY_lower"), None);
        assert_eq!(pf_ref("sk-plain"), None);
        assert_eq!(pf_ref("!cmd"), None);
        assert_eq!(pf_ref(""), None);
    }

    #[test]
    fn env_var_names_are_deterministic_and_collision_free() {
        assert_eq!(env_var_name("deepseek"), "PF_KEY_DEEPSEEK");
        assert_eq!(env_var_name("OpenAI"), "PF_KEY_OPENAI");
        assert_eq!(env_var_name(""), "PF_KEY_X");
        // 全分隔符 → 净化为空 + 哈希后缀防撞
        let dashed = env_var_name("---");
        assert!(dashed.starts_with("PF_KEY_X_"), "{dashed}");
        // a-b 与 a.b 净化同名，必须靠哈希区分
        assert_ne!(env_var_name("a-b"), env_var_name("a.b"));
        // 同输入同输出
        assert_eq!(env_var_name("my-gw"), env_var_name("my-gw"));
    }

    #[test]
    fn classify_modes() {
        assert_eq!(classify("$PF_KEY_A"), StoreMode::Vault);
        assert_eq!(classify("$OTHER"), StoreMode::Ref);
        assert_eq!(classify("!cmd"), StoreMode::Ref);
        assert_eq!(classify("sk-plain"), StoreMode::File);
    }

    #[test]
    fn official_env_table_hits_known_misses_unknown() {
        assert_eq!(official_env_name("deepseek"), Some("DEEPSEEK_API_KEY"));
        assert_eq!(official_env_name("anthropic"), Some("ANTHROPIC_API_KEY"));
        assert_eq!(official_env_name("zai-coding-cn"), Some("ZAI_CODING_CN_API_KEY"));
        assert_eq!(official_env_name("glm"), None); // pi 官方表就没有 glm：诚实降级，不猜
    }

    #[test]
    fn spawn_env_merges_both_ledgers_and_skips_missing() {
        let pf = std::env::temp_dir().join(format!("pf-spawn-pfauth-{}.json", std::process::id()));
        let mp = std::env::temp_dir().join(format!("pf-spawn-models-{}.json", std::process::id()));
        let v = MemVault::default();
        v.set("PF_KEY_DEEPSEEK", "sk-ds").unwrap();
        v.set("PF_KEY_MY_GW", "sk-gw").unwrap();
        crate::pf_auth::set_entry_at(
            &pf,
            "deepseek",
            "$PF_KEY_DEEPSEEK",
            Some("DEEPSEEK_API_KEY"),
            StoreMode::Vault,
        )
        .unwrap();
        // 降级明文：直接按 injectAs 注入
        crate::pf_auth::set_entry_at(
            &pf,
            "openai",
            "sk-plain-fallback",
            Some("OPENAI_API_KEY"),
            StoreMode::File,
        )
        .unwrap();
        // 账本有引用但凭据库缺条目 → 跳过
        crate::pf_auth::set_entry_at(
            &pf,
            "groq",
            "$PF_KEY_GROQ",
            Some("GROQ_API_KEY"),
            StoreMode::Vault,
        )
        .unwrap();
        std::fs::write(
            &mp,
            r#"{ "providers": { "my-gw": { "baseUrl": "https://x/v1", "apiKey": "$PF_KEY_MY_GW" } } }"#,
        )
        .unwrap();
        let env = spawn_env_at(&pf, &mp, &v);
        assert!(env.contains(&("DEEPSEEK_API_KEY".into(), "sk-ds".into())));
        assert!(env.contains(&("OPENAI_API_KEY".into(), "sk-plain-fallback".into())));
        assert!(env.contains(&("PF_KEY_MY_GW".into(), "sk-gw".into())));
        assert!(!env.iter().any(|(n, _)| n == "GROQ_API_KEY"), "缺凭据库条目的应跳过");
        assert_eq!(env.len(), 3);
        let _ = std::fs::remove_file(&pf);
        let _ = std::fs::remove_file(&mp);
    }
}
