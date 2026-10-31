//! PF 插件运行态条目（02 §3）：[`PluginEntry`]（JS/wasm 双载体）+ 行缓存/
//! 徽标/弹窗/功能条状态类型 + [`PluginUi`]（Chat.plugins 字段簇）+
//! [`ChatHost`]（impl JsHost：插件回调宿主）+ 徽标/贡献项图标映射纯函数。
//! 090 重构自 mod.rs 拆出。

use super::*;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui::SharedString;
use pf_plugin_host::js::{JsAction, JsHost, JsPlugin};
use pf_plugin_host::{PluginAction, PluginInstance};

// ---------------------------------------------------------------------------
// Chat 侧状态 + ctx 宿主实现
// ---------------------------------------------------------------------------

/// 单个插件条目：清单 + 懒加载运行时 + 面板行缓存。
pub(crate) struct PluginEntry {
    pub manifest: PluginManifest,
    /// 90-9 §5 生命周期状态（宿主事实；转移逻辑在 lifecycle::transition，
    /// 这里只持字段）
    pub state: lifecycle::State,
    /// 会话级禁用（插件逻辑抛异常后置位；02 §4）
    pub disabled: bool,
    /// QuickJS 运行时（懒建；主线程独占）
    pub js: Option<JsPlugin>,
    /// 内层 wasm 实例（加密包；懒装载——首次开面板时主线程 `load_cached`
    /// 毫秒级实例化，cwasm 缓存由启动预热。验签门在启动后台线程已过一次，
    /// 这里 init 再跑一遍是全新实例的自拒门，语义不变）
    pub wasm: Option<PluginInstance>,
    /// wasm 面板键盘焦点（点击面板时抢入；键盘回灌只在其聚焦时收）
    pub wasm_focus: Option<gpui::FocusHandle>,
    /// 宿主能力载体（与 js 同生；fire 前清 actions）
    pub host: Option<Rc<RefCell<ChatHost>>>,
    /// 最近一次 render 的组件树（01 规范；渲染器消费。v2 无 rows）
    pub body: Option<serde_json::Value>,
    pub tree_loaded: bool,
    /// 右键菜单弹层（None = 收起；面板渲染时消费）
    pub cell_menu: Option<crate::pf_plugin::panel::CellMenuState>,
    /// 分区比例（宿主 UI 状态，内存态；重启回等分）
    pub split_ratios: std::cell::RefCell<Vec<f32>>,
    /// 正在拖动分隔线的瞬时状态
    pub split_drag: std::cell::RefCell<Option<crate::pf_plugin::panel::SplitDrag>>,
    /// 悬停中的分隔线序号（§5.12 统一分隔线：hover 与拖拽同款 4px accent。
    /// hover 走字段而非 gpui hover 样式——后者改布局属性不可靠，且与其他
    /// 两处分隔线会成两套逻辑）
    pub split_hover: std::cell::RefCell<Option<usize>>,
    /// 面板滚动（左列格子容器；跨帧存续）
    /// 面板滚动（要求⑤：上下两分栏各自管理；跨帧存续）
    /// 分栏滚动句柄 + psp 滚动条状态（要求⑤：每 group 一对；渲染期按序
    /// 懒扩容——渲染期 &self 内部可变性，先例 at_filter_cache/bubble_scrolls。
    /// 滚动条 = psp_scrollbar：hover 常显 / 3s 淡出 / 无轨道底，平时隐形，
    /// 面板静止即纯居中排版）
    pub section_scrolls: std::cell::RefCell<Vec<gpui::ScrollHandle>>,
}

impl PluginEntry {
    // ---- 能力面取数（8tb.2）：同一签名，载体分发收在实例调用这一处；
    // 调用方（fn_bar chrome / 徽标 / 弹窗）按清单贡献项取数，不按载体分支。
    // JS 动作入 host 队列随返回带走；wasm 动作走 pump（OpenPopup 等意图）。

    /// 契约 v3 fn_bar chrome（JS fn_bar / wasm fn-bar，同形 JSON）。
    /// 返回 (chrome，期间动作)。chrome = None = 本会话无（含未导出/未实现）。
    pub(crate) fn hook_fn_bar(
        &mut self,
        env: &str,
    ) -> (Result<Option<String>, String>, Vec<JsAction>) {
        if let Some(inst) = self.wasm.as_mut() {
            // chrome 是纯取数面：wasm 动作不在 chrome 期放行（与 JS 同口径，
            // JS 侧例外只有徽标声明，wasm 面无此动作）
            let r = inst.fn_bar_json(env);
            let _ = inst.pump_actions();
            return (r, Vec::new());
        }
        let Some(js) = self.js.as_ref() else {
            return (Ok(None), Vec::new());
        };
        let host = self.host.as_ref().expect("js 载体必有 host").clone();
        host.borrow_mut().actions.clear();
        let r = js.fn_bar_json(env);
        let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
        (r, drained)
    }

