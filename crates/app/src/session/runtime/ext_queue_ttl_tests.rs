//! ext_queue 阻塞件退役闸时效（047-内存削减 P0-4）：新鲜阻塞件拦退役、
//! 超过保护窗放行、fire-and-forget 恒放行。闸本体迁 session/retire.rs，
//! 文件留 runtime 域随 ext_queue 数据类型就近。

use super::*;
use crate::session::retire::ext_queue_allows_retire;
use pi_link::protocol::ExtUiMethod;

fn pending(age: std::time::Duration, blocking: bool) -> ExtPending {
    let method = if blocking {
        ExtUiMethod::Select {
            title: "t".into(),
            options: vec!["a".into()],
        }
    } else {
        ExtUiMethod::Notify {
            message: "m".into(),
            notify_type: None,
        }
    };
    ExtPending {
        at: std::time::Instant::now() - age,
        req: pi_link::protocol::ExtensionUiRequest {
            id: "r1".into(),
            method,
        },
    }
}

#[test]
fn blocking_ttl_gates_retire() {
    // 新鲜阻塞件拦退役
    assert!(!ext_queue_allows_retire(&[pending(
        std::time::Duration::from_secs(1),
        true
    )]));
    // 超过保护窗（600s）的阻塞件放行——泵死扩展不再钉死退役
    assert!(ext_queue_allows_retire(&[pending(
        std::time::Duration::from_secs(601),
        true
    )]));
    // fire-and-forget 恒放行（黄点事故口径）
    assert!(ext_queue_allows_retire(&[pending(
        std::time::Duration::ZERO,
        false
    )]));
    // 混合队列：只要有一个窗内阻塞件就仍拦
    assert!(!ext_queue_allows_retire(&[
        pending(std::time::Duration::from_secs(1), true),
        pending(std::time::Duration::from_secs(700), true),
    ]));
    // 空队列放行
    assert!(ext_queue_allows_retire(&[]));
}
