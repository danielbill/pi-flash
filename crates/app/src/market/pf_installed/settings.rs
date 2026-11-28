//! 统一插件设置区（契约 v6 contributes.settings，003-1 定案 2026-11-28）。
//!
//! 分工：描述归插件（清单 `[[contributes.settings]]`）、渲染归宿主（本模块
//! 在【已安装-插件说明】下方按描述绘制）、存储归宿主（真源
//! `<数据目录>/plugin-settings/<pkg>.json` + 镜像投递 `<pkg>/data/settings.json`，
//! 见 [`pi_link::pf_plugins::settings_store`]）、消费归插件（现读镜像，改后即生效）。
//! 宿主只认字段类型不认语义——任何插件零特判。
//!
//! 控件复用（003-1 指定 Inputpanel 同款）：
//! - model 型 = [`crate::session::input::bar::bar_trigger`] 触发钮 +
//!   [`crate::session::model_picker::model_menu_body`] 模型菜单（session_option
//!   = true 时菜单顶部加【使用会话模型】置顶行）+ 思考强度菜单
//!   [`crate::session::menu_rows::thinking_rows`]（with_thinking = true 并排）
//! - stepper 型 = [`crate::ui::kit::stepper`]（−/+ 夹数值，到边界置灰）
//! - 【保存】右下：dirty（内存草稿 vs 真源）仅改动时 enable，保存后自动 disable
//!
//! 弹层 = composer picker 同一套（picker_card + pill_anchor_edges 定位 +
//! menu_layer 遮挡不穿透），锚点槽位在 [`PfSettingsAnchors`]（页级 Rc 槽，
//! 046 TaskAnchors 同款——不占 PillBtns 槽，与 composer/设置页互不覆盖），
//! 挂载入口 [`menus_overlay`] 由 `shell/render/overlay.rs` 调用（画在 dialogs 之下）。

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use gpui::prelude::*;
use gpui::{AnyElement, Div, FontWeight, MouseButton, SharedString, div, px, rgb};

use crate::Chat;
use crate::appearance::theme::Theme;
use crate::i18n::tr;
use crate::pf_plugin::manifest::{PluginManifest, SettingsField, SettingsKind};
use crate::session::input::bar_trigger;
use crate::session::menu_rows::{PillRow, pill_row, thinking_rows};
use crate::session::model_picker::{ModelPickAction, model_menu_body, model_menu_content_h};
use crate::session::picker_shell::{PAD, PillCardPos, empty_row, picker_card, pill_anchor_edges};

use super::InstalledItem;

// ---------------------------------------------------------------------------
// 值模型（内存态 ↔ settings.json flat map）
// ---------------------------------------------------------------------------

/// 设置字段值（契约 v6 value_shape）。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PfSettingValue {
    /// `{ model: "session" | {provider,id}, thinking: "auto"|档位 }`
    Model { model: PfModelRef, thinking: String },
    /// stepper 数值
    Number(u64),
}

/// model 值的模型引用半边。
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PfModelRef {
    /// 使用会话模型（压缩时取会话当前模型，cache 复用）
    Session,
    /// 固定 pi 注册模型
    Fixed { provider: String, id: String },
}

impl PfSettingValue {
    /// → settings.json 形态（flat map 的值半边）。
    pub(crate) fn to_json(&self) -> serde_json::Value {
        match self {
            PfSettingValue::Model { model, thinking } => {
                let model = match model {
                    PfModelRef::Session => serde_json::json!("session"),
                    PfModelRef::Fixed { provider, id } => {
                        serde_json::json!({ "provider": provider, "id": id })
                    }
                };
                serde_json::json!({ "model": model, "thinking": thinking })
            }
            PfSettingValue::Number(n) => serde_json::json!(n),
        }
    }