    /// PF 钩子 session_row 徽标（JS session_list_render / wasm session-row）。
    pub(crate) fn hook_session_row(
        &mut self,
        env: &str,
    ) -> (Result<Option<String>, String>, Vec<JsAction>) {
        if let Some(inst) = self.wasm.as_mut() {
            let r = inst.session_row_json(env);
            let _ = inst.pump_actions();
            return (r, Vec::new());
        }
        let Some(js) = self.js.as_ref() else {
            return (Ok(None), Vec::new());
        };
        let host = self.host.as_ref().expect("js 载体必有 host").clone();
        host.borrow_mut().actions.clear();
        let r = js.session_row_json(env);
        let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
        (r, drained)
    }

    /// 契约 v3 弹窗体（JS render_popup / wasm render-popup；wasm 未导出时
    /// 退回 v2 render 面）。返回 (体 JSON，期间动作)。
    pub(crate) fn hook_render_popup(
        &mut self,
    ) -> (Result<String, String>, Vec<JsAction>, Vec<PluginAction>) {
        if let Some(inst) = self.wasm.as_mut() {
            let r = inst.render_popup_json();
            let acts = inst.pump_actions();
            return (r, Vec::new(), acts);
        }
        let Some(js) = self.js.as_ref() else {
            return (Ok("null".to_string()), Vec::new(), Vec::new());
        };
        let host = self.host.as_ref().expect("js 载体必有 host").clone();
        host.borrow_mut().actions.clear();
        let r = js.render_popup_json();
        let drained = host.borrow_mut().actions.drain(..).collect::<Vec<_>>();
        (r, drained, Vec::new())
    }
}

/// 会话列表徽标（契约 v3 sessions.setBadge）：落时间位置，owner = 插件 id。
#[derive(Debug, Clone)]
/// PF 钩子 session_list_render 的缓存条目（徽标是渲染结果，缓存只省 JS 调用）。
pub(crate) struct RowCache {
    pub pkg: String,
    pub session_key: String,
    /// None = 本行无徽标（负缓存）
    pub badge: Option<(String, String)>,
    pub at: std::time::Instant,
}

pub(crate) struct PluginBadge {
    pub owner: String,
    pub session: String,
    pub icon: String,
    pub tone: String,
}

/// 弹窗打开态（契约 v3 popups）：一次一个（开新的关旧的）。
#[derive(Debug, Clone)]
pub(crate) struct PluginPopupOpen {
    pub ix: usize,
    pub id: String,
    pub title: String,
    pub body: Option<serde_json::Value>,
    pub tree_loaded: bool,
}

/// fn_bar chrome 缓存（契约 v3）：功能栏每帧渲染，QuickJS 调用不进每帧
/// ——1s TTL + 会话键变更即失效。wasm 面（wit v3 前）不参与。
#[derive(Debug, Clone)]
pub(crate) struct FnBarCache {
    pub pkg: String,
    pub session_key: String,
    pub icon: String,
    pub tone: String,
    pub tip: String,
    pub at: std::time::Instant,
}

/// Chat 上的插件簇（与 aux/ws 等并列的最小簇）。
#[derive(Default)]
pub(crate) struct PluginUi {
    pub plugins: Vec<PluginEntry>,
    /// 组件状态广播的上次已发快照（None = 从未发过/面板刚开，下一拍必发）。
    /// 类型 = broadcast::Snap：(project, model)。见 `broadcast.rs`。
    pub broadcast_last: Option<(Option<String>, Option<(String, String)>)>,
    /// 契约 v3 session 广播的上次已发会话键（差分守门）。
    pub session_last: Option<Option<String>>,
    /// 认领槽：插件实例 × 进行中会话（一个插件同时只有一条进行中线，
    /// 新建即换绑）。见 `Claim`。
    pub claims: Vec<Claim>,
    /// 打开中的插件弹窗（契约 v3；None = 无）。
    pub popup: Option<PluginPopupOpen>,
    /// 弹窗轮询代数：每次开/关自增，旧轮询循环见代数不符自行退出
    /// （重复点击不堆叠 1s 轮询）。
    pub popup_gen: u64,
    /// 弹窗 1s 重取的驱动状态（120ms 主泵调用；不另开循环）
    pub popup_poll_gen: u64,
    pub popup_poll_ix: Option<usize>,
    pub popup_poll_at: Option<std::time::Instant>,
    /// fn_bar chrome 缓存（键 = pkg + session_key，TTL 见 FnBarCache 用法）。
    pub fn_bar_cache: Vec<FnBarCache>,
    /// 会话列表徽标（旧命令式路径，保留字段供存量包；新路径 = session_list_render）
    pub badges: Vec<PluginBadge>,
    /// PF 钩子 session_list_render 的渲染缓存（pkg + 会话键，1s TTL）
    pub row_cache: Vec<RowCache>,
}

/// 插件认领槽记录（匿名归属：宿主只认识 "某插件 × 某 key"）。
#[derive(Debug, Clone)]
pub(crate) struct Claim {
    pub plugin_id: String,
    pub prefix: String,
    pub key: String,
}

