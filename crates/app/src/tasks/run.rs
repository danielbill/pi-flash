//! 046-S3 执行链：把认领到的作业变成一次真实运行。
//!
//! - **agent 任务**（§2.3）：每次触发经 session/runtime 新建会话（后台
//!   draft 懒 spawn，不抢前台）——project 用任务所选项目；model/tools/
//!   thinking 覆盖或继承；投递 prompt（task.desc）；run 记 session_path，
//!   turn 终态（`SessionEvent::TurnSettled`，唯一可靠轮终）回写 outcome。
//!   模型 run 内一次性冻结（ZCode fixRunModelSelection 语义）。
//! - **脚本任务**：独立子进程（不经内置终端），cwd = 脚本所在目录，
//!   **30min 超时强杀**；输出尾 + exit code 写 result_json（§1.4 脚本行
//!   620 弹窗消费）。
//! - **立即运行**：[`Chat::task_run_now`]，trigger = manual——manual 派发
//!   不走 claim、不碰周期指针（「manual 不污染 cron 节奏」，ZCode
//!   automationRepo.ts:666-751）；运行中再触发的 schedule 到点由 claim
//!   接力排队（保持到期，结算后补跑，tasks/sched.rs）。
//!
//! 两类执行体都经 oneshot 回报 [`JobOutcome`]，结算统一走 scheduler
//! `spawn_settle` → TasksSource::settle → settle_job（单一路径；只写结果
//! 不碰调度指针——指针在 claim 认领即消费时已前推）。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use pf_context::domain::tasks::{TaskKind, TaskSpec, Trigger};

use gpui::AppContext as _;

use crate::services::scheduler::{self, JobOutcome};
use crate::session::runtime::SessionRuntime;
use crate::{Chat, services};

use super::store::open_store;
use super::tasks_log;

/// 脚本任务超时（§1.3 说明行：超时 30 分钟自动终止）。
const SCRIPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
/// 脚本轮询间隔（try_wait + kill 旗标检查）。
const SCRIPT_POLL: Duration = Duration::from_millis(500);
/// 输出尾保留字符数（§1.4 hover / 620 弹窗消费）。
const TAIL_LIMIT: usize = 4000;

// ---------------------------------------------------------------------------
// 在途上下文（按 runtime key 索引；main 线程独占访问，无锁）
// ---------------------------------------------------------------------------

/// 一次派发的在途上下文：把「会话事件」翻译回「run 台账」的桥。
pub(crate) struct RunCtx {
    pub def_id: String,
    pub run_id: i64,
    pub trigger: Trigger,
    /// 派发时冻结的模型（"provider/id"；None = pi 默认）。
    pub model_frozen: Option<String>,
    /// agent 派发：会话实体（S4 删除中止走 Interrupt；None = script）。
    #[allow(dead_code)] // 046-S4 删除接线
    pub rt: Option<gpui::Entity<SessionRuntime>>,
    /// script 派发：中止旗标（poll 循环看见即提前 kill；S4 删除接线）。
    #[allow(dead_code)] // 046-S4 删除接线
    pub kill: Option<Arc<AtomicBool>>,
    tx: futures::channel::oneshot::Sender<JobOutcome>,
}

impl std::fmt::Debug for RunCtx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RunCtx")
            .field("def_id", &self.def_id)
            .field("run_id", &self.run_id)
            .field("trigger", &self.trigger)
            .field("model_frozen", &self.model_frozen)
            .finish_non_exhaustive()
    }
}

/// 供 mod.rs 的 TasksState.pending 复用的 map 类型。
pub(crate) type PendingMap = HashMap<String, RunCtx>;

/// 按 def id 找在途上下文的 runtime key（S4 删除中止用）。
#[allow(dead_code)] // 046-S4 删除接线
pub(crate) fn pending_key_for(chat: &Chat, def_id: &str) -> Option<String> {
    chat.tasks
        .pending
        .iter()
        .find(|(_, c)| c.def_id == def_id)
        .map(|(k, _)| k.clone())
}

// ---------------------------------------------------------------------------
// 派发入口
// ---------------------------------------------------------------------------

