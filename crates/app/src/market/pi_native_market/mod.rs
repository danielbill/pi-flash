//! Pi 原生扩展市场（040-2）——pi.dev `Package Catalog` 页的桌面移植。
//!
//! 设计稿：`docs/UI设计/仿-pi.dev-Packages-页面设计/pi-packages.html`
//! （同目录 `DESIGN-HANDOFF.md` 是导出工具给的视觉契约：**像素优先**）。
//! 规格：`docs/模块设计/040-2-Pi 扩展市场.md`。
//!
//! 文件组织：页状态机（`NativeState`）与页内助手在本文件；Chat 动作链
//! （打开/拉页/过滤/安装）在 [`actions`]、页面渲染在 [`render`]；卡分段与
//! 包卡在 [`card`] / [`package`]，工具栏与页码条在 [`controls`]，设计稿
//! 字面量真值在 [`design`]，设计稿时间戳文案在 [`time`]。
//!
//! 与 040-1「Pi 扩展市场」的关系：**数据面同一个**（npm registry，见
//! [`pi_link::market::pi_ext_market`]），**界面与信息组织完全不同**——040-1 是
//! PF 自己设计的单列卡 + 工具栏；本页照抄 pi.dev 官网目录页：四列「最近发布」
//! 条 + 双列大卡（预览块 + 徽标 + 命令条）+ 页码条。
//!
//! 三条实现口径（改之前先读 `docs/agents/market.md`）：
//!
//! 1. **配色/尺寸全部字面量**：本页是「pi.dev 原生皮肤」，不跟随 PF 主题
//!    （设计稿 dark 单档）。真值全部住在 [`design`]，页面代码里不许出现色值。
//! 2. **安装 = 复制命令**（设计稿 `data-copy` 的语义，不是安装动作）：点「安装」
//!    把 `pi install npm:<pkg>` 写入剪贴板，按钮闪 1.2s「已复制」。**不加**二次
//!    确认、不查已装状态——设计稿里没有这两个概念。
//! 3. **打字只过滤当前页**（设计稿 JS 同款：0 网络、160ms 去抖）；**回车**才
//!    发服务端搜索（npm 上逐键搜是灾难）。两者共用同一个输入框，靠
//!    `query == filter` 判定「服务端已按同词搜过」，避免二次过滤把结果吃掉。

mod actions;
mod card;
mod controls;
pub(crate) mod design;
mod package;
mod render;
mod time;

use gpui::SharedString;

use crate::Chat;

use pi_link::market::pi_ext_market::{MarketEntry, MarketKind, PAGE_SIZE};

use crate::i18n::tr;
use crate::ui::TextInput;

pub(crate) use render::page;

/// 「最近发布」条数（设计稿四列 × 两行）。
pub(crate) const RECENT_N: usize = 8;
/// 页码深度上限（与 040-1 同口径：npm 搜索分页不无限翻）。
const MAX_PAGES: usize = 20;
/// 打字过滤去抖（设计稿 JS 160ms）。
const FILTER_DEBOUNCE_MS: u64 = 160;

/// 类型筛选（设计稿 `<select name="type">` 的选项集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum NativeKind {
    #[default]
    All,
    Extension,
    Skill,
    Theme,
    Prompt,
}

impl NativeKind {
    pub(crate) const ALL: [NativeKind; 5] = [
        NativeKind::All,
        NativeKind::Extension,
        NativeKind::Skill,
        NativeKind::Theme,
        NativeKind::Prompt,
    ];

    /// 与 npm 清单 `kinds` 对齐的键（All = None = 不过滤）。
    pub(crate) fn key(self) -> Option<&'static str> {
        match self {
            NativeKind::All => None,
            NativeKind::Extension => Some("extension"),
            NativeKind::Skill => Some("skill"),
            NativeKind::Theme => Some("theme"),
            NativeKind::Prompt => Some("prompt"),
        }
    }

    /// 下拉与徽标共用文案（设计稿全小写，浏览器渲染由 `text-transform` 大写）。
    pub(crate) fn label(self) -> &'static str {
        match self {
            NativeKind::All => "All types",
            NativeKind::Extension => "extension",
            NativeKind::Skill => "skill",
            NativeKind::Theme => "theme",
            NativeKind::Prompt => "prompt",
        }
    }
}

/// 排序（设计稿 `<select name="sort">`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum NativeSort {
    /// npm 服务端序（权重 ≈ 下载量）
    #[default]
    Downloads,
    /// 按发布时间倒序（**当前页内**客户端排序）
    Recent,
    /// 名称 A-Z（当前页内）
    Name,
}

