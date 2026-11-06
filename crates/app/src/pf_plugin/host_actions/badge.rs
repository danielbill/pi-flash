//! PF 插件宿主——会话列表徽标（契约 v3 sessions.setBadge）：每行渲染结果
//! 的载体无关取数（1s row_cache）+ 终装配（pin 优先、上限 5 保右弃左）。

use serde_json::Value;

use crate::Chat;

use super::*;

impl Chat {
    /// 会话列表徽标（契约 v3）：命中该会话的**全部**徽标 → [(icon 资产路径, tone)]，
    /// 每插件一条（owner 唯一）。绘制归宿主：PF 主体 pin 优先最右，插件按注册
    /// 序从右到左、间隔 10px，上限 5 保右弃左（用户定夺 2026-12，替代
    /// 2026-10-28 的"不裁不并"）。
    pub(crate) fn plugin_badge(
        &mut self,
        session: &std::path::Path,
        cx: &mut gpui::Context<Self>,
    ) -> Vec<(SharedString, String)> {
        let key = session.to_string_lossy().into_owned();
        let mut out: Vec<(SharedString, String)> = badges_for(&self.plugins.badges, session);
        const TTL: std::time::Duration = std::time::Duration::from_millis(1000);
        let n = self.plugins.plugins.len();
        // 同包只问一次（2026-12 真机：内存条目异常重复时防双徽标；row_cache
        // 按 pkg 命中已能兜住大部分，这里是显式收口）
        let mut seen_pkg: std::collections::HashSet<String> = std::collections::HashSet::new();
        for ix in 0..n {
            // 能力面（8tb.2）：徽标是每行渲染结果，载体无关——JS 未导出 /
            // wasm 未实现都回 None（负缓存 TTL 内不再问）
            let (pkg, capable) = {
                let e = &self.plugins.plugins[ix];
                (e.manifest.pkg.clone(), !e.disabled)
            };
            if !capable || !seen_pkg.insert(pkg.clone()) {
                continue;
            }
            if let Some(c) = self
                .plugins
                .row_cache
                .iter()
                .find(|c| c.pkg == pkg && c.session_key == key && c.at.elapsed() < TTL)
            {
                if let Some((icon, tone)) = &c.badge {
                    let id = self.plugins.plugins[ix].manifest.id.clone();
                    out.push((contrib_icon_path(&id, icon), tone.clone()));
                }
                continue;
            }
            if !self.plugin_ready(ix, cx) {
                continue;
            }
            let env = serde_json::json!({ "session": { "key": key } }).to_string();
            let (r, _) = {
                let entry = &mut self.plugins.plugins[ix];
                entry.hook_session_row(&env)
            };
            let badge = match r {
                Ok(Some(json)) => {
                    let v: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
                    let icon = v
                        .get("icon")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let tone = v
                        .get("tone")
                        .and_then(Value::as_str)
                        .unwrap_or("accent")
                        .to_string();
                    // 逐行命中追踪，只在 debug 构建打（release 包零输出）
                    if cfg!(debug_assertions) {
                        eprintln!(
                            "[pf-plugin] session_list_render 命中 pkg={} session={} icon={}",
                            self.plugins.plugins[ix].manifest.pkg, key, icon
                        );
                    }
                    if icon.is_empty() {
                        None
                    } else {
                        let id = self.plugins.plugins[ix].manifest.id.clone();
                        out.push((contrib_icon_path(&id, &icon), tone.clone()));
                        Some((icon, tone))
                    }
                }
                _ => None,
            };
            self.plugins.row_cache.push(RowCache {
                pkg,
                session_key: key.clone(),
                badge,
                at: std::time::Instant::now(),
            });
        }
        // 终装配：PF 主体 pin（置顶态）优先最右 + 上限 5 保右弃左
        assemble_row_badges(self.session_pinned(session), out)
    }
}
