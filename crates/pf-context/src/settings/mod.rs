//! settings 子集（001-0 §2 / 001-1）：用户意图的唯一 owner。
//!
//! **读**：进程内一次 load → `Arc<Snapshot>`（typed section 合并 L0/L1/L2/L3，
//! `validate` 后发布），[`SettingsStore::get`] O(1) 零 I/O 跨帧安全；坏 section
//! 回退 Default 并记 [`SettingProblem`]，坏整份保持上一份好快照。
//!
//! **写**：[`SettingsStore::set`] 内存先换（UI 下帧即新值）→ dirty → 单写者
//! 线程 50ms coalesce → 读-改-写（只替换本 section，未知键/未知字段透传，
//! 见 `doc.rs`）→ tmp+rename 原子写 → 同内容跳过；[`SettingsStore::flush`] /
//! [`SettingsStore::save_now`] 同步落盘（退出/迁移/测试）。
//!
//! **外部**：`watch.rs` 哈希比对重载并广播；自写回声用预期哈希窗口吃掉；
//! 写失败内存保留 + 退避重试 + 计数（禁止静默吞，001-0 §5）。
//!
//! 写者/监听线程均为进程级生命周期（Hub 全局单例）；测试丢弃 store 后线程
//! 阻塞在脏队列上，随进程退出，无资源句柄。

mod doc;
mod export;
mod source;
mod watch;

pub mod section;
pub mod sections;

#[cfg(test)]
mod tests;

pub use section::{Layer, Origin, Section, SettingProblem};

use serde_json::Value;
use std::any::Any;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

/// 单次 coalesce 窗口：连续拖 stepper 只产生一次写盘（001-1 §7）。
const COALESCE: Duration = Duration::from_millis(50);
/// 自写回声窗口：落盘后该时长内的同哈希 watch 事件视为回声。
const ECHO_WINDOW: Duration = Duration::from_secs(2);
/// 写失败退避起点 / 上限（001-0 §5：内存保留 + 退避重试）。
const FAIL_BACKOFF_START: Duration = Duration::from_millis(200);
const FAIL_BACKOFF_CAP: Duration = Duration::from_secs(10);
/// problem 累积上限（防失控刷内存；只丢最老的）。
const PROBLEM_CAP: usize = 100;

/// 不可变快照：typed sections + 各段来源 + 问题清单。读方拿 `Arc` 零拷贝。
pub struct Snapshot {
    sections: HashMap<&'static str, Arc<dyn Any + Send + Sync>>,
    /// 各 section 的规范 JSON（validate 后）——写盘与「未落盘改动以内存为准」
    /// 都从这里取，保证读到的 == 将要写的。
    section_json: HashMap<&'static str, Value>,
    origins: HashMap<&'static str, Origin>,
    /// 构建本快照时刻累积的 problem 清单（含历史）。
    pub problems: Arc<[SettingProblem]>,
}

impl Snapshot {
    /// typed section（O(1)）。类型未注册/不匹配 = 编程错误，panic 快速暴露。
    pub fn get<T: Section>(&self) -> Arc<T> {
        let any = self
            .sections
            .get(T::KEY)
            .unwrap_or_else(|| panic!("section 未注册: {}", T::KEY));
        Arc::clone(any)
            .downcast::<T>()
            .ok()
            .unwrap_or_else(|| panic!("section 类型不匹配: {}", T::KEY))
    }

    /// 该 section 当前值的来源（文件层面；过渡期回退判定用）。
    pub fn origin<T: Section>(&self) -> Origin {
        self.origins.get(T::KEY).copied().unwrap_or(Origin::Absent)
    }

    fn clone_shallow(&self) -> Snapshot {
        Snapshot {
            sections: self.sections.clone(),
            section_json: self.section_json.clone(),
            origins: self.origins.clone(),
            problems: Arc::clone(&self.problems),
        }
    }
}

