# 042-1 开发计划 · 模型管理对齐 pi-web（凭据进钥匙库）

上游设计：`docs/模块设计/042-1-模型管理理清对齐.md`（§1 删除清单 / §2 不变量 / §8 三处禁改 / §9 决策记录）。
本文件是**执行清单**：每步给「改哪些文件 / 增删哪些符号 / 怎么验」。**实现不得偏离本文件；要偏离先改本文件并复核。**

---

## 0 规则（先看这段，违反即打回）

### 0.1 禁改区（一行都不许动）

| 位置 | 禁改范围 |
|---|---|
| `settings/custom_models.rs::mj_provider_editor` + `mj_enabled_section` | 布局、组件、控件顺序、文案键 |
| `settings/custom_models.rs::mj_model_modal_overlay` | 同上（500×500 弹窗结构） |
| `settings/custom_models.rs::mj_catalog_fill` / `mj_catalog_apply` / `pi_link::openrouter` | 数据源、覆盖字段集、成功不占提示栏 |

**唯一允许的改动**：`mj_doc()` / `mj_write()` 两个函数体换读写目标（`providers.json` → `models.json`），以及为编译通过而做的**签名不变**的调用点适配。

### 0.2 不变量

1. 用户自己写进 `auth.json` / `models.json` 的内容：**不看、不改、不删**（非 PF 标记一律只读）。
2. PF 保存的 apikey 只进系统钥匙库；盘上永不出现明文。
3. PF 只写/只删**带自己标记**的条目。标记判据见 §2.2。

### 0.3 变更控制

- 任何"顺手优化"（改名、重排、去重、抽公共件、调 UI）**一律禁止**；发现必要→先停手报告。
- 每个里程碑 = 一次 `but commit`，能单独回滚；里程碑内不混入下一里程碑的改动。
- 每步提交前必须：`cargo build -p app` 绿 + `cargo test -p pi-link` 绿。

---

## 1 目标形态

```
凭据：keyring（唯一秘密） ──pf-secret──▶ auth.json {"p":{"type":"api_key","key":"!<abs>/pif-secret get p"}}
模型：models.json（唯一 provider/模型真相源，PF 整份草稿写 + 不可读拒写）
目录：get_available_models（进程答案）优先，冷启动回落 models-store.json + models.json
删除：pf-auth.json / providers.json / registry-cache.json / probe 端点表 / models_dev.rs / -e 注入
```

---

## 2 规格（照抄，不许自由发挥）

### 2.1 文件锁（`crates/pi-link/src/fs_lock.rs`，新增）

复刻 `proper-lockfile` 协议，与 pi / pi-web 共用同一把锁：

- 锁对象：`<target>.lock` **目录**（`create_dir` 成功即持锁）。
- 重试：最多 10 次，`minTimeout 100ms`、`factor 2`、`maxTimeout 10s`、±randomize。
- stale：锁目录 mtime 超 `30s` 视为孤儿 → 删除后重试一次。
- 持锁期间每 `stale/2`（15s）触摸一次 mtime（长任务才有意义；PF 写入应远小于此）。
- 释放：删锁目录（仅当自己创建）。
- API（签名固定）：
  ```rust
  pub struct FileLock;                       // 已持锁守卫，Drop 释放
  pub fn lock(path: &Path, timeout: Duration) -> Result<FileLock, String>;
  ```

### 2.2 `auth.json` 读写（`crates/pi-link/src/auth_json.rs`，新增；`config.rs` 里同名旧函数退役）

- 路径：`pi_link::config::auth_path()`。
- **标记判据**：`credential.type == "api_key"` 且 `credential.key` 以 `"!"` 开头且包含 `"pif-secret get "`。
- 写入：锁内 `read → mutate → write_json_private`（0600）。
- API（签名固定）：
  ```rust
  pub fn marker_key(provider: &str) -> String;              // "!<abs>/pif-secret get <provider>"
  pub fn is_pf_managed(cred: &Value) -> bool;
  pub fn kinds() -> Result<Vec<(String, CredentialKind)>, String>;   // 供 UI「已配置」判定
  pub fn read_credential(provider: &str) -> Result<Option<Value>, String>;
  pub fn store_marker(provider: &str) -> Result<(), String>;        // 锁内写标记条目
  pub fn remove_marker(provider: &str) -> Result<bool, String>;     // 仅删标记条目；非标记→Ok(false)
  pub fn seal_plaintext(provider: &str, secret: &str) -> Result<bool, String>; // 迁移器用（§2.5）
  ```
