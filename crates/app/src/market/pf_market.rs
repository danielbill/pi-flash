//! PF 插件市场页（090-4 v1）——按定稿原型重排：双列卡片、pi market 卡片信息组织。
//!
//! 数据面：内置索引（`assets/plugin-market.json`）+ 台账 `plugins.json`。
//! 卡片：预览槽 72（tool_bg 无边框）｜名称 → 描述（换行截断由卡宽自然约束）→
//! meta 行（发布人 · 发布时间 ····· [安装]）。
//! 安装钮 = 宿主 pi_native_market card.rs:430-476 同款：边框盒（h33.3/min-w72/
//! px9.36/line 描边/mono 11/text_dim）内 `[`+label+`]`（括号 accent）；busy =
//! spinner(11, accent)；已装 = ✓ 绿（accent，不可点）。只有安装一个动作。
//!
//! 配色全部 theme.rs 语义 token（**页面跟主题走**）；字体跟随界面设置，
//! 安装钮 mono = JetBrains Mono（与 040-2 同）。

use std::sync::OnceLock;

use gpui::prelude::*;
use gpui::{AnyElement, FontWeight, MouseButton, SharedString, px};

use pi_link::pf_plugins::{self, IndexEntry};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::tr;

/// 安装钮 mono 字体（与 040-2 design::MONO 同值；该常量在私有 mod 里不可引）。
const MONO: &str = "JetBrains Mono";

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
    /// 安装进行中的包名（该卡按钮转 spinner，其余动作忽略）
    pub pending: Option<String>,
    /// 最近一次失败原因（显示在列表顶部，下一次动作自动清除）
    pub error: Option<String>,
    /// 台账镜像（open 时重读；安装完成后重读）
    pub ledger: Vec<pi_link::pf_plugins::LedgerEntry>,
}

fn state_of(entry: &IndexEntry, pf: &PfState) -> &'static str {
    match pf.ledger.iter().find(|l| l.name == entry.name) {
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

    /// 安装。后台线程跑 npm pack → sha512 校验 → 解包落盘 → 台账。
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
                cx.notify();
            });
        })
        .detach();
    }
}

use crate::i18n::tf;
use gpui::rgb;

// ---------------------------------------------------------------------------
// 页面
// ---------------------------------------------------------------------------

