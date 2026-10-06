//! 010-启动：启动页。参考 codex（docs/模块设计/image.bmp）——近黑背景、
//! logo 居中、普通窗口不最大化；pi 附着（或超时兜底，见 main.rs 的启动
//! 闸门）后由 Chat 揭幕，落点即启动恢复的既有分流：有上次会话 → 030
//! 会话界面，无 → 012 新会话页（session_hero）。

use std::time::Duration;

use gpui::{div, prelude::*, px, rgb, WindowControlArea};

/// 最短展示：从启动页首次绘制起算（见 `mark_splash_painted`）。低于这个数
/// 观感是「闪一下黑」而不是启动页。
pub(crate) const MIN_SPLASH: Duration = Duration::from_millis(500);
/// 就绪兜底：pi 起不来（缺 node / 被占用等）也不能把用户困在启动页。
pub(crate) const SPLASH_TIMEOUT: Duration = Duration::from_millis(3000);

static SPLASH_PAINT: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();

/// 启动页首帧绘制时刻（splash 渲染分支调用；OnceLock 只记第一次）。
pub(crate) fn mark_splash_painted() {
    let _ = SPLASH_PAINT.set(std::time::Instant::now());
}

/// 启动页已绘制时长——None = 还没画出来（忙机首帧可晚于 pi 附着，
/// 揭幕闸门必须等这个有值，否则用户一帧启动页都见不到）。
pub(crate) fn splash_paint_age() -> Option<Duration> {
    SPLASH_PAINT.get().map(|t| t.elapsed())
}

/// 全幅启动页。取色自参考图：底 0x202020、logo 0xa1a1a1。顶部 40px 注册
/// 拖拽区——client-side titlebar 下没有原生拖拽带，否则启动页拖不动窗口
///（关闭走 Alt+F4 / 任务栏；启动页通常亚秒级，控制钮不画）。
pub(crate) fn splash_view() -> gpui::Div {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(0x202020))
        .child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w_full()
                .h(px(40.))
                .window_control_area(WindowControlArea::Drag),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .items_center()
                .child(crate::ui::icon("logo-marks", 88., 0xa1a1a1))
                // logo 下划线：粗 4px，宽对齐 logo 图形（88px 方框里图形占
                // ~69% 宽）。svg 方框在图形下方留了 ~24px 空白（256 画布
                // 底部 71u × 88/256），负 margin 吃掉它 → 视觉间隙 ~8px。
                .child(
                    div()
                        .w(px(60.))
                        .h(px(4.))
                        .rounded(px(2.))
                        .mt(px(-16.))
                        .bg(rgb(0xa1a1a1)),
                ),
        )
}
