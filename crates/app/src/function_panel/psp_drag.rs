//! psp 项目行拖拽排序（020，Zed project_panel drag_and_drop 同款三件套）：
//! `on_drag` 发起（载荷 = 组 ws_key，幽灵 = 全行宽项目行小卡 =「收起为
//! 标签」）、行侧 `on_drag_move` 半行判定插入位、容器 `on_drop` 提交重排
//! 并持久化用户排序。
//!
//! 视觉规格对齐 ZCode 侧栏拖拽（dnd-kit DragOverlay parity）：幽灵宽 =
//! 源行宽、锁 x 只纵向跟随；源行 opacity 0 隐藏；活跃块与落点之间的项目
//! **整块**平移一行高让位（[`shift_delta`]，200ms ease 插值，空槽即落点
//! 指示，无指示线）。
//!
//! 状态生命周期：gpui 的拖拽在列表外松手 = 静默取消，不派发任何事件——
//! 状态里的 `at` 时刻被每次 drag_move 续期，看门狗按 staleness 兜底清理
//! （拖出列表区的清除由容器 on_drag_move 的 contains=false 实时做，看门狗
//! 只管「出窗后松手」这种再无事件的死法）。

use std::time::{Duration, Instant};

use gpui::{SharedString, div, prelude::*, px, rgb};

use crate::Chat;
use crate::appearance::theme;
use crate::services::workspace::same_ws_key;
use crate::ui::icon;
use crate::ui::tokens::align;

/// 拖拽中的项目行收起为标签后，看门狗判定「再无 drag_move 续期」的阈值。
/// 任何一次 mouse move（窗口内）都会续期，正常拖拽远快于该节奏。
const DRAG_STALE: Duration = Duration::from_millis(700);

/// 看门狗轮询间隔（< DRAG_STALE，清理延迟最多一个周期）。
const WATCHDOG_TICK: Duration = Duration::from_millis(400);

/// 避让位移过渡时长（ZCode 侧栏 dnd-kit 默认 `transform 200ms ease`）。
const SHIFT_MS: u64 = 200;

/// 避让动画的重绘驱动节拍（~60fps；仅动画在飞时存活）。
const SHIFT_TICK: Duration = Duration::from_millis(16);

/// 项目行拖拽载荷（`on_drag` 的 value；drop 监听按 TypeId 匹配，与文件树
/// 的 TreeDrag 互不串扰）。key = 组 ws_key。
#[derive(Debug, Clone)]
pub(crate) struct ProjDragTab {
    pub(crate) key: String,
}

/// 拖拽进行中的 Chat 态快照：key 对齐当前载荷，`at` = 最近一次 drag_move
/// 时刻（看门狗 staleness 判据；渲染读它判定「收起为标签」与避让位移）。
#[derive(Debug, Clone)]
pub(crate) struct ProjDragState {
    pub(crate) key: String,
    pub(crate) at: Instant,
}

/// 避让位移动画实例：from/to = 前后两个落点（drop_ix），start = 起始时刻。
/// 渲染期按 elapsed 插值两端的静态位移（[`shift_delta`]），到点由驱动清场。
#[derive(Debug, Clone)]
pub(crate) struct ProjShift {
    pub(crate) from: Option<usize>,
    pub(crate) to: Option<usize>,
    pub(crate) start: Instant,
}

/// 组的排序键（用户排序表 / 拖拽载荷统一用它，与 collapsed_keys 同源）。
pub(crate) fn group_key(g: &crate::ProjectGroup) -> String {
    same_ws_key(&g.path.to_string_lossy())
}

/// 单个目标落点下，第 `block` 个项目块的静态位移（px）：落点在活跃块
/// 下方时中间块上移一行让位，上方时下移一行，其余不动。与 ZCode 侧栏的
/// dnd-kit 自定义 SortingStrategy 同构（PF 行距 0，位移量 = 行高本身）。
pub(crate) fn shift_delta(target: Option<usize>, active: usize, block: usize) -> f32 {
    const ROW: f32 = crate::ui::tokens::row::MAIN;
    let Some(t) = target else { return 0. };
    if t > active && block > active && block < t {
        -ROW
    } else if t < active && block >= t && block < active {
        ROW
    } else {
        0.
    }
}

