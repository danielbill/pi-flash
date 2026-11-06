//! 文件 tab 打开路由（自 tabs.rs 外移）：路径 → 编辑器 kind 判定 →
//! file_cache 装载/复用。

use super::*;

impl Chat {
    /// v54.4：文件打开路由——统一打开为内容区 tab，主体渲染与编辑器配置
    /// 由 kind handler 决定（未注册类型走默认文本 kind）。html/htm 直接
    /// 开源码（2026-10-12 二改拍板，不再旁路系统浏览器；浏览器入口保留在
    /// 右键菜单首位文件动作）。
    ///
    /// 023 改版：编辑器实体懒创建（InputState::new 要 window，本调用链
    /// 没有——渲染帧在 content.rs 里补），二进制/超限文件提示不打开。
    pub(crate) fn open_file_tab(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        // 消息文本/远程插件可能传来相对路径：统一按 cwd 绝对化（已开复用
        // 的 same_path 比较与磁盘读取都基于绝对形态）
        let path = if path.is_absolute() {
            path
        } else {
            // 相对路径按当前项目绝对化；无项目态无从解析，拒绝打开
            let Some(cwd) = self.ws.cwd.clone() else {
                return;
            };
            cwd.join(path)
        };
        const MAX: u64 = 10 * 1024 * 1024;
        if let Ok(md) = std::fs::metadata(&path) {
            // 目录不是编辑器对象：资源管理器打开（zed 路径链接同型）。
            // 消息里的项目根/目录路径、插件误传都落这里——Windows 上
            // fs::read 目录 = ERROR_ACCESS_DENIED (5)，不再冒出误导性的
            // 「读取失败：拒绝访问」（2026-11-04 pf_test 项目根路径实案）
            if md.is_dir() {
                crate::services::shell_open::open_in_explorer(&path);
                return;
            }
            if md.len() > MAX {
                self.set_status(
                    format!(
                        "{}：{}",
                        crate::i18n::tr("file_view.too_large"),
                        path.display()
                    ),
                    cx,
                );
                return;
            }
        }
        // 已打开则直接切过去（Zed 行为）：不重读磁盘、不整文重灌——那会
        // 复位滚动/光标，并在大文件上冻结数百毫秒。外部改动由 fs watcher
        // 负责：cwd 内文件走 cwd 的递归 watch，cwd 外文件由
        // retain_file_watches 按所在目录单挂监听，两边信号都经
        // check_external_file_changes 按 disk_sig 复查后自动重载/标冲突。
        if let Some(ix) = self
            .ws
            .panel_tabs
            .iter()
            .position(|t| matches!(t, PanelTab::File(p) if same_path(p, &path)))
        {
            self.activate_panel_tab(ix, cx);
            self.set_content_view(ContentView::File);
            self.ws.pending_focus_file = Some(path.clone());
            cx.notify();
            return;
        }
        // 装载 + 挂带（读盘/解码/入缓存/单文件监听），不激活不切视图
        // ——与启动 tab 恢复共用同一条装载路（pi-flash-weia）
        if !self.load_file_tab_core(path.clone(), cx) {
            return;
        }
        let ix = self.ws.panel_tabs.len() - 1;
        self.activate_panel_tab(ix, cx);
        self.set_content_view(ContentView::File);
        cx.notify();
    }

    /// 文件 tab 装载核心：读盘 + kind 认领/文本解码 + 入 file_cache +
    /// cwd 外单文件监听重算 + tab 带挂尾。**不激活、不切视图、不落盘**
    ///（open_file_tab 与启动恢复共用；激活语义两路不同：用户点开要
    /// 前台+焦点，恢复路最后按存档统一复位）。读失败落状态条返回 false。
    pub(super) fn load_file_tab_core(&mut self, path: PathBuf, cx: &mut Context<Self>) -> bool {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                // 带上完整路径：消息里的路径点击失败（相对解析/拼写/盘符），
                // 没有路径无法定位是哪个文件没打开。
                // 目录漏进来（open_file_tab 守卫与读取间的竞态/恢复路）：
                // Windows fs::read 目录报「拒绝访问 (os error 5)」，直译会
                // 误导排障——换目录提示（2026-11-04 pf_test 实案）
                let detail = if path.is_dir() {
                    crate::i18n::tr("file_view.is_dir").to_string()
                } else {
                    e.to_string()
                };
                self.set_status(
                    format!(
                        "{} {}: {}",
                        crate::i18n::tr("file_view.read_failed"),
                        path.display(),
                        detail
                    ),
                    cx,
                );
                return false;
            }
        };
        // 打开路由对齐 zed ProjectItemRegistry 逆序尝试：图片类 kind 先认领
        // （ImageItem::try_open 先于 editor），认领失败/文本类 kind 落文本
        // 管线（decode_text），Binary → 无法打开灰屏（InvalidItemView 同构）
        // ——不再「含 NUL 一律灰屏」，png/bmp 等真图片由此可开。
        let kind = crate::editor::kind::kind_of(&path);
        let mut ft = FileTab::from_disk(String::new());
        if let Some(format) = kind.try_binary(&bytes) {
            // 图片查看 tab（zed ImageView）：无缓冲，保存/冲突检测按 binary 位跳过
            ft.binary = true;
            ft.image = Some(format);
        } else {
            // 文本管线：zed decode_text 同款——BOM/UTF-16/chardetng 嗅探；
            // 二进制判败 → 灰屏 tab 照开让用户看见是哪个文件，主体渲染
            // 「无法打开 {ext} 类型文件」（view.rs unsupported_body）
            match crate::editor::file_content::decode_text(bytes) {
                Ok(decoded) => {
                    ft = FileTab::from_disk(decoded.text);
                    ft.encoding = decoded.encoding;
                    ft.has_bom = decoded.has_bom;
                }
                Err(_) => {
                    ft.binary = true;
                }
            }
        }
        let binary = ft.binary;
        if binary {
            // 二进制（图片/无法打开）没有行可跳：搜索结果里点到它时清掉
            // 待跳请求，免得滞留在同路径的下一次文本打开上
            self.ws.pending_jump = None;
        } else {
            ft.disk_sig = file_sig(&path);
        }
        self.ws.file_cache.insert(path.clone(), ft);
        if !binary {
            self.ws.pending_focus_file = Some(path.clone());
        }
        self.ws.panel_tabs.push(PanelTab::File(path));
        // cwd 外文件：按所在目录补一条单文件监听（否则磁盘改了不同步）
        self.retain_file_watches();
        true
    }
}
