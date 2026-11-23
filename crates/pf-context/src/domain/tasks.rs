//! 046 自动任务持久层（domain；001-3 S5 落种子，046-S1 扩列定稿）。
//!
//! `task_defs` = 任务定义 + 调度指针（enabled / next_run_at / anchor_at），
//! `task_runs` = 运行台账（trigger / outcome / session_path）。存储层只懂数据：
//!
//! - spec_json 反序列化即校验（[`TaskSpec`]，非法值拒写——写库前拒绝）；
//! - 创建事务内 COUNT ≤ [`TASK_CREATE_LIMIT`]；删除同事务连带删 runs；
//! - prune 每任务保最近 [`RUN_KEEP_LIMIT`] 条（046-0：必须真挂泵，不留死 API）。
//!
//! 周期计算（next_run）与触发判定（misfire / in-flight skip）归 app 侧 046
//! 模块（crates/app/src/tasks/，046-S2）；存储层不关心时间语义，所有时间戳
//! （Unix ms）由调用方传入，测试可注入。

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::DbSource;

/// 任务定义总数上限（enabled/disabled 全部状态计入；ZCode
/// AUTOMATION_CREATE_LIMIT 同值同语义）。
pub const TASK_CREATE_LIMIT: usize = 20;

/// 每任务运行记录保留条数（[`TaskStore::prune_runs`] 调用方传此档）。
pub const RUN_KEEP_LIMIT: usize = 50;

/// 工具权限五档键（034 档位复用）。与 app `session/menu_rows.rs`
/// `tools_preset_rows` 同源——改清单必须两处同步。
pub const TOOLS_PRESETS: [&str; 5] = ["chat-only", "read-only", "default", "full", "custom"];

/// 思考档位键（auto = 继承 pi 默认；off..max 与 pi `ThinkingLevel` 七档一一
/// 对应）。与 app `session/menu_rows.rs` `thinking_rows` 同源——改清单必须
/// 两处同步。
pub const THINKING_LEVELS: [&str; 8] = [
    "auto", "off", "minimal", "low", "medium", "high", "xhigh", "max",
];

/// 触发来源（`task_runs.trigger`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    /// 定时命中（调度泵）。
    Schedule,
    /// ⋯菜单「立即运行」。
    Manual,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Schedule => "schedule",
            Trigger::Manual => "manual",
        }
    }

    fn from_db(s: &str) -> Result<Self, String> {
        match s {
            "schedule" => Ok(Trigger::Schedule),
            "manual" => Ok(Trigger::Manual),
            other => Err(format!("未知 trigger: {other}")),
        }
    }
}

/// 运行结果（`task_runs.outcome`；§1.4 四态 + running spinner）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 运行中（spinner；finished_at = NULL）。
    Running,
    /// 正常完成（通知点青蓝）。
    Ok,
    /// 失败（danger 红；原因摘要进 result_json）。
    Failed,
    /// 超时（30 分钟强杀，WARN 黄）。
    Timeout,
    /// 跳过（misfire / 运行中再触发；text_faint）。
    Skipped,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Running => "running",
            Outcome::Ok => "ok",
            Outcome::Failed => "failed",
            Outcome::Timeout => "timeout",
            Outcome::Skipped => "skipped",
        }
    }

    fn from_db(s: &str) -> Result<Self, String> {
        match s {
            "running" => Ok(Outcome::Running),
            "ok" => Ok(Outcome::Ok),
            "failed" => Ok(Outcome::Failed),
            "timeout" => Ok(Outcome::Timeout),
            "skipped" => Ok(Outcome::Skipped),
            other => Err(format!("未知 outcome: {other}")),
        }
    }
}

/// 任务类型（spec_json.kind）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// 子进程跑脚本文件（services/script_exec）。
    Script,
    /// 每次触发新建 agent 会话投递 prompt。
    Agent,
}

/// 周期档位（spec_json.schedule.unit；五档，§1.3 运行周期下拉同清单）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleUnit {
    /// 每 interval 分钟（从 anchor 推进，不对齐墙钟刻度）。
    Minute,
    /// 每小时，对齐 minute 分。
    Hourly,
    /// 每天 hour:minute。
    Daily,
    /// 每周 weekdays（1–7，周一=1）的 hour:minute。
    Weekly,
    /// 每月 month_day 日的 hour:minute（溢出日如 2 月 31 日自然跳过）。
    Monthly,
}

/// 周期规则（spec_json.schedule；字段适用性随 unit，见 [`ScheduleUnit`]）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSchedule {
    pub unit: ScheduleUnit,
    /// 每 N 分钟（minute 档用；其余档恒 1）。
    #[serde(default = "default_interval")]
    pub interval: u32,
    /// 时（0–23；daily/weekly/monthly）。
    #[serde(default)]
    pub hour: u8,
    /// 分（0–59；hourly/daily/weekly/monthly）。
    #[serde(default)]
    pub minute: u8,
    /// 周几（1–7，周一=1；weekly 必填非空）。
    #[serde(default)]
    pub weekdays: Vec<u8>,
    /// 每月几号（1–31；monthly 必填）。
    #[serde(default)]
    pub month_day: u8,
}

fn default_interval() -> u32 {
    1
}

