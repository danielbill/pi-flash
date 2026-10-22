//! PF 插件市场页（090-4 v1 最小切片）。
//!
//! 数据面：内置索引（`assets/plugin-market.json`，随 PF 发版更新）+ 台账
//! `plugins.json`。UI 面逐条对齐原型 `docs/UI设计/PF插件市场-原型.html`：
//! 卡片 100px 高 / padding 10 / 图标 36 顶部对齐无边框 / 垃圾桶标题行右缘顶部
//! 对齐 / 安装更新 BTN_SM 底部对齐 / 已装 = 版本号前绿✓（ICON_INDICATOR 10）。
//! 配色全部走 theme.rs 语义 token（跟随当前主题，不硬编码色板）。
//!
//! 交互三态：未装 [安装] / 有更新 [🗑][更新] / 已最新 [🗑]（垃圾桶两击确认）。
//! 索引外的包不存在的入口——「只能从市场获得」由校验链保证。
//!
//! dev-flags `plugins`（默认关）：入口在 `open_pf_market` / 菜单项 / 菜单壳
//! 三处闸门（同 040-1 纪律）。

use std::sync::OnceLock;

use gpui::prelude::*;
use gpui::{AnyElement, FontWeight, MouseButton, SharedString, px};

use pi_link::pf_plugins::{self, IndexEntry};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::{tf, tr};
use crate::ui::tooltip::WithTip;

// ---------------------------------------------------------------------------
// 内置索引（单例）
// ---------------------------------------------------------------------------

fn index() -> &'static pi_link::pf_plugins::MarketIndex {
    static IDX: OnceLock<pi_link::pf_plugins::MarketIndex> = OnceLock::new();
    IDX.get_or_init(|| {
        pi_link::pf_plugins::MarketIndex::parse(include_str!("../../assets/plugin-market.json"))
            .unwrap_or_default()
    })
}

// ---------------------------------------------------------------------------
// 状态（挂 Chat.aux.market.pf）
// ---------------------------------------------------------------------------

/// 页面状态。台账在打开页面时从磁盘重读（唯一真相在文件，内存不缓存写）。
#[derive(Default)]
pub struct PfState {
    /// 安装/更新进行中的包名（该卡按钮转 busy 文案，其余动作忽略）
    pub pending: Option<String>,
    /// 二次确认卸载的包名（点别的任何地方即清除）
    pub confirm: Option<String>,
    /// 最近一次失败原因（显示在列表顶部，下一次动作自动清除）
    pub error: Option<String>,
    /// 台账镜像（open 时重读；安装/卸载完成后重读）
    pub ledger: Vec<pi_link::pf_plugins::LedgerEntry>,
}

fn state_of(entry: &IndexEntry, pf: &PfState) -> &'static str {
    match pf.ledger.iter().find(|l| l.name == entry.name) {
        Some(l) if l.version != entry.version => "update",
        Some(_) => "ok",
        None => "install",
    }
}

// ---------------------------------------------------------------------------
// 打开 / 动作
// ---------------------------------------------------------------------------

