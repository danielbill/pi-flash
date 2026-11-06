//! PF 插件宿主——包内 pi 扩展挂载通道（契约 v3 镜像协议）：挂载/摘除/存活
//! 判定（闲置回收豁免）与 pi 进程拉起。挂载台账合并写、版本层路径自愈等
//! 陷阱注释随实现保留。

use crate::Chat;

use super::*;

impl Chat {
    /// 该会话是否有「绑定中的服务实例」（闲置回收豁免判据）：**两条通道都算**。
    /// - 执行域（契约 v4 `contributes.service`）：注册表有 token + 实例配置在位；
    /// - 挂载通道（契约 v3 `contributes.pi_extensions`）：会话挂载台账里挂着任一
    ///   启用中插件的 pi 扩展——ext 进程就是通信/轮询本体。
    ///
    /// 挂载通道是 2026-11-01 补的漏：豁免原先只认执行域，而按用户 2026-10-30
    /// 定案回滚到镜像协议的插件（pf-wxbot）只走挂载通道 → 永远不豁免 → 绑定的
    /// **非活动**会话闲置满 `idle_cap` 即被回收，微信侧静默而镜像里绑定指针还在
    /// → icon 假绿。这正是本豁免注释当年记的事故形态（2026-10-30「发消息收不
    /// 到」），换了个挂载通道又复现。
    pub(crate) fn has_live_service(&self, key: &str, _cx: &mut gpui::Context<Self>) -> bool {
        if let Some(base) = pi_link::paths::dir()
            && pi_link::service_ext::config_path_if_any(&base, key).is_some()
        {
            return true;
        }
        let Some(store) = pi_link::session_ext::store_path() else {
            return false;
        };
        let mounts = pi_link::session_ext::read_for(&store, key);
        if mounts.is_empty() {
            return false;
        }
        let live: Vec<String> = self
            .plugins
            .plugins
            .iter()
            .filter(|e| !e.disabled && !e.manifest.pi_extensions.is_empty())
            .flat_map(|e| {
                e.manifest
                    .pi_extensions
                    .iter()
                    .map(|rel| e.manifest.dir.join(rel).to_string_lossy().into_owned())
            })
            .collect();
        mounts_overlap_live(&mounts, &live)
    }

    /// 摘除活跃运行时里本插件的 ext（attach 逆操作，90-9 §8）：evict/停用后
    /// 运行中的进程还带着已挂载 ext——**停用不再立即杀进程重启摘除**（2026-
    /// 11-01，pf-wxbot 下线通知）：壳 `unload()` 已写 `unload_seq` 镜像标记
    ///（PF↔插件唯一对话通道），ext 泵见标记自我隔离——停渠道、发下线通知、
    /// 释放锁，进程存活即可完成；立即 kill 则 ext 一拍都跑不到，通知永远发
    /// 不出。进程内物理摘除落在下次 spawn（重新启用 attach 走「空闲重启挂
    /// 载」重启时）。忙 → 不动进程，行为同前（ext 见标记同样自我隔离）。
    pub(crate) fn plugin_detach_active_ext(&mut self, ix: usize, cx: &mut gpui::Context<Self>) {
        let ext_paths: Vec<String> = match self.plugins.plugins.get(ix) {
            Some(e) => e
                .manifest
                .pi_extensions
                .iter()
                .map(|rel| e.manifest.dir.join(rel).to_string_lossy().into_owned())
                .collect(),
            None => return,
        };
        if ext_paths.is_empty() {
            return;
        }
        if let Some(rt) = self.pool.runtimes.get(&self.pool.active_key).cloned() {
            rt.update(cx, |r, _cx| {
                // 防御性再摘（attach 不入 ext_sources，路径型条目混入会污染）
                r.ext_sources.retain(|e| !ext_paths.iter().any(|p| e == p));
            });
        }
    }