/// 渲染期统一入口：第 `block` 块当前应画的位移（含 200ms 插值）。无拖拽
/// = 0。
pub(crate) fn row_shift(chat: &Chat, block: usize) -> f32 {
    let Some(d) = &chat.psp.proj_drag else {
        return 0.;
    };
    let Some(active) = chat.psp.projects.iter().position(|g| group_key(g) == d.key) else {
        return 0.;
    };
    match &chat.psp.proj_shift {
        Some(s) if (s.start.elapsed().as_millis() as u64) < SHIFT_MS => {
            let p = (s.start.elapsed().as_secs_f32() / (SHIFT_MS as f32 / 1000.)).min(1.);
            let p = gpui::ease_in_out(p);
            let d_from = shift_delta(s.from, active, block);
            let d_to = shift_delta(s.to, active, block);
            d_from + (d_to - d_from) * p
        }
        _ => shift_delta(chat.psp.proj_drop_ix, active, block),
    }
}

/// 拖拽 move 的状态同步漏斗（容器与行侧共用）：在列表区内续期/重建状态，
/// 拖出列表区立即清态（此刻用户已把项目拖离合法落区，空槽该闭合）。
/// 重建点 = 状态缺失（上轮取消后再入）或键不符（新一轮拖拽）。
pub(crate) fn sync_drag(chat: &mut Chat, key: String, inside: bool, cx: &mut gpui::Context<Chat>) {
    if inside {
        if chat.psp.proj_drag.as_ref().is_some_and(|d| d.key == key) {
            chat.psp.proj_drag.as_mut().unwrap().at = Instant::now();
        } else {
            // 重建点：状态缺失（上轮取消后再入，看门狗重新起一个）或键
            // 不符（新一轮拖拽，旧看门狗仍在轮询会自然接手/退出）
            let fresh = chat.psp.proj_drag.is_none();
            chat.psp.proj_drag = Some(ProjDragState {
                key,
                at: Instant::now(),
            });
            chat.psp.proj_drop_ix = None;
            chat.psp.proj_shift = None;
            if fresh {
                spawn_watchdog(cx);
            }
        }
        // drop_ix 由行侧 handler 经 set_drop_target 另算（需要行几何）
    } else if chat.psp.proj_drag.is_some()
        || chat.psp.proj_drop_ix.is_some()
        || chat.psp.proj_shift.is_some()
    {
        chat.psp.proj_drag = None;
        chat.psp.proj_drop_ix = None;
        chat.psp.proj_shift = None;
        cx.notify();
    }
}

/// 落点变化的唯一写入口（行侧半行判定调用）：变化即记一帧避让动画
/// （from = 旧落点，插值从它的静态位移出发）并起重绘驱动；原地返回。
pub(crate) fn set_drop_target(chat: &mut Chat, new: Option<usize>, cx: &mut gpui::Context<Chat>) {
    if chat.psp.proj_drop_ix == new {
        return;
    }
    let from = chat.psp.proj_drop_ix;
    let shift = ProjShift {
        from,
        to: new,
        start: Instant::now(),
    };
    chat.psp.proj_drop_ix = new;
    chat.psp.proj_shift = Some(shift.clone());
    spawn_shift_driver(shift.start, cx);
    cx.notify();
}

/// 拖拽落位重排：把 `key` 的组移到插入位 `target`（拖拽前的下标空间，
/// 0..=len）。返回是否真的移动——拖回原地（target == cur）与相邻
/// （target == cur+1，即自己下缘）都是 no-op。
pub(crate) fn apply_drop(
    projects: &mut Vec<crate::ProjectGroup>,
    key: &str,
    target: usize,
) -> bool {
    let Some(cur) = projects.iter().position(|g| group_key(g) == key) else {
        return false;
    };
    if target == cur || target == cur + 1 {
        return false;
    }
    let g = projects.remove(cur);
    let t = if target > cur { target - 1 } else { target };
    projects.insert(t.min(projects.len()), g);
    true
}