impl NativeSort {
    pub(crate) const ALL: [NativeSort; 3] =
        [NativeSort::Downloads, NativeSort::Recent, NativeSort::Name];

    pub(crate) fn label(self) -> &'static str {
        match self {
            NativeSort::Downloads => "Most downloads",
            NativeSort::Recent => "Recently published",
            NativeSort::Name => "A-Z",
        }
    }
}

/// 页面状态（挂 `aux.market.native`）。
pub(crate) struct NativeState {
    /// 搜索框（页面首次打开时创建）
    pub(crate) input: Option<gpui::Entity<TextInput>>,
    /// 服务端搜索词（回车/搜索钮）
    pub(crate) query: String,
    /// 页内实时过滤词（打字，0 网络）
    pub(crate) filter: String,
    pub(crate) kind: NativeKind,
    pub(crate) sort: NativeSort,
    /// 类型/排序两个下拉的收起守卫（共享一个实例，互斥由 open 标志各管）
    pub(crate) kind_guard: Option<gpui::Entity<crate::ui::DropdownState>>,
    pub(crate) sort_guard: Option<gpui::Entity<crate::ui::DropdownState>>,
    pub(crate) kind_open: bool,
    pub(crate) sort_open: bool,
    pub(crate) entries: Vec<MarketEntry>,
    pub(crate) recent: Vec<MarketEntry>,
    pub(crate) total: usize,
    pub(crate) page: usize,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    /// 请求代际（换词/翻页/重置 ++；回包不符即丢）
    pub(crate) gen_ix: u64,
    pub(crate) loaded_once: bool,
    /// 行内安装中的包来源（`npm:<name>`）：按钮转圈 + 全卡动作禁用
    pub(crate) pending: Option<String>,
    /// 过滤去抖代际（每键 ++；到点的旧轮次不再 notify）
    pub(crate) filter_gen: u64,
    /// kinds 后台补齐在途（fetch 落地置位、补齐/失败清除）：空窗期空态要显示
    /// 「类型信息加载中」而不是「无匹配」——否则选类型后 ~15s 里像坏了。
    pub(crate) kinds_pending: bool,
    /// 会话级 kinds 表（name → 资源类型；首载时从磁盘缓存灌入）：重查补齐
    /// 命中即 0 网络，选类型立即出结果。落盘 = `KindsCache`（跨会话）。
    pub(crate) kinds: std::collections::HashMap<String, Vec<MarketKind>>,
    /// 变高虚拟列表（整页分段：最近发布 / 头+工具栏 / 卡行×N / 页码条）
    pub(crate) list: gpui::ListState,
    /// 列表竖向滚动条状态
    pub(crate) sb: gpui_component::scroll::ScrollbarState,
}

impl Default for NativeState {
    fn default() -> Self {
        Self {
            input: None,
            query: String::new(),
            filter: String::new(),
            kind: NativeKind::All,
            sort: NativeSort::default(),
            kind_guard: None,
            sort_guard: None,
            kind_open: false,
            sort_open: false,
            entries: Vec::new(),
            recent: Vec::new(),
            total: 0,
            page: 1,
            loading: false,
            error: None,
            gen_ix: 0,
            loaded_once: false,
            pending: None,
            filter_gen: 0,
            kinds_pending: false,
            kinds: std::collections::HashMap::new(),
            list: gpui::ListState::new(0, gpui::ListAlignment::Top, gpui::px(2000.)),
            sb: gpui_component::scroll::ScrollbarState::default(),
        }
    }
}

impl NativeState {
    pub(crate) fn total_pages(&self) -> usize {
        self.total.div_ceil(PAGE_SIZE).max(1).min(MAX_PAGES)
    }

    /// 当前页可见条目（过滤 + 排序的唯一口径；count 文案也读它）。
    pub(crate) fn visible(&self) -> Vec<&MarketEntry> {
        // 服务端已按 `query` 搜过时不再二次过滤同词（否则 npm 按 keywords
        // 命中的条目会被描述里没这个词判掉）
        let needle = if self.filter.trim().is_empty() || self.query.trim() == self.filter.trim() {
            ""
        } else {
            self.filter.trim()
        };
        let needle_lc = needle.to_lowercase();
        let mut list: Vec<&MarketEntry> = self
            .entries
            .iter()
            .filter(|e| {
                if needle_lc.is_empty() {
                    return true;
                }
                let hay = format!(
                    "{} {} {}",
                    e.name,
                    e.description,
                    e.author.clone().unwrap_or_default()
                )
                .to_lowercase();
                hay.contains(&needle_lc)
            })
            .filter(|e| match self.kind.key() {
                None => true,
                Some(k) => e.kinds.iter().any(|x| x.key() == k),
            })
            .collect();
        match self.sort {
            NativeSort::Downloads => {}
            NativeSort::Recent => list.sort_by(|a, b| b.updated.cmp(&a.updated)),
            NativeSort::Name => {
                list.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            }
        }
        list
    }
}