- 硬规则：任何函数都不得删除/改写**非标记**条目（OAuth 与用户明文一律原样透传）。
- 旧的 `config::set_api_key` / `remove_credential_if_api_key` 标注 `#[deprecated]` 并在 M7 删除。

### 2.3 `models.json` 读写（`crates/pi-link/src/models_json.rs` 扩展）

- 已有变更助手全部保留：`providers / provider_entry / provider_models / upsert_provider / rename_provider / remove_provider / add_model / update_model / remove_model / ensure_model_defaults`。
- 新增（签名固定）：
  ```rust
  pub fn save_at_locked(path: &Path, doc: &Value) -> Result<(), String>;
  pub fn save(doc: &Value) -> Result<(), String>;
  ```
- **不可读拒写**：`read_at` 失败（JSON 坏/BOM 以外的语法错）时 `save*` 必须返回 Err，**绝不覆盖**（pi-web `ModelsConfigReadError` 同款）。
- 写入前跑一遍 `ensure_model_defaults`（保持现有"缺省形状补齐"行为）。
- `input` 只允许 `text`/`image`（pi `MODEL_INPUT_MODALITIES`）；写入前校验，出现其他值直接 Err。

### 2.4 `pif-secret` helper（`crates/pi-link/src/bin/pif-secret.rs`，新增 bin）

```
pif-secret get <provider>    # stdout 仅 secret，无换行以外的装饰
```
- 读 `KeyringVault` 条目名 = `credentials::env_var_name(provider)`。
- 成功 exit 0；缺失/失败 exit 1（pi 侧按"未配置"处理，不会误发）。
- stdout **只输出秘密**（`print!` 不用 `println!` 也可，pi 会 trim）。
- 随安装分发到安装目录；`auth.json` 里写**绝对路径**，路径分隔符用 `/`，整体用单引号包（pi 在 Windows 走 `bash -c`，`sh -c` 也兼容单引号）。
- 启动校验（§2.6）负责发现"路径失效"。

### 2.5 迁移器（`crates/pi-link/src/migrate_0421.rs`，新增；启动后台调一次）

| 输入 | 动作 | 幂等 |
|---|---|---|
| `providers.json` 存在 | 逐商 `upsert_provider` 进 `models.json`（同名以 `models.json` 现有为准，不覆盖）；凭据（`$PF_KEY_*` 引用，钥匙库可解）→ `seal_plaintext` 写标记条目；完成后 `providers.json` 改名 `.bak` | 是（`.bak` 存在即跳过） |
| `pf-auth.json` 存在 | 逐商：`KeyringVault.get(env_var_name)` 成功 → `store_marker`；失败跳过；文件改名 `.bak` | 是 |
| `auth.json` 用户明文条目 | **不动**（规则 1）；仅在设置页提供手动【收进钥匙库】 | — |
| `models_dev-cache.json` / `registry-cache.json` | 直接删（纯缓存） | 是 |

- 每条改写后自校验：`vendor::pi_auth_check(provider)`（复用 `pi auth check --provider X --json`）不 ready → 回滚该条并记日志，不中断整体。

### 2.6 启动校验与目录单源

- 启动（后台线程）：
  1. 跑迁移器（一次性）。
  2. 校验每个**标记条目**：helper 文件存在？钥匙库条目在？任一缺 → 记一条 `mc_cred_warnings`（设置页详情区显示文案），不阻塞。
- 目录：`catalog::disk_models()` 去掉 `providers.json` 源（只留 `models.json` + `models-store.json`）；RPC 答案合并逻辑不变（`startup::merge_models`）。

---

## 3 里程碑

### M1 · 文件层（无 UI 变化，可独立验收）