/// Agent 任务业务字段（spec_json.agent）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentSpec {
    /// 投递给新会话的任务描述（= UI「任务描述」，§1.3）。
    pub prompt: String,
    /// 会话所在项目路径（每次触发新建会话用）。
    pub project: String,
    /// 工具权限档位键（[`TOOLS_PRESETS`] 之一）。
    pub tools: String,
    /// 模型覆盖 (provider, id)；None = 继承新会话默认（§1.3「继承新会话
    /// 默认」）。派发时一次性冻结进 run 行（046-S3）。
    #[serde(default)]
    pub model: Option<(String, String)>,
    /// 思考强度键（[`THINKING_LEVELS`] 之一）。
    pub thinking: String,
    /// full+ 档（`tools = custom`）的任务专属扩展清单（`entry_source()` 源串
    /// = spawn 的 `-e` 参数）。`None` = 沿用全局启用集（存量 spec 与没动过
    /// 扩展菜单的任务）；`Some` = 每任务配方，空表合法（一个扩展都不挂）。
    #[serde(default)]
    pub extensions: Option<Vec<String>>,
}

/// 脚本任务业务字段（spec_json.script）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScriptSpec {
    /// 脚本文件绝对路径（运行目录 = 脚本所在目录，§1.3 说明行）。
    pub path: String,
}

/// 任务定义 JSON 的类型视图（spec_json；反序列化即校验，非法值拒写）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskSpec {
    pub kind: TaskKind,
    pub schedule: TaskSchedule,
    #[serde(default)]
    pub agent: Option<AgentSpec>,
    #[serde(default)]
    pub script: Option<ScriptSpec>,
}

impl TaskSpec {
    /// 解析 + 校验一体入口（存储层写前调用；app 侧读回也走这里）。
    pub fn from_json(json: &str) -> Result<TaskSpec, String> {
        let spec: TaskSpec =
            serde_json::from_str(json).map_err(|e| format!("spec_json 解析失败: {e}"))?;
        spec.validate()?;
        Ok(spec)
    }

    /// 领域校验（对齐 ZCode automationCronValidation「写库前拒绝」精神）。
    pub fn validate(&self) -> Result<(), String> {
        let s = &self.schedule;
        if s.interval == 0 {
            return Err("interval 必须是正整数".into());
        }
        if s.hour > 23 {
            return Err("hour 必须是 0-23 的整数".into());
        }
        if s.minute > 59 {
            return Err("minute 必须是 0-59 的整数".into());
        }
        if s.weekdays.iter().any(|w| !(1..=7).contains(w)) {
            return Err("weekdays 必须是 1-7（周一=1）的整数".into());
        }
        if s.unit == ScheduleUnit::Weekly && s.weekdays.is_empty() {
            return Err("weekly 规则必须包含有效的 weekdays".into());
        }
        if s.unit == ScheduleUnit::Monthly && !(1..=31).contains(&s.month_day) {
            return Err("monthly 规则必须是 1-31 的运行日".into());
        }
        match self.kind {
            TaskKind::Agent => {
                if self.script.is_some() {
                    return Err("agent 任务不应携带 script 字段".into());
                }
                let a = self
                    .agent
                    .as_ref()
                    .ok_or_else(|| "agent 任务缺少 agent 字段".to_string())?;
                if a.prompt.trim().is_empty() {
                    return Err("agent 任务 prompt 不能为空".into());
                }
                if a.project.trim().is_empty() {
                    return Err("agent 任务 project 不能为空".into());
                }
                if !TOOLS_PRESETS.contains(&a.tools.as_str()) {
                    return Err(format!(
                        "tools 档位非法: {}（合法: {}）",
                        a.tools,
                        TOOLS_PRESETS.join("/")
                    ));
                }
                if !THINKING_LEVELS.contains(&a.thinking.as_str()) {
                    return Err(format!(
                        "thinking 档位非法: {}（合法: {}）",
                        a.thinking,
                        THINKING_LEVELS.join("/")
                    ));
                }
                if a.extensions
                    .as_ref()
                    .is_some_and(|l| l.iter().any(|s| s.trim().is_empty()))
                {
                    return Err("extensions 条目不能为空串".into());
                }
            }
            TaskKind::Script => {
                if self.agent.is_some() {
                    return Err("script 任务不应携带 agent 字段".into());
                }
                let sc = self
                    .script
                    .as_ref()
                    .ok_or_else(|| "script 任务缺少 script 字段".to_string())?;
                if sc.path.trim().is_empty() {
                    return Err("script 任务 path 不能为空".into());
                }
            }
        }
        Ok(())
    }
}

/// 任务定义（task_defs 一行）。
#[derive(Debug, Clone, PartialEq)]
pub struct TaskDef {
    pub id: String,
    pub title: String,
    /// 已通过 [`TaskSpec::from_json`] 校验的定义 JSON（透明存取）。
    pub spec_json: String,
    /// 启用开关（停用 = 不参与 claim，next_run_at 保留原值）。
    pub enabled: bool,
    /// 下次调度时刻（Unix ms；None = 未排期/停用后未重排）。
    pub next_run_at: Option<i64>,
    /// 周期锚点（防漂移：minute 档从锚点推进 k·interval，046-S2 用）。
    pub anchor_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 一次运行记录（task_runs 一行；耗时由 started/finished 现算不落列）。
#[derive(Debug, Clone, PartialEq)]
pub struct TaskRun {
    pub id: i64,
    pub def_id: String,
    pub trigger: Trigger,
    pub started_at: i64,
    /// `None` = 运行中（outcome = running）。
    pub finished_at: Option<i64>,
    pub outcome: Outcome,
    /// agent 任务跳转会话用（脚本任务 = None）。
    pub session_path: Option<String>,
    /// 派发时一次性冻结的模型（"provider/id"；None = pi 默认，046-S3）。
    pub model: Option<String>,
    /// 结果载荷：失败原因摘要 / 脚本输出尾+exit code（§1.4 hover 消费）。
    pub result_json: Option<String>,
}

/// 任务存储（046 唯一持久层门面，挂 [`DbSource`]）。
pub struct TaskStore<'a> {
    db: &'a DbSource,
}

