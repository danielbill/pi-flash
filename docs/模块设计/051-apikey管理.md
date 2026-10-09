# API Key 管理（系统安全存储）

模块代码：
apikeyManager

对应界面：
1、settings - 模型（provider 详情的 API Key 区）

## 背景与现状

- API key 明文存 `~/.pi/agent/auth.json`（`{ "<provider>": { "type": "api_key", "key": "..." } }`），与 pi 共用；f61d2a7 已保证写入不放宽 0600 权限。
- pi 原生支持 key 值三种形态（resolve-config-value.js）：
  1. 明文字符串；
  2. `$ENV_VAR` / `${ENV_VAR}` 模板——发请求时从 **pi 子进程的环境变量**取值，取不到则 key 未解析；
  3. `!command` 间接引用（本设计不使用）。
- pi 子进程由 PF spawn，唯一 spawn 点：`crates/pi-link/src/client.rs`（`node cli.js --mode rpc`）。

## 目标

1. 明文 key 只存在于三处：PF 内存、OS 凭据库、pi 子进程环境变量——**磁盘上零明文**。
2. auth.json 只放变量名（`$PF_KEY_*`），继续与 pi 共用，格式仍是 pi 原生模板形态，不引入私有魔法。
3. 登录会话内无密码 UX：凭据随 OS 登录自动解锁，不新增任何密码。
4. 存量明文自动迁移；凭据库不可用时自动降级回文件存储，功能不中断。

## 非目标

- 防以当前用户身份运行的恶意软件——任何本地方案都防不了（它能像 PF 一样解密），诚实边界。
- 多设备同步 / 团队共享密钥。
- 管理 OAuth token：仍由 pi 写 auth.json（type: oauth），PF 不碰。
- 给裸跑 pi CLI 供 key：PF 管理的 key 只保证 PF 拉起的会话可用（env 在 spawn 时注入）；用户在终端裸跑 pi 需自行 export 同名变量。用户手写的明文 / `$其他VAR` / `!cmd` 完全不受影响。

## 威胁模型

| 场景 | 现状（明文 0600） | 本设计 |
|---|---|---|
| 其他本地低权限用户读文件 | 防（ACL） | 防（ACL） |
| 磁盘失窃 / 备份 / 网盘同步 / 误提交 git | **不防**（文件里就是明文） | **防**（凭据库不随文件走，auth.json 只有变量名） |
| 同用户恶意软件 | 不防 | 不防（边界同上） |
| 内存扫描 / 键盘记录 | 不防 | 不防 |

核心增益：**rest 状态（磁盘）零明文**。

## 方案选型

| 方案 | 评价 |
|---|---|
| 明文 0600（现状） | CLI 行业基线，rest 状态暴露 |
| **OS 凭据库（选定）** | Windows Credential Manager / macOS Keychain，`keyring` crate 封装；随登录解锁、无密码、Chrome/VS Code 同款 |
| 密码数据库（SQLCipher 或 Argon2id+AES-GCM） | 真增益仅当「每次使用都输密码且不缓存」，杀启动体验；免输则主密码又得靠 DPAPI/钥匙串包一层，绕回凭据库。留作将来可选强化层（见 M2） |

## 总体架构

```
【保存】PF 输入框明文
          │ keyring set（service="PiFlash", entry=provider id）
          ▼
     Windows 凭据管理器 / macOS 钥匙串          ← 明文唯一 rest 归宿
          │ 成功后
          ▼
     auth.json: { "glm": { "type": "api_key", "key": "$PF_KEY_GLM" } }   ← 只有变量名

【spawn】PF 读 auth.json → 收集 $PF_KEY_* 引用 → 凭据库逐个解出
          → cmd.envs(...) 注入 pi 子进程（client.rs spawn 点）

【请求】pi resolveConfigValue("$PF_KEY_GLM") → process.env → Authorization 头
```

## 设计细节

### 1. 凭据库条目规范

- service：`PiFlash`（常量）
- 条目名（user）：provider id 原样（`glm`、`deepseek`、自定义 provider 用其在 models.json 的名字）
- 平台映射与限制：

| | Windows | macOS |
|---|---|---|
| 后端 | Credential Manager（generic credential） | Keychain（generic password，login keychain） |
| 单条上限 | 2560 字节（blob 限制，key 足够） | 约 4KB |
| PF 读自建条目 | 静默 | 静默（自建自读不弹授权） |
| 不可用时 | 组策略禁用 CM 等（罕见） | 钥匙串损坏等（罕见） |

- 依赖：`keyring = { version = "3", features = ["windows-native", "apple-native"] }`

### 2. 变量名规范

- 格式：`PF_KEY_<大写净化provider>`——provider id 中非 `[A-Z0-9]` 字符替换为 `_` 后大写：
  `glm` → `PF_KEY_GLM`；`my-gw` → `PF_KEY_MY_GW`
- 净化后冲突（如 `a-b` 与 `a.b` 同为 `A_B`）：追加原始 id 短哈希后缀 `PF_KEY_A_B_X1Y2`。映射函数确定性，同名必同条目。
- `PF_KEY_` 前缀为 PF 保留命名空间：spawn 注入、迁移只认它；用户手写的其他 `$VAR` 一律不注入、不迁移。

### 3. auth.json 引用格式

```json
{ "glm": { "type": "api_key", "key": "$PF_KEY_GLM" } }
```