    /// 包内 pi 扩展 → 当前会话扩展清单（契约 v3：绑定目标 = 用户当前普通
    /// 会话，挂载改在绑定时完成）。会话进程空闲 → 立即重启挂载；忙 → 清单
    /// 已写，下次 spawn 生效。
    ///
    /// `force_restart`：跳过「清单已含」短路、空闲即重启。**启用链必传
    /// true**——ext 的 unload 水位是进程终态（BOOT_TS 只增不退，「本进程
    /// 不复活」），停用→启用循环里若 pi 不重启，ext 永远停在终态、通信死
    /// （真机 2026-11-02：停用摘条目口径失配 → 短路成立 → 重启没发生）。
    pub(crate) fn plugin_attach_pi_exts(
        &mut self,
        ix: usize,
        pf_session_key: &str,
        force_restart: bool,
        cx: &mut gpui::Context<Self>,
    ) {
        let Some(entry) = self.plugins.plugins.get(ix) else {
            return;
        };
        if entry.manifest.pi_extensions.is_empty() || pf_session_key.is_empty() {
            return;
        }
        let pi_exts: Vec<String> = entry
            .manifest
            .pi_extensions
            .iter()
            .map(|rel| entry.manifest.dir.join(rel).to_string_lossy().into_owned())
            .collect();
        // 台账**合并写**（勿整表覆写）：会话自选的 npm 清单与插件挂载通道
        // 同住一份台账，覆写 = 把用户勾选抹掉（真机 2026-10-31：观宏知微
        // 会话台账只剩一条插件路径，就是被覆写没的）
        let Some(store) = pi_link::session_ext::store_path() else {
            return;
        };
        let mut merged = pi_link::session_ext::read_for(&store, pf_session_key);
        // 090-10 版本层自愈：先重解析已有条目（旧版本目录的路径 → 当前版本，
        // 死条目摘除）再并入新路径。不自愈就会出现「台账同时留着 0.1.4 与
        // 0.2.0 两条」，死的那条让 pi 整个起不来（真机 2026-11-03 重启哑火）
        merged = pi_link::session_ext::resolve_sources(&merged).0;
        if !force_restart
            && pi_exts.iter().all(|p| {
                merged
                    .iter()
                    .any(|h| pi_link::session_ext::same_source(h, p))
            })
        {
            return; // 清单已含，无需重启
        }
        for p in &pi_exts {
            if !merged
                .iter()
                .any(|h| pi_link::session_ext::same_source(h, p))
            {
                merged.push(p.clone());
            }
        }
        if let Err(e) = pi_link::session_ext::write_for(&store, pf_session_key, &merged) {
            eprintln!("[pf-plugin] pi 扩展挂载失败（{pf_session_key}）: {e}");
            return;
        }
        // 005 §③：挂载成功 = 运行许可发放（宿主协议文件）。含 force 重启
        // 链（启用/更新）：ext 无论新旧进程，泵见 on 即可恢复服务。
        crate::pf_plugin::pf_runtime_write(&entry.manifest, true, "enable");
        if cfg!(debug_assertions) {
            eprintln!(
                "[pf-plugin] pi 扩展已入会话清单（{} → {}），空闲重启挂载",
                entry.manifest.id, pf_session_key
            );
        }
        // 空闲会话立即重启挂载（忙 = 有任务在跑，不动，下次 spawn 生效）。
        // **不 push 进 ext_sources**：挂载本体在 spawn 前按台账统一 `-e`
        // （runtime spawn 的 store 消费段，所有档位）；ext_sources 只装用户
        // 勾选的 scheme 源——路径型条目混进去会把胶囊计数和勾选面板搞劈叉
        // （真机 2026-10-31：面板勾 2 项、胶囊显 [3]）
        if let Some(rt) = self.pool.runtimes.get(pf_session_key).cloned() {
            let busy = rt.read(cx).agent_running;
            if !busy {
                rt.update(cx, |r, cx| {
                    r.shutdown_process();
                    r.ensure_spawned(cx);
                });
            } else {
                let msg = crate::i18n::tf(
                    "plugin.ext_wait_idle",
                    &[("name", entry.manifest.id.clone())],
                );
                self.set_status(msg, cx);
            }
        }
    }

    /// 确保当前会话的 pi 进程已起（执行域实例随之激活）。
    /// 幂等：已在跑直接返回；起不来就安静跳过（调用方继续走，不阻塞 UI）。
    pub(crate) fn ensure_active_process(&mut self, cx: &mut gpui::Context<Self>) {
        let Some(rt) = self.pool.runtimes.get(&self.pool.active_key).cloned() else {
            return;
        };
        let _ = rt.update(cx, |r, cx| {
            r.ensure_spawned(cx);
        });
        // 进程起来后实例配置才被消费；顺手同步一次（幂等）
        svc::svc_sync_configs(self);
    }
}

// ---------------------------------------------------------------------------
// 挂载通道的「存活扩展」判定（闲置回收豁免，见 Chat::has_live_service）
// ---------------------------------------------------------------------------

/// 路径归一：台账与清单两侧写法不保证一致（`C:\` / `C:/`、盘符与目录大小写），
/// 字符串直比会把「已挂载」判成未挂载。Windows 文件系统大小写不敏感，全小写安全。
fn norm_mount_path(p: &str) -> String {
    // 92 = 反斜杠码位（写字符字面量在多种工具链转义层里容易被吞，用码位免歧义）
    p.replace(char::from(92), "/").to_lowercase()
}

