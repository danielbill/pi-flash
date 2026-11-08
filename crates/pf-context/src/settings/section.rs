//! Section 契约与分层（001-1 §3/§4）。
//!
//! 新增一个 setting 的 diff 应该是：1 个字段 + 1 个控件（app `ui/kit`）+
//! 0 新文件 + 0 新 env——字段进对应 section，clamp 只写 [`Section::validate`]。

use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fmt;
use std::time::Instant;

/// 设置分层（001-0 §4，固定禁止加层）。
///
/// 读取 = L3 > L2 > L1 > L0 合并后的 typed snapshot；每个 key 只有一个
/// primary source，其它层只做读覆盖**永不回写**。L4 env 不进本模型——
/// 仅 bootstrap 路径（如 `PI_FLASH_DIR`）在 pi-link 里生效。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// L1 全局：`app-settings.json` 的 section（写通道落盘层）
    Global,
    /// L2 项目：同文档 `projects.<id>.{…}` 段（任务/管家 per-project 参数）
    Project,
    /// L3 会话：内存（可选落会话元数据），单次对话临时覆盖
    Session,
}

/// section 值在**文件**里的来源（过渡期回退判定用，如容量先问
/// `agents/config.json`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// v2 section 键直接命中（`Section::KEY`）
    Section,
    /// v1 平铺键命中（section 的 legacy 映射，001-1 §3 读迁移）
    Legacy,
    /// 文件没有该段（当前值 = 合并缺省）
    Absent,
}

/// 坏档 / 异常记录（001-0 §5：坏文件不许静默改行为）。
#[derive(Debug, Clone)]
pub struct SettingProblem {
    /// 出问题的 section 键（整份非法时为 `<doc>`）
    pub key: String,
    pub detail: String,
    pub at: Instant,
}

impl fmt::Display for SettingProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "[{}] {}: {}",
            self.key,
            humantime_millis(self.at),
            self.detail
        )
    }
}

fn humantime_millis(at: Instant) -> String {
    // Instant 无墙钟语义，只给相对毫秒（观测排序够用）
    format!("+{}ms", at.elapsed().as_millis())
}

/// Section 契约（001-1 §4）。
///
/// `validate` 在「解析后」「set 后」「（S3）从投影读回后」三处调用，保证
/// 内存里永远是合法值；clamp 范围写进 section 的注释与单测，不在 UI、不在
/// 扩展、不在文档里各写一遍。
pub trait Section: Default + Clone + Serialize + DeserializeOwned + Send + Sync + 'static {
    /// v2 文档里的 section 键名（= 文档键名，001-1 §1）
    const KEY: &'static str;

    /// 唯一 clamp / 归一化落点。
    fn validate(&mut self);

    /// 是否可写入跨进程投影 `ext-context.json`（001-2，S3 起；默认否）。
    fn exportable(&self) -> bool {
        false
    }

    /// v1 平铺键读迁移：文件缺 v2 section 时按旧键合成该段（001-1 §3）。
    /// `None` = 文件里没有该段的 legacy 键。
    fn from_legacy(_map: &serde_json::Map<String, serde_json::Value>) -> Option<Self> {
        None
    }

    /// v2 写迁移：同值回写 legacy 平铺键（001-1 §3「旧平铺键保留一版」——
    /// 旧写入者/回滚版本读平铺键仍能拿到当前值）。默认不写。
    fn to_legacy(&self, _map: &mut serde_json::Map<String, serde_json::Value>) {}
}

/// 对象级深合并：`over` 覆盖 `base` 的同名标量/数组，object 递归合并。
///
/// 两处用途：① L0→L3 层合并（上层只覆盖自己声明的字段）；② section 写盘时
/// 与盘上既有值合并（typed 字段覆盖，**section 内未知字段透传**）。
pub(crate) fn deep_merge(base: &mut serde_json::Value, over: &serde_json::Value) {
    match (base, over) {
        (serde_json::Value::Object(b), serde_json::Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(slot) if slot.is_object() && v.is_object() => deep_merge(slot, v),
                    _ => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, o) => *b = o.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deep_merge_overrides_scalars_and_merges_objects() {
        let mut base = json!({"a": 1, "b": {"x": 1, "y": 2}, "c": [1, 2]});
        let over = json!({"a": 9, "b": {"y": 20, "z": 30}, "c": [3]});
        deep_merge(&mut base, &over);
        assert_eq!(
            base,
            json!({"a": 9, "b": {"x": 1, "y": 20, "z": 30}, "c": [3]})
        );
    }

    #[test]
    fn deep_merge_scalar_over_object_replaces() {
        let mut base = json!({"a": {"x": 1}});
        deep_merge(&mut base, &json!({"a": "flat"}));
        assert_eq!(base, json!({"a": "flat"}));
    }
}
