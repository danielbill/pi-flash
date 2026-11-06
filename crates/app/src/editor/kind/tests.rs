//! kind 注册表测试外移（按后缀分派 / 双态门控）。

use super::*;
use std::path::PathBuf;

fn kind(ext: &str) -> &'static dyn FileKind {
    kind_of(&PathBuf::from(format!("x.{ext}")))
}

/// 后缀 → kind 路由（原 ts_language 映射表的注册表化断言）。
#[test]
fn ext_routing() {
    assert_eq!(kind("rs").editor_config().unwrap().ts_lang, "rust");
    assert_eq!(kind("pyw").editor_config().unwrap().ts_lang, "python");
    assert_eq!(kind("tsx").editor_config().unwrap().ts_lang, "tsx");
    assert_eq!(kind("toml").editor_config().unwrap().ts_lang, "toml");
    assert_eq!(kind("cpp").editor_config().unwrap().ts_lang, "cpp");
    assert_eq!(kind("hpp").editor_config().unwrap().ts_lang, "cpp");
    assert_eq!(kind("yml").editor_config().unwrap().ts_lang, "yaml");
    // 大小写不敏感（后缀统一 lower 进表）
    assert_eq!(kind("MD").name(), "markdown");
    assert_eq!(kind("RS").editor_config().unwrap().ts_lang, "rust");
    // 无后缀 / 未注册 → 默认 TextKind（csv 走 default，行为同今天）
    assert_eq!(kind_of(&PathBuf::from("README")).name(), "text");
    assert_eq!(kind("csv").name(), "text");
    assert_eq!(kind("unknownext").name(), "text");
    // vendored 裁剪的语言（rb/sql）保持回退纯色文本
    assert_eq!(kind("rb").name(), "text");
    assert_eq!(kind("sql").name(), "text");
}

/// html：源码进编辑器（2026-10-12 二改拍板），且**并入代码类处理器**——
/// 与 xml/vue 同一个 CodeKind（ts_lang "html"）。浏览器入口不再是 kind
/// 差异，而是路径判据（`browser_openable`，见下个测试）。
#[test]
fn html_opens_as_source() {
    assert_eq!(kind("html").name(), "code");
    assert_eq!(kind("htm").name(), "code");
    assert_eq!(kind("xml").name(), "code");
    // svg 同为树状标记文本（zed is_image_file 排除 svg，默认开源码）
    assert_eq!(kind("svg").name(), "code");
    let cfg = kind("html").editor_config().unwrap();
    assert_eq!(cfg.ts_lang, "html");
    assert!(cfg.soft_wrap);
    assert!(!kind("html").supports_view_mode());
    assert_eq!(kind("html").toolbar(), &[ToolButton::Search]);
    // 文件动作块 = 默认骨架（浏览器项由 browser_openable 打头渲染）
    assert_eq!(kind("html").menu_file_actions(true), DEFAULT_FILE_ACTIONS);
}

/// 图片二进制认领（zed create_gpui_image 同款）：magic bytes 裁决——
/// 后缀在注册表只当「后缀门」，字节不是真图/格式 gpui 不认 → None
/// 落文本管线（decode_text Binary → 灰屏）。
#[test]
fn image_binary_claim_by_magic() {
    // 真 magic：zed create_gpui_image 全集八种格式全认领
    assert_eq!(
        kind("png").try_binary(b"\x89PNG\r\n\x1a\nrest"),
        Some(gpui::ImageFormat::Png)
    );
    assert_eq!(
        kind("jpg").try_binary(b"\xFF\xD8\xFFxx"),
        Some(gpui::ImageFormat::Jpeg)
    );
    assert_eq!(
        kind("bmp").try_binary(b"BM\x00\x00"),
        Some(gpui::ImageFormat::Bmp)
    );
    assert_eq!(
        kind("webp").try_binary(b"RIFF\x00\x00\x00\x00WEBPVP8 "),
        Some(gpui::ImageFormat::Webp)
    );
    assert_eq!(
        kind("gif").try_binary(b"GIF89axxx"),
        Some(gpui::ImageFormat::Gif)
    );
    assert_eq!(
        kind("tiff").try_binary(b"II*\x00tiff"),
        Some(gpui::ImageFormat::Tiff)
    );
    assert_eq!(
        kind("ico").try_binary(b"\x00\x00\x01\x00ico"),
        Some(gpui::ImageFormat::Ico)
    );
    assert_eq!(
        kind("pbm").try_binary(b"P6\n1 1\n255\n"),
        Some(gpui::ImageFormat::Pnm)
    );
    // 后缀骗不过：.png 装着文本字节 → None（zed 开图失败 → 灰屏同构）
    assert_eq!(kind("png").try_binary(b"plain text no magic"), None);
    // avif 等过扩展门但 magic 解不出（不在 create_gpui_image 表内）→ None
    assert_eq!(kind("png").try_binary(b"\x00\x00\x00 ftypavif"), None);
    // 文本类 kind 不认领二进制
    assert_eq!(kind("txt").try_binary(b"\x89PNG\r\n\x1a\n"), None);
    assert_eq!(kind("rs").try_binary(b"\xFF\xD8\xFF"), None);
}