/// 观测计数（测试断言 + 未来状态栏提示）。
#[derive(Debug, Clone, Copy, Default)]
pub struct SettingsStats {
    /// 实际写盘次数（同内容跳过不计）
    pub writes: u64,
    /// 写失败次数（内存保留、退避重试）
    pub write_failures: u64,
    /// 外部改动触发的重载次数
    pub reloads: u64,
    /// 投影实际写盘次数（001-2，同内容跳过不计）
    pub exports: u64,
}

struct WatchState {
    /// 预期回声哈希窗口：(盘上应出现的内容哈希, 登记时刻)
    expected: VecDeque<(u64, Instant)>,
    /// 最近一次见过的盘上内容哈希（读到什么记什么；同哈希事件 = 无变化）
    last_seen: u64,
    handle: Option<std::thread::JoinHandle<()>>,
    /// notify 监听器必须活满进程：存这里（Mutex 内只持有、不 poll）
    watcher: Option<notify::RecommendedWatcher>,
}

impl Default for WatchState {
    fn default() -> Self {
        Self {
            expected: VecDeque::new(),
            last_seen: 0,
            handle: None,
            watcher: None,
        }
    }
}

struct StoreInner {
    /// `app-settings.json` 绝对路径；`None` = 纯内存态（测试）
    path: Option<PathBuf>,
    snap: RwLock<Arc<Snapshot>>,
    /// L2/L3 层覆盖值（index 0=Project / 1=Session）。S1 仅供测试与占位；
    /// S2/S4 接线。`Value::Null` = 移除覆盖。**永不回写文件**（001-0 §4）。
    overrides: Mutex<[HashMap<String, Value>; 2]>,
    /// boot-only 段的值（S2：dev_flags，源 = dev-flags.json + PI_FLASH_* env，
    /// open 时一次性灌入，改了需重启）。boot 值压过文件层，且**永不回写**。
    boot: HashMap<&'static str, Value>,
    /// set_preview 过的 section（只换内存未标脏；save_now 时转 dirty 落盘，
    /// 001-1 §2「拖拽预览松手才写」）
    previewed: Mutex<HashSet<&'static str>>,
    /// 投影 payload 哈希（0 = 尚未导出过，001-2 §3 同内容跳过）
    projection_payload: Mutex<u64>,
    dirty: Mutex<HashSet<&'static str>>,
    dirty_cv: Condvar,
    /// 写者线程与同步 [`SettingsStore::flush`] 互斥（flush 先到 = 写者扑空）
    write_lock: Mutex<()>,
    watch: Mutex<WatchState>,
    /// 订阅者（快照换新即广播；回调内不得再进 store API）
    subs: Mutex<Vec<Box<dyn Fn(&Arc<Snapshot>) + Send + Sync>>>,
    /// problem 累积日志（快照携带其时点副本）
    problems: Mutex<Vec<SettingProblem>>,
    writes: AtomicU64,
    write_failures: AtomicU64,
    reloads: AtomicU64,
    exports: AtomicU64,
    /// 当前写失败退避时长（成功即复位）
    fail_backoff: Mutex<Duration>,
}

/// settings 存储：一个 [`Hub`] 生命周期内的读模型 + 写通道 + watcher 门面。
#[derive(Clone)]
pub struct SettingsStore {
    inner: Arc<StoreInner>,
}