/// 派发一次运行：start_run 落台账（running）→ 按类型起执行体 → oneshot。
/// 起执行体失败（目录/脚本不存在等）直接 Err——scheduler/run_now 的统一
/// 失败路径会结算该 run（running 行在，settle_job 收口；周期已由 claim
/// 认领即消费前推，此处不再管调度）。
pub(crate) fn dispatch(
    chat: &mut Chat,
    def_id: &str,
    trigger: Trigger,
    cx: &mut gpui::Context<Chat>,
) -> Result<scheduler::JobRx, String> {
    let store = open_store()?;
    let def = store
        .get_def(def_id)?
        .ok_or_else(|| format!("任务不存在: {def_id}"))?;
    let spec = TaskSpec::from_json(&def.spec_json)?;
    let run_id = store.start_run(def_id, trigger, scheduler::now_ms(), None)?;
    let (tx, rx) = futures::channel::oneshot::channel();
    let spawned = match &spec.kind {
        TaskKind::Agent => spawn_agent_turn(chat, def_id, run_id, trigger, &spec, tx, cx),
        TaskKind::Script => spawn_script_run(&spec, tx),
    };
    match spawned {
        Ok(()) => {
            tasks_log(&format!(
                "派发: task={def_id} trigger={} title={}",
                trigger.as_str(),
                def.title
            ));
            Ok(rx)
        }
        Err(e) => {
            // run 行已落（running）——执行体没起来，先标记派发失败再交回
            // 统一失败路径（settle_job 找不到 running 行 → 只重排周期）
            let _ = store.settle_run(
                run_id,
                pf_context::domain::tasks::Outcome::Failed,
                scheduler::now_ms(),
                Some(&error_json(&format!("派发失败: {}", e.replace('"', "'")))),
            );
            Err(e)
        }
    }
}

// ---------------------------------------------------------------------------
// agent 任务：新建会话 + 投递 prompt
// ---------------------------------------------------------------------------

fn spawn_agent_turn(
    chat: &mut Chat,
    def_id: &str,
    run_id: i64,
    trigger: Trigger,
    spec: &TaskSpec,
    tx: futures::channel::oneshot::Sender<JobOutcome>,
    cx: &mut gpui::Context<Chat>,
) -> Result<(), String> {
    let a = spec
        .agent
        .as_ref()
        .ok_or_else(|| "agent 任务缺少 agent 字段".to_string())?;
    let cwd = PathBuf::from(&a.project);
    if !cwd.is_dir() {
        return Err(format!("项目目录不存在: {}", a.project));
    }

    // 模型冻结：派发时刻解析（任务覆盖 > 新会话默认 > pi 默认）——ZCode
    // fixRunModelSelection「回看不重读默认」
    let frozen_pair = a
        .model
        .clone()
        .or_else(|| services::workspace::default_model_pref());

    // 后台铸 draft（不 switch_to 不抢前台）。task_owned 防两处误伤：
    // 新会话复用逻辑捡走任务 draft；FileBound 清掉用户同 cwd 草稿
    let key = format!("draft-{}", chat.pool.draft_seq);
    chat.pool.draft_seq += 1;
    // 扩展清单先于 runtime 定稿（含台账 draft 键对齐，见 dispatch_ext_sources）
    let ext_sources = dispatch_ext_sources(a.extensions.as_deref(), &key);
    let rt = cx.new(|_| {
        let mut r = SessionRuntime::new(key.clone(), cwd.clone(), None);
        r.task_owned = true;
        // 会话 type（2026-11-22 拍板）：spawn env → session_type.ts 扩展落
        // pf/session-type entry 进会话文件——任务会话不进用户会话口径的
        // 事实源。置位点唯一（SessionRuntime.session_type 注释）。
        r.session_type = Some("task");
        if let Some((p, m)) = &frozen_pair {
            r.pending_model = Some((p.clone(), m.clone()));
        }
        // thinking：auto = 继承 pi 默认（不落 override）；其余档 spawn 带
        // --thinking（§1.3 与 PillMenu::Thinking 同清单）
        if a.thinking != "auto" {
            r.thinking_override = Some(a.thinking.clone());
        }
        // tools 档位：spawn 期 CLI flags 生效（034 档位键）。full+（custom）
        // 档的扩展集 = 任务 spec 的 extensions（每任务配方，2026-11-23；
        // 此前「沿用会话级配方」= fresh runtime 回落全局启用集，任务页勾的
        // 清单从来到不了这里——beads pi-flash-jt44）
        r.tools_preset = a.tools.clone();
        r.ext_sources = ext_sources;
        r.status = "任务运行中".into();
        r
    });
    chat.pool.runtimes.insert(key.clone(), rt.clone());
    chat.subscribe_runtime(&rt, cx);
    pi_link::projects::register_project(&cwd);

    // 模型冻结写台账 + 在途上下文（TurnSettled 钩子消费）
    let model_str = frozen_pair.as_ref().map(|(p, m)| format!("{p}/{m}"));
    let _ = open_store()?.bind_run(run_id, None, model_str.as_deref());
    chat.tasks.pending.insert(
        key,
        RunCtx {
            def_id: def_id.to_string(),
            run_id,
            trigger,
            model_frozen: model_str,
            rt: Some(rt.clone()),
            kill: None,
            tx,
        },
    );

    // 投递 prompt（懒 spawn：首个 send 起进程；file 随首个 prompt 落盘后
    // FileBound 事件回填 session_path）
    rt.update(cx, |r, cx| {
        r.input = a.prompt.clone();
        r.send_input(cx);
    });
    Ok(())
}