    /// ← settings.json 形态（坏值返回 None，回落清单 default）。
    pub(crate) fn from_json(v: &serde_json::Value, kind: SettingsKind) -> Option<Self> {
        match kind {
            SettingsKind::Stepper => v.as_u64().map(|n| PfSettingValue::Number(n)),
            SettingsKind::Model => {
                let obj = v.as_object()?;
                let model = match obj.get("model")? {
                    serde_json::Value::String(s) if s == "session" => PfModelRef::Session,
                    serde_json::Value::Object(m) => PfModelRef::Fixed {
                        provider: m.get("provider")?.as_str()?.to_string(),
                        id: m.get("id")?.as_str()?.to_string(),
                    },
                    _ => return None,
                };
                let thinking = obj
                    .get("thinking")
                    .and_then(|t| t.as_str())
                    .unwrap_or("auto")
                    .to_string();
                Some(PfSettingValue::Model { model, thinking })
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 锚点槽位 + 草稿（内存态；真源落盘走 settings_store）
// ---------------------------------------------------------------------------

/// 设置区下拉锚点槽位（页级 Rc 槽，046 TaskAnchors 同款：tracker 每帧覆盖，
/// 与 composer/设置页槽位互不覆盖）。
#[derive(Clone, Default)]
pub(crate) struct PfSettingsAnchors {
    pub model: Rc<RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
    pub thinking: Rc<RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
}

/// 设置区编辑草稿：首次交互时按选中插件物化（字段 = 清单描述快照，值 =
/// 真源 ⊕ 清单 default），dirty 只由交互置位、保存后清零。
pub(crate) struct PfSettingsDraft {
    pub pkg: String,
    /// 清单描述快照（免 overlay 每帧重扫盘）
    pub fields: Vec<SettingsField>,
    /// 字段 id → 当前编辑值
    pub values: BTreeMap<String, PfSettingValue>,
    /// 打开的下拉：（0=模型 1=思考, 字段 id）
    pub popup: Option<(u8, String)>,
    pub dirty: bool,
}

impl PfSettingsDraft {
    /// 由清单描述 + 真源物化草稿（值 = default ⊕ store）。
    pub(crate) fn materialize(m: &PluginManifest) -> Self {
        let fields = m.settings.clone();
        let mut values = BTreeMap::new();
        for f in &fields {
            values.insert(f.id.clone(), default_value(f));
        }
        let store = pi_link::pf_plugins::settings_store::load(&m.pkg);
        for (k, v) in &store {
            let Some(f) = fields.iter().find(|f| f.id == *k) else {
                continue; // 不在描述里的键：原样留在真源，不进草稿
            };
            if let Some(pv) = PfSettingValue::from_json(v, f.kind) {
                values.insert(k.clone(), pv);
            }
        }
        Self {
            pkg: m.pkg.clone(),
            fields,
            values,
            popup: None,
            dirty: false,
        }
    }
}

/// 清单 default → 值（缺省键按契约兜底：model=session/auto，stepper=min）。
pub(crate) fn default_value(f: &SettingsField) -> PfSettingValue {
    match f.kind {
        SettingsKind::Stepper => PfSettingValue::Number(f.default_num.or(f.min).unwrap_or(0)),
        SettingsKind::Model => PfSettingValue::Model {
            model: match f.default_model.as_deref() {
                Some("session") if f.session_option => PfModelRef::Session,
                Some(s) => match s.split_once('/') {
                    Some((p, i)) => PfModelRef::Fixed {
                        provider: p.to_string(),
                        id: i.to_string(),
                    },
                    _ => PfModelRef::Session,
                },
                _ => PfModelRef::Session,
            },
            thinking: f.default_thinking.clone().unwrap_or_else(|| "auto".into()),
        },
    }
}

// ---------------------------------------------------------------------------
// Chat 动作（编辑 / 菜单开合 / 保存）
// ---------------------------------------------------------------------------

/// 找选中插件的带设置清单（停用插件被 scan 过滤 → None，设置区随行隐身）。
fn settings_manifest(pkg: &str) -> Option<PluginManifest> {
    crate::pf_plugin::scan_manifests()
        .into_iter()
        .find(|m| m.pkg == pkg && !m.settings.is_empty())
}

/// 当前生效值（草稿优先，回落 default ⊕ store）。
fn effective_values(
    chat: &Chat,
    pkg: &str,
    fields: &[SettingsField],
) -> BTreeMap<String, PfSettingValue> {
    if let Some(d) = chat.aux.market.pf.settings.as_ref()
        && d.pkg == pkg
    {
        return d.values.clone();
    }
    let mut values: BTreeMap<String, PfSettingValue> = fields
        .iter()
        .map(|f| (f.id.clone(), default_value(f)))
        .collect();
    let store = pi_link::pf_plugins::settings_store::load(pkg);
    for (k, v) in &store {
        if let Some(f) = fields.iter().find(|f| f.id == *k)
            && let Some(pv) = PfSettingValue::from_json(v, f.kind)
        {
            values.insert(k.clone(), pv);
        }
    }
    values
}

impl Chat {
    /// 草稿物化（首次交互/换选中插件时）：清单描述 + 真源合成。清单缺失
    /// （停用/失效包）返回 false，交互静默忽略。
    fn pf_settings_ensure_draft(&mut self, pkg: &str) -> bool {
        let fresh = self
            .aux
            .market
            .pf
            .settings
            .as_ref()
            .is_none_or(|d| d.pkg != pkg);
        if !fresh {
            return true;
        }
        match settings_manifest(pkg) {
            Some(m) => {
                self.aux.market.pf.settings = Some(PfSettingsDraft::materialize(&m));
                true
            }
            None => {
                self.aux.market.pf.settings = None;
                false
            }
        }
    }

    /// 编辑一个字段值（菜单行/步进件回调）：物化草稿 → 写值 → dirty。
    pub(crate) fn pf_setting_edit(
        &mut self,
        pkg: &str,
        field_id: &str,
        v: PfSettingValue,
        close_menu: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.pf_settings_ensure_draft(pkg) {
            return;
        }
        if let Some(d) = self.aux.market.pf.settings.as_mut() {
            d.values.insert(field_id.to_string(), v);
            d.dirty = true;
            if close_menu {
                d.popup = None;
            }
        }
        cx.notify();
    }

    /// 下拉开合（触发钮点击）：物化草稿 → 记锚点 + 翻转槽位。
    pub(crate) fn pf_settings_menu(
        &mut self,
        pkg: &str,
        slot: u8,
        field_id: &str,
        at: gpui::Point<gpui::Pixels>,
        slot_bounds: &Rc<RefCell<Option<gpui::Bounds<gpui::Pixels>>>>,
        cx: &mut gpui::Context<Self>,
    ) {
        if !self.pf_settings_ensure_draft(pkg) {
            return;
        }
        let anchor = crate::PillBtns::anchor(slot_bounds, at);
        self.dlg.pill_anchor = Some(anchor);
        if let Some(d) = self.aux.market.pf.settings.as_mut() {
            d.popup = if d
                .popup
                .as_ref()
                .is_some_and(|(s, f)| *s == slot && f == field_id)
            {
                None
            } else {
                Some((slot, field_id.to_string()))
            };
        }
        cx.notify();
    }

    /// 保存：草稿全量写真源（settings_store 同步镜像投递到 data/settings.json）
    /// → dirty 清零。失败进头段错误行。
    pub(crate) fn pf_settings_save(&mut self, pkg: &str, cx: &mut gpui::Context<Self>) {
        let Some(d) = self.aux.market.pf.settings.as_ref() else {
            return;
        };
        if d.pkg != pkg || !d.dirty {
            return;
        }
        let mut out = pi_link::pf_plugins::settings_store::load(pkg); // 保留描述外键
        for (k, v) in &d.values {
            out.insert(k.clone(), v.to_json());
        }
        match pi_link::pf_plugins::settings_store::save(pkg, &out) {
            Ok(()) => {
                if let Some(d) = self.aux.market.pf.settings.as_mut() {
                    d.dirty = false;
                }
                self.aux.market.pf.error = None;
            }
            Err(e) => {
                self.aux.market.pf.error = Some(crate::i18n::tf(
                    "market.pf.settings_save_failed",
                    &[("reason", e)],
                ));
            }
        }
        cx.notify();
    }
}

// ---------------------------------------------------------------------------
// 渲染（detail_pane 设置区：整段字段区之后、横线分隔——用户定 2026-11-28）
// ---------------------------------------------------------------------------

/// 设置区容器（单个元素，Option = 无设置段/停用插件不渲染）。容器自带
/// `border_top` 横线（独立 1px div 在此面板不可靠——用户实测没看到，改描边
/// 机制，与垃圾桶/保存钮描边同源必然可见）。首行 = 【设置】标题左 + 【保存】
/// 右（用户定：保存上移至标题行）；仅 dirty 时 enable，保存后自动 disable。
pub(crate) fn settings_rows(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    item: &InstalledItem,
    anchors: &PfSettingsAnchors,
    t: &'static Theme,
) -> Option<AnyElement> {
    if item.disabled {
        return None;
    }
    let m = settings_manifest(&item.name)?;
    let lang2 = crate::i18n::lang_ix() == 2;
    let pick_text = |zh: &str, en: &Option<String>| {
        if lang2 {
            en.clone().unwrap_or_else(|| zh.to_string())
        } else {
            zh.to_string()
        }
    };
    let values = effective_values(chat, &item.name, &m.settings);
    let dirty = chat
        .aux
        .market
        .pf
        .settings
        .as_ref()
        .is_some_and(|d| d.pkg == item.name && d.dirty);

    // 保存钮（标题行右侧）：仅 dirty 时 enable（保存后自动 disable——003-1 定案）
    let pkg = item.name.clone();
    let weak_save = weak.clone();
    let mut save = div()
        .id("pf-settings-save")
        .h(px(26.))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(5.))
        .border_1()
        .border_color(rgb(t.border))
        .text_size(crate::appearance::ui_size(11.))
        .child(SharedString::from(tr("market.pf.save")));
    if dirty {
        save = save
            .text_color(rgb(t.accent))
            .cursor_pointer()
            .hover(|s| s.bg(rgb(t.bg_hover)))
            .on_mouse_down(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let _ = weak_save.update(cx, |c, cx| c.pf_settings_save(&pkg, cx));
            });
    } else {
        save = save.opacity(0.5).text_color(rgb(t.text_faint));
    }

    // 标题行：【设置】左 + 【保存】右
    let head_row = div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .flex_1()
                .font_weight(FontWeight::SEMIBOLD)
                .text_size(crate::appearance::ui_size(11.))
                .text_color(rgb(t.text_dim))
                .child(SharedString::from(tr("market.pf.settings_head"))),
        )
        .child(save);

    let mut col = div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .border_t_1()
        .border_color(rgb(t.border))
        .pt(px(14.))
        .child(head_row);
    for f in m.settings.iter() {
        let value = values
            .get(&f.id)
            .cloned()
            .unwrap_or_else(|| default_value(f));
        let label = pick_text(&f.label_zh, &f.label_en);
        let control: AnyElement = match f.kind {
            SettingsKind::Stepper => stepper_control(weak, &item.name, f, &value),
            SettingsKind::Model => model_control(chat, weak, &item.name, f, &value, anchors),
        };
        let hint = f.hint_zh.as_deref().map(|hint| pick_text(hint, &f.hint_en));
        col = col.child(field_row(&label, control, hint, t));
    }
    Some(col.into_any_element())
}

/// 字段行：label 左 + 控件右（detail_pane field() 同款两栅语言）。首行统一
/// 28px 高、label 与控件行内居中——标题（1 行）与控件行（1 行）水平对齐，
/// 不随控件自身高度漂移（28 = bar_trigger 胶囊高，stepper 24px 在内居中；
/// 旧 pt(5) 放宽写法在 stepper 行实测标题偏低，用户 2026-11-28 定稿废除）。
fn field_row(
    label: &str,
    control: AnyElement,
    hint: Option<String>,
    t: &'static Theme,
) -> AnyElement {
    let mut content = div()
        .flex()
        .flex_col()
        .child(div().h(px(28.)).flex().items_center().child(control));
    if let Some(hint) = hint {
        content = content.child(
            div()
                .mt(px(3.))
                .text_size(crate::appearance::ui_size(10.5))
                .text_color(rgb(t.text_faint))
                .child(SharedString::from(hint)),
        );
    }
    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .child(
            div()
                .w(px(120.))
                .h(px(28.))
                .flex()
                .flex_shrink_0()
                .items_center()
                .text_size(crate::appearance::ui_size(12.))
                .text_color(rgb(t.text_muted))
                .child(SharedString::from(label.to_string())),
        )
        .child(content.flex_1().min_w_0())
        .into_any_element()
}

/// stepper 型控件（ui/kit 全局件复用）：−/+ 夹数值，到边界置灰。
/// `cur` 取渲染快照（回调每帧重建，陈值窗口 = 单帧内连点，步进自校正）。
fn stepper_control(
    weak: &gpui::WeakEntity<Chat>,
    pkg: &str,
    f: &SettingsField,
    value: &PfSettingValue,
) -> AnyElement {
    let PfSettingValue::Number(v) = value else {
        return div().into_any_element();
    };
    let min = f.min.unwrap_or(0);
    let max = f.max.unwrap_or(*v);
    let step = f.step.unwrap_or(1);
    let cur_v = *v;
    let pkg = pkg.to_string();
    let id = f.id.clone();
    crate::ui::kit::stepper(
        "pf-set",
        cur_v,
        min,
        max,
        step as i64,
        |n| {
            if n >= 1000 && n % 1000 == 0 {
                format!("{}K", n / 1000)
            } else {
                n.to_string()
            }
        },
        move || cur_v,
        move |c, cx, nv| {
            c.pf_setting_edit(&pkg, &id, PfSettingValue::Number(nv), false, cx);
        },
        weak,
    )
}

/// model 型控件（InputPanel 同款）：模型触发钮（+ with_thinking 并排思考钮）。
fn model_control(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    pkg: &str,
    f: &SettingsField,
    value: &PfSettingValue,
    anchors: &PfSettingsAnchors,
) -> AnyElement {
    let PfSettingValue::Model { model, thinking } = value else {
        return div().into_any_element();
    };
    let t = crate::appearance::theme::theme();
    let popup = chat
        .aux
        .market
        .pf
        .settings
        .as_ref()
        .filter(|d| d.pkg == pkg)
        .and_then(|d| d.popup.clone());
    let model_open = popup
        .as_ref()
        .is_some_and(|(s, fid)| *s == 0 && *fid == f.id);
    let thinking_open = popup
        .as_ref()
        .is_some_and(|(s, fid)| *s == 1 && *fid == f.id);
    let trg_color = |is_open: bool| {
        if is_open { t.accent } else { t.text_muted }
    };
    let model_label = match model {
        PfModelRef::Session => tr("market.pf.use_session").to_string(),
        PfModelRef::Fixed { provider, id } => format!("{provider}/{id}"),
    };
    let weak_m = weak.clone();
    let at_model = pkg.to_string();
    let fid_model = f.id.clone();
    let anchor_m = anchors.model.clone();
    let model_btn = bar_trigger(
        "pf-set-model-trg",
        Some("cpu"),
        &model_label,
        true,
        trg_color(model_open),
        true,
    )
    // 统一定位测量端（paint 期每帧覆盖）+ 点击开合
    .child(crate::PillBtns::tracker(&anchor_m))
    .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
        cx.stop_propagation();
        let at = ev.position;
        let pkg = at_model.clone();
        let fid = fid_model.clone();
        let anchor = anchor_m.clone();
        let _ = weak_m.update(cx, |c, cx| {
            c.pf_settings_menu(&pkg, 0, &fid, at, &anchor, cx);
        });
    });
    let mut row = div().flex().items_center().gap(px(15.)).child(model_btn);
    if f.with_thinking {
        let weak_t = weak.clone();
        let pkg_t = pkg.to_string();
        let fid_thinking = f.id.clone();
        let anchor_t = anchors.thinking.clone();
        let thinking_btn = bar_trigger(
            "pf-set-thinking-trg",
            Some("lightbulb"),
            thinking,
            true,
            trg_color(thinking_open),
            true,
        )
        .child(crate::PillBtns::tracker(&anchor_t))
        .on_mouse_down(MouseButton::Left, move |ev, _, cx| {
            cx.stop_propagation();
            let at = ev.position;
            let pkg = pkg_t.clone();
            let fid = fid_thinking.clone();
            let anchor = anchor_t.clone();
            let _ = weak_t.update(cx, |c, cx| {
                c.pf_settings_menu(&pkg, 1, &fid, at, &anchor, cx);
            });
        });
        row = row.child(thinking_btn);
    }
    row.into_any_element()
}