impl SettingsStore {
    /// 打开（一次读盘 + 单写者线程）。`None` = 纯内存态。
    /// `[perf] settings.load` 打点走 `PI_FLASH_PERF=1` 门控（app 同约定）。
    pub fn open(path: Option<PathBuf>) -> SettingsStore {
        let t0 = Instant::now();
        // boot-only 段（S2：dev_flags）：dev-flags.json + PI_FLASH_* env 的一次性
        // 解析（语义全在 pi-link::dev_flags，env 最高优先；boot-only = 改了需重启）
        let mut boot = HashMap::new();
        boot.insert(sections::DevFlags::KEY, sections::dev_flags::boot_value());
        let inner = Arc::new(StoreInner {
            path,
            snap: RwLock::new(Arc::new(Snapshot {
                sections: HashMap::new(),
                section_json: HashMap::new(),
                origins: HashMap::new(),
                problems: Arc::from(vec![]),
            })),
            overrides: Mutex::new([HashMap::new(), HashMap::new()]),
            boot,
            previewed: Mutex::new(HashSet::new()),
            projection_payload: Mutex::new(0),
            dirty: Mutex::new(HashSet::new()),
            dirty_cv: Condvar::new(),
            write_lock: Mutex::new(()),
            watch: Mutex::new(WatchState::default()),
            subs: Mutex::new(Vec::new()),
            problems: Mutex::new(Vec::new()),
            writes: AtomicU64::new(0),
            write_failures: AtomicU64::new(0),
            reloads: AtomicU64::new(0),
            exports: AtomicU64::new(0),
            fail_backoff: Mutex::new(FAIL_BACKOFF_START),
        });
        let snapshot = match inner.build_snapshot() {
            Ok(s) => s,
            Err(e) => {
                // 首启即坏档：全 Default 起步，problem 记录在案（不许静默）
                inner.push_problem("<doc>", &e);
                inner.build_snapshot_from(None)
            }
        };
        *inner.snap.write().unwrap() = Arc::new(snapshot);
        inner.update_last_seen();
        perf_log("settings.load", t0.elapsed());
        spawn_writer(Arc::clone(&inner));
        // 001-S3：初次投影（后台线程，启动热路径零 IO；扩展首次委派前就绪）
        std::thread::Builder::new()
            .name("pf-settings-export".into())
            .spawn({
                let inner = Arc::clone(&inner);
                move || {
                    if let Err(e) = inner.export_projection() {
                        eprintln!("[settings] 初次投影写盘失败（下次写盘重试）: {e}");
                    }
                }
            })
            .ok();
        SettingsStore { inner }
    }

    /// 启动外部改动监听（PF 数据目录，非递归）。路径缺失/目录未建时静默降级。
    pub fn start_watcher(&self) {
        let Some(path) = self.inner.path.as_ref() else {
            return;
        };
        let Some(dir) = path.parent() else {
            return;
        };
        let _ = std::fs::create_dir_all(dir);
        watch::spawn(Arc::clone(&self.inner), dir.to_path_buf(), path.clone());
    }

    // ------------------------------------------------------------------
    // 读
    // ------------------------------------------------------------------

    /// 当前合并快照里的 typed section（O(1)、零 I/O、无锁争用面极小）。
    pub fn get<T: Section>(&self) -> Arc<T> {
        let snap = Arc::clone(&self.inner.snap.read().unwrap());
        snap.get::<T>()
    }

    /// 该 section 在**文件**里的来源（v2 section / v1 平铺键 / 缺失）。
    /// 过渡期判定用：如容量在 v2 段出现前先问 `agents/config.json`。
    pub fn origin<T: Section>(&self) -> Origin {
        self.inner.snap.read().unwrap().origin::<T>()
    }

    /// 订阅快照换新（set 与外部重载都会广播）。回调在广播线程上跑，
    /// 必须轻（置标志即可），且回调内不得再进 store API。
    pub fn subscribe(&self, f: Box<dyn Fn(&Arc<Snapshot>) + Send + Sync>) {
        self.inner.subs.lock().unwrap().push(f);
    }

    /// 当前快照 Arc（测试/观测用；O(1)）。
    pub(crate) fn snapshot_clone(&self) -> Arc<Snapshot> {
        Arc::clone(&self.inner.snap.read().unwrap())
    }

    /// 累积 problem 清单副本（坏档/坏段都进这里，001-0 §5）。
    pub fn problems(&self) -> Vec<SettingProblem> {
        self.inner.problems.lock().unwrap().clone()
    }