impl<'a> TaskStore<'a> {
    pub fn new(db: &'a DbSource) -> Self {
        Self { db }
    }

    /// 校验任务定义（title 非空 + spec_json 反序列化即校验）。
    fn validate_def(def: &TaskDef) -> Result<(), String> {
        if def.title.trim().is_empty() {
            return Err("任务名称不能为空".into());
        }
        TaskSpec::from_json(&def.spec_json).map(|_| ())
    }

    /// 创建任务：事务内 COUNT ≤ [`TASK_CREATE_LIMIT`]（所有状态计入），
    /// spec 校验非法拒写。id 由调用方生成（重复 = PK 冲突 Err）。
    pub fn create(&self, def: &TaskDef) -> Result<(), String> {
        Self::validate_def(def)?;
        self.db.with_tx(|tx| {
            let n: i64 = tx
                .query_row("SELECT COUNT(*) FROM task_defs", [], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            if n >= TASK_CREATE_LIMIT as i64 {
                return Err(format!("任务数已达上限 {TASK_CREATE_LIMIT}，请先删除任务"));
            }
            tx.execute(
                "INSERT INTO task_defs
                     (id, title, spec_json, enabled, next_run_at, anchor_at, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    def.id,
                    def.title,
                    def.spec_json,
                    def.enabled as i64,
                    def.next_run_at,
                    def.anchor_at,
                    def.created_at,
                    def.updated_at
                ],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        })
    }

    /// 整行更新任务定义（created_at 不动；编辑周期时的重锚由调用方改
    /// next_run_at/anchor_at 后一并传入——单条 UPDATE 原子生效）。
    pub fn update_def(&self, def: &TaskDef) -> Result<(), String> {
        Self::validate_def(def)?;
        self.db.with_tx(|tx| {
            let n = tx
                .execute(
                    "UPDATE task_defs
                     SET title = ?2, spec_json = ?3, enabled = ?4,
                         next_run_at = ?5, anchor_at = ?6, updated_at = ?7
                     WHERE id = ?1",
                    params![
                        def.id,
                        def.title,
                        def.spec_json,
                        def.enabled as i64,
                        def.next_run_at,
                        def.anchor_at,
                        def.updated_at
                    ],
                )
                .map_err(|e| e.to_string())?;
            (n == 1)
                .then_some(())
                .ok_or_else(|| format!("任务不存在: {}", def.id))
        })
    }

    /// 全部任务定义（updated_at 新在前）。
    pub fn list_defs(&self) -> Result<Vec<TaskDef>, String> {
        self.db.with_conn(|c| {
            let mut stmt = c
                .prepare("SELECT id, title, spec_json, enabled, next_run_at, anchor_at, created_at, updated_at
                          FROM task_defs ORDER BY updated_at DESC, id")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], Self::row_to_def)
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
    }

    /// 启用中的任务（调度 claim 扫描源）。
    pub fn list_enabled(&self) -> Result<Vec<TaskDef>, String> {
        self.db.with_conn(|c| {
            let mut stmt = c
                .prepare("SELECT id, title, spec_json, enabled, next_run_at, anchor_at, created_at, updated_at
                          FROM task_defs WHERE enabled = 1 ORDER BY id")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], Self::row_to_def)
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
    }

    /// 单个任务定义（编辑表单预填）。
    pub fn get_def(&self, id: &str) -> Result<Option<TaskDef>, String> {
        self.db.with_conn(|c| {
            let mut stmt = c
                .prepare("SELECT id, title, spec_json, enabled, next_run_at, anchor_at, created_at, updated_at
                          FROM task_defs WHERE id = ?1")
                .map_err(|e| e.to_string())?;
            let mut rows = stmt
                .query_map([id], Self::row_to_def)
                .map_err(|e| e.to_string())?;
            match rows.next() {
                Some(row) => row.map(Some).map_err(|e| e.to_string()),
                None => Ok(None),
            }
        })
    }

    /// 启用⇄停用（不动 next_run_at/updated_at——重排归 reschedule）。
    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        self.db.with_tx(|tx| {
            let n = tx
                .execute(
                    "UPDATE task_defs SET enabled = ?2 WHERE id = ?1",
                    params![id, enabled as i64],
                )
                .map_err(|e| e.to_string())?;
            (n == 1)
                .then_some(())
                .ok_or_else(|| format!("任务不存在: {id}"))
        })
    }

    /// 重排调度指针（结算后重算 next_run / 编辑周期后重锚 anchor 都走这）。
    pub fn reschedule(
        &self,
        id: &str,
        next_run_at: Option<i64>,
        anchor_at: Option<i64>,
    ) -> Result<(), String> {
        self.db.with_tx(|tx| {
            let n = tx
                .execute(
                    "UPDATE task_defs SET next_run_at = ?2, anchor_at = ?3 WHERE id = ?1",
                    params![id, next_run_at, anchor_at],
                )
                .map_err(|e| e.to_string())?;
            (n == 1)
                .then_some(())
                .ok_or_else(|| format!("任务不存在: {id}"))
        })
    }

    /// 删除任务（同事务连带删全部运行记录——046-0「任务删除则相关日志连带
    /// 删除」）。返回连带删除的 run 条数；任务不存在 = Err（防双删竞态静默过）。
    pub fn delete(&self, id: &str) -> Result<usize, String> {
        self.db.with_tx(|tx| {
            let runs = tx
                .execute("DELETE FROM task_runs WHERE def_id = ?1", [id])
                .map_err(|e| e.to_string())?;
            let n = tx
                .execute("DELETE FROM task_defs WHERE id = ?1", [id])
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err(format!("任务不存在: {id}"));
            }
            Ok(runs)
        })
    }

    /// 记一次运行开始（outcome = running；def 必须存在，防孤儿台账行）。
    /// agent 任务的 session_path 随会话绑文件回填（[`bind_run`]），模型冻结
    /// 也在派发时走 [`bind_run`]（046-S3）。返回 run id。
    pub fn start_run(
        &self,
        def_id: &str,
        trigger: Trigger,
        started_at: i64,
        session_path: Option<&str>,
    ) -> Result<i64, String> {
        self.db.with_tx(|tx| {
            let n: i64 = tx
                .query_row(
                    "SELECT COUNT(*) FROM task_defs WHERE id = ?1",
                    [def_id],
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            if n == 0 {
                return Err(format!("任务不存在: {def_id}"));
            }
            tx.execute(
                "INSERT INTO task_runs (def_id, trigger, started_at, outcome, session_path)
                 VALUES (?1, ?2, ?3, 'running', ?4)",
                params![def_id, trigger.as_str(), started_at, session_path],
            )
            .map_err(|e| e.to_string())?;
            Ok(tx.last_insert_rowid())
        })
    }

    /// 派发上下文回填（046-S3）：session_path（agent 会话绑文件后）/ model
    /// （派发时一次性冻结）。None = 保留现值。
    pub fn bind_run(
        &self,
        run_id: i64,
        session_path: Option<&str>,
        model: Option<&str>,
    ) -> Result<(), String> {
        self.db.with_tx(|tx| {
            let n = tx
                .execute(
                    "UPDATE task_runs
                     SET session_path = COALESCE(?2, session_path), model = COALESCE(?3, model)
                     WHERE id = ?1",
                    params![run_id, session_path, model],
                )
                .map_err(|e| e.to_string())?;
            (n == 1)
                .then_some(())
                .ok_or_else(|| format!("run {run_id} 不存在"))
        })
    }

    /// 运行结算（整行 UPDATE）：outcome/finished_at/result_json 一次写齐。
    /// 仅 running 态可结算（防双结算覆盖终态）。
    pub fn settle_run(
        &self,
        run_id: i64,
        outcome: Outcome,
        finished_at: i64,
        result_json: Option<&str>,
    ) -> Result<(), String> {
        self.db.with_tx(|tx| {
            let n = tx
                .execute(
                    "UPDATE task_runs
                     SET outcome = ?2, finished_at = ?3, result_json = ?4
                     WHERE id = ?1 AND outcome = 'running'",
                    params![run_id, outcome.as_str(), finished_at, result_json],
                )
                .map_err(|e| e.to_string())?;
            (n == 1)
                .then_some(())
                .ok_or_else(|| format!("run {run_id} 不存在或已结算"))
        })
    }

    /// 运行历史（UI 历史 tab）：`def_id` = None 时跨任务（「全部任务」过滤
    /// 档）；started_at 倒序，`limit` ≤ 0 = 不截断。**只含真实执行**——
    /// skipped（历史跳过账）不回（2026-11-23 用户令：没执行不属于执行
    /// log；新账也不再落 skipped，行随 prune 自然淘汰）。
    pub fn list_runs(&self, def_id: Option<&str>, limit: i64) -> Result<Vec<TaskRun>, String> {
        self.db.with_conn(|c| {
            let (sql, bind_def): (&str, Option<&str>) = match def_id {
                Some(_) => (
                    "SELECT id, def_id, trigger, started_at, finished_at, outcome, session_path, model, result_json
                     FROM task_runs WHERE def_id = ?1 AND outcome <> 'skipped'
                     ORDER BY started_at DESC, id DESC LIMIT ?2",
                    def_id,
                ),
                None => (
                    "SELECT id, def_id, trigger, started_at, finished_at, outcome, session_path, model, result_json
                     FROM task_runs WHERE outcome <> 'skipped'
                     ORDER BY started_at DESC, id DESC LIMIT ?1",
                    None,
                ),
            };
            let mut stmt = c.prepare(sql).map_err(|e| e.to_string())?;
            let rows = if let Some(def) = bind_def {
                stmt.query_map(params![def, limit], Self::row_to_run)
            } else {
                stmt.query_map(params![limit], Self::row_to_run)
            }
            .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
    }

    /// 某任务**上次运行**（列表行通知点 / agent 任务名可点击跳会话）。
    pub fn last_run(&self, def_id: &str) -> Result<Option<TaskRun>, String> {
        Ok(self.list_runs(Some(def_id), 1)?.into_iter().next())
    }

    /// 全部 running 态运行（启动 recover：上个实例残留 → 置 failed，046-0）。
    pub fn running_runs(&self) -> Result<Vec<TaskRun>, String> {
        self.db.with_conn(|c| {
            let mut stmt = c
                .prepare("SELECT id, def_id, trigger, started_at, finished_at, outcome, session_path, model, result_json
                          FROM task_runs WHERE outcome = 'running'
                          ORDER BY started_at")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], Self::row_to_run)
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
    }

    /// 每任务只保留最近 `keep` 条运行（started_at 新在前），超出即删。
    /// 046-0「触发」：启动 recover 跑一次 + 泵每小时 tick 顺带跑一次；
    /// 只删台账行，不动 agent 会话文件（用户资产，归 052）。返回删除总条数。
    pub fn prune_runs(&self, keep: usize) -> Result<usize, String> {
        self.db.with_tx(|tx| {
            let mut stmt = tx
                .prepare("SELECT DISTINCT def_id FROM task_runs")
                .map_err(|e| e.to_string())?;
            let def_ids: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            drop(stmt);
            let mut total = 0usize;
            for def_id in def_ids {
                total += tx
                    .execute(
                        "DELETE FROM task_runs
                         WHERE def_id = ?1 AND id NOT IN (
                             SELECT id FROM task_runs WHERE def_id = ?1
                             ORDER BY started_at DESC, id DESC LIMIT ?2
                         )",
                        params![def_id, keep as i64],
                    )
                    .map_err(|e| e.to_string())? as usize;
            }
            Ok(total)
        })
    }

    fn row_to_def(r: &rusqlite::Row<'_>) -> rusqlite::Result<TaskDef> {
        Ok(TaskDef {
            id: r.get(0)?,
            title: r.get(1)?,
            spec_json: r.get(2)?,
            enabled: r.get::<_, i64>(3)? != 0,
            next_run_at: r.get(4)?,
            anchor_at: r.get(5)?,
            created_at: r.get(6)?,
            updated_at: r.get(7)?,
        })
    }

    fn row_to_run(r: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRun> {
        let trigger: String = r.get(2)?;
        let outcome: String = r.get(5)?;
        let trigger = Trigger::from_db(&trigger).map_err(rusqlite_err)?;
        let outcome = Outcome::from_db(&outcome).map_err(rusqlite_err)?;
        Ok(TaskRun {
            id: r.get(0)?,
            def_id: r.get(1)?,
            trigger,
            started_at: r.get(3)?,
            finished_at: r.get(4)?,
            outcome,
            session_path: r.get(6)?,
            model: r.get(7)?,
            result_json: r.get(8)?,
        })
    }
}

/// 领域校验错误转 rusqlite 行映射错误（枚举值只会因手改库出现）。
fn rusqlite_err(e: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        0,
        rusqlite::types::Type::Text,
        Box::new(std::io::Error::other(e)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::tmp_dir;

    /// 返回 (临时目录, DbSource)：目录必须活得比连接久，
    /// 否则 Drop 删目录时 rusqlite 还持有 context.db。
    fn db() -> (crate::tests_support::TestDir, DbSource) {
        let dir = tmp_dir("tasks");
        let db = DbSource::open(&dir).unwrap();
        (dir, db)
    }

    fn agent_schedule() -> TaskSchedule {
        TaskSchedule {
            unit: ScheduleUnit::Daily,
            interval: 1,
            hour: 8,
            minute: 30,
            weekdays: vec![],
            month_day: 0,
        }
    }

    fn agent_spec(prompt: &str) -> TaskSpec {
        TaskSpec {
            kind: TaskKind::Agent,
            schedule: agent_schedule(),
            agent: Some(AgentSpec {
                prompt: prompt.into(),
                project: r"D:\work\demo".into(),
                tools: "default".into(),
                model: None,
                thinking: "auto".into(),
                extensions: None,
            }),
            script: None,
        }
    }

    fn script_spec() -> TaskSpec {
        TaskSpec {
            kind: TaskKind::Script,
            schedule: TaskSchedule {
                unit: ScheduleUnit::Minute,
                interval: 10,
                ..agent_schedule()
            },
            agent: None,
            script: Some(ScriptSpec {
                path: r"D:\scripts\clean.ps1".into(),
            }),
        }
    }

    fn spec_json(spec: &TaskSpec) -> String {
        serde_json::to_string(spec).unwrap()
    }

    fn def(id: &str, spec: &TaskSpec) -> TaskDef {
        TaskDef {
            id: id.into(),
            title: "任务".into(),
            spec_json: spec_json(spec),
            enabled: true,
            next_run_at: None,
            anchor_at: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn agent_def(id: &str) -> TaskDef {
        def(id, &agent_spec("写日报"))
    }

    // ---- spec 校验 ----

    #[test]
    fn spec_roundtrip_keeps_fields() {
        let spec = agent_spec("写日报");
        let parsed = TaskSpec::from_json(&spec_json(&spec)).unwrap();
        assert_eq!(parsed, spec);
        let parsed = TaskSpec::from_json(&spec_json(&script_spec())).unwrap();
        assert_eq!(parsed, script_spec());
    }

    #[test]
    fn agent_extensions_roundtrip_and_legacy_default() {
        // 每任务配方：显式清单（含空表 = 一个扩展都不挂）零漂移
        let lists: [&[&str]; 2] = [&["npm:pi-web-access"], &[]];
        for list in lists {
            let mut spec = agent_spec("写日报");
            let want: Vec<String> = list.iter().map(|s| s.to_string()).collect();
            spec.agent.as_mut().unwrap().tools = "custom".into();
            spec.agent.as_mut().unwrap().extensions = Some(want.clone());
            let parsed = TaskSpec::from_json(&spec_json(&spec)).unwrap();
            assert_eq!(parsed, spec);
            assert_eq!(parsed.agent.unwrap().extensions, Some(want));
        }
        // 存量 spec（修复前落库，无 extensions 键）= None：沿用全局启用集
        let legacy = r#"{"kind":"agent","schedule":{"unit":"daily","interval":1,"hour":8,"minute":30},
                      "agent":{"prompt":"x","project":"p","tools":"custom","thinking":"auto"}}"#;
        let parsed = TaskSpec::from_json(legacy).unwrap();
        assert_eq!(parsed.agent.unwrap().extensions, None);
    }

    #[test]
    fn agent_extensions_reject_blank_entry() {
        let (_dir, db) = db();
        let tasks = TaskStore::new(&db);
        let mut spec = agent_spec("写日报");
        spec.agent.as_mut().unwrap().tools = "custom".into();
        spec.agent.as_mut().unwrap().extensions = Some(vec!["npm:a".into(), "  ".into()]);
        assert!(
            spec.validate().err().unwrap().contains("extensions"),
            "空串条目拒"
        );
        let mut d = agent_def("t");
        d.spec_json = spec_json(&spec);
        assert!(tasks.create(&d).is_err(), "写库前拒绝");
        assert!(tasks.list_defs().unwrap().is_empty());

        spec.agent.as_mut().unwrap().extensions = Some(vec!["npm:a".into()]);
        d.spec_json = spec_json(&spec);
        tasks.create(&d).unwrap();
        let got = TaskSpec::from_json(&tasks.get_def("t").unwrap().unwrap().spec_json).unwrap();
        assert_eq!(got, spec, "落库读回清单一致");
    }

    #[test]
    fn spec_validation_rejects_illegal() {
        // schedule 各档边界
        let mut s = agent_spec("x");
        s.schedule.interval = 0;
        assert!(s.validate().err().unwrap().contains("interval"));
        s.schedule.interval = 1;
        s.schedule.hour = 24;
        assert!(s.validate().err().unwrap().contains("hour"));
        s.schedule = agent_schedule();
        s.schedule.minute = 60;
        assert!(s.validate().err().unwrap().contains("minute"));
        // weekly：weekdays 必填且 1-7（周一=1）
        let mut s = agent_spec("x");
        s.schedule = TaskSchedule {
            unit: ScheduleUnit::Weekly,
            weekdays: vec![],
            ..agent_schedule()
        };
        assert!(s.validate().err().unwrap().contains("weekdays"));
        s.schedule.weekdays = vec![0, 1];
        assert!(s.validate().err().unwrap().contains("weekdays"));
        s.schedule.weekdays = vec![1, 8];
        assert!(s.validate().err().unwrap().contains("weekdays"));
        // monthly：month_day 1-31
        let mut s = agent_spec("x");
        s.schedule = TaskSchedule {
            unit: ScheduleUnit::Monthly,
            month_day: 0,
            ..agent_schedule()
        };
        assert!(s.validate().err().unwrap().contains("运行日"));
        s.schedule.month_day = 32;
        assert!(s.validate().err().unwrap().contains("运行日"));
        // kind 与业务字段互斥/必填
        let mut s = agent_spec("x");
        s.agent = None;
        assert!(s.validate().err().unwrap().contains("agent 字段"));
        s = agent_spec("x");
        s.script = Some(ScriptSpec {
            path: "a.ps1".into(),
        });
        assert!(s.validate().is_err(), "agent + script 同时携带应拒");
        let mut s = script_spec();
        s.script = None;
        assert!(s.validate().err().unwrap().contains("script 字段"));
        let mut s = script_spec();
        s.agent = Some(AgentSpec {
            prompt: "x".into(),
            project: "p".into(),
            tools: "default".into(),
            model: None,
            thinking: "auto".into(),
            extensions: None,
        });
        assert!(s.validate().is_err(), "script + agent 同时携带应拒");
        // 业务字段内容
        let mut s = agent_spec("  ");
        assert!(s.validate().err().unwrap().contains("prompt"));
        let mut s = agent_spec("x");
        s.agent.as_mut().unwrap().project = "".into();
        assert!(s.validate().err().unwrap().contains("project"));
        let mut s = agent_spec("x");
        s.agent.as_mut().unwrap().tools = "yolo".into();
        assert!(s.validate().err().unwrap().contains("tools"));
        let mut s = agent_spec("x");
        s.agent.as_mut().unwrap().thinking = "ultra".into();
        assert!(s.validate().err().unwrap().contains("thinking"));
        let mut s = script_spec();
        s.script.as_mut().unwrap().path = "  ".into();
        assert!(s.validate().err().unwrap().contains("path"));
        // 未知枚举值在反序列化即拒（写库前拒绝）
        let bad = r#"{"kind":"script","schedule":{"unit":"yearly","interval":1},
                      "script":{"path":"a.ps1"}}"#;
        assert!(TaskSpec::from_json(bad).is_err());
    }

    #[test]
    fn def_title_must_not_be_empty() {
        let (_dir, db) = db();
        let tasks = TaskStore::new(&db);
        let mut d = agent_def("t");
        d.title = "  ".into();
        assert!(tasks.create(&d).err().unwrap().contains("任务名称"));
    }

    // ---- 任务定义 CRUD ----

    #[test]
    fn create_limit_counts_all_states() {
        let (_dir, db) = db();
        let tasks = TaskStore::new(&db);
        for i in 0..TASK_CREATE_LIMIT {
            tasks.create(&agent_def(&format!("t{i}"))).unwrap();
        }
        let err = tasks.create(&agent_def("overflow")).err().unwrap();
        assert!(err.contains("上限"), "{err}");
        // 停用不计入豁免——所有状态都计入（ZCode 同语义）
        tasks.set_enabled("t0", false).unwrap();
        assert!(tasks.create(&agent_def("overflow")).is_err());
        // 删一个腾出名额
        tasks.delete("t0").unwrap();
        tasks.create(&agent_def("overflow")).unwrap();
        assert_eq!(tasks.list_defs().unwrap().len(), TASK_CREATE_LIMIT);
    }

    #[test]
    fn enable_list_reschedule_roundtrip() {
        let (_dir, db) = db();
        let tasks = TaskStore::new(&db);
        let mut off = agent_def("b");
        off.enabled = false;
        tasks.create(&agent_def("a")).unwrap();
        tasks.create(&off).unwrap();

        assert_eq!(tasks.list_defs().unwrap().len(), 2);
        assert_eq!(
            tasks
                .list_enabled()
                .unwrap()
                .iter()
                .map(|d| d.id.as_str())
                .collect::<Vec<_>>(),
            vec!["a"]
        );

        tasks.set_enabled("b", true).unwrap();
        assert_eq!(tasks.list_enabled().unwrap().len(), 2);
        tasks.set_enabled("b", false).unwrap();
        assert!(tasks.set_enabled("no-such", true).is_err());

        // reschedule 写调度指针；None = 清空
        tasks.reschedule("a", Some(123_456), Some(100)).unwrap();
        let d = tasks.get_def("a").unwrap().unwrap();
        assert_eq!(d.next_run_at, Some(123_456));
        assert_eq!(d.anchor_at, Some(100));
        tasks.reschedule("a", None, None).unwrap();
        let d = tasks.get_def("a").unwrap().unwrap();
        assert_eq!(d.next_run_at, None);
        assert_eq!(tasks.get_def("no-such").unwrap(), None);
        assert!(tasks.reschedule("no-such", Some(1), None).is_err());
    }

    #[test]
    fn update_def_overwrites_row_keeps_created_at() {
        let (_dir, db) = db();
        let tasks = TaskStore::new(&db);
        let mut d = agent_def("t");
        d.created_at = 7;
        tasks.create(&d).unwrap();

        let mut edited = d.clone();
        edited.title = "改名".into();
        edited.spec_json = spec_json(&script_spec());
        edited.enabled = false;
        edited.next_run_at = Some(42);
        edited.anchor_at = Some(40);
        edited.updated_at = 9;
        tasks.update_def(&edited).unwrap();

        let got = tasks.get_def("t").unwrap().unwrap();
        assert_eq!(got.title, "改名");
        assert_eq!(got.spec_json, spec_json(&script_spec()));
        assert!(!got.enabled);
        assert_eq!(got.next_run_at, Some(42));
        assert_eq!(got.anchor_at, Some(40));
        assert_eq!(got.updated_at, 9);
        assert_eq!(got.created_at, 7, "created_at 不被 update 覆盖");

        // 非法 spec 拒写，原行完好
        let mut bad = edited;
        bad.spec_json = r#"{"kind":"script"}"#.into();
        assert!(tasks.update_def(&bad).is_err());
        assert_eq!(tasks.get_def("t").unwrap().unwrap().title, "改名");
        assert!(
            tasks
                .update_def(&TaskDef {
                    id: "ghost".into(),
                    title: "x".into(),
                    spec_json: spec_json(&agent_spec("x")),
                    enabled: true,
                    next_run_at: None,
                    anchor_at: None,
                    created_at: 1,
                    updated_at: 1,
                })
                .is_err()
        );
    }

    // ---- 运行台账 ----

    #[test]
    fn run_lifecycle_start_then_settle() {
        let (_dir, db) = db();
        let tasks = TaskStore::new(&db);
        tasks.create(&agent_def("a")).unwrap();

        assert!(tasks.last_run("a").unwrap().is_none());
        let run_id = tasks.start_run("a", Trigger::Manual, 100, None).unwrap();
        assert_eq!(
            tasks.running_runs().unwrap().len(),
            1,
            "运行中可见（recover 扫描源）"
        );

        // bind_run：模型派发时冻结 + 会话绑文件回填（None = 保留现值）
        tasks
            .bind_run(run_id, None, Some("bigmodel/glm-5.3"))
            .unwrap();
        tasks
            .bind_run(run_id, Some("sess\\abc.jsonl"), None)
            .unwrap();
        assert!(tasks.bind_run(999, Some("x"), None).is_err());

        tasks
            .settle_run(run_id, Outcome::Ok, 200, Some(r#"{"summary":"done"}"#))
            .unwrap();
        let last = tasks.last_run("a").unwrap().expect("有运行记录");
        assert_eq!(last.id, run_id);
        assert_eq!(last.trigger, Trigger::Manual);
        assert_eq!(last.outcome, Outcome::Ok);
        assert_eq!(last.finished_at, Some(200));
        assert_eq!(last.session_path.as_deref(), Some("sess\\abc.jsonl"));
        assert_eq!(last.model.as_deref(), Some("bigmodel/glm-5.3"));
        assert_eq!(last.result_json.as_deref(), Some(r#"{"summary":"done"}"#));
        assert!(
            tasks.running_runs().unwrap().is_empty(),
            "结算后不再 running"
        );
        assert!(
            tasks
                .settle_run(run_id, Outcome::Timeout, 300, None)
                .is_err()
        );

        // 超时结算 + 失败结算各留终态
        let t = tasks.start_run("a", Trigger::Schedule, 300, None).unwrap();
        tasks.settle_run(t, Outcome::Timeout, 400, None).unwrap();
        let f = tasks.start_run("a", Trigger::Schedule, 500, None).unwrap();
        tasks
            .settle_run(f, Outcome::Failed, 600, Some(r#"{"error":"boom"}"#))
            .unwrap();
        let last = tasks.last_run("a").unwrap().unwrap();
        assert_eq!(last.outcome, Outcome::Failed);
        assert!(
            tasks
                .start_run("no-such", Trigger::Manual, 1, None)
                .is_err()
        );
    }

    #[test]
    fn list_runs_filters_legacy_skips_and_orders() {
        let (_dir, db) = db();
        let tasks = TaskStore::new(&db);
        tasks.create(&agent_def("a")).unwrap();
        tasks.create(&agent_def("b")).unwrap();

        // 历史跳过账（旧版落过 skipped；新账已不再写）——直插 DB 模拟遗留
        for (def_id, at) in [("a", 100), ("b", 50)] {
            db.with_conn(|c| {
                c.execute(
                    "INSERT INTO task_runs (def_id, trigger, started_at, finished_at, outcome, result_json)
                     VALUES (?1, 'schedule', ?2, ?2, 'skipped', '{\"reason\":\"错过触发窗口\"}')",
                    params![def_id, at],
                )
                .map_err(|e| e.to_string())
            })
            .unwrap();
        }
        let manual = tasks.start_run("a", Trigger::Manual, 200, None).unwrap();
        tasks.settle_run(manual, Outcome::Ok, 250, None).unwrap();

        // 只回真实执行：skipped 行不论 def 过滤与否都不出现
        let runs_a = tasks.list_runs(Some("a"), -1).unwrap();
        assert_eq!(runs_a.len(), 1, "skipped 历史行被过滤");
        assert_eq!(runs_a[0].started_at, 200);

        let all = tasks.list_runs(None, -1).unwrap();
        assert_eq!(all.len(), 1, "跨任务查询同样过滤");
        assert_eq!(all[0].def_id, "a");
        assert_eq!(tasks.list_runs(None, 1).unwrap().len(), 1, "limit 截断");
        assert!(tasks.list_runs(Some("b"), -1).unwrap().is_empty());

        // last_run 同口径：上次「真实执行」
        assert_eq!(tasks.last_run("a").unwrap().unwrap().outcome, Outcome::Ok);
        assert!(tasks.last_run("b").unwrap().is_none());
    }

    #[test]
    fn prune_runs_keeps_newest_per_def() {
        let (_dir, db) = db();
        let tasks = TaskStore::new(&db);
        tasks.create(&agent_def("a")).unwrap();
        tasks.create(&agent_def("b")).unwrap();
        for t in 0..60 {
            let id = tasks.start_run("a", Trigger::Schedule, t, None).unwrap();
            tasks.settle_run(id, Outcome::Ok, t + 1, None).unwrap();
        }
        for t in 0..3 {
            let id = tasks.start_run("b", Trigger::Schedule, t, None).unwrap();
            tasks.settle_run(id, Outcome::Ok, t + 1, None).unwrap();
        }

        assert_eq!(tasks.prune_runs(RUN_KEEP_LIMIT).unwrap(), 10);
        let runs_a = tasks.list_runs(Some("a"), -1).unwrap();
        assert_eq!(runs_a.len(), 50);
        assert_eq!(
            runs_a.iter().map(|r| r.started_at).min(),
            Some(10),
            "最老 10 条被删"
        );
        assert_eq!(
            runs_a.iter().map(|r| r.started_at).max(),
            Some(59),
            "最新保留"
        );
        assert_eq!(
            tasks.list_runs(Some("b"), -1).unwrap().len(),
            3,
            "未超量不动"
        );
        assert_eq!(tasks.prune_runs(RUN_KEEP_LIMIT).unwrap(), 0, "幂等");
        assert_eq!(
            tasks.prune_runs(2).unwrap(),
            49,
            "keep 收紧即再删（a 48 + b 1）"
        );
        assert_eq!(tasks.list_runs(Some("a"), -1).unwrap().len(), 2);
        assert_eq!(tasks.list_runs(Some("b"), -1).unwrap().len(), 2);
    }

    #[test]
    fn delete_cascades_runs() {
        let (_dir, db) = db();
        let tasks = TaskStore::new(&db);
        tasks.create(&agent_def("a")).unwrap();
        for t in 0..3 {
            tasks.start_run("a", Trigger::Schedule, t, None).unwrap();
        }

        assert_eq!(tasks.delete("a").unwrap(), 3, "连带删 3 条 run");
        assert!(tasks.list_defs().unwrap().is_empty());
        assert!(tasks.list_runs(None, -1).unwrap().is_empty());
        assert!(tasks.delete("a").is_err(), "重复删除报任务不存在");
    }

    // ---- 枚举 DB 字符串 ----

    #[test]
    fn trigger_outcome_db_strings_roundtrip() {
        for (t, s) in [(Trigger::Schedule, "schedule"), (Trigger::Manual, "manual")] {
            assert_eq!(t.as_str(), s);
        }
        for (o, s) in [
            (Outcome::Running, "running"),
            (Outcome::Ok, "ok"),
            (Outcome::Failed, "failed"),
            (Outcome::Timeout, "timeout"),
            (Outcome::Skipped, "skipped"),
        ] {
            assert_eq!(o.as_str(), s);
        }
        assert_eq!(Trigger::from_db("manual").unwrap(), Trigger::Manual);
        assert_eq!(Outcome::from_db("timeout").unwrap(), Outcome::Timeout);
        assert!(Trigger::from_db("cron").is_err());
        assert!(Outcome::from_db("pending").is_err());
    }
}