/// 浏览器打开判据：只认 html/htm（大小写不敏感）；xml 同 kind 但不开。
#[test]
fn browser_openable_only_pages() {
    assert!(browser_openable(Path::new("a.html")));
    assert!(browser_openable(Path::new("A.HTM")));
    assert!(!browser_openable(Path::new("a.xml")));
    assert!(!browser_openable(Path::new("a.md")));
    assert!(!browser_openable(Path::new("README")));
}

/// txt 与 code 分离：两者都是编辑器型，但高亮与身份不同。
#[test]
fn text_vs_code_distinct() {
    assert_eq!(kind("txt").name(), "text");
    assert_eq!(kind("rs").name(), "code");
    assert_eq!(kind("txt").editor_config().unwrap().ts_lang, "text");
}

/// 编辑器配置：全部 kind 软换行开（md 的关折行只存在于 WYSIWYG 装饰
/// 激活期间，由 sync_live_decorations 运行时收口）；md 带图片粘贴钩子；
/// 图片无编辑器。
#[test]
fn editor_configs() {
    let md = kind("md").editor_config().unwrap();
    assert_eq!(md.ts_lang, "markdown");
    assert!(md.soft_wrap);
    assert!(md.image_paste);
    let txt = kind("txt").editor_config().unwrap();
    assert!(txt.soft_wrap);
    assert!(!txt.image_paste);
    assert!(kind("png").editor_config().is_none());
}

/// 工具栏：默认只有搜索；仅 markdown 追加预览切换（023/024）。
#[test]
fn toolbars() {
    let default_btns = kind("txt").toolbar();
    assert_eq!(default_btns, &[ToolButton::Search]);
    assert_eq!(kind("rs").toolbar(), &[ToolButton::Search]);
    assert_eq!(kind("md").toolbar(), &[ToolButton::Eye, ToolButton::Search]);
}

/// 右键菜单文件动作块：默认骨架；md 源码态带预览项（浏览器项是路径
/// 判据，不在本表）。
#[test]
fn menus() {
    assert_eq!(kind("txt").menu_file_actions(true), DEFAULT_FILE_ACTIONS);
    let md_src = kind("md").menu_file_actions(true);
    assert_eq!(md_src[0], MenuAction::PreviewToggle);
    assert_eq!(kind("md").menu_file_actions(false), DEFAULT_FILE_ACTIONS);
    assert_eq!(kind("html").menu_file_actions(true), DEFAULT_FILE_ACTIONS);
}

/// 双态：仅 markdown 支持 view_mode（file.view_mode op 门控）。
#[test]
fn view_mode_support() {
    assert!(kind("md").supports_view_mode());
    assert!(!kind("txt").supports_view_mode());
    assert!(!kind("rs").supports_view_mode());
    assert!(!kind("png").supports_view_mode());
}

/// 图片格式实例：后缀 → gpui ImageFormat。
#[test]
fn image_formats() {
    assert_eq!(kind("png").name(), "image");
    assert_eq!(kind("jpeg").name(), "image");
    assert_eq!(kind("webp").name(), "image");
}
