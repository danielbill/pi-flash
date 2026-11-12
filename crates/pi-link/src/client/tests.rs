//! client spawn 参数解析单测（047-P2-1）。env 本体不在此测——edition 2024
//! 的 set_var 是 unsafe 且并行测试共享环境；parse_heap_mb 抽成纯函数直接
//! 覆盖口径，env 读取只剩一行薄封装。

use super::{DEFAULT_NODE_HEAP_MB, parse_heap_mb};

/// 三态口径：未设/非法 → None（spawn 回落缺省 3072）；"0" → Some(0) 显式
/// 禁用（≠回落）；数值容忍周边空白；负数/溢出按非法处理。
#[test]
fn heap_env_parse_three_states() {
    assert_eq!(parse_heap_mb(None), None, "未设 → 回落缺省");
    assert_eq!(
        parse_heap_mb(Some("abc")),
        None,
        "非法 → 回落缺省（不裸奔）"
    );
    assert_eq!(parse_heap_mb(Some("")), None, "空串 → 回落缺省");
    assert_eq!(parse_heap_mb(Some("-1")), None, "负数非法");
    assert_eq!(parse_heap_mb(Some(" 256 ")), Some(256), "容忍周边空白");
    assert_eq!(parse_heap_mb(Some("0")), Some(0), "0 = 显式禁用，不是缺省");
    assert_eq!(
        parse_heap_mb(Some("99999999999999999999")),
        None,
        "溢出 → 非法"
    );
}

/// 缺省值口径：spawn 未设 env 时必须插 --max-old-space-size=3072。
/// （插旗位置测试做不了——那要真 spawn node；位置约束「flag 必须在
/// cli.js 之前」由 spawn 组装处注释锁定，P2-1 验收走真机 env=256。）
#[test]
fn heap_default_is_3072() {
    assert_eq!(DEFAULT_NODE_HEAP_MB, 3072);
}