/// 容器 on_drop 的提交路径：清拖拽态；有合法落点就重排并把结果记为用户
/// 排序持久化（020：拖完下次打开依旧）。无落点（原地松手）= 纯取消。
pub(crate) fn commit_drop(chat: &mut Chat, target: Option<usize>, cx: &mut gpui::Context<Chat>) {
    let key = chat.psp.proj_drag.as_ref().map(|d| d.key.clone());
    chat.psp.proj_drag = None;
    chat.psp.proj_drop_ix = None;
    chat.psp.proj_shift = None;
    if let (Some(key), Some(target)) = (key, target)
        && apply_drop(&mut chat.psp.projects, &key, target)
    {
        chat.psp.proj_order = Some(chat.psp.projects.iter().map(group_key).collect());
        chat.persist_ui();
    }
    cx.notify();
}

/// 拖拽看门狗：gpui 对「窗口外松手」不派发任何事件（active_drag 静默清
/// 零），拖拽态只能靠 `at` staleness 兜底——周期检查，无续期即清态收场
/// （隐藏的源行复原、空槽闭合）。有续期则继续轮询到拖拽结束为止。
fn spawn_watchdog(cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(WATCHDOG_TICK).await;
            let alive = this
                .update(cx, |chat, cx| match &chat.psp.proj_drag {
                    None => false,
                    Some(d) => {
                        if d.at.elapsed() > DRAG_STALE {
                            chat.psp.proj_drag = None;
                            chat.psp.proj_drop_ix = None;
                            chat.psp.proj_shift = None;
                            cx.notify();
                            false
                        } else {
                            true
                        }
                    }
                })
                .unwrap_or(false);
            if !alive {
                break;
            }
        }
    })
    .detach();
}

