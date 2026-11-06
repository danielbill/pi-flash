//! Chat 渲染根（P3 自 main.rs 拆出；itlq.1 按布局再拆）：impl Render 只做
//! 编排——帧头仪器、揭幕、焦点仲裁（focus）、pill 弹层（pill）、三段布局
//! 装配（body）、overlay 叠放（overlay）。各段以 impl Chat 私有方法住在
//! 同目录子模块。

use crate::*;

mod body;
mod focus;
mod overlay;
mod pill;

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

// toast 不加阴影：早前的手工双层黑色 halo 贴片（因 gpui box_shadow 在
// DirectX 下过淡而自绘）在浅色主题下被用户判定为「一圈脏灰边」，已整体
// 移除——toast 只保留 bg_panel + border 的贴片本体。

impl Render for Chat {
    fn render(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) -> impl IntoElement {
        let __frame_t = std::time::Instant::now();
        // 焦点快照（给「agent 结束时窗口没焦点就给点」用）：平台层在每次
        // 激活变化都会 window.refresh() 强制重绘，所以这里的值总是新鲜的
        self.psp.window_active = window.is_window_active();
        // ctx.ui.setTitle → OS 窗口标题（zi2s.9，pi-web document.title parity）：
        // apply-diff 守卫，变了才落平台调用（每帧 set_title 会抖动任务栏）
        if let Some(title) = &self.aux.ext_title {
            if *title != self.aux.ext_title_applied {
                window.set_window_title(title);
                self.aux.ext_title_applied = title.clone();
            }
        }
        static FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
        if PERF.load(std::sync::atomic::Ordering::Relaxed)
            && !FIRST.swap(true, std::sync::atomic::Ordering::Relaxed)
        {
            if let Some(t0) = T0.get() {
                eprintln!("[perf] first frame: {:?}", t0.elapsed());
            }
        }
        // [perf] 仪器：Chat 每秒实际渲染次数（PI_FLASH_PERF=1 开启）。idle
        // 应≈1（光标闪烁）；编辑/拖选时≈事件率；无事件却持续 >30/s = 存在
        // 自持重绘循环（v62 排查文本编辑卡顿的探针，留作常备仪器）。
        static RENDER_N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        static RENDER_T: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
        static RENDER_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *RENDER_ON.get_or_init(|| std::env::var("PI_FLASH_PERF").is_ok()) {
            let n = RENDER_N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let mut slot = RENDER_T.lock().unwrap();
            let report = match *slot {
                Some(t0) if t0.elapsed().as_secs_f32() >= 1.0 => {
                    Some(n as f32 / t0.elapsed().as_secs_f32())
                }
                Some(_) => None,
                None => {
                    *slot = Some(std::time::Instant::now());
                    None
                }
            };
            if let Some(rate) = report {
                eprintln!("[perf] chat renders/s: {rate:.0}");
                RENDER_N.store(0, std::sync::atomic::Ordering::Relaxed);
                *slot = Some(std::time::Instant::now());
            }
        }
        // gpui 解码位图回收：PF 侧图片缓存（消息图/mermaid/math/本地图）
        // 上一帧驱逐的条目在这里统一从 gpui loading_assets 摘除——该表
        // 只进不出，不主动摘的话解码位图常驻到进程退出（见 asset_evict）
        crate::services::asset_evict::drain(cx);
        // 010-启动：揭幕前整幅启动页（黑底、居中 logo、非最大化）；揭幕帧
        // 再按 §4 默认最大化（0.2.2 建窗即 Maximized 不可靠，显式 zoom 是
        // 项目已验证路径——原建窗回调里的 zoom 后移到了这里）。
        if !self.pool.booted {
            startup::mark_splash_painted();
            return startup::splash_view();
        }
        if self.pool.pending_zoom {
            self.pool.pending_zoom = false;
            window.zoom_window();
        }
        // keep terminal focus alive across frames (render focuses chat input
        // otherwise, which would steal it back every redraw) —— 见 focus.rs
        self.focus_arbitrate(window, cx);
        let t = T();

        let entity = cx.entity();
        let weak = entity.downgrade();
        let weak_for_dialog = weak.clone();

        // toolbar pill 弹层（thinking/tools 菜单，见 pill.rs）
        let pill_menu_el = self.pill_menu_element(&weak, t, window, cx);

        // ---- v54 body: panel-col (topbar-l + dock + statusbar) | content-col
        let body = self.body_shell(entity.clone(), weak.clone(), t, window, cx);
        let root = body::root_shell(body, t, cx);

        // overlay 叠放：settings → dialogs → 确认浮层 → psp 悬浮 → picker
        // → toast（见 overlay.rs）
        let root = self.overlay_stack(root, weak_for_dialog, pill_menu_el, t, window, cx);

        // blocking extension dialog (select/confirm/input/editor)：已迁
        // session/mod.rs 会话域居中浮层槽（chat-wrap 中栏，与 composer 同
        // 基准居中；ask_card 同语义），不再挂全窗根
        crate::perf_span("frame.render", __frame_t);
        static MJ_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        if *MJ_ON.get_or_init(|| std::env::var("PI_FLASH_PERF").is_ok()) {
            eprintln!(
                "[perf] frame.mj_reads_total: {}",
                crate::MJ_READS.load(std::sync::atomic::Ordering::Relaxed)
            );
        }
        root
    }
}
