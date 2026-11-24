//! 046 任务管理页（2026-11-16 用户五条拍板，不再参考设计 HTML）：
//!
//! ① 列表/详情 = 40%/60% 分栏（列表最小宽 400）；② 页头二级 tab = PF 插件
//! 市场简易 tab（MONO 大字 + `|` 分隔，弃融底卡避免与一级页签栏撞样式）；
//! ③ 详情 label/控件左右排（subagent 表单同款 w96/w250）；④ 任务列表默认
//! 分两组：上【脚本任务】下【Agent 任务】（subagent list 同款结构样式）；
//! ⑤ 空列表提示「暂无任务」。数据面读 `chat.tasks`（泵/CRUD 刷新，不自建
//! 轮询）。

use chrono::TimeZone as _;
use std::path::PathBuf;

use gpui::{AnyElement, MouseButton, SharedString, div, prelude::*, px, rgb};

use pf_context::domain::tasks::{Outcome, TaskKind, TaskRun};

use crate::Chat;
use crate::appearance::theme::{self, Theme, theme};
use crate::appearance::ui_size;
use crate::editor::markdown::MONO_FAMILY;
use crate::i18n::{tf, tr};
use crate::services::format::fmt_msg_time;
use crate::tasks::DefRow;
use crate::tasks::form::{field, form_head, kind_block, sched_row, type_seg};
use crate::ui::icon;
use crate::ui::kit::{Btn, config_button, icon_btn, list_row_divider, sidebar_item, status_dot};
use crate::ui::overlay;
use crate::ui::spark_spinner;
use crate::ui::tooltip::WithTip;

// ---------------------------------------------------------------------------
// 页装配
// ---------------------------------------------------------------------------