impl Chat {
    /// 打开「Pi-Flash 插件」标准页（菜单项是唯一入口）。
    pub(crate) fn open_pf_market(&mut self, cx: &mut gpui::Context<Self>) {
        self.dlg.market_menu = None;
        match self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, crate::PanelTab::PfMarket))
        {
            Some(ix) => self.activate_panel_tab(ix, cx),
            None => {
                self.ws.panel_tabs.push(crate::PanelTab::PfMarket);
                let ix = self.ws.panel_tabs.len() - 1;
                self.activate_panel_tab(ix, cx);
            }
        }
        self.set_content_view(crate::ContentView::PfMarket);
        self.aux.market.pf.ledger = pf_plugins::load_ledger();
        cx.notify();
    }

    /// 安装/更新（同一入口；同版本重装覆盖）。后台线程跑 npm pack → 校验 → 落盘。
    fn pf_install(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        if self.aux.market.pf.pending.is_some() {
            return;
        }
        let Some(entry) = index().entry(&name).cloned() else {
            return; // 索引外：理论上不可达（按钮只按索引渲染）
        };
        // engines.pf 下限（semver；app 已有依赖，pi-link 不引）
        if !entry.min_pf.is_empty() {
            let cur = semver::Version::parse(env!("CARGO_PKG_VERSION")).ok();
            let req = semver::VersionReq::parse(&entry.min_pf).ok();
            if let (Some(cur), Some(req)) = (cur, req) {
                if !req.matches(&cur) {
                    self.aux.market.pf.error = Some(tf(
                        "market.pf.need_newer_pf",
                        &[
                            ("min", entry.min_pf.clone()),
                            ("cur", env!("CARGO_PKG_VERSION").to_string()),
                        ],
                    ));
                    cx.notify();
                    return;
                }
            }
        }
        self.aux.market.pf.pending = Some(name.clone());
        self.aux.market.pf.confirm = None;
        self.aux.market.pf.error = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { pf_plugins::install(&entry) })
                .await;
            let _ = this.update(cx, |c, cx| {
                let pf = &mut c.aux.market.pf;
                pf.pending = None;
                match result {
                    Ok(_) => pf.error = None,
                    Err(e) => pf.error = Some(e),
                }
                pf.ledger = pf_plugins::load_ledger();
                pf.confirm = None;
                cx.notify();
            });
        })
        .detach();
    }

    /// 垃圾桶：两击确认（第一次记下包名并变红洗底，第二次真删）。
    fn pf_uninstall(&mut self, name: String, cx: &mut gpui::Context<Self>) {
        let pf = &mut self.aux.market.pf;
        if pf.pending.is_some() {
            return;
        }
        if pf.confirm.as_deref() != Some(name.as_str()) {
            pf.confirm = Some(name);
            cx.notify();
            return;
        }
        pf.confirm = None;
        pf.pending = Some(name.clone());
        pf.error = None;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { pf_plugins::uninstall(&name) })
                .await;
            let _ = this.update(cx, |c, cx| {
                let pf = &mut c.aux.market.pf;
                pf.pending = None;
                match result {
                    Ok(_) => pf.error = None,
                    Err(e) => pf.error = Some(e),
                }
                pf.ledger = pf_plugins::load_ledger();
                cx.notify();
            });
        })
        .detach();
    }
}

// ---------------------------------------------------------------------------
// 页面
// ---------------------------------------------------------------------------

/// 页面主体（content.rs `ContentView::PfMarket` 分支）。
pub(crate) fn page(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    _window: &mut gpui::Window,
) -> AnyElement {
    let t = crate::appearance::theme::theme();
    let pf = &chat.aux.market.pf;
    let entries = &index().packages;

    let mut col = gpui::div()
        .id("pf-market-list")
        .size_full()
        .overflow_y_scroll()
        .child(
            gpui::div()
                .max_w(px(720.))
                .mx_auto()
                .flex()
                .flex_col()
                .gap(px(8.))
                .p(px(12.))
                .children(entries.iter().map(|e| card(chat, weak, e, t))),
        );

    if entries.is_empty() {
        col = col.child(
            gpui::div().max_w(px(720.)).mx_auto().child(
                gpui::div()
                    .m(px(12.))
                    .p(px(32.))
                    .flex()
                    .justify_center()
                    .text_color(rgb(t.text_muted))
                    .border_1()
                    .border_color(rgb(t.border))
                    .rounded(px(8.))
                    .child(tr("market.pf.empty")),
            ),
        );
    }

    // 最近一次失败原因（下一次动作自动清除）
    if let Some(err) = &pf.error {
        col = col.child(
            gpui::div().max_w(px(720.)).mx_auto().child(
                gpui::div()
                    .mx(px(12.))
                    .mb(px(8.))
                    .text_size(px(11.))
                    .text_color(rgb(t.danger))
                    .child(err.clone()),
            ),
        );
    }

    col.into_any_element()
}

use gpui::rgb;

