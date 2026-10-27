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
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            }
        })
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

    
    }