/// 任务管理页主体（content.rs `ContentView::Tasks` 分支）。2026-11-22 用户
/// 令再改版：左列表 100% 高 + 右列 = 上详情 / 1px 横线 / 下任务日志区 50%
/// （模型列表卡样式，只呈现选中任务；2026-11-23 用户令：区域内表格卡向
/// 上对齐、高度随内容条数自动调整上限 360，不随区域拉伸）。
pub(crate) fn page(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> AnyElement {
    let t = theme();
    let sel = chat.tasks.page.sel.clone();

    let list = tasks_list(chat, weak, t);
    let detail = tasks_detail(chat, weak, t, window, cx);
    let log = run_log_panel(chat, weak, t, &sel);

    div()
        .id("task-page")
        .flex_1()
        .min_h_0()
        .flex()
        .bg(rgb(t.bg))
        .child(list)
        // 列表 ⇆ 右列竖分隔线：§5.13 统一实现（常驻 1px 细线灰，比 0x45
        // 自调灰重一档，2026-11-22 用户令：页面分割线统一左面板样式）。
        // 上下端缩进 15 对齐页边距（同日用户令：竖隔条也要上下留白）
        .child(
            div()
                .relative()
                .w(px(crate::ui::tokens::divider::LINE))
                .flex_none()
                .child(crate::ui::divider::line(
                    crate::ui::divider::LineSpec::vertical(15.),
                    false,
                    t,
                )),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(detail)
                // 详情 ⇄ 日志横线：左面板横分隔线同款（§5.13 统一实现，
                // 两端缩进 15 对齐本页边距，带内居中留白）
                .child(
                    div()
                        .relative()
                        .h(px(crate::ui::tokens::divider::HIT))
                        .flex_none()
                        .child(crate::ui::divider::line(
                            crate::ui::divider::LineSpec::horizontal(
                                15.,
                                crate::ui::tokens::divider::HIT,
                            ),
                            false,
                            t,
                        )),
                )
                .child(log),
        )
        .into_any_element()
}

// ---------------------------------------------------------------------------
// 共用行视觉（任务列表行 / 日志行同语言，截图拍板版）
// ---------------------------------------------------------------------------

/// 行结果槽：running = spinner / ok = 绿点 / failed = 红点 / timeout = 黄点 /
/// skipped = 灰点（纯兜底：2026-11-23 用户令「没执行不属于执行 log」后
/// 不再落 skipped 账、list_runs 已过滤历史行——正常路径到不了这里）。
pub(crate) fn run_slot(r: &TaskRun, t: &'static Theme) -> Option<AnyElement> {
    match r.outcome {
        Outcome::Running => Some(spark_spinner(12., t.accent)),
        Outcome::Ok => Some(status_dot(theme::UNREAD)),
        Outcome::Failed => Some(status_dot(t.danger)),
        Outcome::Timeout => Some(status_dot(theme::WARN)),
        Outcome::Skipped => Some(status_dot(t.text_faint)),
    }
}

/// mono 时间戳 `MM-DD HH:MM:SS`（截图拍板格式）。
/// 时间列占位串（与 fmt_stamp 同格式同长，定时间列宽）
const STAMP_PLACEHOLDER: &str = "00-00 00:00:00";

pub(crate) fn fmt_stamp(ms: i64) -> String {
    match chrono::Local.timestamp_millis_opt(ms) {
        chrono::LocalResult::Single(t) => t.format("%m-%d %H:%M:%S").to_string(),
        _ => String::new(),
    }
}

/// 行 hover 摘要（时间 + 结果 + 原因）。
fn run_tip(r: &TaskRun) -> String {
    let outcome = match r.outcome {
        Outcome::Running => tr("task.running"),
        Outcome::Ok => tr("task.outcome_ok"),
        Outcome::Failed => tr("task.outcome_failed"),
        Outcome::Timeout => tr("task.outcome_timeout"),
        Outcome::Skipped => tr("task.outcome_skipped"),
    };
    let reason = r
        .result_json
        .as_deref()
        .and_then(|j| serde_json::from_str::<serde_json::Value>(j).ok())
        .and_then(|v| {
            v.get("reason")
                .or_else(|| v.get("error"))
                .and_then(|x| x.as_str().map(str::to_string))
        });
    let mut tip = format!("{} · {}", fmt_msg_time(r.started_at), outcome);
    if let Some(reason) = reason {
        tip.push_str(" · ");
        tip.push_str(&reason);
    }
    tip
}

/// 行装配（两 tab 共用，subagent list 同款 30px sidebar_item）：类型 icon
/// （2026-11-17 拍板：icon 用设计 HTML 的 tico——脚本=file-code、
/// Agent=bot，None = 不出）+ 名（纯文本；2026-11-22 拍板：去下划线链接态，
/// 点行 = 详情回填，不再开话）+ 结果槽 + 右缘 mono 时间。`select` =
/// Some(def_id) 时行点击 = 详情回填（任务列表行），且行尾 hover 显影动作钮
/// （立即运行/停用/移除，2026-11-22 用户令从详情表头挪来，子代理行同款）。
/// `open_path` = Some(session_path) 时行名可点 = 开该次会话的可交互任
/// 务页签（046-T，2026-11-24 拍板：可看可聊可介入，永不降级只读——
/// 原只读观察窗口径作废。日志行专属：任务会话已从用户列表隐身，这里
/// 是唯一入口；Agent 任务才落 session_path，脚本行自然不可点；与
/// select 互斥使用）。
#[allow(clippy::too_many_arguments)]
fn task_row(
    row_id: SharedString,
    title: String,
    gray: bool,
    tico: Option<&'static str>,
    slot: Option<AnyElement>,
    tip: Option<String>,
    stamp: String,
    selected: bool,
    weak: &gpui::WeakEntity<Chat>,
    select: Option<String>,
    open_path: Option<PathBuf>,
    running: bool,
    t: &'static Theme,
) -> AnyElement {
    let dot_id = SharedString::from(format!("{row_id}-dot"));
    // 行尾动作钮与行点击/行名各要一份 select/title（主体装配会按值吃掉）
    let select_act = select.clone();
    let title_act = title.clone();
    // 显影组 id（hover 时 时间/动作钮 visibility 互斥，见右槽注释）
    let reveal = select_act
        .as_ref()
        .map(|def_id| SharedString::from(format!("task-row-act-{def_id}")));
    // 颜色继承 sidebar_item（active text / 非活动 text_muted，subagent 行同款）；
    // 仅停用/跳过灰名覆盖。日志行（open_path Some）名可点 = 开该次会话的
    // 可交互任务页签（046-T：open_task_session，可发送可介入；派发本身
    // 不开页签不打扰，这里仍是唯一入口）
    let weak_open = weak.clone();
    let name_el = div()
        .id(SharedString::from(format!("{row_id}-name")))
        .flex_1()
        .min_w_0()
        .truncate()
        .text_size(ui_size(12.))
        .when(gray, |d| d.text_color(rgb(t.text_faint)))
        .when_some(open_path, |d, sp| {
            // 日志行名正常色（2026-11-22 用户令：不出 muted——可点即正名）
            d.when(!gray, |d| d.text_color(rgb(t.text)))
                .cursor_pointer()
                .hover(|h| h.text_color(rgb(t.accent)))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_open.update(cx, |c, cx| {
                        c.open_task_session(sp.clone(), cx);
                    });
                })
        })
        .child(SharedString::from(title));
    let weak_row = weak.clone();
    sidebar_item(row_id, selected)
        // 046 任务行规格（2026-11-22 用户令）：行高 36 + 行间 4（§5.8 新档
        // row::TASK；基座 30/mb1 按后写覆盖）
        .h(px(crate::ui::tokens::row::TASK))
        .mb(px(crate::ui::tokens::space::SP2))
        // subagent 列表同款：列表区与详情区同底，只留分组头与右分隔线
        .bg(rgb(t.bg))
        .when_some(select, |d, def_id| {
            // 显影 group 挂行容器（gpui 陷阱：group 与 group_hover 同元素
            // 永不生效——compute_style 先于自身 push，见 docs/agents/gpui.md）
            d.group(SharedString::from(format!("task-row-act-{def_id}")))
                .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                    let _ = weak_row.update(cx, |c, cx| {
                        c.task_select(&def_id, cx);
                    });
                })
        })
        // 设计 HTML sp-li 行序：tico → 名 → 结果槽 → 时间。行首 tico =
        // 类型图标（脚本 file-code / Agent bot，2026-11-22 用户澄清：状态
        // 图标在行尾动作钮，不在此处）；尺寸走规范 MD。
        .children(tico.map(|name| {
            icon(
                name,
                crate::ui::tokens::icon::MD,
                if gray { t.text_faint } else { t.text_dim },
            )
        }))
        .child(
            div()
                .id(dot_id)
                .w(px(16.))
                .flex_none()
                .flex()
                .items_center()
                .when_some(slot, |d, s| d.child(s))
                .when_some(tip, |d, tip| d.with_tip_text(tip)),
        )
        .child(name_el)
        // 右槽（2026-11-22 用户令）：时间恒右对齐；列表行 hover 时三动作钮
        // 盖在时间上——visibility 互斥（notification.rs 同款 invisible ⇄
        // visible；invisible 不吃命中，时间区点击照常选中行）。槽 min_w =
        // 三钮宽（22×3 + SP2×2，与 task_act_btn 尺寸同源），时间短于钮区时
        // 名字也不被 hover 钮盖住。日志行无动作钮，恒显时间。无时间戳行
        // 以占位串撑满时间列宽——名字截断位 = 时间列左缘，各行对齐。
        .child({
            // 时间在 hover 组内反向隐身（reveal 组挂行容器，见上）
            let stamp_el = div()
                .flex_none()
                .font_family(MONO_FAMILY)
                // 活动行 SEMIBOLD 不下渗进时间戳
                .font_weight(gpui::FontWeight::NORMAL)
                .text_size(ui_size(11.))
                .text_color(rgb(t.text_faint))
                // 运行中恒隐（2026-11-24 用户令：运行时行不显时间，右槽
                // 直接显 stop；日志列表保持原设计不受影响）
                .when(running, |d| d.invisible())
                .when_some(reveal, |d, r| d.group_hover(r, |s| s.invisible()))
                // 无时间戳（未跑过）行占位同宽假串：时间列恒定宽，名字统一
                // 截断在时间列左缘（否则退到三钮 min_w，截断位落到时间列中间
                // ——2026-11-23 用户报）；invisible 只留布局宽、不吃命中
                .when(stamp.is_empty(), |d| d.invisible())
                .child(SharedString::from(if stamp.is_empty() {
                    STAMP_PLACEHOLDER.to_string()
                } else {
                    stamp
                }));
            let mut slot = div()
                .relative()
                .h(px(crate::ui::tokens::row::TASK))
                .flex_none()
                .flex()
                .items_center()
                .child(stamp_el);
            if let Some(def_id) = select_act {
                slot = slot
                    .justify_end()
                    .min_w(px(22. * 3. + crate::ui::tokens::space::SP2 * 2.))
                    .child(task_row_actions(&def_id, &title_act, !gray, running, weak));
            }
            slot
        })
        .into_any_element()
}

