//! Models tab（042 定稿）：无顶部 banner、无页面 footer。左栏**单一列表**：
//! Provider 行下直接平铺该商已启用的模型（自动展开）；排序三档——默认模型
//! 所在 Provider → 有启用模型的 → 没模型的，档内按显示名。头行
//! 【+添加Provider】开清单弹窗，行 switch 整商启停；模型行与 inputpanel
//! 菜单共用组件（缩进 / 五角星设默认）。右栏详情 = API KEY 表单 + 通路
//! 探查行 + 可用模型区（内置商）/ 自定义编辑器（表单头保存即写盘）。
//!
//! itlq.2 按区块拆子模块（只搬不改）：panel = 开关面板/选中/移除流；
//! catalog = 目录与排序供给；listing = 目录装载与列表数据；actions = 启停/
//! 默认/key/探查/模式编辑；view = 左栏列表渲染；detail = 右栏详情渲染；
//! list = vlist 行高与移除确认浮层；states = 探查/测速/目录状态枚举。

use super::*;

pub(crate) mod actions;
pub(crate) mod catalog;
pub(crate) mod detail;
pub(crate) mod list;
pub(crate) mod listing;
pub(crate) mod panel;
pub(crate) mod states;
pub(crate) mod view;

// 原 models.rs 的自由项原路径保持（settings/mod.rs 与 shell/ 调用点零改动）。
pub(crate) use list::{model_list_shown, model_list_vlist};
pub(crate) use panel::remove_provider_confirm_overlay;
pub(crate) use states::{McProbe, MjCatalogState, MjSpeedState, MjTestState};
pub(crate) use view::{detail_button, mc_models_view};