/// 页面主体（content.rs `ContentView::PfMarket` 分支）。
pub(crate) fn page(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &mut gpui::Window,
) -> AnyElement {
    let t = crate::appearance::theme::theme();
    let pf = &chat.aux.market.pf;
    let entries = &index().packages;

    // 外部 layout = pi market 页同构：内容宽（扣左 dock）→ 壳内距 clamp(18,3vw,36)
    // → 壳列宽 min(内容宽, 1485−壳距×2)，分段定宽 + 显式左右留白居中
    let vw = f32::from(window.viewport_size().width);
    let content_w = vw
        - if chat.layout.panes_hidden {
            0.
        } else {
            chat.layout.slp_w
        };
    let shell_pad = (0.03 * content_w).clamp(18., 36.);
    let shell_w = (content_w - shell_pad * 2.)
        .min(1485. - shell_pad * 2.)
        .max(1.);
    let side_pad = ((content_w - shell_w) / 2.).max(0.);
    let pad_body = (0.014 * content_w).clamp(15.3, 18.9); // 卡片体水平内距（同源 clamp）

    // 总数计数（无分页：1-N / N，与 pi market 头栏同口径）
    let n = entries.len();
    let count = format!("1-{n} / {n}");

    // ① 头段：所有插件 + 计数（card_header 同构：mono 大写间距字 · baseline 两端）
    let head_title = gpui::div()
        .font_family(MONO)
        .text_size(px(14.76))
        .text_color(rgb(t.text))
        .child(spaced_upper("所有插件"));
    let head_count = gpui::div()
        .font_family(MONO)
        .text_size(px(13.68))
        .text_color(rgb(t.text_faint))
        .child(count);
    let head = segment(shell_w, side_pad, t, true, false)
        .child(
            gpui::div()
                .flex()
                .items_baseline()
                .justify_between()
                .gap(px(18.))
                .px(px(pad_body))
                .py(px(14.04))
                .border_b_1()
                .border_color(rgb(t.border))
                .child(head_title)
                .child(head_count),
        )
        .into_any_element();

    // ② 体段：卡行（每行 gap 18 · 行底距 18；末位补 flex_1 占位）
    let mut body = gpui::div()
        .px(px(pad_body))
        .pt(px(pad_body))
        .pb(px(16.2))
        .flex()
        .flex_col();

    if let Some(err) = &pf.error {
        body = body.child(
            gpui::div()
                .pb(px(8.))
                .text_size(px(11.))
                .text_color(rgb(t.danger))
                .child(err.clone()),
        );
    }

    if entries.is_empty() {
        body = body.child(
            gpui::div()
                .p(px(32.))
                .flex()
                .justify_center()
                .text_color(rgb(t.text_muted))
                .child(tr("market.pf.empty")),
        );
    } else {
        for pair in entries.chunks(2) {
            let mut row = gpui::div().w_full().flex().gap(px(18.)).pb(px(18.));
            for e in pair {
                row = row.child(card(chat, weak, e, t));
            }
            if pair.len() < 2 {
                row = row.child(gpui::div().flex_1());
            }
            body = body.child(row);
        }
    }

    let body_seg = segment(shell_w, side_pad, t, false, true)
        .child(body)
        .into_any_element();

    gpui::div()
        .id("pf-market-list")
        .size_full()
        .overflow_y_scroll()
        .bg(rgb(t.bg))
        .child(head)
        .child(body_seg)
        .into_any_element()
}

/// 分段外壳（card.rs slice_shell 同构）：定宽壳列 + 显式左右留白；
/// 面板底 + 左右描边，首段补上边、末段补下边。返回外壳 Div，内容由调用方 child。
fn segment(shell_w: f32, side_pad: f32, t: &Theme, top: bool, bottom: bool) -> gpui::Div {
    let mut boxed = gpui::div()
        .w(px(shell_w))
        .flex()
        .flex_col()
        .bg(rgb(t.bg_panel))
        .border_l_1()
        .border_r_1()
        .border_color(rgb(t.border));
    if top {
        boxed = boxed.border_t_1();
    }
    if bottom {
        boxed = boxed.border_b_1();
    }
    gpui::div().pl(px(side_pad)).pr(px(side_pad)).child(boxed)
}

/// 头栏标题的字符间空（design::spaced 同款：U+200A，非 wide）。
fn spaced_upper(s: &str) -> String {
    let upper = s.to_uppercase();
    let mut out = String::with_capacity(upper.len() * 2);
    for (i, ch) in upper.chars().enumerate() {
        if i > 0 {
            out.push('\u{200a}');
        }
        out.push(ch);
    }
    out
}