- 新增：`pi-link/src/fs_lock.rs`、`pi-link/src/auth_json.rs`、`pi-link/src/bin/pif-secret.rs`；`models_json.rs` 加 `save_at_locked/save`。
- 修改：`pi-link/src/lib.rs` 挂模块；`pi-link/Cargo.toml` 加 `[[bin]] pif-secret`。
- 验收：
  - `cargo test -p pi-link` 绿，且新增单测 8 条：
    1. `lock` 互斥（第二把在 100ms 内失败）；2. 锁目录 stale 30s 被接管；3. 释放后目录消失；
    4. `is_pf_managed` 判据（标记/明文/oauth 三态）；5. `remove_marker` 遇明文返回 false 且文件字节不变；
    6. `models_json::save` 在坏文件上 Err 且文件字节不变；7. `input:["video"]` 写入 Err；8. `ensure_model_defaults` 补齐后 `save` 幂等。
  - `pif-secret get <不存在>` exit 1；`get <已存>` 输出等于钥匙库值（用 `MemVault` 注入测）。
- 提交：`feat(042-1): pi-link 文件层——proper-lockfile 复刻 + auth.json/models.json 锁内读写 + pif-secret helper`

### M2 · 凭据链路切换（设置页行为不变，只换底座）

- 修改 `settings/models.rs`：
  - `mc_save_key`：`pf_auth::store_catalog_key` → ①`KeyringVault.set` + 读回校验 ②`auth_json::store_marker` ③失败回滚钥匙库。
  - `mc_configured`：`pf_auth::kinds()` → `auth_json::kinds()`。
  - `mc_select_provider`：已存 key 预填 → `KeyringVault.get`（标记条目）/ 明文条目直接读（只读）。
  - `mc_remove_provider_close`：`pf_auth::delete_catalog_key` → `auth_json::remove_marker` + `KeyringVault.delete`；`removed_providers` 墓碑**删除**（改为：移除 = 删凭据 + 从 `models.json` 删该商）。
  - `apply_pattern_edit` / `mc_toggle_*` 不变（仍写 `settings.json` 的 `enabledModels`）。
- 修改 `settings/custom_models.rs`：`mj_store_key` 同样改走钥匙库 + 标记条目（**不写** `models.json.apiKey`）。
- 修改 `settings/mod.rs` + `shell/chat.rs`：`mc_creds` 语义不变（数据源换 `auth_json::kinds`）。
- 验收：
  - 详情页保存 key 后 `auth.json` 出现标记条目、`models.json` 无 `apiKey`、钥匙库有条目；删 key 三处同步清。
  - 用户手写明文条目存在时，PF【保存】覆盖该商条目**不弹确认**（决策 §9-4）；PF【移除】遇到非标记条目 → 不删并提示。
  - pif-ui：`settings.model.save_key` 后快照 `model_rows` 与「已配置」状态一致。
- 提交：`feat(042-1): 凭据链路切到 auth.json 标记条目 + 钥匙库`

### M3 · 模型/provider 数据源切换（三处禁改区只换底层）

- 修改 `settings/custom_models.rs`：**只改两个函数体**
  - `mj_doc()`：`pf_providers::read()` → `models_json::read()`。
  - `mj_write(doc)`：`pf_providers::write` → `models_json::save`（含不可读拒写）。
  - 连带：`mj_speedtest_start` / `mj_fetch_models` / `mj_delete_provider` / `mj_apply_provider` 里对 `pf_providers::*` 的调用换成 `models_json::*` 等价物（签名不变）。
- 修改 `settings/models.rs`：`ledger_models` / `catalog_with_ledger` 删除（自定义商来自 `catalog_for`，因为 `models.json` 已在磁盘源里）。
- 验收：
  - 新增自定义商 → `models.json` 出现条目，pi 会话能看到该商模型（`get_available_models` 含之）。
  - 详情页/模型弹窗/自动填参**逐像素行为不变**（pif-ui 截图对比 + 交互脚本回归）。
  - `models.json` 手工写坏 → 保存被拒，文件字节不变，界面给错误条。
- 提交：`feat(042-1): 自定义 provider/模型改直写 models.json（UI 零改动）`

### M4 · 目录单源

