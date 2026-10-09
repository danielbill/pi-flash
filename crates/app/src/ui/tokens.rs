//! UI 组件规范令牌 —— `docs/UI设计/UI组件规范.md` §1–§5 的唯一代码落点。
//!
//! 规则：界面上的一切 高度/内边距/间距/图标/圆角 必须取自这里的常量；
//! 规范外的尺寸视为 bug。改值先改规范再改这里（下方单测锁定全部数值，
//! 意外改动会编译失败）。色板不在本文件——见 `crate::theme`。

/// §1 间距（px）：2px 半步进，≥12 走 4 的倍数；禁止 5/7/9/11 及 .5 步进。
pub mod space {
    pub const SP1: f32 = 2.;
    pub const SP2: f32 = 4.;
    pub const SP3: f32 = 6.;
    pub const SP4: f32 = 8.;
    pub const SP5: f32 = 10.;
    pub const SP6: f32 = 12.;
    pub const SP7: f32 = 16.;
    pub const SP8: f32 = 20.;
    pub const SP9: f32 = 24.;
    pub const SP10: f32 = 32.;
}

/// §3 图标五档（px）。菜单/列表行/按钮内默认 SM。
pub mod icon {
    pub const INDICATOR: f32 = 10.;
    pub const XS: f32 = 12.;
    pub const SM: f32 = 14.;
    pub const MD: f32 = 16.;
    pub const LG: f32 = 18.;
}

/// §4 圆角（px）。同语义同值：8=菜单壳/列表行，10=弹窗，12=大卡。
pub mod radius {
    pub const XS: f32 = 3.;
    pub const CTRL: f32 = 6.;
    pub const CARD: f32 = 8.;
    pub const MODAL: f32 = 10.;
    pub const SHEET: f32 = 12.;
}

/// §5.1 文字按钮三档（高 / 水平 padding，px；字号 ui(11)/ui(12)/ui(12)）。
pub mod btn {
    pub const SM_H: f32 = 24.;
    pub const SM_PX: f32 = 8.;
    pub const MD_H: f32 = 28.;
    pub const MD_PX: f32 = 12.;
    pub const LG_H: f32 = 32.;
    pub const LG_PX: f32 = 14.;
}

/// §5.3 开关两档（2026-10-09 定夺 28×16 为默认档；存量 32×18 收敛于此）。
pub mod switch {
    pub const MD_W: f32 = 28.;
    pub const MD_H: f32 = 16.;
    pub const MD_KNOB: f32 = 10.;
    pub const MD_ON_ML: f32 = 14.;
    pub const MD_OFF_ML: f32 = 2.;
    pub const SM_W: f32 = 24.;
    pub const SM_H: f32 = 14.;
    pub const SM_KNOB: f32 = 8.;
    pub const SM_ON_ML: f32 = 10.;
    pub const SM_OFF_ML: f32 = 2.;
}

/// §5.4 复选 / 单选（px）。勾色 = accent_contrast。
pub mod checkbox {
    pub const SIZE: f32 = 14.;
    pub const CHECK: f32 = 10.;
    pub const RADIO_DOT: f32 = 6.;
}

/// §5.5 输入框（px）：单行 30 / 多行 170；聚焦 accent 1px。
pub mod input {
    pub const H: f32 = 30.;
    pub const MULTI_H: f32 = 170.;
}

/// §5.6 菜单 / 弹层（px）。
pub mod menu {
    pub const MIN_W: f32 = 200.;
    pub const PAD: f32 = 4.;
    pub const ROW_PX: f32 = 10.;
    pub const ROW_PY: f32 = 7.;
    pub const ROW_GAP: f32 = 8.;
    pub const SEP_MY: f32 = 6.;
}

/// §5.7 弹窗（px）：宽度四档 + 标准内边距/圆角/标题行高；大卡 = 0.7×0.98。
pub mod dialog {
    pub const W_SM: f32 = 380.;
    pub const W_MD: f32 = 500.;
    pub const W_LG: f32 = 620.;
    pub const W_XL: f32 = 760.;
    pub const RADIUS: f32 = 10.;
    pub const PAD: f32 = 16.;
    pub const TITLE_H: f32 = 36.;
}

/// §5.8 列表行高四档（px）。
pub mod row {
    pub const TREE: f32 = 24.;
    pub const STD: f32 = 30.;
    pub const MAIN: f32 = 32.;
    pub const PICKER: f32 = 40.;
}

/// §5.9 滚动条（px）：容器统一 w10，thumb 6→悬停 10。
pub mod scrollbar {
    pub const W: f32 = 10.;
    pub const THUMB: f32 = 6.;
    pub const THUMB_ACTIVE: f32 = 10.;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 锁定规范数值：改任何一格必须先改 `docs/UI设计/UI组件规范.md` 再动这里。
    #[test]
    fn spec_values_are_locked() {
        // §1 间距
        assert_eq!(
            [
                space::SP1, space::SP2, space::SP3, space::SP4, space::SP5,
                space::SP6, space::SP7, space::SP8, space::SP9, space::SP10,
            ],
            [2., 4., 6., 8., 10., 12., 16., 20., 24., 32.]
        );
        // ≥12 必须是 4 的倍数（步进规则）
        for v in [space::SP6, space::SP7, space::SP8, space::SP9, space::SP10] {
            assert_eq!(v % 4., 0., "spacing ≥12 must be a multiple of 4: {v}");
        }
        // §3 图标
        assert_eq!(
            [
                icon::INDICATOR, icon::XS, icon::SM, icon::MD, icon::LG
            ],
            [10., 12., 14., 16., 18.]
        );
        // §4 圆角
        assert_eq!(
            [
                radius::XS, radius::CTRL, radius::CARD, radius::MODAL, radius::SHEET
            ],
            [3., 6., 8., 10., 12.]
        );
        // §5.1 按钮
        assert_eq!(
            [
                btn::SM_H, btn::SM_PX, btn::MD_H, btn::MD_PX, btn::LG_H, btn::LG_PX
            ],
            [24., 8., 28., 12., 32., 14.]
        );
        // §5.3 开关
        assert_eq!(
            [
                switch::MD_W, switch::MD_H, switch::MD_KNOB,
                switch::SM_W, switch::SM_H, switch::SM_KNOB,
            ],
            [28., 16., 10., 24., 14., 8.]
        );
        // §5.4 复选 / §5.5 输入框
        assert_eq!(
            [checkbox::SIZE, checkbox::CHECK, checkbox::RADIO_DOT],
            [14., 10., 6.]
        );
        assert_eq!([input::H, input::MULTI_H], [30., 170.]);
        // §5.6 菜单 / §5.7 弹窗
        assert_eq!(
            [
                menu::MIN_W, menu::PAD, menu::ROW_PX, menu::ROW_PY,
                menu::ROW_GAP, menu::SEP_MY,
            ],
            [200., 4., 10., 7., 8., 6.]
        );
        assert_eq!(
            [
                dialog::W_SM, dialog::W_MD, dialog::W_LG, dialog::W_XL,
                dialog::RADIUS, dialog::PAD, dialog::TITLE_H,
            ],
            [380., 500., 620., 760., 10., 16., 36.]
        );
        // §5.8 行高 / §5.9 滚动条
        assert_eq!(
            [row::TREE, row::STD, row::MAIN, row::PICKER],
            [24., 30., 32., 40.]
        );
        assert_eq!(
            [scrollbar::W, scrollbar::THUMB, scrollbar::THUMB_ACTIVE],
            [10., 6., 10.]
        );
    }
}