/// 认领会话名：`{prefix}YYMMDDHHMMSSmmm`（与 wx 线同款 15 位时间戳口径，
/// 002 §5）。纯函数便于单测。
pub(crate) fn claim_session_name(prefix: &str) -> String {
    format!("{prefix}{}", chrono::Local::now().format("%y%m%d%H%M%S%3f"))
}

/// JsHost 实现（02 §3 提问类同步、动作类入队）。
/// 纯数据，不持 Chat——动作由 Chat 方法在 JS 返回后消费。
pub(crate) struct ChatHost {
    /// 插件 id（执行域信箱投递署名，90-6）
    pub(crate) id: String,
    /// 插件私有目录（`plugins/<id>/data/`）——插件自管，宿主不解析内容
    pub(crate) dir: PathBuf,
    pub actions: Vec<JsAction>,
}

impl ChatHost {
    /// `name` 只许是本目录下的文件名：拒绝分隔符、`..`、隐藏文件（02 §3 越界拒绝）
    pub(crate) fn safe_path(&self, name: &str) -> Option<PathBuf> {
        let bad = name.is_empty()
            || name.contains('/')
            || name.contains('\\')
            || name.contains("..")
            || name.starts_with('.');
        if bad {
            eprintln!("[pf-plugin] fs 越界拒绝: {name}");
            return None;
        }
        Some(self.dir.join(name))
    }
}

impl JsHost for ChatHost {
    fn host_proof(&mut self) -> String {
        // 090-5 L3：官方构建由 build.rs 注入签名材料；无注入构建为占位符（fail-closed）。
        let proof = option_env!("PF_HOST_PROOF").unwrap_or("PROOF_PLACEHOLDER");
        let kid = option_env!("PF_HOST_PROOF_KID").unwrap_or("");
        serde_json::json!({ "kid": kid, "proof": proof }).to_string()
    }
    fn is_executive(&mut self, path: &str) -> bool {
        crate::services::script_exec::exec_kind(Path::new(path)).is_some()
    }
    fn file_icon(&mut self, path: &str) -> String {
        crate::services::file_icons::get_icon(Path::new(path)).to_string()
    }
    fn dir(&mut self) -> String {
        self.dir.to_string_lossy().into_owned()
    }
    fn fs_read(&mut self, name: &str) -> String {
        // 缺失/不可读 → ""（02 §3：兜底是插件的责任，不是宿主的）
        self.safe_path(name)
            .and_then(|f| std::fs::read_to_string(f).ok())
            .unwrap_or_default()
    }
    fn fs_write(&mut self, name: &str, data: &str) {
        let Some(path) = self.safe_path(name) else {
            return;
        };
        // 原子写 = 临时文件 + rename（插件管内容，原子性归宿主）
        let tmp = path.with_extension("tmp");
        if let Err(e) = (|| -> std::io::Result<()> {
            if let Some(p) = tmp.parent() {
                std::fs::create_dir_all(p)?;
            }
            std::fs::write(&tmp, data.as_bytes())?;
            std::fs::rename(&tmp, &path)
        })() {
            eprintln!("[pf-plugin] fs 写入失败（{}）: {e}", path.display());
        }
    }
    fn svc_post(&mut self, anchor: &str, payload: &str) -> bool {
        svc::svc_post(&self.id, anchor, payload)
    }
    fn svc_query(&mut self, anchor: &str) -> String {
        svc::svc_query(anchor)
    }
    fn fs_list(&mut self) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(&self.dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_file())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out
    }
    fn queue(&mut self, action: JsAction) {
        self.actions.push(action);
    }
}

/// 命中会话的全部徽标（纯函数，测试锚点）：过滤 + icon 映射，**保序**
/// （声明序 = 绘制从右到左，首条最右）。
pub(crate) fn badges_for(
    badges: &[PluginBadge],
    session: &std::path::Path,
) -> Vec<(SharedString, String)> {
    badges
        .iter()
        .filter(|b| {
            crate::services::workspace::same_path(std::path::Path::new(&b.session), session)
        })
        .map(|b| (contrib_icon_path(&b.owner, &b.icon), b.tone.clone()))
        .collect()
}

/// 贡献项图标三来源映射：宿主内置名 / `assets:<名>` / `assets/` 相对路径。
pub(crate) fn contrib_icon_path(id: &str, icon: &str) -> SharedString {
    // `assets/` 是目录结构的一段，**不能剥**——曾剥掉导致
    // plugins/<slug>/bolt.svg NotFound（图标空白，还进负缓存）
    if icon.starts_with("assets/") {
        SharedString::from(format!("plugins/{id}/{icon}"))
    } else if let Some(name) = icon.strip_prefix("assets:") {
        SharedString::from(format!("plugins/{id}/assets/{name}"))
    } else {
        SharedString::from(format!("icons/{icon}.svg"))
    }
}
