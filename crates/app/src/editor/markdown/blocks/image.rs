//! 图片块渲染（自 blocks.rs 外移）：本地文件 gpui img() 直渲染 +
//! 本地图片 LRU 缓存 + 探针；相对路径按 md 文件目录解析（IMG_BASE）。

use super::*;

/// 图片：本地文件 gpui img() 直渲染；http/不存在 → alt 文本占位。
/// 相对路径按 md 文件目录解析（IMG_BASE，render_themed 设置）。
pub(super) fn render_image(
    url: &str,
    width: Option<f32>,
    alt: &[Run],
    t: &Theme,
) -> gpui::AnyElement {
    let placeholder = || {
        div()
            .w_full()
            .text_color(rgb(t.text_dim))
            .italic()
            .child(SharedString::from(format!("🖼 {}", runs_text(alt))))
            .into_any_element()
    };
    if url.starts_with("http://") || url.starts_with("https://") || url.is_empty() {
        return placeholder();
    }
    let ext = std::path::Path::new(url)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let format = match ext.as_str() {
        "png" => Some(gpui::ImageFormat::Png),
        "jpg" | "jpeg" => Some(gpui::ImageFormat::Jpeg),
        "gif" => Some(gpui::ImageFormat::Gif),
        "bmp" => Some(gpui::ImageFormat::Bmp),
        "svg" => Some(gpui::ImageFormat::Svg),
        "webp" => Some(gpui::ImageFormat::Webp),
        _ => None,
    };
    let path = std::path::Path::new(url);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        resolve_relative_image(path)
    };
    let Some((bytes, format)) = std::fs::read(&path).ok().zip(format) else {
        return placeholder();
    };
    div()
        .w_full()
        .child(
            gpui::img(local_image_cached(format, bytes))
                .max_w_full()
                .when_some(width, |d, w| d.w(px(w.max(24.))))
                .rounded(px(6.)),
        )
        .into_any_element()
}

// 本地图片缓存：md 源里的 ![](本地路径) 每帧重建元素树，原先每帧
// std::fs::read + 新建 Arc<Image>；gpui 按内容哈希去重 loading_assets，
// 但解码位图只进不出。这里线程局部 VecDeque LRU（同 MD_PARSE_CACHE 的
// 理由，元素构建只在主线程），驱逐时经 asset_evict 同步放掉 gpui 侧位图。
// 字节预算按源文件字节计（解码位图与源同量级）。
thread_local! {
    static LOCAL_IMG_CACHE: std::cell::RefCell<
        std::collections::VecDeque<
            ((u64, usize, u8), (Vec<u8>, std::sync::Arc<gpui::Image>)),
        >,
    > = const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
const LOCAL_IMG_CACHE_CAP: usize = 32;
const LOCAL_IMG_CACHE_BUDGET_BYTES: usize = 64 * 1024 * 1024;

fn local_image_cached(format: gpui::ImageFormat, bytes: Vec<u8>) -> std::sync::Arc<gpui::Image> {
    let key = (hash_bytes(&bytes), bytes.len(), format as u8);
    LOCAL_IMG_CACHE.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(pos) = cache.iter().rposition(|(k, _)| *k == key) {
            let entry = cache.remove(pos).expect("pos 来自刚才的迭代");
            if entry.1.0 == bytes {
                let image = entry.1.1.clone();
                cache.push_back(entry); // 刷新到队尾（LRU）
                return image;
            }
        }
        let image = std::sync::Arc::new(gpui::Image::from_bytes(format, bytes.clone()));
        cache.push_back((key, (bytes, image.clone())));
        while cache.len() > LOCAL_IMG_CACHE_CAP
            || cache.iter().map(|(_, v)| v.0.len()).sum::<usize>() > LOCAL_IMG_CACHE_BUDGET_BYTES
        {
            match cache.pop_front() {
                Some((_, (_, evicted))) => {
                    // gpui 侧解码位图同步放掉（见 services::asset_evict）
                    crate::services::asset_evict::queue_evicted(evicted);
                }
                None => break,
            }
        }
        image
    })
}

/// 探针（automation snapshot）：（条数，源文件字节）
pub(crate) fn local_img_probe() -> (usize, usize) {
    LOCAL_IMG_CACHE.with(|cell| {
        let cache = cell.borrow();
        (
            cache.len(),
            cache.iter().map(|(_, v)| v.0.len()).sum::<usize>(),
        )
    })
}