/// 挂载路径的比较键：抹掉插件**版本层目录**（090-10 市场布局
/// `…/plugins/<pkg>/<ver>/pi/index.mjs`）。台账是绑定当时写的，插件升级后
/// 路径指向旧版本——按原样路径比永远比不中，闲置回收豁免失效 → 绑定会话被
/// 回收，微信侧静默而镜像里绑定指针还在（真机 2026-11-03：重启后收发死）。
/// 同包同文件的换代前后必须视为同一挂载，故比较键不带版本段。
fn mount_key(p: &str) -> String {
    let norm = norm_mount_path(p);
    let Some(i) = norm.rfind("/plugins/") else {
        return norm;
    };
    let head = &norm[..i + "/plugins/".len()];
    let rest = &norm[i + "/plugins/".len()..];
    let mut segs = rest.split('/');
    let pkg = segs.next().unwrap_or("");
    let after: Vec<&str> = segs.collect();
    if after.len() > 1 && after[0].starts_with(|c: char| c.is_ascii_digit()) {
        format!("{head}{pkg}/{}", after[1..].join("/"))
    } else {
        format!("{head}{pkg}/{}", after.join("/"))
    }
}

/// 会话挂载台账 ∩ 启用中插件的扩展路径。纯函数（单测面）：
/// `mounts` = 台账里该会话的条目，`live` = 启用中插件声明的扩展绝对路径。
fn mounts_overlap_live(mounts: &[String], live: &[String]) -> bool {
    let live: Vec<String> = live.iter().map(|p| mount_key(p)).collect();
    mounts
        .iter()
        .map(|m| mount_key(m))
        .any(|m| live.iter().any(|l| *l == m))
}

#[cfg(test)]
mod has_live_service_tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn mount_hit_exempts_session() {
        // 真机形态：清单给反斜杠绝对路径，台账可能落正斜杠小写
        let live = s(&[r"D:\ai_workspace\pi-flash\plugins\pf-wxbot\pi/index.mjs"]);
        let mounts = s(&["d:/ai_workspace/pi-flash/plugins/pf-wxbot/pi/index.mjs"]);
        assert!(
            mounts_overlap_live(&mounts, &live),
            "同一扩展的两种写法必须命中"
        );
    }

    /// 版本层换代（090-10）：台账写的是绑定当时的 `0.1.4` 路径，插件已升到
    /// `0.2.0`。按原样比会落空 → 绑定会话失去闲置回收豁免 → 微信静默。
    #[test]
    fn version_layer_rewrite_still_counts_as_live() {
        let live =
            s(&[r"C:\Users\u\.pi-flash\plugins\@pi-flash-plugins__pf-wxbot\0.2.0\pi\index.mjs"]);
        let mounts =
            s(&["c:/users/u/.pi-flash/plugins/@pi-flash-plugins__pf-wxbot/0.1.4/pi/index.mjs"]);
        assert!(mounts_overlap_live(&mounts, &live), "换代前后是同一挂载");
        // 不同文件仍不命中
        let other =
            s(&[r"C:\Users\u\.pi-flash\plugins\@pi-flash-plugins__pf-wxbot\0.2.0\pi\other.mjs"]);
        assert!(!mounts_overlap_live(&mounts, &other));
    }

    #[test]
    fn unrelated_mounts_do_not_exempt() {
        // 用户勾选的 scheme 源不是插件挂载，进程里没有轮询器，不构成豁免
        let live = s(&["D:/pf/plugins/pf-wxbot/pi/index.mjs"]);
        let mounts = s(&["npm:pi-freeflow", "git:https://github.com/u/r"]);
        assert!(!mounts_overlap_live(&mounts, &live));
    }

    #[test]
    fn other_plugin_mounts_do_not_exempt() {
        // 台账挂着插件 A，存活清单只有插件 B（A 已停用/已卸载）→ 不豁免
        let live = s(&["D:/pf/plugins/pf-wxbot/pi/index.mjs"]);
        let mounts = s(&["D:/pf/plugins/quick-panel/pi/index.mjs"]);
        assert!(!mounts_overlap_live(&mounts, &live));
    }

    #[test]
    fn empty_sides_never_exempt() {
        assert!(!mounts_overlap_live(&[], &[]));
        assert!(!mounts_overlap_live(&s(&["D:/pf/plugins/x/pi/a.mjs"]), &[]));
        assert!(!mounts_overlap_live(&[], &s(&["D:/pf/plugins/x/pi/a.mjs"])));
    }
}