// ---------------------------------------------------------------------------
// 弹层挂载（overlay_stack 调用；画在 dialogs 之下）
// ---------------------------------------------------------------------------

/// 已安装页设置区两菜单弹层（模型 / 思考）。挂载点在 dialogs 之前 = 模态之下；
/// 点外收走（menu_layer 遮挡不穿透），点卡片本身不关（stop_click）。
pub(crate) fn menus_overlay(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    window: &gpui::Window,
) -> Option<AnyElement> {
    let st = chat.aux.market.pf.settings.as_ref()?;
    let (slot, field_id) = st.popup.clone()?;
    let f = st.fields.iter().find(|f| f.id == field_id)?.clone();
    let pkg = st.pkg.clone();
    let value = st.values.get(&field_id)?.clone();
    let t = crate::appearance::theme::theme();
    let menu_w = match slot {
        0 => 380.,
        1 => 320.,
        _ => return None,
    };
    let anchor = chat.dlg.pill_anchor;
    let vp = window.viewport_size();
    let pos: PillCardPos = pill_anchor_edges(anchor, vp, px(menu_w));
    let weak_card = weak.clone();
    let content: Div = match slot {
        0 => model_card(chat, &weak_card, &pkg, &f, &value, t),
        1 => thinking_card(&weak_card, &pkg, &f, &value),
        _ => return None,
    };
    let layer = crate::ui::menu::menu_layer(None, weak, |c, cx| {
        if let Some(d) = c.aux.market.pf.settings.as_mut()
            && d.popup.take().is_some()
        {
            cx.notify();
        }
    });
    Some(
        layer
            .child(crate::ui::overlay::stop_click(
                picker_card(pos, px(menu_w), t).child(content),
            ))
            .into_any_element(),
    )
}