/// 条目主类型（徽标文案；无清单 = package）。
pub(crate) fn primary_kind(e: &MarketEntry) -> &'static str {
    match e.kinds.first() {
        Some(k) => k.key(),
        None => "package",
    }
}

/// 千分位（`12,365`）：设计稿结果计数用 `5,573` 这种写法。
pub(crate) fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// 页面文案：设计稿里的英文原串（官网同款，**不进 i18n 表**——这是移植稿的
/// 「原文照抄」部分；只有 安装/已复制 走 i18n，因为设计稿自己就是中文）。
pub(crate) fn label(s: &str) -> SharedString {
    SharedString::from(s.to_string())
}

/// 页内「install」按钮文案（busy 时改「…」由 card 侧处理）。
pub(crate) fn install_label() -> SharedString {
    SharedString::from(tr("market.install").to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pi_link::market::pi_ext_market::MarketKind;

    fn entry(name: &str, desc: &str, kinds: Vec<MarketKind>, updated: &str) -> MarketEntry {
        MarketEntry {
            name: name.to_string(),
            description: desc.to_string(),
            kinds,
            updated: updated.to_string(),
            ..Default::default()
        }
    }

    fn state(entries: Vec<MarketEntry>) -> NativeState {
        NativeState {
            entries,
            ..Default::default()
        }
    }

    #[test]
    fn filter_matches_name_desc_author() {
        let mut a = entry("pi-alpha", "does things", vec![], "2026-01-01");
        a.author = Some("alice".into());
        let st = state(vec![a, entry("beta", "other", vec![], "2026-01-02")]);
        assert_eq!(st.visible().len(), 2);
        let mut st = st;
        st.filter = "alpha".into();
        assert_eq!(st.visible().len(), 1);
        st.filter = "alice".into();
        assert_eq!(st.visible().len(), 1);
        st.filter = "zzz".into();
        assert!(st.visible().is_empty());
    }

    #[test]
    fn server_query_is_not_reapplied_locally() {
        // 服务端已按同词搜过 → 描述里没有该词也不许判掉
        let st = {
            let mut s = state(vec![entry("mcp-thing", "no match word", vec![], "x")]);
            s.query = "mcp".into();
            s.filter = "mcp".into();
            s
        };
        assert_eq!(st.visible().len(), 1);
        // 词不同则本地过滤照常生效
        let st = {
            let mut s = state(vec![entry("mcp-thing", "no match word", vec![], "x")]);
            s.query = "mcp".into();
            s.filter = "brandnew".into();
            s
        };
        assert!(st.visible().is_empty());
    }

    #[test]
    fn kind_filter_uses_manifest_kinds() {
        let st = {
            let mut s = state(vec![
                entry("a", "", vec![MarketKind::Extension], "x"),
                entry("b", "", vec![MarketKind::Skill], "x"),
                entry("c", "", vec![], "x"),
            ]);
            s.kind = NativeKind::Extension;
            s
        };
        let names: Vec<String> = st.visible().iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, vec!["a".to_string()]);
    }

    #[test]
    fn sort_by_name_and_recent() {
        let mut st = state(vec![
            entry("b", "", vec![], "2026-01-02"),
            entry("a", "", vec![], "2026-01-03"),
        ]);
        st.sort = NativeSort::Name;
        assert_eq!(st.visible()[0].name, "a");
        st.sort = NativeSort::Recent;
        assert_eq!(st.visible()[0].name, "a");
    }

    #[test]
    fn total_pages_caps() {
        let mut st = NativeState::default();
        st.total = PAGE_SIZE * 3;
        assert_eq!(st.total_pages(), 3);
        st.total = 0;
        assert_eq!(st.total_pages(), 1);
        st.total = PAGE_SIZE * 100;
        assert_eq!(st.total_pages(), MAX_PAGES);
    }
}
