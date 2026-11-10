//! 044-0 子代理模块配置（`<PF数据目录>/agents/config.json`）读写。
//!
//! **唯一事实源**：扩展 `pf-subagents` 每次委派都重读这份文件
//! （`index.ts::syncModuleConfig` → `module-config.ts::loadModuleConfig`）。
//! 所以设置页的改动必须实时落到这里——只写 `app-settings.json`、只传 env
//! 都不会生效（2026-11-08 真机：设置页 10、env 10，被残留的
//! `capacity.default: 5` 压成 5 并发）。
//!
//! 语义对齐扩展侧 `module-config.ts`：`capacity` / `maxDepth` 都是
//! `{ "default", "min", "max" }`，未知字段原样保留（扩展明确允许宿主写
//! `version`、UI 提示等）。
//!
//! **退役键**：模块级 `model`（2026-11-10 用户定夺废除）——扩展不再读它，
//! 本模块在启动补齐与每次容量写入时清掉残留值（`strip_retired_model_at`）；
//! 子代理默认模型只剩 profile frontmatter 与调用级参数两条路。写入走「读-改-原子替换」（同目录 tmp + rename），
//! 扩展的重读并发永远读不到半份 JSON。

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{Value, json};

/// 模块配置文件名（与扩展 `MODULE_CONFIG_FILENAME` 一致）。
pub const MODULE_CONFIG_FILENAME: &str = "config.json";
/// 设置页步进器范围（2026-11-07 用户定稿 2-10）。
pub const CAPACITY_UI_MIN: u64 = 2;
pub const CAPACITY_UI_MAX: u64 = 10;
/// 新建文件时写入的嵌套深度档（PF 恒定 1，禁套娃）。
const DEPTH_DEFAULT: u64 = 1;

/// `<PF数据目录>/agents/config.json`（与 profile 文档同目录，扩展按
/// `PI_SUBAGENTS_PROFILES_DIR` 找到它）。
pub fn module_config_path() -> PathBuf {
    super::agents_dir().join(MODULE_CONFIG_FILENAME)
}

// ---------------------------------------------------------------------------
// 缓存：设置页每帧读容量，不能每帧打盘。写入方（设置页 / 启动期补齐）走
// 同一把锁更新缓存；文件被外部手改的情况到下次写入或重启才反映（扩展侧
// 每次委派重读，实时性在那边）。
// ---------------------------------------------------------------------------

enum Cache {
    Unloaded,
    Loaded(Option<u64>),
}

static CAPACITY: Mutex<Cache> = Mutex::new(Cache::Unloaded);

/// 当前生效的容量上限（读取 `config.json`，进程内缓存一次）。
/// `None` = 文件缺失/损坏/没有 `capacity.default`，调用方自行回退。
pub fn capacity() -> Option<u64> {
    let mut guard = CAPACITY.lock().unwrap_or_else(|e| e.into_inner());
    if let Cache::Loaded(value) = *guard {
        return value;
    }
    let value = read_capacity_at(&module_config_path());
    *guard = Cache::Loaded(value);
    value
}

/// 设置页写容量：实时落 `config.json` 并刷新缓存，返回**实际生效值**
/// （可能被文件里更窄的 `capacity.min/max` 夹住）。
pub fn set_capacity(value: u64) -> io::Result<u64> {
    let effective = write_capacity_at(&module_config_path(), value)?;
    *CAPACITY.lock().unwrap_or_else(|e| e.into_inner()) = Cache::Loaded(Some(effective));
    Ok(effective)
}

/// 启动期补齐：`config.json` 不存在时按 PF 设置写一份。
///
/// 否则扩展落到自己的 `FALLBACK_MODULE_CONFIG`（并发 8），与设置页默认 3
/// 静默分叉。已存在的文件只做**退役键清理**（`model`，2026-11-10 废除），
/// 其余字段一律不动——它是唯一事实源。
pub fn ensure_module_config(default_capacity: u64) -> io::Result<u64> {
    let path = module_config_path();
    if path.exists() {
        let value = capacity().unwrap_or(default_capacity.clamp(CAPACITY_UI_MIN, CAPACITY_UI_MAX));
        let _ = strip_retired_model_at(&path);
        return Ok(value);
    }
    set_capacity(default_capacity)
}