/// 行尾 hover 显影动作钮（2026-11-22 用户令：立即运行/停用/移除从详情表头
/// 挪来；同日用户令：绝对定位盖时间——时间右对齐，hover 时钮/时间 visibility
/// 互斥）。钮 = kit icon_btn（§5.2 IB_SM，2026-11-22 收编）。移除走确认浮
/// 层（confirm_del）。**运行中**（2026-11-24 用户令）：三 icon 换单枚
/// stop（占运行钮原槽，lucide square），停用/移除隐藏——点击与 InputPanel
/// 停止按钮同效（task_stop_now）。
fn task_row_actions(
    def_id: &str,
    title: &str,
    enabled: bool,
    running: bool,
    weak: &gpui::WeakEntity<Chat>,
) -> AnyElement {
    // group 在行容器（task_row），此处只挂 group_hover——同元素自引用永不
    // 显影（docs/agents/gpui.md「group/group_hover 必须两元素分离」）。
    // 显影 = visibility 互斥（invisible 默认 + hover visible，notification.rs
    // 同款）——绝对定位盖在时间上（右槽见 task_row），invisible 不吃命中
    let reveal = SharedString::from(format!("task-row-act-{def_id}"));
    let weak_run = weak.clone();
    let id_run = def_id.to_string();
    let weak_tog = weak.clone();
    let id_tog = def_id.to_string();
    let weak_del = weak.clone();
    let id_del = def_id.to_string();
    let title_del = title.to_string();
    div()
        .absolute()
        .top(px(0.))
        .bottom(px(0.))
        .right(px(0.))
        .flex()
        .items_center()
        .gap(px(crate::ui::tokens::space::SP2))
        // 非运行态 hover 显影（visibility 互斥：时间组内反向隐身）；运行
        // 态恒显，不挂 invisible/group_hover
        .when(!running, |d| {
            d.invisible().group_hover(reveal, |s| s.visible())
        })
        .font_weight(gpui::FontWeight::NORMAL)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(if running {
            // 046-T2 stop：占运行钮原槽；点击 = InputPanel 停止按钮同效；
            // 图标 = 实心方块 assets/icons/stop.svg（square.svg 是 12×12
            // 细描边轮廓，16px 下净宽仅 8px，2026-11-24 用户报太小）
            task_act_btn(
                format!("task-act-stop-{id_run}"),
                "stop",
                "task.stop",
                false,
                &weak_run,
                move |c, cx| c.task_stop_now(&id_run, cx),
            )
        } else {
            task_act_btn(
                format!("task-act-run-{id_run}"),
                "play",
                "task.run_now",
                false,
                &weak_run,
                move |c, cx| c.task_run_now(&id_run, cx),
            )
        })
        .when(!running, |d| {
            // 停用/移除仅非运行态显影（运行中只留 stop，2026-11-24 用户令）
            d.child(task_act_btn(
                format!("task-act-toggle-{id_tog}"),
                // 图标 = 当前状态（2026-11-22 用户令：状态与 icon 对应，不反着）
                // ：可执行态 rotate-clock / 暂停态 pause；点击翻状态
                if enabled { "rotate-clock" } else { "pause" },
                if enabled {
                    "task.disable"
                } else {
                    "task.enable"
                },
                false,
                &weak_tog,
                move |c, cx| c.task_toggle(&id_tog, cx),
            ))
            .child(task_act_btn(
                format!("task-act-del-{id_del}"),
                "trash",
                "task.remove",
                false,
                &weak_del,
                move |c, cx| {
                    c.tasks.page.confirm_del = Some((id_del.clone(), title_del.clone()));
                    cx.notify();
                },
            ))
        })
        .when(running, |d| {
            // 运行中时间恒隐、stop 恒显（不 hover 显影，2026-11-24 用户
            // 令）；stop 后两枚 22px 幽灵占位撑出与三钮态同宽（22×3+
            // SP2×2）的行尾组——stop 落在 play 原槽（右边第三位），两态
            // 切换钮不跳动
            d.child(div().w(px(22.)).flex_none())
                .child(div().w(px(22.)).flex_none())
        })
        .into_any_element()
}