- 删除：`pi-link/src/registry.rs`、`pi-link/src/probe.rs`、`pi-link/src/models_dev.rs` 及 `paths.rs` 对应缓存路径函数。
- 修改：
  - `session/model_picker.rs`：`ensure_registry` 删除；弹窗数据源 = SDK 全量清单改为**只用 `catalog_for`**（已配置商过滤逻辑保留在渲染层）。
  - `dialogs.rs::render_provider_picker`：去掉 `pf_providers` 与 `registry::RegistryDump::oauth_display_name` 依赖 → 显示名回落 `ui::provider_display_name`。
  - `settings/models.rs`：`merge_registry_models` 删除；【获取模型】改走「按 `models.json` 的 `baseUrl`+`api` 推导端点」的单一函数（复用 `openrouter.rs` 的网络层，**不新建 probe 表**）。
  - `shell/chat.rs`：删 `provider_registry` 字段及全部读点。
- 验收：全仓 `grep -r "registry::\|probe::\|models_dev"` 为空（除 git 历史）；新增 Provider 弹窗、模型菜单、设置页照常工作；pif-ui 全量快照回归。
- 提交：`refactor(042-1): 模型目录收敛为单一来源（删 registry dump / probe 表 / models_dev）`

### M5 · spawn 与注入退役

- 修改 `pi-link/src/client.rs`：删 `spawn_env_at` 调用、删 `-e pf-providers.mjs` 挂载。
- 修改 `pi-link/src/credentials.rs`：删 `spawn_env_at`、`official_env_name`、`pf_ref`、`classify`（保留 `SecretVault/KeyringVault/MemVault/FailVault/env_var_name`）。
- 删除文件：`pi-link/src/pf_auth.rs`、`pi-link/src/pf_providers.rs`。
- 验收：spawn 参数里不再有 `-e`；`pf-providers.mjs` 不再生成；会话内自定义商仍可用（凭据靠 `auth.json` 标记 + helper）；`cargo test -p pi-link` 绿。
- 提交：`refactor(042-1): 退役 env 注入与 pf-providers 扩展挂载`

### M6 · 迁移器 + 启动校验

- 新增 `pi-link/src/migrate_0421.rs`（§2.5 表）；`startup.rs` 后台线程调用 + 写 `mc_cred_warnings`。
- 修改 `startup.rs`：删 117–180 行的 `ensure_extension_template_or_warn` / `migrate_infer_api` / `migrate_model_defaults` / `adopt_from_models_json_at` / `adopt_from_auth_at` / `delete_catalog_key` 段。
- 验收：
  - 造 fixture：`providers.json` 2 商 + `pf-auth.json` 2 商 + 明文 `models.json` → 启动后 `models.json` 有 2 商、`auth.json` 有 2 标记条目、两个旧文件变 `.bak`、**用户手写条目字节不变**。
  - 重复启动：无二次改动（幂等）。
  - helper 被删 → 启动后设置页出现"凭据失效"提示。
- 提交：`feat(042-1): 042 账本一次性迁移 + 凭据/helper 启动校验`

### M7 · 收尾

- 删 `config.rs` 的 `CredentialKind` 旧用法注释、`set_api_key`、`remove_credential_if_api_key`（若已无调用点）。
- `AGENTS.md` 红线改口径（决策 §9-7）；`042-模型管理.md` 标注"已被 042-1 取代"；`docs/buglist.md` / `progress.md` 同步。
- i18n：新增键（`models.cred_helper_missing`、`models.cred_vault_missing`、`models.migrate_done`、`models.json_write_refused`），三语齐。
- 验收：`cargo build --release` 绿；`cargo test -p pi-link` 全绿；`pif-ui` 冒烟（启动 → 设置-模型 → 保存 key → 开新会话确认可用）。
- 提交：`chore(042-1): 收尾——红线改口径/文档同步/i18n`

---

## 4 测试清单（汇总）

**pi-link 单测（新增）**：锁协议 3 条 / auth.json 标记 3 条 / models.json 拒写+模态校验 3 条 / helper 2 条 / 迁移器 4 条（含"用户条目不变"）。

