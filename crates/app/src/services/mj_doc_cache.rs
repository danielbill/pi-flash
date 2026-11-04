//! models.json 读缓存（mtime 校验）——「不做内存副本」铁律的兼容实现。
//!
//! 背景（2026-11-04 模型详情页卡顿归因）：vendored gpui 0.2.2 任何
//! InputState notify = 全窗重建（见 docs/agents/gpui.md），设置 Models 页
//! 一帧会经 N 条路径各同步读一次 models.json（实测 1 商 = 11 次/帧）。
//! 本模块把「读盘」收敛成「stat 校验 + 内存复用」：每次调用 stat 一次，
//! (path, mtime, len) 未变 → 复用上份解析结果；变了（pi / pi-web 外部写盘、
//! PF 自己 save）→ 立即重读。`mj_doc` 的 read→mutate→write 语义不变：
//! 任何写之后的下一次读必然看到新内容——PF 侧各 save 落点再显式
//! [`invalidate()`] 兜底（防 FAT 级 mtime 粒度），外部写盘全靠 mtime。
//!
//! 仅限主线程（GPUI 渲染/事件都在主线程），故 thread_local。

use std::cell::RefCell;
use std::time::SystemTime;

use crate::MJ_READS;

struct Slot {
    path: std::path::PathBuf,
    /// (mtime, len)；None = 文件当前不存在
    stamp: Option<(SystemTime, u64)>,
    doc: serde_json::Value,
}

thread_local! {
    static SLOT: RefCell<Option<Slot>> = RefCell::new(None);
}

/// PF 写盘落点显式失效（`models_json::save` 后调用）；外部写盘主靠 mtime。
pub(crate) fn invalidate() {
    SLOT.with(|s| *s.borrow_mut() = None);
}

/// models.json 读（缓存）。Ok 值可缓存复用；Err（坏文件）**不缓存**——
/// 每次重读，避免「一次读失败粘住直到 mtime 变化」。
pub(crate) fn read() -> Result<serde_json::Value, String> {
    let path = pi_link::models_json::path();
    let stamp = match std::fs::metadata(&path) {
        Ok(md) => Some((md.modified().unwrap_or(SystemTime::UNIX_EPOCH), md.len())),
        Err(_) => None,
    };
    let hit = SLOT.with(|s| {
        s.borrow()
            .as_ref()
            .and_then(|slot| (slot.path == path && slot.stamp == stamp).then(|| slot.doc.clone()))
    });
    if let Some(doc) = hit {
        return Ok(doc);
    }
    MJ_READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    match pi_link::models_json::read() {
        Ok(doc) => {
            SLOT.with(|s| {
                *s.borrow_mut() = Some(Slot {
                    path,
                    stamp,
                    doc: doc.clone(),
                });
            });
            Ok(doc)
        }
        Err(e) => Err(e),
    }
}