    /// 观测计数副本。
    pub fn stats(&self) -> SettingsStats {
        let i = &self.inner;
        SettingsStats {
            writes: i.writes.load(Ordering::Relaxed),
            write_failures: i.write_failures.load(Ordering::Relaxed),
            reloads: i.reloads.load(Ordering::Relaxed),
            exports: i.exports.load(Ordering::Relaxed),
        }
    }

    // ------------------------------------------------------------------
    // 写
    // ------------------------------------------------------------------

    /// 更新一个 section：内存立即换（UI 下帧即新值）→ 标脏 → 单写者线程
    /// 50ms coalesce 后读-改-写原子落盘。**唯一 clamp 点是 `T::validate`**
    /// （这里调过），调用方不必也不应再钳。
    pub fn set<T: Section>(&self, f: impl FnOnce(&mut T)) {
        let cur = self.get::<T>();
        let mut t = (*cur).clone();
        f(&mut t);
        t.validate();
        let json = serde_json::to_value(&t).expect("section 序列化");
        let new_snap = {
            let mut guard = self.inner.snap.write().unwrap();
            let mut s = guard.clone_shallow();
            s.sections.insert(T::KEY, Arc::new(t));
            s.section_json.insert(T::KEY, json);
            // set 过即 PF 侧事实源归位：过渡期回退判定（origin）同步翻 Section
            s.origins.insert(T::KEY, Origin::Section);
            let arc = Arc::new(s);
            *guard = Arc::clone(&arc);
            arc
        };
        self.inner.previewed.lock().unwrap().remove(&T::KEY);
        self.inner.dirty.lock().unwrap().insert(T::KEY);
        self.inner.dirty_cv.notify_all();
        self.inner.broadcast(&new_snap);
    }

    /// 拖拽预览通道（001-1 §2）：同 [`set`](Self::set) 的内存换新与广播，
    /// 但**不标脏**——连续 MouseMove 不产生写盘；松手后
    /// [`save_now`](Self::save_now) 把预览过的 section 转 dirty 落盘。
    pub fn set_preview<T: Section>(&self, f: impl FnOnce(&mut T)) {
        let cur = self.get::<T>();
        let mut t = (*cur).clone();
        f(&mut t);
        t.validate();
        let json = serde_json::to_value(&t).expect("section 序列化");
        let new_snap = {
            let mut guard = self.inner.snap.write().unwrap();
            let mut s = guard.clone_shallow();
            s.sections.insert(T::KEY, Arc::new(t));
            s.section_json.insert(T::KEY, json);
            let arc = Arc::new(s);
            *guard = Arc::clone(&arc);
            arc
        };
        self.inner.previewed.lock().unwrap().insert(T::KEY);
        self.inner.broadcast(&new_snap);
    }

    /// L2/L3 层覆盖（S1 仅供测试与占位；L2 项目段 / L3 会话覆盖在 S2+/S4
    /// 接线）。`Value::Null` = 移除覆盖。只换内存并广播，**永不回写文件**。
    pub fn set_layer(&self, layer: Layer, key: &'static str, value: Value) {
        let idx = match layer {
            Layer::Global => return, // L1 走 set() 写通道，不走覆盖表
            Layer::Project => 0,
            Layer::Session => 1,
        };
        {
            let mut ov = self.inner.overrides.lock().unwrap();
            match value {
                Value::Null => {
                    ov[idx].remove(key);
                }
                v => {
                    ov[idx].insert(key.to_string(), v);
                }
            }
        }
        self.inner.reload_memory_only();
    }

    /// 同步落盘当前全部未写改动（退出 / 切项目 / 切会话 / 迁移 / 测试）。
    /// 返回最后一个写错误（无脏或全成功 = `Ok`）。
    pub fn flush(&self) -> Result<(), String> {
        let _guard = self.inner.write_lock.lock().unwrap();
        self.inner.flush_locked()
    }