/// 派发入口：按任务 spec 定稿本次运行的扩展清单（full+ 档 `-e` 源）。
fn dispatch_ext_sources(extensions: Option<&[String]>, key: &str) -> Vec<String> {
    dispatch_ext_sources_in(
        extensions,
        key,
        pi_link::session_ext::store_path().as_deref(),
        pi_link::skills::enabled_package_sources,
    )
}

/// 派发时刻定稿任务的扩展清单，返回值落 `SessionRuntime::ext_sources`：
///
/// - `Some(list)` = 任务专属配方：原样用，并写入台账 draft 键——会话绑
///   文件后 `on_file_bound` 把条目迁到会话文件键，任务会话日后再打开仍按
///   当时配方挂载（否则回落全局集，与 jt44 同类的「配置不跟身」）；
/// - `None` = 沿用全局启用集（`inherit`），与新会话创建同一条回落。
///
/// 两种情况都**清掉 draft 键上的残留**：fresh runtime 的 key 是 `draft-N`，
/// 序号跨实例复用，台账里可能留着上一实例同名草稿的自选清单——
/// `SessionRuntime::new` 会把它当本任务的初始清单读进来（任务扩展集
/// 串味）。键是本实例刚铸的，名下不可能有本实例写的合法挂载条目，整条
/// 覆写/删除即可。空表落台账 = 无条目（台账表达的固有限制）：运行期以
/// 返回值为准（一个扩展都不挂）。
///
/// 纯内核（`store` / `inherit` 显式注入，便于断言）。
fn dispatch_ext_sources_in(
    extensions: Option<&[String]>,
    key: &str,
    store: Option<&Path>,
    inherit: impl FnOnce() -> Vec<String>,
) -> Vec<String> {
    match extensions {
        Some(list) => {
            if let Some(store) = store {
                let _ = pi_link::session_ext::write_for(store, key, list);
            }
            list.to_vec()
        }
        None => {
            if let Some(store) = store {
                let _ = pi_link::session_ext::remove_for(store, key);
            }
            inherit()
        }
    }
}

// ---------------------------------------------------------------------------
// 脚本任务：子进程 + 30min 超时强杀
// ---------------------------------------------------------------------------

fn spawn_script_run(
    spec: &TaskSpec,
    tx: futures::channel::oneshot::Sender<JobOutcome>,
) -> Result<(), String> {
    let sc = spec
        .script
        .as_ref()
        .ok_or_else(|| "script 任务缺少 script 字段".to_string())?;
    let path = PathBuf::from(&sc.path);
    if !path.is_file() {
        return Err(format!("脚本文件不存在: {}", sc.path));
    }
    // 配方解析（扩展名支持集/探测链与文件树【执行】同源）；ws_cwd 传 None
    // ——脚本任务的 py 解释器探测不走「当前项目 venv」（运行目录=脚本目录）
    let (program, args) = services::script_exec::spawn_plan(&path, None).ok_or_else(|| {
        format!(
            "不支持的脚本类型: {}",
            path.extension().unwrap_or_default().to_string_lossy()
        )
    })?;
    let cwd = path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "脚本路径无父目录".to_string())?;
    let kill = Arc::new(AtomicBool::new(false));

    std::thread::spawn(move || {
        let outcome = run_script_block(&program, &args, &cwd, &kill, SCRIPT_TIMEOUT);
        let _ = tx.send(outcome);
    });
    Ok(())
}