/// 模型卡：session_option 时【使用会话模型】置顶行（分隔线隔离）+ composer
/// 同款分组模型列表（003-1：菜单上方加【使用会话模型】）。
fn model_card(
    chat: &Chat,
    weak: &gpui::WeakEntity<Chat>,
    pkg: &str,
    f: &SettingsField,
    value: &PfSettingValue,
    t: &'static Theme,
) -> Div {
    let (models, _total) = chat.picker_models_from(None, None);
    let default_ref = chat
        .cfg
        .mc_default_model
        .as_ref()
        .map(|(p, id)| format!("{p}/{id}"));
    let PfSettingValue::Model { model, thinking } = value else {
        return div();
    };
    // 闭包全部捕获自有值（value 是借用，'static 闭包不得带走引用）
    let thinking_keep = thinking.clone();
    let current_ref = match model {
        PfModelRef::Session => None,
        PfModelRef::Fixed { provider, id } => Some(format!("{provider}/{id}")),
    };
    // 【使用会话模型】置顶行（active = session 值）
    let pkg_s = pkg.to_string();
    let fid_s = f.id.clone();
    let think_s = thinking.clone();
    let session_row = f.session_option.then(|| {
        pill_row(
            weak,
            move |c, cx| {
                c.pf_setting_edit(
                    &pkg_s,
                    &fid_s,
                    PfSettingValue::Model {
                        model: PfModelRef::Session,
                        thinking: think_s.clone(),
                    },
                    true,
                    cx,
                );
            },
            PillRow {
                key: "pf-use-session",
                label: tr("market.pf.use_session").to_string(),
                desc: String::new(),
                active: matches!(model, PfModelRef::Session),
            },
        )
    });
    // 模型行点击 → 固定模型（保留思考档）
    let pkg_p = pkg.to_string();
    let fid_p = f.id.clone();
    let pick: ModelPickAction = Rc::new(move |c: &mut Chat, cx, p: String, id: String| {
        c.pf_setting_edit(
            &pkg_p,
            &fid_p,
            PfSettingValue::Model {
                model: PfModelRef::Fixed { provider: p, id },
                thinking: thinking_keep.clone(),
            },
            true,
            cx,
        );
    });
    let mut body = model_menu_body(
        weak,
        &models,
        current_ref.as_deref(),
        default_ref.as_deref(),
        |_| false,
        Some(pick),
        t,
    );
    let list_h = if models.is_empty() {
        body = body.child(empty_row(SharedString::from(tr("action.no_models_match"))));
        60.
    } else {
        model_menu_content_h(&models).min(300.)
    };
    let mut card = div().flex().flex_col();
    if let Some(row) = session_row {
        card = card
            .child(row)
            .child(div().h(px(1.)).mx(px(PAD)).my(px(4.)).bg(rgb(t.border)));
    }
    card.child(
        div()
            .id("pf-set-model-list")
            .h(px(list_h))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .pb(px(PAD))
            .pr(px(10.))
            .child(body),
    )
}

/// 思考卡：auto（= 不覆盖，落会话当前档）+ pi 七档，档位表复用 composer 胶囊菜单。
fn thinking_card(
    weak: &gpui::WeakEntity<Chat>,
    pkg: &str,
    f: &SettingsField,
    value: &PfSettingValue,
) -> Div {
    let PfSettingValue::Model { thinking, .. } = value else {
        return div();
    };
    let rows = thinking_rows(thinking)
        .into_iter()
        .map(|row| {
            let weak = weak.clone();
            let key = row.key.to_string();
            let pkg = pkg.to_string();
            let fid = f.id.clone();
            pill_row(
                &weak,
                move |c, cx| {
                    let model = match &c
                        .aux
                        .market
                        .pf
                        .settings
                        .as_ref()
                        .and_then(|d| d.values.get(&fid))
                    {
                        Some(PfSettingValue::Model { model, .. }) => model.clone(),
                        _ => PfModelRef::Session,
                    };
                    c.pf_setting_edit(
                        &pkg,
                        &fid,
                        PfSettingValue::Model {
                            model,
                            thinking: key.clone(),
                        },
                        true,
                        cx,
                    );
                },
                row,
            )
        })
        .collect::<Vec<_>>();
    div().flex().flex_col().children(rows)
}