/// 避让动画的重绘驱动：动画期间 ~60fps notify，到点清场（渲染自然落回
/// to 的静态位移，无跳变）。被更新的动画实例接管（start 不同）即退场，
/// 不堆积。
fn spawn_shift_driver(start: Instant, cx: &mut gpui::Context<Chat>) {
    cx.spawn(async move |this, cx| {
        loop {
            cx.background_executor().timer(SHIFT_TICK).await;
            let keep = this
                .update(cx, |chat, cx| {
                    let Some(s) = &chat.psp.proj_shift else {
                        return false;
                    };
                    if s.start != start {
                        return false;
                    }
                    let done = s.start.elapsed().as_millis() as u64 >= SHIFT_MS;
                    if done {
                        chat.psp.proj_shift = None;
                    }
                    cx.notify();
                    !done
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        }
    })
    .detach();
}

/// 拖拽幽灵（gpui 把它画在光标处）：与项目行同款的全行宽小卡——
/// 「项目拖拽时收起为标签」的可见形态。gpui 以 min-content 约束布局
/// 幽灵，必须显式定宽（否则塌缩成内容宽 =「半个标签」）。
pub(crate) struct ProjDragGhost {
    pub(crate) name: String,
    /// 源行宽度（slp_w − 2×LIST_PX）
    pub(crate) width: f32,
    /// 拖起瞬间鼠标 x（锁 x 补偿基准）
    pub(crate) start_mouse_x: f32,
}

impl gpui::Render for ProjDragGhost {
    fn render(
        &mut self,
        window: &mut gpui::Window,
        _cx: &mut gpui::Context<Self>,
    ) -> impl IntoElement {
        let t = theme::theme();
        // 锁 x（ZCode restrictVerticalDragWithinContainer 同款）：gpui 把
        // 幽灵放在 mouse − cursor_offset，横向随鼠标漂移；这里反向补偿，
        // chip 恒与源行左缘对齐、只纵向跟随
        let dx = f32::from(window.mouse_position().x) - self.start_mouse_x;
        div()
            .w(px(self.width))
            .h(px(crate::ui::tokens::row::MAIN))
            .relative()
            .left(px(-dx))
            .flex()
            .items_center()
            .gap(px(8.))
            .pl(px(align::ROW_PL))
            .pr(px(align::LIST_PX))
            .rounded(px(8.))
            .border_1()
            .border_color(rgb(t.border))
            .bg(rgb(t.nav))
            .shadow_lg()
            .text_size(crate::appearance::ui_size(12.))
            .text_color(rgb(t.text_dim))
            .child(icon("folder", 15., t.text_dim))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .child(SharedString::from(self.name.clone())),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_drop, shift_delta};
    use crate::ProjectGroup;
    use std::path::PathBuf;

    fn group(name: &str) -> ProjectGroup {
        ProjectGroup {
            name: name.to_string(),
            path: PathBuf::from(format!(r"D:\{name}")),
            sessions: Vec::new(),
            session_keys: Vec::new(),
            opened_ms: 0,
        }
    }

    fn names(projects: &[ProjectGroup]) -> Vec<String> {
        projects.iter().map(|g| g.name.clone()).collect()
    }

    #[test]
    fn move_down_and_up() {
        // a 下移到 c 之前（target=2）→ [b,a,c]；c 上移到顶（target=0）→ [c,b,a]
        let mut p = vec![group("a"), group("b"), group("c")];
        assert!(apply_drop(&mut p, r"d:\a", 2));
        assert_eq!(names(&p), ["b", "a", "c"]);
        assert!(apply_drop(&mut p, r"d:\c", 0));
        assert_eq!(names(&p), ["c", "b", "a"]);
    }

    #[test]
    fn self_and_neighbor_targets_are_no_ops() {
        // 拖回自己（上缘 cur / 下缘 cur+1）都不动
        let mut p = vec![group("a"), group("b"), group("c")];
        assert!(!apply_drop(&mut p, r"d:\b", 1));
        assert!(!apply_drop(&mut p, r"d:\b", 2));
        assert_eq!(names(&p), ["a", "b", "c"]);
    }

    #[test]
    fn stale_and_unknown_targets_are_safe() {
        // 陈旧落点越界钳尾；载荷键不存在 = False
        let mut p = vec![group("a"), group("b")];
        assert!(apply_drop(&mut p, r"d:\a", 99));
        assert_eq!(names(&p), ["b", "a"]);
        assert!(!apply_drop(&mut p, r"d:\missing", 0));
        assert_eq!(names(&p), ["b", "a"]);
    }

    #[test]
    fn shift_moves_blocks_between_active_and_target() {
        const ROW: f32 = crate::ui::tokens::row::MAIN;
        // [a,b,c,d] 拖 a（active=0）到 d 前（target=3）：b、c 上移一行
        assert_eq!(shift_delta(Some(3), 0, 0), 0.); // 活跃块自己不动
        assert_eq!(shift_delta(Some(3), 0, 1), -ROW);
        assert_eq!(shift_delta(Some(3), 0, 2), -ROW);
        assert_eq!(shift_delta(Some(3), 0, 3), 0.); // 落点之后的块不动
        // 拖 d（active=3）到 b 前（target=1）：b、c 下移一行
        assert_eq!(shift_delta(Some(1), 3, 0), 0.);
        assert_eq!(shift_delta(Some(1), 3, 1), ROW);
        assert_eq!(shift_delta(Some(1), 3, 2), ROW);
        assert_eq!(shift_delta(Some(1), 3, 3), 0.);
        // 相邻落点（no-op 区）与无落点：全零
        assert_eq!(shift_delta(Some(1), 0, 1), 0.);
        assert_eq!(shift_delta(None, 0, 2), 0.);
    }
}