/// 阻塞跑脚本（独立线程）：输出双管道并发抽干（防 pipe 满死锁）+
/// 轮询 try_wait，超时/中止即 kill。exit code + 输出尾写 result_json。
fn run_script_block(
    program: &Path,
    args: &[String],
    cwd: &Path,
    kill: &AtomicBool,
    timeout: Duration,
) -> JobOutcome {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .stdin(std::process::Stdio::null());
    // GUI 宿主裸 spawn 控制台程序会闪黑窗（services/git.rs 同款治理）
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return JobOutcome::failed("failed", Some(error_json(&format!("起进程失败: {e}"))));
        }
    };

    // 双 reader：从 spawn 起抽干管道，kill/退出后管道闭合自然 EOF
    let mut pipes: Vec<Box<dyn std::io::Read + Send>> = Vec::new();
    if let Some(p) = child.stdout.take() {
        pipes.push(Box::new(p));
    }
    if let Some(p) = child.stderr.take() {
        pipes.push(Box::new(p));
    }
    let readers: Vec<_> = pipes
        .into_iter()
        .map(|mut pipe| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = std::io::Read::read_to_end(&mut pipe, &mut buf);
                buf
            })
        })
        .collect();

    let started = Instant::now();
    let mut timed_out = false;
    let mut exit_status: Option<ExitStatus> = None;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                exit_status = Some(status);
                break;
            }
            Ok(None) => {}
            Err(e) => {
                return JobOutcome::failed("failed", Some(error_json(&format!("wait 失败: {e}"))));
            }
        }
        if started.elapsed() >= timeout || kill.load(Ordering::Relaxed) {
            let aborted = kill.load(Ordering::Relaxed);
            let _ = child.kill();
            let _ = child.wait();
            if aborted {
                return JobOutcome::failed("failed", Some(error_json("运行被中止")));
            }
            timed_out = true;
            break;
        }
        std::thread::sleep(SCRIPT_POLL);
    }

    let mut out = Vec::new();
    for h in readers {
        if let Ok(part) = h.join() {
            out.extend_from_slice(&part);
        }
    }
    let tail: String = String::from_utf8_lossy(&out)
        .chars()
        .rev()
        .take(TAIL_LIMIT)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let result = serde_json::json!({
        "exit": exit_status.and_then(|s| s.code()),
        "timeout": timed_out,
        "tail": tail.trim_end(),
    })
    .to_string();
    if timed_out {
        JobOutcome::failed("timeout", Some(result))
    } else if exit_status.and_then(|s| s.code()) == Some(0) {
        JobOutcome::ok(Some(result))
    } else {
        JobOutcome::failed("failed", Some(result))
    }
}

fn error_json(msg: &str) -> String {
    serde_json::json!({ "error": msg }).to_string()
}

// ---------------------------------------------------------------------------
// 会话事件钩子（pool/runtime_link.rs / session_ops.rs 调用）
// ---------------------------------------------------------------------------

/// `SessionEvent::TurnSettled`（唯一可靠轮终）：在途 agent 上下文 →
/// session_path 回填 + outcome 经 oneshot 送回统一结算路径。
/// 非任务会话（pending 无条目）零成本直接返回。
pub(crate) fn on_turn_settled(
    chat: &mut Chat,
    rt: &gpui::Entity<SessionRuntime>,
    cx: &mut gpui::Context<Chat>,
) {
    let (key, turn_error, turn_interrupted, file) = {
        let r = rt.read(cx);
        (
            r.key.clone(),
            r.turn_error,
            r.turn_interrupted,
            r.file.clone(),
        )
    };
    let Some(ctx) = chat.tasks.pending.remove(&key) else {
        return;
    };
    if let Some(f) = &file {
        if let Ok(store) = open_store() {
            let _ = store.bind_run(ctx.run_id, Some(&f.to_string_lossy()), None);
        }
        // 任务会话不进用户会话口径（2026-11-22 拍板）：事实源 = 会话文件里的
        // pf/session-type custom entry（spawn env + session_type.ts 扩展落盘，
        // 见 spawn_agent_turn）；session_names 前缀方案已废弃——显示层过滤
        // 统一走 SessionInfo.kind。
    }
    // 结局口径与三色点同源（turn_outcome_tests.rs）：error → failed、
    // length/aborted → failed（黄档语义在台账里归 failed 带原因；成功行
    // 的模型已有独立列，result_json 保持干净）
    let bad = turn_error || turn_interrupted;
    let outcome = if bad {
        let reason = if turn_error {
            "turn error（模型/接口层错误）"
        } else {
            "输出中断（length/aborted）"
        };
        JobOutcome::failed("failed", Some(error_json(reason)))
    } else {
        JobOutcome::ok(None)
    };
    tasks_log(&format!(
        "turn 终态: task={} run={} outcome={} session={}",
        ctx.def_id,
        ctx.run_id,
        outcome.kind,
        file.map(|f| f.to_string_lossy().to_string())
            .unwrap_or_default()
    ));
    let _ = ctx.tx.send(outcome);
}

