//! markdown 链接目标：渲染构建期捕获点击目标（thread_local MD_TARGET）
//! 与本地路径判定/百分号解码。

/// 点击打开文件标签页的目标：渲染入口由调用方 set（消息区 / 文件 md 预览），
/// 元素**构建期** clone 进事件闭包——事件期不读 thread_local，无跨帧陈旧。
#[derive(Clone)]
pub(super) struct MdTarget {
    chat: gpui::WeakEntity<crate::Chat>,
    /// md 预览场景的相对路径基准（预览文件所在目录）；聊天消息 None =
    /// 点击时按 chat.ws.cwd 解析
    base: Option<std::path::PathBuf>,
}

thread_local! {
    static MD_TARGET: std::cell::RefCell<Option<MdTarget>> =
        const { std::cell::RefCell::new(None) };
}

/// markdown 渲染入口处由调用方设置；消息区传 `None` base（相对路径按
/// chat.ws.cwd），文件 md 预览传预览文件所在目录（pi-web baseDir 同型）。
pub(crate) fn set_link_target(
    chat: gpui::WeakEntity<crate::Chat>,
    base: Option<std::path::PathBuf>,
) {
    MD_TARGET.with(|c| *c.borrow_mut() = Some(MdTarget { chat, base }));
}

/// 构建期取当前目标（clone 进闭包）。
pub(super) fn md_target() -> Option<MdTarget> {
    MD_TARGET.with(|c| c.borrow().clone())
}

/// 点击打开路径：有目标走 `Chat::open_file_tab`（已开复用置顶 / 二进制·
/// 超限·缺失兜底都在其中；html 开源码 tab）；无目标退回系统关联打开
/// （cx.open_url 对本地路径 = ShellExecute）。
pub(super) fn open_path_with(target: Option<MdTarget>, path: String, cx: &mut gpui::App) {
    if let Some(MdTarget { chat, base }) = target {
        let _ = chat.update(cx, |c, cx| {
            let p = std::path::Path::new(&path);
            let p = if p.is_absolute() {
                p.to_path_buf()
            } else {
                // base（预览文件所在目录）优先；无 base 且无项目 → 无从解析
                let Some(cwd) = c.ws.cwd.as_deref() else {
                    return;
                };
                base.as_deref().unwrap_or(cwd).join(p)
            };
            c.open_file_tab(p, cx);
        });
    } else {
        cx.open_url(&path);
    }
}

/// TextView 消息区链接点击的统一入口（gpui-component link handler 转发至
/// 此）：本地文件 → 应用内文件 tab（open_path_with），其余 → 系统浏览器。
/// MD_TARGET 事件期读 thread_local（最后一次 render_msg 设置的 chat weak
/// + None base），与自研渲染器 InteractiveText 闭包同值。
pub(crate) fn dispatch_md_link(url: &str, cx: &mut gpui::App) {
    if let Some(path) = as_file_path(url) {
        open_path_with(md_target(), path, cx);
    } else {
        cx.open_url(url);
    }
}

/// 行内 code 文本 / 链接 href 形似本地文件路径 → 可打开的路径串（已剥
/// `:行[:列]` 后缀）。判定锚定 code span / href 边界，路径内允许空格与
/// CJK（「已写入 `D:\...\1 每日博文\x.md`」场景）；不做盘上存在性检查
/// （渲染期 fs 调用不可接受，缺失由 open_file_tab 状态栏兜底；目录由
/// open_file_tab 转资源管理器打开，zed 路径链接同型）。
pub(crate) fn as_file_path(input: &str) -> Option<String> {
    let mut s = input.trim();
    // 成对引号包裹（agent 常见 `'D:\x y.md'` / `"D:\x.md"`）
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        s = s[1..s.len() - 1].trim();
    }
    if s.is_empty() {
        return None;
    }
    // file:// URL 解码后按本地路径规则重走
    if let Some(rest) = s.strip_prefix("file://") {
        let decoded = percent_decode(rest);
        if let Some(body) = decoded.strip_prefix('/') {
            let body = body.trim_start_matches('/'); // 容错多余斜杠
            // file:///C:/x → C:/x（盘符）；file:///home/u → /home/u（POSIX 根）
            if looks_like_win_drive(body) {
                return Some(body.to_string());
            }
            return is_local_path(&decoded).then(|| decoded);
        }
        // file://server/share/x → UNC \\server\share\x（Windows 惯用反斜杠）
        let unc = format!("\\\\{}", decoded.replace('/', "\\"));
        return is_local_path(&unc).then_some(unc);
    }
    // 其他 scheme 一律拒绝（http:/https:/data:…；单字母 `X:` 是盘符不算）
    if let Some(colon) = s.find(':') {
        let head = &s[..colon];
        if head.len() >= 2 && head.chars().all(|c| c.is_ascii_alphabetic()) {
            return None;
        }
    }
    // 剥尾部行列号（:12 / :12:3，纯数字才算；盘符 `C:\x` 的冒号后非数字不受影响）
    let mut s = s;
    for _ in 0..2 {
        let Some(colon) = s.rfind(':') else { break };
        let tail = &s[colon + 1..];
        if colon > 0 && !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
            s = &s[..colon];
        } else {
            break;
        }
    }
    is_local_path(s).then(|| s.to_string())
}

/// 路径形态判定：绝对（盘符/UNC/POSIX 根）或含分隔符的相对路径；纯文件名
/// 不判路径（防 `TODO`/`README.md` 误判——写过的文件已有轮末 chips 链路）。
fn is_local_path(s: &str) -> bool {
    if s.is_empty() || s.starts_with('#') || s.starts_with('?') {
        return false;
    }
    if looks_like_win_drive(s) || s.starts_with("\\\\") || s.starts_with('/') {
        return true;
    }
    s.contains('/') || s.contains('\\')
}

fn looks_like_win_drive(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%'
            && i + 2 < b.len()
            && let Some(hi) = (b[i + 1] as char).to_digit(16)
            && let Some(lo) = (b[i + 2] as char).to_digit(16)
        {
            out.push((hi * 16 + lo) as u8);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
