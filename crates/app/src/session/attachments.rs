//! composer 附件：图片选择/剪贴板图片 → AttachedImage（base64 供 RPC、
//! 预构建缩略图供渲染；10 张/单张 10MB 上限）。
//! （P2 归并：actions_panels 的附件侧。）

use crate::*;

/// 附件上限（pi-web image-attachments.ts parity）：10 张、单张解码后 10MB
const MAX_ATTACHED_IMAGES: usize = 10;
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// 由原始字节构造附件：base64 供 RPC、thumb 预构建供缩略图渲染。
fn attached_image_from_bytes(mime: String, bytes: Vec<u8>) -> AttachedImage {
    use base64::Engine as _;
    let data_b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let thumb = crate::session::messages::mime_to_image_format(&mime)
        .map(|f| std::sync::Arc::new(gpui::Image::from_bytes(f, bytes)));
    AttachedImage {
        data_b64,
        mime,
        thumb,
    }
}

impl Chat {
    /// Ctrl+V 剪贴板图片 → 附件（pi-web handlePaste parity：剪贴板含图
    /// 即接管本次粘贴，文本不再落输入框）。返回 true 表示已消费（宿主
    /// 不再重派发组件 Paste）；false 交还原文本粘贴路径。
    /// 超限（张数/体积）与不支持的格式（svg 等）静默跳过——与 pi-web
    /// isBase64ImageWithinLimits 的过滤行为一致。
    pub(crate) fn attach_clipboard_image(&mut self, cx: &mut Context<Self>) -> bool {
        use gpui::ClipboardEntry;
        let Some(item) = cx.read_from_clipboard() else {
            return false;
        };
        let Some(image) = item.entries().iter().find_map(|e| match e {
            ClipboardEntry::Image(img) => Some(img.clone()),
            _ => None,
        }) else {
            return false;
        };
        let mime = match image.format {
            gpui::ImageFormat::Png => "image/png",
            gpui::ImageFormat::Jpeg => "image/jpeg",
            gpui::ImageFormat::Gif => "image/gif",
            gpui::ImageFormat::Webp => "image/webp",
            // 剪贴板可能给出 svg 文本（复制矢量图）——缩略图与 pi 均不收
            _ => return true,
        };
        let bytes = image.bytes;
        if bytes.len() > MAX_IMAGE_BYTES
            || self.composer.pending_images.len() >= MAX_ATTACHED_IMAGES
        {
            return true;
        }
        self.composer
            .pending_images
            .push(attached_image_from_bytes(mime.into(), bytes));
        cx.notify();
        true
    }
}
