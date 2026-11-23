# API Key 管理（系统安全存储 · 凭据库 + auth.json 标记条目）

模块代码：
apikeyManager

对应界面：
1、settings - 模型（provider 详情的 API Key 区、自定义 provider/添加 Provider 的 API Key 字段）

> 042-1 已重构本模块：PF 私有双账本（`pf-auth.json` / `providers.json`）退役，
> 凭据只进系统钥匙库，`auth.json` 写 PF 标记条目（`!pif-secret get <provider>`），
> pi 自己执行 helper 取回。凭据模型详见
> [`042-1-模型管理理清对齐.md`](042-1-模型管理理清对齐.md) §2。

## 铁律

1. `models.json` / `auth.json` 是 pi 的文件，但 042-1 起 PF 会写：整份草稿保存、
   不可读拒写、proper-lockfile 同款锁（`fs_lock.rs`）。
2. 用户手写的明文条目与 pi 的 OAuth 条目一律只读不改、不收编。
3. 明文密钥唯一 rest 归宿 = OS 凭据库（Windows Credential Manager / macOS
   Keychain，`keyring` crate）；盘上永不落明文。

## 核心架构（一句话）

**秘密只存钥匙库，auth.json 只放一行 helper 引用**：保存 = 钥匙库写入（读回
校验）→ 锁内写 `auth.json` 标记条目；pi 请求时执行 `!pif-secret get <provider>`
取回——任何前端（PF / pi TUI / pi-web）都能取到，不依赖 PF 进程在场。

## pi 侧事实（vendored 源码核实）

- pi 的 `AuthStorage.read()` 对 api_key 的 `key` 走 `resolveConfigValue`：
  `!` 前缀 = 执行命令取 stdout 当 key。Windows 走 Git Bash 的 `bash -c`
  （无 Git Bash 回落 `execSync`），都走 PATH 解析、自动补 `.exe`，裸命令名
  即可命中；命令失败（非 0 退出）= 该 provider「未配置」（安全失败）。
- pi 的 `commandResultCache` 是进程级：换 key 后需**重开会话**生效。

## 存储分工

| 内容 | 存哪 | pi 怎么拿到 |
|---|---|---|
| apikey 明文 | OS 凭据库（service `PiFlash`，条目名 `PF_KEY_*`） | 无需给 pi——helper 自己读钥匙库 |
| 引用 | `auth.json` 标记条目 `!pif-secret get <provider>`（PATH 模式）或 `!'…/pif-secret.exe' get <provider>`（回落） | pi 执行该命令 |
| 自定义 provider 定义 | models.json（骨架 + 勾选投影）+ `custom-provider.json` 清单仓 | pi 读 models.json；key 不落其中（见 042-1） |
| 用户手写条目（明文 / `$VAR` / `!cmd` / oauth） | `~/.pi/agent/` pi 地盘 | pi 自己读；PF 只读不改 |

## 目标

1. 磁盘零明文（auth.json 只放 helper 引用）。
2. 凭据库不可用 = 保存失败并报错（不降级明文；key 不写盘）。
3. 登录会话内无密码 UX；helper 由 PATH 解析，PF 不在场也能取回。

## 威胁模型

| 场景 | 效果 |
|---|---|
| 其他本地低权限用户读文件 | 防（用户目录 ACL + 0600） |
| 磁盘失窃 / 备份 / 网盘 / 误提交 | **防**（凭据库不随文件走，文件里只有 helper 引用） |
| 同用户恶意软件 / 内存扫描 | 不防（诚实边界） |

## 设计细节

### 1. pf-auth.json（目录 provider 账本）

已退役（042-1）：目录 provider 的 key 不再记 PF 私有账本，直接写
`auth.json` 标记条目（凭据模型见 042-1 §2）。存量账本由 `migrate_0421`
一次性迁移后改名 `.bak`。

### 2. providers.json（自定义 provider 账本）

已退役（042-1）：自定义 provider 直写 pi 的 models.json（骨架 + 勾选投影），
完整定义存 `~/.pi-flash/custom-provider.json` 清单仓；key 与目录 provider
同一条 auth.json 路径。存量由 `migrate_0421` 一次性迁移（同名 provider 以
models.json 现有为准，不覆盖；用户手写条目一律只读）。

### 3. 扩展注入（pf-providers.mjs）

已退役（042-1）：不再生成 `-e` 扩展、不再 spawn 注入 env——pi 直接读
models.json 与 auth.json，PF 不需要任何注入通道。

### 4. 凭据库条目规范

- service `PiFlash`；条目名 = `env_var_name(provider)`；Windows Credential
  Manager / macOS Keychain（keyring v3，`windows-native`/`apple-native` features）。

### 5. 变量名规范

- `PF_KEY_<大写净化provider>`，非 `[A-Za-z0-9]` 字符净化为 `_` 且整体追加 6 位 FNV 短哈希（`my-gw` → `PF_KEY_MY_GW_XXXXXX`，`a-b`/`a.b` 不冲突）；纯字母数字 id 保持干净（`deepseek` → `PF_KEY_DEEPSEEK`）。
- `PF_KEY_` 前缀 PF 保留（`pf_ref` 只认 `PF_KEY_` + 大写/数字/下划线形态）；用户手写的其他 `$VAR` / `!cmd` 原样留在盘上、不迁移。

