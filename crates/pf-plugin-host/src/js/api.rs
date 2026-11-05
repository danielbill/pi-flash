//! JS 插件入口方法（activate/unload/render/面板与命令族）——itlq.7 自
//! js.rs 外迁：第二 `impl JsPlugin` 块，只搬不改，字段经 pub(super) 触达。

use super::*;

/// 单次 JS 调用上限（ms）。插件逻辑是清单级小计算，5s 已是百倍宽裕。
const JS_CALL_DEADLINE_MS: u64 = 5_000;

impl JsPlugin {
    /// 布防死线：本次入口调用最多 JS_CALL_DEADLINE_MS。
    fn arm_deadline(&self) {
        self.deadline.store(
            now_ms() + JS_CALL_DEADLINE_MS,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    /// `activate(ctx)`：注册 onMenu / onCommand / onPanelOpen（无 activate 导出 = 纯渲染插件，放行）。
    pub fn activate(&self) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> rquickjs::Result<()> {
                let f: Function = ctx.globals().get("__pf_activate")?;
                f.call::<(), ()>(())?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// `unload()`（90-9 §2）：停用/卸载/更新覆盖/退出前的最后一次调用。
    /// 未导出 = no-op（`__pf_unload` 胶水恒在，内部按导出判空）。Err 只落
    /// plugin.log，宿主不等待确认（best-effort，关不掉的插件不能卡宿主）。
    pub fn unload(&self) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> rquickjs::Result<()> {
                let f: Function = ctx.globals().get("__pf_unload")?;
                f.call::<(), ()>(())?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// `render(ctx)` → 组件树 JSON（01 规范；v2 无 rows）。
    pub fn render_json(&self) -> Result<String, String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<String, rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_render_str")?;
                f.call::<(), String>(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发右键菜单回调 `onMenu(id, {path, fileIcon})`（未注册 = no-op）。
    pub fn fire_menu(&self, id: &str, path: &str, file_icon: &str) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_menu")?;
                f.call::<(&str, &str, &str), ()>((id, path, file_icon))?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发命令回调（env 负载形态，契约 v3）：fn_bar 点击 =
    /// {"session":{"key":"…"}}，switch 翻转 = {"value":bool}；未注册 = no-op。
    pub fn fire_command_env(&self, id: &str, env_json: &str) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_cmd_env")?;
                f.call::<(&str, &str), ()>((id, env_json))?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发组件菜单回调 `onCommand(id, {path})`（未注册 = no-op）。
    pub fn fire_command(&self, id: &str, path: &str) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_cmd")?;
                f.call::<(&str, &str), ()>((id, path))?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// 调 fn_bar(ctx, env)（契约 v3 功能栏 chrome）。未导出 → Ok(None)。
    /// 返回 JSON 串 {"icon":"…","tone":"…","tip":…}。
    pub fn fn_bar_json(&self, env_json: &str) -> Result<Option<String>, String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> rquickjs::Result<String> {
                let f: Function = ctx.globals().get("__pf_fn_bar_str")?;
                f.call::<(&str,), String>((env_json,))
            })
            .map(|s| if s == "null" { None } else { Some(s) })
            .map_err(|e| e.to_string())
    }

    /// PF 钩子 page_active(ctx, env)：页面激活推送（返回值恒 null，忽略）。
    pub fn page_active(&self, env_json: &str) -> Result<String, String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> rquickjs::Result<String> {
                let f: Function = ctx.globals().get("__pf_page_active_str")?;
                f.call::<(&str,), String>((env_json,))
            })
            .map_err(|e| e.to_string())
    }

    /// PF 钩子 session_row(ctx, env) → null | {icon,tone}（会话列表时间位徽标）。
    pub fn session_row_json(&self, env_json: &str) -> Result<Option<String>, String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> rquickjs::Result<String> {
                let f: Function = ctx.globals().get("__pf_session_row_str")?;
                f.call::<(&str,), String>((env_json,))
            })
            .map(|s| if s == "null" { None } else { Some(s) })
            .map_err(|e| e.to_string())
    }

    /// render_popup(ctx)（契约 v3 弹窗内容；无导出退回 render）→ 树 JSON。
    pub fn render_popup_json(&self) -> Result<String, String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<String, rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_render_popup_str")?;
                f.call::<(), String>(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发面板激活回调 `onPanelOpen()`（未注册 = no-op）。
    pub fn fire_panel_open(&self) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_panel_open")?;
                f.call::<(), ()>(())?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发组件状态广播回调 `onBroadcast(name, payload)`（未注册 = no-op）。
    /// payload = 负载 JSON 串（契约 v2.4 broadcast_events）。
    pub fn fire_broadcast(&self, name: &str, payload: &str) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_broadcast")?;
                f.call::<(&str, &str), ()>((name, payload))?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }

    /// 触发执行域推送回调 `onMessage(payload, meta)`（未注册 = no-op）。
    /// payload = 不透明负载 JSON 串；meta = {anchor, key}（来源实例锚点 + 会话当前键）。
    pub fn fire_svc_message(&self, payload: &str, meta: &str) -> Result<(), String> {
        self.arm_deadline();
        self.ctx
            .with(|ctx| -> Result<(), rquickjs::Error> {
                let f: Function = ctx.globals().get("__pf_fire_svc_message")?;
                f.call::<(&str, &str), ()>((payload, meta))?;
                Ok(())
            })
            .map_err(|e| e.to_string())
    }
}