**pif-ui 断言**：
- `settings.models` 列表三档排序不变（模型行数、Provider 顺序）
- 保存 key → 已配置状态翻转 + `auth.json` 标记条目存在（shell 断言文件）
- 自定义商详情页控件树与改动前 `snapshot` 逐键一致
- 模型详情弹窗控件树一致；自动填参一次 → `models.json` 该条 `contextWindow/maxTokens/input/reasoning` 被覆盖、`cost` 只在空时填

**手工验收（一次性）**：
1. 全新环境：加自定义商 → 会话可用 → 关 PF → 重启 → 仍可用。
2. 钥匙库删条目 → 重启 → 该商"未配置" + 设置页提示。
3. 用户手写明文 `auth.json` 条目 + PF 也给它存 key → PF 覆盖（无确认）；PF 移除 → 只清自己那条。

---

## 5 风险与回滚

| 风险 | 处置 |
|---|---|
| 锁协议实现偏差 → 与 pi 并发写坏 `auth.json` | M1 单测钉死 mkdir/stale/释放；写入一律"锁内整份读改写" |
| 迁移器把用户条目改坏 | 迁移前后做字节级 diff 断言（只允许目标 provider 键变化）；失败跳过该条不中断 |
| `models.json` schema 拒整份（用户手写非法值） | 读失败即拒写（§2.3），UI 错误条引导用户修文件 |
| helper 路径失效（升级/换安装目录） | 启动校验 + 设置页提示 + 重新保存任一 key 即重写全部标记 |
| 三处禁改区被顺手改动 | 每个里程碑提交前 `git diff` 只允许出现在计划列出的文件/函数内 |

回滚：每个里程碑独立提交，`but unapply <id>` 即回上一绿点；M2–M4 之间不共享半成品状态（M2 完成后系统可正常用，M3 才换模型源）。

---

## 6 顺序与工作量

```
M1 文件层 ──▶ M2 凭据 ──▶ M3 模型源 ──▶ M4 目录单源 ──▶ M5 spawn 退役 ──▶ M6 迁移器 ──▶ M7 收尾
   (0.5d)      (0.5d)      (0.5d)        (0.5d)          (0.2d)          (0.5d)       (0.3d)
```

依赖：M5 必须在 M6 之前（否则旧注入与迁移并存会产生双份凭据）；M3 必须在 M2 之后（自定义商 key 走新链路）；M4 可与 M5 并行。

---

## 7 执行记录与偏离（实现后回填）

已按计划完成 M1–M7。三处**有意偏离**，理由如下：

| 偏离 | 计划原文 | 实际做法 | 理由 |
|---|---|---|---|
| 保留 `registry.rs` | M4 删除 | 保留，但**只给「新增 Provider」弹窗供元数据**（hasApiKeyLogin / hasOAuth / 模型计数 / 凭据类型），不再喂任何模型清单（`merge_registry_models` 已删） | Rust 侧没有 provider 认证声明的来源；删掉会让新装机器的「添加 Provider」列表为空（目录只含 pi 已刷过的商） |
| 保留 `probe.rs` | M4 删除 | 保留 | 它是「自定义 Provider 详情页【获取模型】」与「内置商保存后探查」的实现，属 §8 禁改区的行为；pi 无等价 RPC |
| 迁移自校验 | 用 `pi auth check` 逐条验 | 用 `auth_json::resolve_secret`（标记条目 → 钥匙库） | 逐条起 node 进程在启动路径上（每家 ~0.5s），违背启动热路径铁律；`resolve_secret` 与 pi 取值同一机制 |

另：`pif-secret` 由 cargo 的 `src/bin/` 自动发现，未加显式 `[[bin]]`（等价）。

已验证：
- `cargo test -p pi-link` 212 passed（新增 fs_lock 3 / auth_json 7 / models_json save 5 / migrate 1）。
- `cargo build -p app` 绿（仅剩 1 条历史和模型管理无关的 dead-code warning）。
- 真机（vendored pi 1.4.0）：
  - `auth.json` 写 `!'…/pif-secret.exe' get deepseek` → `pi auth check --provider deepseek --json` = `ready`，取到的正是钥匙库里的真值；
  - helper 不存在 / 钥匙库无条目 → `not_ready`（回落到 provider 的官方 env，无 env 即未配置）——**安全失败，不会把命令串当 key 发出去**。