    /// 立刻落盘：预览过的 section 转 dirty + 同步 flush（迁移/测试/拖拽松手）。
    pub fn save_now(&self) -> Result<(), String> {
        {
            let mut pv = self.inner.previewed.lock().unwrap();
            if !pv.is_empty() {
                let mut d = self.inner.dirty.lock().unwrap();
                for k in pv.drain() {
                    d.insert(k);
                }
            }
        }
        self.flush()
    }

    // ------------------------------------------------------------------
    // 外部改动
    // ------------------------------------------------------------------

    /// 判定并处理一次外部改动（watcher 线程调用；测试直接调）。
    /// 返回是否换上了**新快照**（自写回声 / 同内容 / 坏档 = false）。
    pub fn check_external_change(&self) -> bool {
        self.inner.check_external_change()
    }
}

impl StoreInner {
    /// 判定并处理一次外部改动：哈希比对（回声窗口 / 同内容短路）→ 重载广播。
    fn check_external_change(&self) -> bool {
        let Some(path) = self.path.as_ref() else {
            return false;
        };
        let Some(h) = std::fs::read_to_string(path)
            .ok()
            .map(|t| content_hash(t.as_bytes()))
        else {
            // 读不到（被删/替换中）：不当外部改动；下次本进程写盘会重建
            return false;
        };
        {
            let mut w = self.watch.lock().unwrap();
            let now = Instant::now();
            while let Some((_, at)) = w.expected.front() {
                if now.duration_since(*at) > ECHO_WINDOW {
                    w.expected.pop_front();
                } else {
                    break;
                }
            }
            if w.expected.iter().any(|(eh, _)| *eh == h) {
                return false; // 自写回声（预期哈希窗口）
            }
            if w.last_seen == h {
                return false; // 内容没变（编辑器一串事件 / touch）
            }
            w.last_seen = h;
            // 多实例忙提示（001-0 §5 规则 3）：S1 只日志，状态栏提示留接口
            if !self.dirty.lock().unwrap().is_empty() {
                eprintln!(
                    "[settings] 另一写入者修改了 app-settings.json，本进程有未落盘改动（随后写盘将以本进程为准）"
                );
            }
        }
        self.reload()
    }

    fn broadcast(&self, snap: &Arc<Snapshot>) {
        let subs = self.subs.lock().unwrap();
        for f in subs.iter() {
            f(snap);
        }
    }

    /// 重建快照并广播（overrides / 外部重载路径）。
    /// 坏整份 = 保持旧快照 + problem（返回 false）；坏段 = 该段 Default + problem。
    fn reload(&self) -> bool {
        match self.build_snapshot() {
            Ok(s) => {
                let arc = Arc::new(s);
                *self.snap.write().unwrap() = Arc::clone(&arc);
                self.reloads.fetch_add(1, Ordering::Relaxed);
                self.update_last_seen();
                self.broadcast(&arc);
                // 001-S3：外部改动落进快照 → 投影跟进（同 payload 自动跳过）
                if let Err(e) = self.export_projection() {
                    eprintln!("[settings] 投影写盘失败（下次重载重试）: {e}");
                }
                true
            }
            Err(e) => {
                self.push_problem("<doc>", &e);
                false
            }
        }
    }

    /// overrides 变更后的重建（build_snapshot 内部以「未落盘改动以内存为准」
    /// 保护，重读盘不会回退用户意图）。
    fn reload_memory_only(&self) {
        self.reload();
    }
}

impl StoreInner {
    /// 从盘面（或 None = 纯内存）构建快照。整份非法 → `Err`。
    fn build_snapshot(&self) -> Result<Snapshot, String> {
        let doc = match self.path.as_ref() {
            Some(p) => source::read_doc(p),
            None => source::Doc::Missing,
        };
        match doc {
            source::Doc::Broken => {
                Err("app-settings.json 非法（非 JSON 对象），保持上一份快照".into())
            }
            source::Doc::Missing => Ok(self.build_snapshot_from(None)),
            source::Doc::Ok(map) => Ok(self.build_snapshot_from(Some(&map))),
        }
    }