### 6. 保存 / 替换 / 断开 / 改名

- 保存（目录商与自定义商同一条链，`auth_json::store_secret`）：钥匙库写入 +
  读回校验 → PATH 模式未决时先同步 `register_to_sys::ensure_installed` 一次
  （幂等毫秒级，让新标记直接落零路径裸形态）→ 锁内写 `auth.json` 标记条目；
  任一步失败回滚钥匙库并报错。覆盖同商旧条目（含用户手写明文——那是用户在
  该商详情页的显式保存意图）。空 key 拒绝保存。
- 断开（内置商详情【移除】，确认后）：`auth_json::delete_api_key` 删该商
  api_key 条目（**不论谁写的**）+ 钥匙库条目（幂等）；oauth 不动；models.json
  不碰（列表成员资格靠真实状态，凭据没了商家自然出列）。内部保守变体
  `delete_secret`/`remove_marker` 只删 PF 标记条目（改名搬家等路径用）。
- 自定义 provider 改名：钥匙库条目 + auth.json 标记条目跟名搬家
  （`resolve_secret(old)` → `store_secret(new)` → `delete_secret(old)`，失败
  静默跳过）；手写条目不搬——它的 key 在 models.json 里随条目走，不得把
  用户明文收进凭据库。
- 显示：目录与自定义商表单同规——PF 标记条目从钥匙库解出掩码预填（眼睛可
  复核显隐）；用户明文条目展示掩码；解不出（库缺条目）留空；`$VAR`/`!cmd`
  引用不执行不显示。

### 7. 迁移（一次性，幂等）

- 启动（`migrate_0421::run`，先于任何会话 spawn，失败只记日志不阻塞）：
  PF 私有账本 → pi 的两个文件。`providers.json` 的自定义条目进 models.json
  （provider 不存在时才新增）、`$PF_KEY_*` 引用转 auth.json 标记条目（秘密
  本来就在钥匙库）；`pf-auth.json` 同理。旧文件改名 `.bak` 防重跑；用户手写
  条目一律只读；顺带清理退役缓存（provider-catalog.json 等）。
- 标记去路径化（`auth_json::migrate_markers_pathless`，启动后台 + 保存时
  同步 ensure）：PATH 模式就绪后把绝对路径形态的 PF 标记条目改写为裸命令
  形态（绝对路径随版本化载荷目录漂移，升级即失效——裸命令永不失效）；
  oauth / 用户明文 / 用户引用不动，幂等。

### 8. 行为约束（UI 已提示）

- 换 key 需重启会话生效（pi 的命令结果按进程缓存）。
- OAuth 归 pi：PF 模型页不管理 OAuth 登录态（订阅组卡片点击仅提示未接入）。
- 跨机 / 远程使用 pi-web 时，PF 管理的商显示「未配置」（钥匙库与 helper
  不在场）。

## 代码落点

| 文件 | 职责 |
|---|---|
| `pi-link/src/credentials.rs` | SecretVault trait + KeyringVault/MemVault/FailVault；`$PF_KEY_*` 识别（pf_ref）、形态分类（classify）、env_var_name（FNV 短哈希防撞） |
| `pi-link/src/auth_json.rs`（+ `auth_json/`） | auth.json 的 PF 写入视图：标记判据/保存/移除/明文收编（seal_plaintext）/标记去路径化迁移；锁内整份读改写、不可读拒写 |
| `pi-link/src/register_to_sys.rs` | pif-secret 用户级稳定安装：`%LOCALAPPDATA%\pi-flash\bin` + HKCU Path **首位**段（判重、保原类型、WM_SETTINGCHANGE 广播、本进程 PATH 同步），全幂等；另挂全局 `pi` shim（081） |
| `pi-link/src/bin/pif-secret.rs` | helper：`pif-secret get <provider>` → 钥匙库取值打 stdout；非 0 退出 = pi 判「未配置」 |
| `pi-link/src/migrate_0421.rs` | 一次性迁移：PF 私有账本 → models.json + auth.json 标记条目（旧文件 .bak、幂等、自校验走 resolve_secret） |
| `pi-link/src/fs_lock.rs` | proper-lockfile 同款锁（与 pi 的 AuthStorage 共用一把锁） |
| `pi-link/src/models_json.rs` | models.json 写入视图（normalize / validate / save，锁内、不可读拒写） |
| `app/src/settings/models/actions.rs` | 目录商保存（`mc_save_key` → store_secret → 保存后连通探查） |
| `app/src/settings/custom_models/apply.rs` | 自定义商 key 分流（`mj_store_key`）+ 改名搬家 + 删除清凭据 |
| `app/src/startup.rs`、`startup/pumps.rs` | boot：migrate_0421；后台：ensure_installed + 标记去路径化迁移 |

## 测试

- pi-link 单测：标记判据三形态、保存/移除往返、用户明文与 oauth 零字节不碰、
  坏文件拒写、0600/0700、锁协议（独占 + stale 接管）、变量名确定性/防撞、
  迁移幂等（去路径化/账本迁移只认 PF 前缀引用）。
- 离线用例：`test/042-1/marker-pathless/`、`test/042-1/pif-secret-packaging/`。
