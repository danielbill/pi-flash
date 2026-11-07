//! composer 控件行：左 = 图片/工具预设，右 = 用量环/模型/思考/发送停止；
//! 附插件与 MCP 两个胶囊按钮。BAR_FONT 单点在此。

use gpui::{Context, MouseButton, SharedString, div, prelude::*, px, rgb};

use super::*;
use crate::Chat;
use crate::PillMenu;
use crate::i18n::tr;
use crate::ui::WithTip;
use crate::ui::{icon, icon_hover};

/// 操作栏字体大小（工具预设/模型/思考统一；单点改这里）。
/// inputpanel 操作栏文字 = 面板设置值（字体大小设置.md §1）
const BAR_FONT: f32 = 12.;

/// 悬停标志更新 + 淡出状态机（乱序免疫）：只按 (was_open, open) 转移，

/// 扩展按钮（040 定稿 + 2026-10 改版）：工具胶囊右边，**仅「full+」档激活**。
/// 点击开合扩展菜单（勾选即生效，无确认按钮）；非 full+ 档置灰不可点。
pub(crate) mod toggle;
pub(super) use toggle::{composer_bar, status_bar_toggle};
/// 扩展按钮（040 定稿 + 2026-10 改版）：工具胶囊右边，**仅「full+」档激活**。
/// 点击开合扩展菜单（勾选即生效，无确认按钮）；非 full+ 档置灰不可点。
fn plugin_button(
    chat: &mut Chat,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    // 激活条件只有一个：**当前档是「full+」**（内部键 custom）。运行中也能
    // 点开勾选，清单在下一轮发送前重绑生效（pending_ext_sources 链路）。
    let (is_custom, n, open) = {
        let r = chat.rt().read(cx);
        (
            r.tool_preset_key() == "custom",
            r.ext_sources.len(),
            chat.dlg.pi_extension_picker.is_some(),
        )
    };
    let active = is_custom;
    // 2026-10-13 用户定夺：去掉「扩展」二字 + 方括号计数——按钮 = plug 图标
    // + `[n]`；n=0 也显示 `[0]`（已挂载扩展数的常驻视觉提示）
    let label = format!("[{n}]");
    let color = if !active {
        t.text_faint
    } else if open {
        t.accent
    } else {
        t.text_muted
    };
    let mut el = bar_trigger(
        "plugins-menu",
        Some("plug"),
        16., // 2026-10-15 用户定夺：plug glyph 天然窄长（宽 fill 仅 50%），小档下
        // 光学偏小，上 ICON_MD(16) 补视觉质量（18 试过稍大，13 太小）
        &label,
        false,
        color,
        active,
    );
    if active {
        el = el.on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &gpui::MouseDownEvent, _w, cx| {
                this.toggle_plugin_menu_at(event.position, cx);
            }),
        );
    }
    // 统一定位测量端：抓按钮 bounds（paint 期，每帧覆盖）
    el.child(crate::PillBtns::tracker(&chat.dlg.pill_btn.ext))
        .into_any_element()
}

/// MCP 按钮（043 定稿 + 2026-11 改版）：扩展按钮右边的兄弟节点。**不设档位
/// 守卫**——扩展只在 full+ 档挂载，MCP 各档都装载。按钮 = server 图标 +
/// `[n]`（n=已启用数；n=0 也显示 `[0]`，与扩展按钮同款常驻提示），常驻渲染。
/// 点击开合 MCP 勾选菜单（勾选即生效，写盘 + 运行中标记重绑）。
fn mcp_button(
    chat: &mut Chat,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    let (n, open) = (
        chat.cfg
            .mcp_servers
            .iter()
            .filter(|s| s.scope == pi_link::mcp::Scope::Global && s.enabled)
            .count(),
        chat.dlg.mcp_picker.is_some(),
    );
    let label = format!("[{n}]");
    let color = if open { t.accent } else { t.text_muted };
    let el = bar_trigger("mcp-menu", Some("server"), 13., &label, false, color, true)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &gpui::MouseDownEvent, _w, cx| {
                this.toggle_mcp_menu_at(event.position, cx);
            }),
        );
    // 统一定位测量端：抓按钮 bounds（paint 期，每帧覆盖）
    el.child(crate::PillBtns::tracker(&chat.dlg.pill_btn.mcp))
        .into_any_element()
}

/// 操作栏触发钮基座（InputPanel 五颗触发钮唯一样式落点：工具/模型/思考/
/// 扩展/MCP，2026-11-07 自各处内联样式收口）：28px 高、gap5、圆角 8、
/// BAR_FONT 字号；可选前置 icon（`icon_hover` 悬停提亮，尺寸 `icon_size`，
/// plug 特档 16 见下）+ 文案 + 可选右挂 chevron。`color` = icon+文案色
/// （open→accent / 锁定→text_faint / 常态→text_muted 由调用方按各自语义
/// 给），`hoverable` = 可点态（挂 cursor + hover 洗底提字；锁定钮不挂）。
/// 点击行为、tracker、守卫由调用方续挂——设置·子代理页表单
/// （settings/agents）同款复用。
pub(crate) fn bar_trigger(
    id: &'static str,
    icon_name: Option<&'static str>,
    icon_size: f32,
    label: &str,
    chevron: bool,
    color: u32,
    hoverable: bool,
) -> gpui::Stateful<gpui::Div> {
    let ui = crate::appearance::ui_size;
    let t = crate::appearance::theme::theme();
    let mut el = div()
        .id(id)
        .h(px(28.))
        .flex()
        .items_center()
        .gap(px(5.))
        .rounded(px(8.))
        .text_size(ui(BAR_FONT))
        .text_color(rgb(color))
        .children(icon_name.map(|name| icon_hover(name, icon_size, color)))
        .child(SharedString::from(label.to_string()))
        .when(chevron, |d| {
            d.child(icon(
                "chevron-down",
                10.,
                if hoverable { t.text_dim } else { t.text_faint },
            ))
        });
    if hoverable {
        el = el
            .cursor_pointer()
            .hover(move |s| s.bg(rgb(t.bg_hover)).text_color(rgb(t.text)));
    }
    el
}

