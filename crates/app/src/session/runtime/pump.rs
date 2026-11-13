//! RPC 事件泵（自由异步函数）：epoch 守卫在此——路由本会话事件进本
//! runtime；泵尾通道关闭 = pi 进程死亡，轮终语义在这里一次补全。

use std::pin::Pin;
use std::task::Poll;

use futures::StreamExt;
use futures::channel::mpsc::UnboundedReceiver;
use futures::future::poll_fn;
use pi_link::protocol::Event;

use crate::i18n::tr;

use super::*;

/// Runtime-local event pump: routes THIS session's events into THIS runtime
/// (stale-guarded by epoch). Chat is not involved — running sessions keep
/// accumulating while parked.
///
/// **批窗口**（2026-11-06 流式卡死修复 A1）：快流模型 100+ delta/s 时每事
/// 件一次 `WeakEntity::update` + notify 会把主线程前台队列塞满——帧重建排
/// 不上队，输入饿死（116 t/s 真机：切面板/打字全冻结）。改为首条事件到达
/// 后等 30ms，把窗口内积压整批一次 update 按序喂 on_event：gpui 在 update
/// 返回后才统一失效重绘，批内 N 条事件只付一次传播成本。事件顺序不变；
/// epoch 守卫按批判定；单事件（权限弹窗等交互事件）多付 ≤30ms 延迟，无感。
pub(super) async fn consume_runtime_events(
    this: gpui::WeakEntity<SessionRuntime>,
    cx: &mut gpui::AsyncApp,
    mut rx: UnboundedReceiver<Event>,
    epoch: u64,
) {
    const BATCH_WINDOW: std::time::Duration = std::time::Duration::from_millis(30);
    // 上限防长轮独占主线程：超限部分留给下一轮（通道里还在，顺序不变）
    const BATCH_CAP: usize = 256;
    loop {
        let first = match rx.next().await {
            Some(ev) => ev,
            None => break, // 通道关闭 → 轮终结算
        };
        let mut batch = vec![first];
        cx.background_executor().timer(BATCH_WINDOW).await;
        // 非阻塞排干窗口：Pending = 已排干；Ready(None) = 通道已关且排干
        // （关态由外层下一轮 next() 兜住，这里当普通排干处理）
        while batch.len() < BATCH_CAP {
            let step = poll_fn(|cx| match Pin::new(&mut rx).poll_next_unpin(cx) {
                Poll::Ready(Some(ev)) => Poll::Ready(Some(ev)),
                _ => Poll::Ready(None),
            })
            .await;
            let Some(ev) = step else {
                break;
            };
            batch.push(ev);
        }
        let t0 = std::time::Instant::now();
        let stale = this
            .update(cx, |rt, cx| {
                if rt.agent.epoch != epoch {
                    return true;
                }
                rt.touch();
                for event in batch {
                    rt.on_event(event, cx);
                }
                false
            })
            .unwrap_or(true);
        if std::env::var("PI_FLASH_PERF").is_ok() {
            eprintln!("[perf] pump 批处理耗时 {:?}", t0.elapsed());
        }
        if stale {
            return;
        }
    }
    let _ = this.update(cx, |rt, cx| {
        if rt.agent.epoch == epoch {
            // 轮中死亡 = 外部错误：补全轮终语义。此前只有一条静默的
            // "pi exited" 状态行——标签栏三色点只认 TurnSettled，永不亮；
            // 半截工具卡永转；无提示音（用户实测：跑着跑着就没了，标签
            // 栏毫无动静）。
            let code = rt.exit_code.take();
            let was_running = rt.agent_running || rt.stream_started.is_some();
            if was_running {
                rt.turn_error = true;
                rt.fail_dangling_tool_calls();
            }
            // 死因留痕：stderr 尾部（node OOM / unhandled rejection /
            // 扩展 fatal 只出现在这里）。最后一行非空 stderr 直接挂进状态条——
            // 此前只 eprintln，用户看到的是永远转圈的会话，不知道 pi 其实在
            // 启动时就 `Error: Model "x/y" not found` 退出了（2026-11-02 真机）
            let stderr = rt
                .agent
                .session
                .as_ref()
                .map(|s| s.stderr_tail())
                .unwrap_or_default();
            let last_line = stderr
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .next_back()
                .map(|l| l.chars().take(160).collect::<String>());
            rt.status = match code {
                Some(c) => crate::i18n::tf("status.pi_exited_code", &[("code", c.to_string())]),
                None => tr("status.pi_exited").to_string(),
            };
            if let Some(l) = last_line {
                rt.status = format!("{} — {l}", rt.status);
            }
            rt.compacting = false;
            // 进程没了就不存在「在跑」：不就地复位，agent_running 与快照
            // is_streaming 会永久卡在 true——psp 状态槽的旋转圈永不灭
            //（用户实测：旧版还在跑的会话被掐掉后一直转）
            rt.agent_running = false;
            rt.phase_waiting = false;
            rt.phase_command = false;
            rt.streaming_content = false;
            // stream_started 闩交给 settle_turn 消费（有音开关才响；用户
            // 已 abort 则闩已清，静音）
            rt.settle_turn();
            if let Some(s) = rt.state.as_mut() {
                s.is_streaming = false;
            }
            if !stderr.trim().is_empty() {
                eprintln!("pi stderr tail:\n{stderr}");
            }
            // 死进程句柄必须摘掉：留着它 select_model 把 SetModel 写进死管道
            //（✓ 永远不动）、ensure_spawned 认为还活着不重 spawn（发送空转）
            //——pi 启动即死时第一次暴露（2026-11-02 真机：models.json 拒载 →
            // `Error: Model not found` 退出，列表点不动、消息转圈）。与
            // shutdown_process 同口径；重开/发送由 ensure_spawned 重起
            rt.agent.session = None;
            // 一次性改名进程死在这儿（启动即崩/OOM）：标记就地清——残留的
            // rename_transient 会在**下一个** set_session_name 回执上误杀
            // 后来重绑的正经进程（pi-flash-n3k8）
            rt.rename_transient = false;
            // 进程死了：文件落过盘就补绑定（真标签接管；重启后可重开），
            // 没落盘（boot 即死 / 全程无消息）撤伪标签
            if !rt.try_bind_reported_file(cx) {
                cx.emit(SessionEvent::PendingTab(None));
            }
            // 轮中死亡按轮终冒泡：订阅方给不在场的会话点红点（turn_error
            // 已置）。空闲死亡（闲置进程被系统清掉）不值得打扰，不冒泡。
            if was_running {
                cx.emit(SessionEvent::TurnSettled);
            }
            // 在途扩展 UI 随进程一起死（zi2s.3）：ext_queue 里的积压请求指向
            // 死管道就地清；在屏弹窗/问卷卡在 Chat.aux（runtime 摸不到），
            // 冒泡给 Chat ext_cancel_all——否则弹窗留着，点击全发进死管道，
            // handler 侧 await 早已无人应答。空闲死亡也要发（mock 崩溃场景
            // handled 后才死，弹窗照旧在屏）。
            rt.ext_queue.clear();
            cx.emit(SessionEvent::ExtUiDied);
            cx.emit(SessionEvent::Changed);
            cx.notify();
        }
    });
}