    fn build_snapshot_from(&self, map: Option<&serde_json::Map<String, Value>>) -> Snapshot {
        let empty = serde_json::Map::new();
        let map = map.unwrap_or(&empty);
        // 锁序约定：先 snap 后 dirty（set() 同序），防交叉死锁
        let cur_json: HashMap<&'static str, Value> = {
            let snap = self.snap.read().unwrap();
            snap.section_json.clone()
        };
        let problems = self.problems.lock().unwrap().clone();
        let overrides = self.overrides.lock().unwrap();
        let dirty = self.dirty.lock().unwrap();

        let mut sections = HashMap::new();
        let mut section_json = HashMap::new();
        let mut origins = HashMap::new();
        for def in sections::registered() {
            let key = def.key;
            let mut l1 = source::layer1_for(key, map);
            let origin = if map.contains_key(key) {
                Origin::Section
            } else if l1.is_some() {
                Origin::Legacy
            } else {
                Origin::Absent
            };
            origins.insert(key, origin);
            // boot-only 段（dev_flags）：boot 值压过文件层（它不在 app-settings）
            if let Some(b) = self.boot.get(key) {
                l1 = Some(b.clone());
            }
            // 本进程有未落盘改动：以内存为准——外部重载 / 层覆盖不得回退用户意图
            if dirty.contains(key) {
                if let Some(cur) = cur_json.get(key) {
                    l1 = Some(cur.clone());
                }
            }
            let layers = [
                None,
                l1,
                overrides[0].get(key).cloned(),
                overrides[1].get(key).cloned(),
            ];
            match sections::build(key, &layers) {
                Ok((arc, json)) => {
                    sections.insert(key, arc);
                    section_json.insert(key, json);
                }
                Err(e) => {
                    // 单段坏：该段 Default，其它段正常（001-3 §3）；日志留底
                    self.push_problem(key, &e);
                    let (arc, json) = sections::build_default(key);
                    sections.insert(key, arc);
                    section_json.insert(key, json);
                }
            }
        }
        Snapshot {
            sections,
            section_json,
            origins,
            problems: Arc::from(problems),
        }
    }

    /// 持 write_lock 调用（flush / 写者线程）。失败键回填 dirty 由下轮重试。
    fn flush_locked(&self) -> Result<(), String> {
        let keys: Vec<&'static str> = {
            let mut d = self.dirty.lock().unwrap();
            d.drain().collect()
        };
        if keys.is_empty() {
            return Ok(());
        }
        let t0 = Instant::now();
        let mut wrote = false;
        let mut first_err = None;
        for key in keys {
            match self.flush_section(key) {
                Ok(true) => wrote = true,
                Ok(false) => {}
                Err(e) => {
                    self.write_failures.fetch_add(1, Ordering::Relaxed);
                    // 内存保留（快照未动 = 用户意图不丢），退避重试
                    self.dirty.lock().unwrap().insert(key);
                    eprintln!("[settings] 写盘失败（内存保留，将重试）{key}: {e}");
                    if first_err.is_none() {
                        first_err = Some(e);
                    }
                }
            }
        }
        if wrote {
            perf_log("settings.flush", t0.elapsed());
            // 001-S3：段内容变了 → 投影跟进（同 payload 自动跳过）
            if let Err(e) = self.export_projection() {
                eprintln!("[settings] 投影写盘失败（下次写盘重试）: {e}");
            }
        }
        match first_err {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// 单个 section 的读-改-写落盘（doc.rs 管线），成功写后登记回声窗口。
    /// 传整个快照：首次写（v1 文档）触发整份升级（version:2 + 全 section）。
    fn flush_section(&self, key: &'static str) -> Result<bool, String> {
        let Some(path) = self.path.as_ref() else {
            return Ok(false);
        };
        let snap = Arc::clone(&self.snap.read().unwrap());
        if !snap.section_json.contains_key(key) {
            return Err(format!("{key} 不在快照"));
        }
        let written = doc::write_section(path, key, &snap)?;
        if written {
            self.writes.fetch_add(1, Ordering::Relaxed);
            if let Ok(mut w) = self.watch.lock() {
                let now = Instant::now();
                while let Some((_, at)) = w.expected.front() {
                    if now.duration_since(*at) > ECHO_WINDOW {
                        w.expected.pop_front();
                    } else {
                        break;
                    }
                }
                let h = std::fs::read_to_string(path)
                    .map(|t| content_hash(t.as_bytes()))
                    .unwrap_or(0);
                w.expected.push_back((h, now));
                w.last_seen = h;
            }
        }
        Ok(written)
    }

    /// 投影落盘（001-2）：exportable 白名单段 → ext-context.json，
    /// 同 payload 跳过。settings 文件缺失（纯内存态）= 无投影。
    fn export_projection(&self) -> Result<bool, String> {
        let Some(dir) = self.path.as_ref().and_then(|p| p.parent()) else {
            return Ok(false);
        };
        let snap = Arc::clone(&self.snap.read().unwrap());
        let mut last = self.projection_payload.lock().unwrap();
        let wrote = export::export(&snap, dir, &mut last)?;
        if wrote {
            self.exports.fetch_add(1, Ordering::Relaxed);
        }
        Ok(wrote)
    }

    /// 盘上当前内容哈希 → last_seen（open / reload / 写后对齐用）。
    fn update_last_seen(&self) {
        if let Some(path) = self.path.as_ref() {
            if let Ok(text) = std::fs::read_to_string(path) {
                if let Ok(mut w) = self.watch.lock() {
                    w.last_seen = content_hash(text.as_bytes());
                }
            }
        }
    }

    fn push_problem(&self, key: &str, detail: &str) {
        let mut log = self.problems.lock().unwrap();
        log_problem(&mut log, key, detail);
    }
}