/// 任务会话在跑被「点名开观察窗」：驱动 runtime 让出会话路径 key（观察
/// 窗 identity = 会话路径，见 open_subagent_session_window）——进程照跑
/// （abort 会杀运行中任务），池 key 换成 `task:{path}` 继续驱动，在途账
/// 随 key 迁移（TurnSettled 按 runtime key 查账）。结算后驱动体留在池里
/// 按新 key 静置（session/retire.rs 任务窗 30s 到期即退役——结算后内存
/// 无消费者，回看走 observer 磁盘重建），不再与任何路径键相撞。
pub(crate) fn task_driver_rekey(
    chat: &mut Chat,
    rt: &gpui::Entity<SessionRuntime>,
    path_key: &str,
    cx: &mut gpui::Context<Chat>,
) {
    let new_key = format!("task:{path_key}");
    if let Some(entity) = chat.pool.runtimes.remove(path_key) {
        chat.pool.runtimes.insert(new_key.clone(), entity);
    }
    rt.update(cx, |r, _| r.key = new_key.clone());
    if let Some(mut ctx) = chat.tasks.pending.remove(path_key) {
        ctx.rt = Some(rt.clone());
        chat.tasks.pending.insert(new_key.clone(), ctx);
    }
    tasks_log(&format!(
        "任务会话让出路径 key 给观察窗: {path_key} -> {new_key}"
    ));
}

/// `Chat::on_file_bound`（draft 晋级）：任务会话绑到真实文件 → 迁移在途
/// 上下文的 key + session_path **即知即写**（2026-11-22 用户报障：运行中
/// 日志行点击开不了会话——此前只在 TurnSettled 回填，运行中 run 行
/// path 为空、行名不可点）。TurnSettled 还会再绑一次（幂等，兜住无
/// FileBound 的路径）。
pub(crate) fn on_session_bound(
    chat: &mut Chat,
    old_key: &str,
    path: &Path,
    cx: &mut gpui::Context<Chat>,
) {
    let Some(mut ctx) = chat.tasks.pending.remove(old_key) else {
        return;
    };
    let new_key = path.to_string_lossy().to_string();
    ctx.rt = chat.pool.runtimes.get(&new_key).cloned();
    if let Ok(store) = open_store() {
        let _ = store.bind_run(ctx.run_id, Some(&new_key), None);
    }
    chat.tasks.pending.insert(new_key, ctx);
    // 台账缓存即刻对齐（≤20 行 µs 级）：日志行名当场可点，不等泵 tick
    chat.tasks_refresh();
    cx.notify();
}