/// 034 会话状态带显示开关：MCP 胶囊右侧（badge-info ICON_SM + SW_SM 24×14）。
/// 关 = 状态带整条不渲染（数据面照旧维护）；状态直切无动画。
/// 左组：`!` shell 模式提示，或 图片 + 工具预设 + 扩展/MCP 胶囊。
/// （自 composer_bar 拆出，函数行数守卫。）
#[allow(clippy::too_many_arguments)]
fn bar_left_group(
    chat: &mut Chat,
    locked: bool,
    tools_locked: bool,
    tools_open: bool,
    tools_label: &str,
    bash_mode: bool,
    bash_excluded: bool,
    t: &'static crate::appearance::theme::Theme,
    cx: &mut Context<Chat>,
) -> gpui::AnyElement {
    if bash_mode {
        // `!` shell 模式：左组换成模式提示（覆盖图片/工具/插件位，
        // 不额外占一行；右侧环/模型/思考/发送照旧）
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .pl(px(8.))
            .h(px(28.))
            .child(icon("icon-terminal-solid", 13., t.text_dim))
            .child(
                div()
                    .text_size(crate::appearance::ui_size(BAR_FONT))
                    .text_color(rgb(t.text_dim))
                    .child(SharedString::from(if bash_excluded {
                        tr("composer.shell_local_only")
                    } else {
                        tr("composer.shell_to_model")
                    })),
            )
            .into_any_element()
    } else {
        // 左侧：图片 + 工具预设（内容裸宽，组内间距 15px——2026-10-13 用户
        // 定夺；盒间距即字形间距，不做按钮内边距）
        div()
            .flex()
            .items_center()
            .gap(px(15.))
            .child(
                // 图片
                div()
                    .id("attach-image")
                    .pl(px(8.)) // 图片与正文首行左对齐：正文=编辑行6+组件12=18，图片=栏10+8=18
                    .flex()
                    .items_center()
                    .text_color(rgb(if locked { t.text_faint } else { t.text_muted }))
                    .when(!locked, |d| d.cursor_pointer())
                    .hover(move |s| {
                        if locked {
                            s
                        } else {
                            s.bg(rgb(t.bg_hover)).text_color(rgb(t.text))
                        }
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &gpui::MouseDownEvent, _w, cx| {
                            if this.rt().read(cx).compacting {
                                return;
                            }
                            this.attach_images(cx);
                        }),
                    )
                    .child(icon_hover(
                        "image",
                        15.,
                        if locked { t.text_faint } else { t.text_muted },
                    )),
            )
            .child(
                // 工具预设「默认∨」（基座 = bar_trigger，点击行为与锚点留在本处）
                bar_trigger(
                    "tools-menu",
                    Some("wrench"),
                    13.,
                    tools_label,
                    true,
                    if tools_locked {
                        t.text_faint
                    } else if tools_open {
                        t.accent
                    } else {
                        t.text_muted
                    },
                    !tools_locked,
                )
                .when(!tools_locked, |d| {
                    d.on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, event: &gpui::MouseDownEvent, _w, cx| {
                            if this.rt().read(cx).compacting {
                                return;
                            }
                            if this.rt().read(cx).agent_running {
                                // 置灰之外再给一句说明：工具预设换不了是因为要重绑进程
                                this.set_status(
                                    crate::i18n::tr("compaction.preset_locked").to_string(),
                                    cx,
                                );
                                return;
                            }
                            this.dlg.pill_anchor = Some(crate::PillBtns::anchor(
                                &this.dlg.pill_btn.tools,
                                event.position,
                            ));
                            this.dlg.pill_menu = match this.dlg.pill_menu {
                                Some(PillMenu::Tools) => None,
                                _ => Some(PillMenu::Tools),
                            };
                            cx.notify();
                        }),
                    )
                })
                // 统一定位测量端：抓按钮 bounds（paint 期，每帧覆盖）
                .child(crate::PillBtns::tracker(&chat.dlg.pill_btn.tools)),
            )
            // 扩展按钮（040 定稿）：**工具胶囊的兄弟节点**，不是子节点——
            // 塞进胶囊里点它会冒泡触发工具菜单（用户踩过）。仅「full+」档
            // 激活，点开扩展菜单（勾选即生效，单列已勾置顶）。
            .child(plugin_button(chat, t, cx))
            // MCP 按钮（043）：扩展按钮右边的兄弟节点（常驻渲染，n=0 显 [0]）
            .child(mcp_button(chat, t, cx))
            // 034 会话状态带显示开关：MCP 右侧（用户定案）
            .child(status_bar_toggle(t, cx))
            .into_any_element()
    }
}