/// 22px 小图标动作钮（§5.2 IB_SM 行尾动作钮）：2026-11-22 收编至
/// [`crate::ui::kit::icon_btn`]（几何逐值平移，kit 版为唯一画法）。
fn task_act_btn(
    id: String,
    name: &'static str,
    tip_key: &'static str,
    locked: bool,
    weak: &gpui::WeakEntity<Chat>,
    on_click: impl Fn(&mut Chat, &mut gpui::Context<Chat>) + 'static,
) -> AnyElement {
    icon_btn(id, weak, name, tip_key, locked, on_click)
}

// ---------------------------------------------------------------------------
// ① 任务页：左列表（100% 高）+ 右列（详情 / 日志）
// ---------------------------------------------------------------------------

/// 左列表（40% 最小宽 400，2026-11-16 拍板；2026-11-22 用户令改 100% 高，
/// 日志区挪右列）：单一标题「全部任务 [n]」+ 右缘「新增」钮；空库整列
/// 「暂无任务」。
fn tasks_list(chat: &mut Chat, weak: &gpui::WeakEntity<Chat>, t: &'static Theme) -> AnyElement {
    let sel = chat.tasks.page.sel.clone();
    let mut list = div()
        .id("task-sp-list")
        .w(gpui::relative(0.4))
        .min_w(px(400.))
        .flex_none()
        // 右缘分隔线已由页装配的 §5.13 竖 divider band 承担（2026-11-22），
        // 这里不再叠 0x45 旧 border——双线会并肩出 1px 错位
        .overflow_y_scroll()
        .px(px(6.))
        .pt(px(8.))
        .flex()
        .flex_col();
    // 标题栏（空库也出——「新增」是唯一建任务入口）：全部任务 [n] + 右缘
    // 新增。pb 13（2026-11-22 用户令：栏下 padding +10）；pr 8 对齐行内容
    // 缩进——新建钮右缘与任务行时间戳右缘同一竖线（行内容吃 sidebar_item
    // 的 px8，标题栏原来只 pl 不 pr，钮比时间戳探出 8px）
    list = list.child(
        div()
            .flex()
            .items_center()
            .pl(px(8.))
            .pr(px(8.))
            .pt(px(8.))
            .pb(px(13.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(ui_size(12.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(t.text_muted))
                    .child(SharedString::from(format!(
                        "{} {}",
                        tr("task.filter_all"),
                        chat.tasks.defs.len()
                    ))),
            )
            .child(config_button(
                "task-list-new",
                weak,
                &tr("task.new"),
                Btn::Primary,
                true,
                false,
                |c, cx| c.task_new_form(cx),
            )),
    );
    if chat.tasks.defs.is_empty() {
        list = list.child(
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(ui_size(12.))
                .text_color(rgb(t.text_faint))
                .child(tr("task.empty").to_string()),
        );
    } else {
        for DefRow { def, spec, .. } in &chat.tasks.defs {
            let last = chat.tasks.runs.iter().find(|r| r.def_id == def.id);
            let running = chat.tasks.running.iter().any(|k| *k == def.id);
            let (slot, tip) = if running {
                (Some(spark_spinner(12., t.accent)), None)
            } else {
                match last {
                    Some(r) => (run_slot(r, t), Some(run_tip(r))),
                    None => (None, None),
                }
            };
            let stamp = last.map(|r| fmt_stamp(r.started_at)).unwrap_or_default();
            let id = def.id.clone();
            // 设计 HTML tico：脚本行 file-code / Agent 行 bot（状态图标在
            // 行尾动作钮，2026-11-22 用户澄清）
            let tico: &'static str = if matches!(spec.kind, TaskKind::Agent) {
                "bot"
            } else {
                "file-code"
            };
            list = list.child(task_row(
                SharedString::from(format!("task-sp-{id}")),
                def.title.clone(),
                !def.enabled,
                Some(tico),
                slot,
                tip,
                stamp,
                sel.as_deref() == Some(id.as_str()),
                weak,
                Some(id.clone()),
                None,
                chat.tasks.running.iter().any(|k| k.as_str() == id),
                t,
            ));
        }
    }
    list.into_any_element()
}

/// 右详情（60%）：subagent 表单同构（label w96 左 + 控件右）。
/// flex_1 保持——在右列纵排里吃上半区。
fn tasks_detail(
    chat: &mut Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    window: &gpui::Window,
    cx: &mut gpui::Context<Chat>,
) -> AnyElement {
    let detail = div()
        .id("task-sp-detail")
        .flex_1()
        .min_w_0()
        .overflow_y_scroll()
        .p(px(15.))
        .flex()
        .flex_col()
        .child(form_head(chat, weak, t, cx))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(crate::ui::tokens::space::SP7))
                .mt(px(crate::ui::tokens::space::SP7))
                .flex_1()
                .child(field("task.type", type_seg(chat, weak, t, window), false))
                .child(field("task.sched", sched_row(chat, weak, t), false))
                .children(kind_block(chat, weak, t, window, cx)),
        );
    detail.into_any_element()
}