- `type` 仍是 `api_key`：`read_credential_kinds`、详情页状态点、「断开连接」的 OAuth 保护逻辑全部零改动。
- 高级形态共存：用户在输入框写 `!cmd` 或 `$VAR` 时按现状原样落盘（见「保存」分流）。

### 4. spawn 注入（client.rs 唯一 spawn 点）

- spawn 前读 auth.json，收集值匹配 `^\$PF_KEY_[A-Z0-9_]+$` 的引用；
- 逐个从凭据库解出 → `cmd.envs(map)` 一次性注入；
- 解不出的引用：跳过注入，并给对应会话上报提示「provider X 的密钥不在系统凭据库，请在设置-模型中重新保存」；
- env 只写进子进程，PF 自身 `process.env` 不碰；不写日志。

### 5. 保存 / 替换 / 断开

- **保存**（模型页 & 自定义 provider 编辑器共用）：
  - 输入以 `!` 或 `$` 开头 → 视为高级引用，原样写 auth.json（现状行为，不碰凭据库）；
  - 否则视为明文 → `vault.set` 成功 → auth.json 写 `$PF_KEY_*` 引用；
  - `vault.set` 失败 → 降级：明文写 auth.json + 错误条提示「系统凭据库不可用，已降级为文件存储」。
- **替换**：同名条目 `set` 天然覆盖；无需先删。
- **断开连接**：删 auth.json 条目 + `vault.delete`（幂等，不存在忽略）。降级存储的条目同样适用。
- **显示按钮**：改为「系统凭据库」徽标，不再回看明文（不可导出是特性）。输入新 key 即替换，语义不变。

### 6. 迁移（存量明文 → 凭据库）

- 时机：应用启动后台执行一次（startup.rs），**先于任何会话 spawn**——否则老进程读到的 auth.json 已变成 `$VAR` 而 env 未注入，请求会挂。
- 判定（幂等）：`type == "api_key"` 且值不以 `$` 开头 → 迁移；`$`/`!` 引用与 `type: oauth` 跳过。
- 步骤：`vault.set(明文)` → 成功后重写 auth.json 为引用；任一步失败停在原状，下次启动重试。
- 升级场景：PF 更新需重启应用，旧 pi 进程随应用退出，不存在「老进程 + 新 auth.json」窗口。

### 7. 行为变化（需在 UI 告知用户）

- **换 key 生效时机**：现状明文由 pi 每请求重读 auth.json（立即生效）；改后 env 按子进程固定，**换 key 需重启会话生效**。设置保存时提示。
- **裸跑 pi CLI**：PF 管的 key 不可见（env 未注入）；需两栖时自行 `export PF_KEY_GLM=...` 或在 auth.json 手写明文/引用。

### 8. UI 变化（settings - 模型）

- API Key 区状态文案：`已配置` → `已配置 · 系统凭据库` / `已配置 · 文件存储`（降级时）。
- note 文案：「密钥存入 Windows 凭据管理器 / macOS 钥匙串，auth.json 只留变量名」。
- 其余布局不动（沿用 042 定稿样式）。

## 代码落点

| 文件 | 改动 |
|---|---|
| `crates/pi-link/src/credentials.rs`（新） | `trait SecretVault` + `KeyringVault` 实现 + 测试用内存 Fake；`env_var_name(provider)`、引用生成/识别、迁移判定纯函数 |
| `crates/pi-link/Cargo.toml` | keyring 依赖 |
| `crates/pi-link/src/client.rs` | spawn 点收集引用 + `envs()` 注入 |
| `crates/pi-link/src/config.rs` | `set_api_key` 分流（明文→凭据库+引用；高级引用→原样）；降级路径 |
| `crates/app/src/startup.rs` | 启动迁移（后台，先于 spawn） |
| `crates/app/src/settings/models.rs`、`custom_models.rs` | 保存/断开接新路径、状态与 note 文案 |
| 会话层（spawn 调用方） | 缺钥提示透传到会话 UI |

## 测试

- 单测（pi-link，铁律测试位）：变量名净化/冲突哈希、引用生成与识别、迁移判定矩阵（明文→迁、`$OTHER`/`!`→跳过、oauth→跳过）、FakeVault 全流程、降级路径。
- 集成：真实 keyring roundtrip 标 `#[ignore]`（默认跳，本机手动跑；CI runner 凭据库可用性见开放问题）。
- pif-ui：保存后断言 auth.json 含 `$PF_KEY_*` 且无明文；断开后条目消失；高级引用不被劫持。
- 手工验收：Windows 凭据管理器目视条目；macOS 钥匙串访问；降级注入（临时禁用 CM）。

## 分期

- **M1（本设计落地范围）**：credentials 模块 + 保存/断开改道 + spawn 注入 + 启动迁移 + 降级 + UI 文案。
- **M2（可选）**：「复制到剪贴板」显式导出；存储方式设置项（凭据库/仅文件）；保存后自动重启受影响会话的 pi 进程（消除重启生效）；主密码保险库（Argon2id + AES-256-GCM，主密码可 DPAPI/钥匙串包装免输）——叠加层，不替代凭据库。

## 开放问题

1. keyring v3 在 GitHub Actions runner 的可用性：Windows runner 读 Credential Manager 应无碍；macOS runner 读自建钥匙串条目是否需要 codesign，待验证（不阻塞 M1，集成测试本就 `#[ignore]`）。
2. 迁移完成是否需要一次性告知（静默 vs 提示条），实现时定。