/// 删掉模块级 `model` 键（2026-11-10 用户定夺：全局默认模型不该存在）。
///
/// 默认模型只有 profile frontmatter 与调用级参数两条路，两条都没有 = 继承
/// 父会话模型；文件里残留的 `model` 扩展已按未知键忽略，这里把键清掉免得
/// 用户以为它还在生效。返回是否真的改写过文件；无该键 = `Ok(false)`，
/// 文件缺失/损坏 = `Ok(false)`（不是错误，交给下次容量写入重写）。
pub(crate) fn strip_retired_model_at(path: &Path) -> io::Result<bool> {
    let mut doc = match parse_doc(path) {
        Some(doc) => doc,
        None => return Ok(false),
    };
    let removed = doc
        .as_object_mut()
        .map(|o| o.remove("model").is_some())
        .unwrap_or(false);
    if !removed {
        return Ok(false);
    }
    atomic_write(path, &doc)?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// 纯函数（路径解耦，测试直接打临时文件）
// ---------------------------------------------------------------------------

/// 读 `config.json` 的容量上限；`None` = 读不到 / 解析失败 / 无该字段。
pub fn read_capacity_at(path: &Path) -> Option<u64> {
    let doc = parse_doc(path)?;
    let cap = doc.get("capacity")?;
    let value = cap.get("default")?.as_u64()?;
    let (lo, hi) = bounds(Some(cap)).unwrap_or((1, u64::MAX));
    Some(value.clamp(lo, hi))
}

/// 写 `config.json` 的容量上限（保留其余字段），返回实际生效值。
///
/// 不变量：写出的文档一定过得了扩展的 `parseModuleConfig`
/// （`default` 落在 `min..=max` 内），否则扩展会整体回退 FALLBACK。
pub fn write_capacity_at(path: &Path, value: u64) -> io::Result<u64> {
    let requested = value.clamp(CAPACITY_UI_MIN, CAPACITY_UI_MAX);
    let mut doc = parse_doc(path).unwrap_or_else(fresh_doc);
    let obj = doc.as_object_mut().expect("fresh_doc/parse_doc 保证对象");
    let (lo, hi) = bounds(obj.get("capacity")).unwrap_or((CAPACITY_UI_MIN, CAPACITY_UI_MAX));
    let effective = requested.clamp(lo, hi);
    // 退役键（2026-11-10）：模块级 model 已废除，写路径顺手清掉
    obj.remove("model");

    let slot = obj.entry("capacity").or_insert_with(|| json!({}));
    if !slot.is_object() {
        *slot = json!({});
    }
    let cap = slot.as_object_mut().expect("上面已归一为对象");
    cap.insert("default".into(), Value::from(effective));
    // 只有本来就缺边界时才补（已有 min/max 是文件政策，不覆盖）
    cap.entry("min").or_insert(Value::from(CAPACITY_UI_MIN));
    cap.entry("max").or_insert(Value::from(CAPACITY_UI_MAX));

    atomic_write(path, &doc)?;
    Ok(effective)
}

/// 新建文档：字段与旧版手写文件同形（扩展缺键时也是同一组缺省）。
fn fresh_doc() -> Value {
    json!({
        "version": 1,
        "capacity": { "default": CAPACITY_UI_MIN, "min": CAPACITY_UI_MIN, "max": CAPACITY_UI_MAX },
        "maxDepth": { "default": DEPTH_DEFAULT, "min": DEPTH_DEFAULT, "max": DEPTH_DEFAULT },
    })
}

fn parse_doc(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    let doc: Value = serde_json::from_str(&text).ok()?;
    doc.is_object().then_some(doc)
}

/// `capacity` 的有效边界（缺键按扩展 fallback：min 1 / max 无限；
/// 边界自相矛盾视为无效）。
fn bounds(capacity: Option<&Value>) -> Option<(u64, u64)> {
    let cap = capacity?.as_object()?;
    let pick = |key: &str, fallback: u64| cap.get(key).and_then(Value::as_u64).unwrap_or(fallback);
    let lo = pick("min", 1);
    let hi = pick("max", u64::MAX);
    (lo <= hi).then_some((lo, hi))
}

fn atomic_write(path: &Path, doc: &Value) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut text = serde_json::to_string_pretty(doc)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    text.push('\n');
    let tmp = path.with_extension(format!("json.tmp{}", std::process::id()));
    std::fs::write(&tmp, text)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_path(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("agents-cfg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(MODULE_CONFIG_FILENAME)
    }

    #[test]
    fn creates_document_when_missing() {
        let path = tmp_path("create");
        assert_eq!(read_capacity_at(&path), None);
        assert_eq!(write_capacity_at(&path, 10).unwrap(), 10);
        assert_eq!(read_capacity_at(&path), Some(10));
        // 扩展 parseModuleConfig 的硬要求：default ∈ min..=max、深度同形
        let doc = parse_doc(&path).unwrap();
        assert_eq!(doc["capacity"]["min"], 2);
        assert_eq!(doc["capacity"]["max"], 10);
        assert_eq!(doc["maxDepth"]["default"], 1);
        assert_eq!(doc["version"], 1);
    }

    #[test]
    fn preserves_unrelated_fields_and_order() {
        let path = tmp_path("preserve");
        std::fs::write(
            &path,
            r#"{"version":1,"model":"deepseek/deepseek-flash","maxTurns":100,"maxDepth":{"default":1,"min":1,"max":1},"capacity":{"default":5,"min":2,"max":10},"uiHint":"keep me"}"#,
        )
        .unwrap();
        assert_eq!(write_capacity_at(&path, 8).unwrap(), 8);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("\"model\""), "退役键 model 写路径清除");
        assert!(text.contains("\"maxTurns\": 100"));
        assert!(text.contains("\"uiHint\": \"keep me\""));
        assert_eq!(read_capacity_at(&path), Some(8));
    }

    #[test]
    fn strips_retired_model_key() {
        let path = tmp_path("strip-model");
        std::fs::write(
            &path,
            r#"{"version":1,"model":"deepseek/deepseek-flash","maxTurns":100,"capacity":{"default":3,"min":2,"max":10}}"#,
        )
        .unwrap();
        assert_eq!(strip_retired_model_at(&path).unwrap(), true);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains("\"model\""), "model 键已清除：{text}");
        assert!(text.contains("\"maxTurns\": 100"), "其余字段保留：{text}");
        assert_eq!(read_capacity_at(&path), Some(3));
        // 幂等：再跑一次不再改写
        assert_eq!(strip_retired_model_at(&path).unwrap(), false);
    }

    #[test]
    fn clamps_to_narrower_file_bounds() {
        let path = tmp_path("bounds");
        std::fs::write(&path, r#"{"capacity":{"default":3,"min":2,"max":5}}"#).unwrap();
        assert_eq!(write_capacity_at(&path, 10).unwrap(), 5);
        assert_eq!(read_capacity_at(&path), Some(5));
    }

    #[test]
    fn clamps_to_ui_range_even_when_file_is_wide() {
        let path = tmp_path("wide");
        std::fs::write(&path, r#"{"capacity":{"default":3,"min":1,"max":99}}"#).unwrap();
        assert_eq!(write_capacity_at(&path, 50).unwrap(), 10);
    }

    #[test]
    fn broken_document_is_replaced_by_valid_one() {
        let path = tmp_path("broken");
        std::fs::write(&path, "{not json").unwrap();
        assert_eq!(read_capacity_at(&path), None);
        assert_eq!(write_capacity_at(&path, 7).unwrap(), 7);
        assert_eq!(read_capacity_at(&path), Some(7));
        assert!(serde_json::from_str::<Value>(&std::fs::read_to_string(&path).unwrap()).is_ok());
    }

    #[test]
    fn non_object_capacity_slot_is_normalized() {
        let path = tmp_path("slot");
        std::fs::write(&path, r#"{"capacity":"ten"}"#).unwrap();
        assert_eq!(read_capacity_at(&path), None);
        assert_eq!(write_capacity_at(&path, 4).unwrap(), 4);
        assert_eq!(read_capacity_at(&path), Some(4));
    }
}