// ---------------------------------------------------------------------------
// 单测（脚本 runner 矩阵；agent 链走 S5 pif-ui 验收）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn write_script(dir: &Path, name: &str, content: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        p
    }

    #[test]
    fn bat_exit_zero_is_ok_with_tail() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_script(dir.path(), "ok.bat", "@echo off\r\necho pf-task-ok\r\n");
        let (program, args) = services::script_exec::spawn_plan(&p, None).unwrap();
        let outcome = run_script_block(
            &program,
            &args,
            dir.path(),
            &AtomicBool::new(false),
            SCRIPT_TIMEOUT,
        );
        assert!(outcome.success, "{outcome:?}");
        let detail = outcome.detail.unwrap();
        assert!(detail.contains("\"exit\":0"), "{detail}");
        assert!(detail.contains("pf-task-ok"), "{detail}");
        assert!(detail.contains("\"timeout\":false"), "{detail}");
    }

    #[test]
    fn bat_nonzero_exit_is_failed_with_code() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_script(dir.path(), "bad.bat", "@echo off\r\nexit /b 3\r\n");
        let (program, args) = services::script_exec::spawn_plan(&p, None).unwrap();
        let outcome = run_script_block(
            &program,
            &args,
            dir.path(),
            &AtomicBool::new(false),
            SCRIPT_TIMEOUT,
        );
        assert_eq!(outcome.kind, "failed");
        assert!(outcome.detail.unwrap().contains("\"exit\":3"));
    }

    #[test]
    fn script_timeout_kills_and_reports() {
        let dir = tempfile::tempdir().unwrap();
        // ping -n 挂住 ~10s；700ms 超时即强杀
        let p = write_script(
            dir.path(),
            "slow.bat",
            "@echo off\r\nping -n 10 127.0.0.1 >nul\r\n",
        );
        let (program, args) = services::script_exec::spawn_plan(&p, None).unwrap();
        let outcome = run_script_block(
            &program,
            &args,
            dir.path(),
            &AtomicBool::new(false),
            Duration::from_millis(700),
        );
        assert_eq!(outcome.kind, "timeout");
        assert!(outcome.detail.unwrap().contains("\"timeout\":true"));
    }

    #[test]
    fn kill_flag_aborts_before_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let p = write_script(
            dir.path(),
            "slow2.bat",
            "@echo off\r\nping -n 10 127.0.0.1 >nul\r\n",
        );
        let (program, args) = services::script_exec::spawn_plan(&p, None).unwrap();
        let kill = AtomicBool::new(true);
        let outcome = run_script_block(&program, &args, dir.path(), &kill, SCRIPT_TIMEOUT);
        assert_eq!(outcome.kind, "failed");
        assert!(outcome.detail.unwrap().contains("中止"));
    }

    #[test]
    fn unsupported_ext_has_no_plan() {
        assert!(services::script_exec::spawn_plan(Path::new("readme.txt"), None).is_none());
        assert!(services::script_exec::spawn_plan(Path::new("noext"), None).is_none());
    }

    /// 派发扩展清单：任务专属配方原样返回 + 落台账 draft 键；None = 沿用
    /// 全局启用集。两种情况都清掉 draft 键残留——上一实例同名草稿的自选
    /// 清单不得串进本任务（pi-flash-jt44）
    #[test]
    fn dispatch_ext_sources_pins_task_list_and_clears_stale_draft() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("session-ext.json");
        let key = "draft-3";
        let inherit = || vec!["npm:global".to_string()];

        // 上一实例留下的同名草稿自选清单（残留）
        pi_link::session_ext::write_for(&store, key, &["npm:stale".to_string()]).unwrap();
        assert_eq!(
            dispatch_ext_sources_in(None, key, Some(&store), inherit),
            vec!["npm:global".to_string()],
            "None = 全局启用集，不读残留"
        );
        assert!(
            pi_link::session_ext::read_for(&store, key).is_empty(),
            "残留已清"
        );

        // 任务专属配方：原样 + 落台账（bind 时随身份迁到会话文件键）
        let list = vec!["npm:pi-web-access".to_string()];
        assert_eq!(
            dispatch_ext_sources_in(Some(&list), key, Some(&store), inherit),
            list
        );
        assert_eq!(pi_link::session_ext::read_for(&store, key), list);

        // 残留再落一次：任务配方整条覆写，不与残留合并
        pi_link::session_ext::write_for(&store, key, &["npm:stale".to_string()]).unwrap();
        dispatch_ext_sources_in(Some(&list), key, Some(&store), inherit);
        assert_eq!(pi_link::session_ext::read_for(&store, key), list);

        // 空表 = 一个扩展都不挂（返回值为准；台账无条目是台账的固有表达）
        assert!(dispatch_ext_sources_in(Some(&[]), key, Some(&store), inherit).is_empty());
        assert!(pi_link::session_ext::read_for(&store, key).is_empty());

        // 无台账（数据目录不可用）：清单仍以任务 spec / 全局集为准
        assert_eq!(
            dispatch_ext_sources_in(Some(&list), key, None, inherit),
            list
        );
        assert_eq!(
            dispatch_ext_sources_in(None, key, None, inherit),
            vec!["npm:global".to_string()]
        );
    }
}