fn log_problem(log: &mut Vec<SettingProblem>, key: &str, detail: &str) {
    eprintln!("[settings] problem {key}: {detail}");
    log.push(SettingProblem {
        key: key.to_string(),
        detail: detail.to_string(),
        at: Instant::now(),
    });
    if log.len() > PROBLEM_CAP {
        let drop_n = log.len() - PROBLEM_CAP;
        log.drain(0..drop_n);
    }
}

/// 单写者线程：脏队列等待 → coalesce 静默期 → 抽干落盘；失败退避重试。
fn spawn_writer(inner: Arc<StoreInner>) {
    let _ = std::thread::Builder::new()
        .name("pf-settings-writer".into())
        .spawn(move || {
            loop {
                {
                    let mut d = inner.dirty.lock().unwrap();
                    while d.is_empty() {
                        d = inner.dirty_cv.wait(d).unwrap();
                    }
                }
                std::thread::sleep(COALESCE);
                let failed = {
                    let _guard = inner.write_lock.lock().unwrap();
                    inner.flush_locked().is_err()
                };
                let mut backoff = inner.fail_backoff.lock().unwrap();
                if failed {
                    std::thread::sleep(*backoff);
                    *backoff = (*backoff * 2).min(FAIL_BACKOFF_CAP);
                } else {
                    *backoff = FAIL_BACKOFF_START;
                }
            }
        });
}

/// FNV-1a（非加密，只做内容变更指纹）。
fn content_hash(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// `[perf]` 打点：`PI_FLASH_PERF=1` 门控（app main.rs 同约定）。
fn perf_log(what: &str, d: Duration) {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if *ON.get_or_init(|| std::env::var("PI_FLASH_PERF").is_ok()) {
        eprintln!("[perf] {what}={}µs", d.as_micros());
    }
}