fn card(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    e: &IndexEntry,
    t: &'static Theme,
) -> AnyElement {
    let pf = &chat.aux.market.pf;
    let st = state_of(e, pf);
    let busy = pf.pending.as_deref() == Some(e.name.as_str());

    // ── 预览槽：72×72 顶部对齐 · tool_bg · 无边框（将来插件带预览图替换此槽）──
    let preview = gpui::div()
        .size(px(72.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(t.tool_bg))
        .rounded(px(8.))
        .text_color(rgb(t.accent))
        .child(crate::ui::icon("package", 28., t.accent));

    // ── 名称（16 semibold —— 源码层级：标题明显大于内容）──
    let name = gpui::div()
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(16.))
        .child(e.display.zh.clone());

    // ── 描述（无则不渲染；卡宽自然约束换行）──
    let desc = e
        .desc
        .as_ref()
        .map(|d| gpui::div().text_color(rgb(t.text_dim)).child(d.zh.clone()));

    // ── meta 行：发布人 · 发布时间 ···· [安装]（同行，贴底）──
    let meta = gpui::div()
        .flex()
        .items_center()
        .gap(px(8.))
        .mt_auto()
        .child(
            gpui::div()
                .text_size(px(11.))
                .text_color(rgb(t.text_dim))
                .child(if e.publisher.is_empty() {
                    "Pi-Flash".to_string()
                } else {
                    e.publisher.clone()
                }),
        )
        .child(
            gpui::div()
                .text_size(px(11.))
                .text_color(rgb(t.text_faint))
                .child(e.published_at.clone()),
        )
        .child(gpui::div().flex_1())
        .child(install_btn(chat, weak, e, st, busy, t));

    let body = gpui::div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .gap(px(2.))
        .child(name)
        .children(desc)
        .child(meta);

    // ── 卡片：150px 高（定稿）· padding 10 · items_start ──
    gpui::div()
        .h(px(150.))
        .p(px(10.))
        .flex()
        .items_start()
        .gap(px(12.))
        .bg(rgb(t.bg_panel))
        .border_1()
        .border_color(rgb(t.border))
        .rounded(px(8.))
        .hover(|s| s.bg(rgb(t.bg_hover)))
        .child(preview)
        .child(body)
        .into_any_element()
}

/// 安装钮（card.rs:430-476 同款）：边框盒内 `[`+安装+`]`；busy = spinner；
/// 已装 = ✓ 绿（同槽，不可点）。只有这一个动作。
fn install_btn(
    _chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    e: &IndexEntry,
    st: &'static str,
    busy: bool,
    t: &'static Theme,
) -> AnyElement {
    // 已装：✓ 绿（源码 0x5db87a → mist 语义 = t.accent，绿只表达正确性）
    if st == "ok" {
        return gpui::div()
            .h(px(33.3))
            .px(px(9.36))
            .flex()
            .items_center()
            .justify_center()
            .flex_shrink_0()
            .text_color(rgb(t.accent))
            .child(crate::ui::icon("check", 12., t.accent))
            .into_any_element();
    }

    let label: AnyElement = if busy {
        crate::ui::spinner(11., t.accent)
    } else {
        let name = e.name.clone();
        let weak_c = weak.clone();
        gpui::div()
            .id(SharedString::from(format!("pf-install-{}", e.name)))
            .flex()
            .items_center()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let name = name.clone();
                let _ = weak_c.update(cx, |c, cx| c.pf_install(name, cx));
            })
            .child(bracket("["))
            .child(tr("market.btn.install"))
            .child(bracket("]"))
            .into_any_element()
    };

    let mut box_el = gpui::div()
        .id(SharedString::from(format!("pf-install-box-{}", e.name)))
        .flex()
        .items_center()
        .justify_center()
        .flex_shrink_0()
        .h(px(33.3))
        .min_w(px(72.))
        .px(px(9.36))
        .border_1()
        .border_color(rgb(t.border))
        .text_size(px(11.))
        .font_family(MONO)
        .text_color(rgb(t.text_dim));
    // 源码 card.rs:478-482：busy = 描边 accent 52%；否则 hover 描边 accent 52% + 文字回全亮
    if busy {
        box_el = box_el.border_color(gpui::rgba((t.accent << 8) | 0x85));
    } else {
        box_el = box_el.cursor_pointer().hover(|s| {
            s.border_color(gpui::rgba((t.accent << 8) | 0x85))
                .text_color(rgb(t.text))
        });
    }
    box_el.child(label).into_any_element()
}

/// 括号子元素（card.rs:627 同款：px4.6 + accent 色）。
fn bracket(ch: &'static str) -> AnyElement {
    gpui::div()
        .px(px(4.6))
        .text_color(rgb(crate::appearance::theme::theme().accent))
        .child(SharedString::from(ch))
        .into_any_element()
}