fn card(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    e: &IndexEntry,
    t: &'static Theme,
) -> AnyElement {
    let pf = &chat.aux.market.pf;
    let st = state_of(e, pf);
    let busy = pf.pending.as_deref() == Some(e.name.as_str());
    let confirming = pf.confirm.as_deref() == Some(e.name.as_str());

    // ── 图标 tile：36×36 顶部对齐（卡片 items_start）· tool_bg 底 · 无边框 ──
    let icon = gpui::div()
        .size(px(36.))
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(t.tool_bg))
        .rounded(px(6.))
        .text_color(rgb(t.accent))
        .child(crate::ui::icon("package", 18., t.accent));

    // ── 标题行：名称 + 版本（已最新 = 绿✓ + 版本）──
    let installed = pf
        .ledger
        .iter()
        .find(|l| l.name == e.name)
        .map(|l| l.version.clone());
    let ver = match st {
        "update" => format!("{} → {}", installed.clone().unwrap_or_default(), e.version),
        _ => installed.clone().unwrap_or_else(|| e.version.clone()),
    };
    let ver_el = gpui::div()
        .id(SharedString::from(format!("pf-ver-{}", e.name)))
        .flex()
        .items_center()
        .gap(px(4.))
        .text_size(px(11.))
        .text_color(if st == "ok" {
            rgb(t.accent)
        } else {
            rgb(t.text_faint)
        })
        .children(if st == "ok" {
            Some(crate::ui::icon("check", 10., t.accent))
        } else {
            None
        })
        .child(ver);

    let titlerow = gpui::div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            gpui::div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(e.display.zh.clone()),
        )
        .child(ver_el);

    // ── 描述（索引 desc；无 desc 的包不渲染该行）──
    let desc = e.desc.as_ref().map(|d| {
        gpui::div()
            .text_color(rgb(t.text_dim))
            .truncate()
            .child(d.zh.clone())
    });

    let p_main = gpui::div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .justify_center()
        .gap(px(4.))
        .child(titlerow)
        .children(desc);

    // ── 动作列：单列双槽（justify_between）——垃圾桶永远顶部，按钮永远底部；
    // 空槽用零高 div 占位，任何状态下右缘都是同一条线 ──
    let weak_trash = weak.clone();
    let mut side = gpui::div()
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .justify_between()
        .items_end();

    let mut top: Option<AnyElement> = None;
    let mut bottom: Option<AnyElement> = None;

    if st != "install" {
        let name = e.name.clone();
        let weak_t = weak_trash.clone();
        // 28×28 danger 键（040-1 card.rs:448 定稿）：描边 35%/底 6%/hover 12%，字形 12px
        let mut trash = gpui::div()
            .id(SharedString::from(format!("pf-trash-{}", e.name)))
            .size(px(28.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(6.))
            .border_1()
            .border_color(gpui::rgba(crate::appearance::theme::danger_alpha(t, 0x59)))
            .bg(gpui::rgba(crate::appearance::theme::danger_alpha(
                t,
                if confirming { 0x1f } else { 0x0f },
            )))
            .text_color(rgb(t.danger))
            .cursor_pointer()
            .hover(|s| s.bg(gpui::rgba(crate::appearance::theme::danger_wash(t))))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let _ = weak_t.update(cx, |c, cx| c.pf_uninstall(name.clone(), cx));
            })
            .child(crate::ui::icon("icon-trash-solid", 12., t.danger));
        if confirming {
            trash = trash.with_tip_text(tr("market.pf.confirm"));
        }
        top = Some(trash.into_any_element());
    }

    let busy_label = |act: &str| -> SharedString {
        if busy {
            tr(if act == "update" {
                "market.pf.updating"
            } else {
                "market.pf.installing"
            })
            .into()
        } else {
            tr(if act == "update" {
                "market.btn.update"
            } else {
                "market.btn.install"
            })
            .into()
        }
    };

    match st {
        "install" => {
            let name = e.name.clone();
            let weak_i = weak.clone();
            let disabled = pf.pending.is_some();
            bottom = Some(crate::ui::button(
                SharedString::from(format!("pf-install-{}", e.name)),
                busy_label("install"),
                crate::ui::button::BtnSize::Sm,
                crate::ui::button::BtnVariant::Primary,
                disabled,
                move |_, _, cx| {
                    let _ = weak_i.update(cx, |c, cx| c.pf_install(name.clone(), cx));
                },
            ));
        }
        "update" => {
            let name = e.name.clone();
            let weak_u = weak.clone();
            let disabled = pf.pending.is_some();
            bottom = Some(crate::ui::button(
                SharedString::from(format!("pf-update-{}", e.name)),
                busy_label("update"),
                crate::ui::button::BtnSize::Sm,
                crate::ui::button::BtnVariant::Primary,
                disabled,
                move |_, _, cx| {
                    let _ = weak_u.update(cx, |c, cx| c.pf_install(name.clone(), cx));
                },
            ));
        }
        _ => {}
    }

    side = side
        .child(top.unwrap_or_else(|| gpui::div().into_any_element()))
        .child(bottom.unwrap_or_else(|| gpui::div().into_any_element()));

    // ── 卡片：100px 高 · padding 10 · items_start（垃圾桶/图标顶部，按钮列内部自行压底）──
    gpui::div()
        .h(px(100.))
        .p(px(10.))
        .flex()
        .items_start()
        .gap(px(12.))
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(8.))
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .child(icon)
        .child(p_main)
        .child(side)
        .into_any_element()
}