/// 右列下方任务日志**区域**（恒 50% 高，2026-11-23 二令恢复定高稳定版
/// 式）：区域内表格卡**向上对齐**，高度随内容条数自动调整（`Capped(360)`
/// 统一模型列表口径，超上限出滚动）——2026-11-23 一令。模型列表卡样式
/// （vlist 壳同款：rounded 6 + 1px 边框 + 行间内缩淡分隔线），只呈现选中
/// 任务的运行记录。行 = 结果点 + 任务名（**恒正常色**——日志是已发生的
/// 历史，与任务启停态无关，2026-11-22 用户令：dim 只表无效/注释，日志不
/// 是注释）+ 右缘 mono 时间；Agent 任务行名可点 = 开该次会话只读观察窗。
/// 未选中任务/无记录出空态。列表 = vlist 组件（uniform_list + 滚动条 +
/// 壳——滚动条不许再手滚丢）。
fn run_log_panel(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    t: &'static Theme,
    sel: &Option<String>,
) -> AnyElement {
    // 行数据先行抽出（vlist 行闭包 'static：不借 chat——TaskRun Clone，
    // 标题/开话路径全部预解析）
    let rows: Vec<(TaskRun, String, Option<PathBuf>)> = chat
        .tasks
        .runs
        .iter()
        // 只出选中任务的日志（2026-11-22 用户令：不再混杂）
        .filter(|r| sel.as_deref() == Some(r.def_id.as_str()))
        .map(|r| {
            let title = chat
                .tasks
                .defs
                .iter()
                .find(|d| d.def.id == r.def_id)
                .map(|d| d.def.title.clone())
                .unwrap_or_else(|| r.def_id.clone());
            (
                r.clone(),
                title,
                r.session_path.as_deref().map(PathBuf::from),
            )
        })
        .collect();

    let empty = sel.is_none()
        || chat
            .tasks
            .defs
            .iter()
            .find(|d| Some(&d.def.id) == sel.as_ref())
            .is_none();
    let empty_key: &'static str = if empty {
        "task.log_none_sel"
    } else {
        "task.log_empty"
    };

    let rows_v = rows.clone();
    let weak_rows = weak.clone();
    let list = crate::ui::vlist::vlist(
        "task-log-vlist",
        rows.len(),
        crate::ui::tokens::row::TASK,
        // 高度随内容条数、上限 360（2026-11-23 用户令：统一模型列表行为）
        crate::ui::vlist::VListHeight::Capped(360.),
        true,
        true,
        empty_key,
        None,
        move |ix, _window, _cx| {
            let (r, title, open_path) = &rows_v[ix];
            let id = SharedString::from(format!("task-log-{}-{}", r.def_id, r.started_at));
            let dot_id = SharedString::from(format!("{id}-dot"));
            let weak_open = weak_rows.clone();
            let open_path = open_path.clone();
            div()
                .id(id)
                .w_full()
                .h(px(crate::ui::tokens::row::TASK))
                .pl(px(crate::ui::tokens::space::SP6))
                .pr(px(crate::ui::tokens::space::SP6))
                .relative()
                .flex()
                .items_center()
                .gap(px(crate::ui::tokens::space::SP4))
                // 行分隔线内缩对齐内容留白（2026-11-23 用户令：不满铺顶两头）
                .when(ix > 0, |d| {
                    d.child(list_row_divider(
                        crate::ui::tokens::space::SP6,
                        crate::ui::tokens::space::SP6,
                    ))
                })
                // 结果点（hover 摘要同前）
                .child(
                    div()
                        .id(dot_id)
                        .w(px(crate::ui::tokens::icon::MD))
                        .flex_none()
                        .flex()
                        .items_center()
                        .when_some(run_slot(r, t), |d, s| d.child(s))
                        .with_tip_text(run_tip(r)),
                )
                // 一行：名恒正常色（日志=历史，不随任务启停变灰，2026-11-22
                // 用户令）+ 右对齐 mono 时间
                .child({
                    let mut name_el = div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(ui_size(12.))
                        .cursor_pointer()
                        .text_color(rgb(t.text))
                        .when(open_path.is_some(), |d| {
                            d.hover(|h| h.text_color(rgb(t.accent)))
                        })
                        .child(SharedString::from(title.clone()));
                    if let Some(sp) = open_path {
                        name_el = name_el.on_mouse_down(MouseButton::Left, move |_, _, cx| {
                            let _ = weak_open.update(cx, |c, cx| {
                                c.open_task_session(sp.clone(), cx);
                            });
                        });
                    }
                    name_el
                })
                .child(
                    div()
                        .flex_none()
                        .font_family(MONO_FAMILY)
                        .text_size(ui_size(11.))
                        .text_color(rgb(t.text_faint))
                        .child(SharedString::from(fmt_stamp(r.started_at))),
                )
                .into_any_element()
        },
    );

    div()
        .id("task-log-panel")
        // 日志**区域**恒 50% 高（2026-11-23 二令：区域定高稳定版式）；区
        // 域内的表格卡**向上对齐**、高度随内容条数（Capped 360，见下），
        // 不再随区域拉伸
        .h(gpui::relative(0.5))
        .flex_none()
        .flex()
        .flex_col()
        .bg(rgb(t.bg))
        // 标题：任务日志 + 数量（2026-11-22 用户令：不加粗不变色，正常
        // 字体——此行只是分区标题，不是组头）
        .child(
            div()
                .flex_none()
                .flex()
                .items_center()
                .gap(px(crate::ui::tokens::space::SP2))
                .px(px(15.))
                .pt(px(10.))
                .pb(px(6.))
                .child(
                    div()
                        .text_size(ui_size(12.))
                        .text_color(rgb(t.text))
                        .child(tr("task.log_title").to_string()),
                )
                .child(
                    div()
                        .text_size(ui_size(12.))
                        .text_color(rgb(t.text))
                        .child(SharedString::from(rows.len().to_string())),
                ),
        )
        // vlist 壳自带 rounded/border/滚动条 + Capped 定高（内容条数，上限
        // 360）：外层只留边距（mx 对齐详情 px 15）
        .child(
            div()
                .id("task-log-list")
                .mx(px(15.))
                .mb(px(12.))
                .flex()
                .flex_col()
                .child(list),
        )
        .into_any_element()
}

/// 移除确认浮层（overlay_stack 挂载；040 卸载确认同款机制）。
pub(crate) fn remove_confirm_overlay(chat: &Chat, cx: &gpui::Context<Chat>) -> Option<AnyElement> {
    let (del_id, del_title) = chat.tasks.page.confirm_del.as_ref()?.clone();
    let weak = cx.entity().downgrade();
    let message = tf(tr("task.delete_confirm"), &[("title", del_title)]);
    let weak_cancel = weak.clone();
    let weak_confirm = weak.clone();
    Some(
        overlay::confirm(
            &chat.dlg.dialog_focus,
            message,
            move |_, cx| {
                let _ = weak_cancel.update(cx, |c, cx| {
                    c.tasks.page.confirm_del = None;
                    cx.notify();
                });
            },
            move |_, cx| {
                let _ = weak_confirm.update(cx, |c, cx| {
                    c.task_delete_confirmed(&del_id, cx);
                });
            },
        )
        .into_any_element(),
    )
}
