# API Key 管理（系统安全存储 · pf-auth 独立账本）

模块代码：
apikeyManager

对应界面：
1、settings - 模型（provider 详情的 API Key 区、自定义 provider/添加 Provider 的 API Key 字段）

## 核心原则（一句话）

**PF 的密钥记 PF 自己的账本** `~/.pi-flash/pf-auth.json`，明文只进系统凭据库；pi 的 `auth.json` PF **不读、不写、不管理**——系统 pi / pi-web 用它们自己的 auth.json，与 PF 完全无关。

## 背景与现状

- 现状 PF 直接读写 `~/.pi/agent/auth.json`（与 pi 共用）——本设计**废除这一共用**。
- pi 对 key 值的原生解析（resolve-config-value.js）：明文 / `$ENV_VAR` 模板（从 **pi 子进程环境**取值）/ `!command`（本设计不使用）。models.json 的 `apiKey` 走同一解析器。
- pi 子进程由 PF spawn，唯一 spawn 点：`crates/pi-link/src/client.rs`（`node cli.js --mode rpc`）。
- PF 自有目录 `~/.pi-flash/`（010-启动 §4「不污染 pi」），`pf-auth.json` 落这里。

## 目标

1. 明文 key 只存在于三处：PF 内存、OS 凭据库、pi 子进程环境变量——**磁盘零明文**（pf-auth.json 与 models.json 都只有变量名）。
2. `auth.json` 是 pi 的地盘：PF 不读（除首启一次性收编）、不写、不删。系统 pi / pi-web 的行为与 PF 装没装、存没存 key **完全无关**。
3. 经 PF 注册的 key 只服务 PF 拉起的会话（spawn 注入），不外溢。
4. 登录会话内无密码 UX：凭据随 OS 登录自动解锁。
5. 凭据库不可用时自动降级为 pf-auth.json 明文（0600），功能不断。

## 威胁模型

| 场景 | 现状（明文 auth.json） | 本设计 |
|---|---|---|
| 其他本地低权限用户读文件 | 防（ACL） | 防（ACL） |
| 磁盘失窃 / 备份 / 网盘同步 / 误提交 git | **不防** | **防**（凭据库不随文件走，文件里只有变量名） |
| 同用户恶意软件 | 不防 | 不防（诚实边界） |
| 内存扫描 / 键盘记录 | 不防 | 不防 |

核心增益：**rest 状态（磁盘）零明文**。

## 方案选型

| 方案 | 评价 |
|---|---|
| 明文 0600（现状） | CLI 行业基线，rest 状态暴露 |
| **OS 凭据库（选定）** | Windows Credential Manager / macOS Keychain，`keyring` crate 封装；随登录解锁、无密码、Chrome/VS Code 同款 |
| 密码数据库（SQLCipher 或 Argon2id+AES-GCM） | 真增益仅当「每次使用都输密码且不缓存」，杀启动体验；免输则主密码又得靠 DPAPI/钥匙串包一层，绕回凭据库。留作 M2 可选强化层 |

## 总体架构

```
【保存】PF 输入框明文（目录 provider / 自定义 provider 同一分流）
          │ keyring set（service="PiFlash", entry=provider id）
          ▼
     Windows 凭据管理器 / macOS 钥匙串            ← 明文唯一 rest 归宿
          │ 成功后
          ▼
     ~/.pi-flash/pf-auth.json  目录 provider 的账本（只有变量名 + 注入目标名）
     ~/.pi/agent/models.json   自定义 provider 的 apiKey（$PF_KEY_* 引用）
     ~/.pi/agent/auth.json     ← PF 不碰（pi / pi-web 的地盘）

【spawn】PF 读 pf-auth.json + models.json → 收集 $PF_KEY_* → 凭据库解出
          → 按条目注入：目录 provider 注 pi 官方 env 名；自定义 provider 注自身变量名
          → client.rs spawn 点 cmd.envs(...)

【请求】pi 从 process.env 拿 key：
        目录 provider → 官方 env 名（DEEPSEEK_API_KEY 等，pi 原生认）
        自定义 provider → models.json apiKey 的 $VAR 解析
```

## 设计细节

### 1. pf-auth.json（PF 独立账本，目录 provider）

- 位置：`~/.pi-flash/pf-auth.json`（`PI_FLASH_DIR` 隔离覆盖同样生效）
- 格式：

```json
{
  "deepseek": { "type": "api_key", "key": "$PF_KEY_DEEPSEEK", "injectAs": "DEEPSEEK_API_KEY" },
  "openai":   { "type": "api_key", "key": "$PF_KEY_OPENAI",   "injectAs": "OPENAI_API_KEY" }
}
```

- `key`：变量名引用，明文在凭据库；
- `injectAs`：spawn 时注入的目标变量名 = **pi 官方 env 名**（docs/providers.md 的映射表，随 vendor pin 固化进 PF，pi-link 符合性测试看护）；pi 对官方 env 名有原生支持，无需 auth.json 条目；
- 权限：`write_json_private`（0600，Unix；与 auth.json 同待遇）。

### 2. 凭据库条目规范

- service：`PiFlash`（常量）；条目名（user）：provider id 原样
- 平台映射：Windows → Credential Manager（单条 2560 字节上限，足够）；macOS → Keychain（自建自读，静默）
- 依赖：`keyring = { version = "3", features = ["windows-native", "apple-native"] }`

### 3. 变量名规范

- `PF_KEY_<大写净化provider>`：非 `[A-Z0-9]` 字符替换为 `_` 后大写（`my-gw` → `PF_KEY_MY_GW`）
- 净化后冲突：追加原始 id 短哈希后缀
- `PF_KEY_` 前缀为 PF 保留；用户手写的其他 `$VAR` / `!cmd` 原样尊重，不注入、不迁移

### 4. 引用格式（pf-auth.json 与 models.json 分工）

| provider 类型 | key 记在哪 | pi 怎么拿到 |
|---|---|---|
| 目录 provider（deepseek/openai/glm…） | `~/.pi-flash/pf-auth.json` | spawn 注入官方 env 名 |
| 自定义 provider（models.json 条目） | models.json 的 `apiKey` 字段 | spawn 注入 `PF_KEY_*`，pi 解析 `$VAR` |

- models.json 的 `apiKey` 是 pi 原生字段，协议层零改动；系统 pi 看到引用但无 env 时该 provider 不可用（即「不外溢」）；若用户在系统 pi 自己的 auth.json 里配了同一 provider，credential 优先级更高，互不干扰。
- 自定义 provider 不写 pf-auth.json（models.json 就是它的账本），目录 provider 不动 models.json。

### 5. spawn 注入（client.rs 唯一 spawn 点）

- 读 pf-auth.json（`injectAs` 目标）+ models.json（`PF_KEY_*` 引用），去重后从凭据库解出 → `cmd.envs(map)` 一次性注入；
- 解不出的引用：跳过注入，给对应会话上报提示「provider X 的密钥不在系统凭据库，请在设置-模型中重新保存」；
- env 只写进子进程，PF 自身 `process.env` 不碰；不写日志。

### 6. 保存 / 替换 / 断开 / 改名

- **保存**（统一分流，两处界面共用）：
  - 输入以 `!` 或 `$` 开头 → 高级引用，原样落盘（不碰凭据库）；
  - 否则视为明文 → `vault.set` 成功 → 目录 provider 写 pf-auth.json / 自定义 provider 写 models.json 引用；
  - `vault.set` 失败 → 降级：明文写 pf-auth.json（目录）或 models.json（自定义）+ 错误条提示「系统凭据库不可用，已降级为文件存储」。
- **替换**：同名条目 `set` 天然覆盖。
- **断开连接**：删自己的账本条目（pf-auth.json 或 models.json 字段置空）+ `vault.delete`（幂等）。**不碰 auth.json。**
- **自定义 provider 改名**：凭据条目以 provider id 为 key → vault 条目搬家 + models.json 引用重写。
- **显示**：不回显明文；输入框回显变量名引用（或空 +「系统凭据库」徽标），输入新值即替换。

### 7. 迁移（存量收编——复制式，一次性）

- **auth.json 里的旧 key**（老版本 PF 写入的）：首启一次性**复制**进 pf-auth.json + 凭据库；auth.json 原文不动——那是 pi 的文件，PF 从此不再碰，用户想清理自己删。
- **models.json 里的明文 apiKey**（PF UI 写入的）：原址迁移 vault + 改引用（该文件本就是 PF 维护自定义 provider 的地方）。
- 判定（幂等）：值不以 `$` 开头 → 收编；`$`/`!` 引用与 `type: oauth` 跳过。
- 时机：应用启动后台、**先于任何 spawn**；任一步失败停在原状，下次重试。
- 升级场景：PF 更新需重启应用，旧 pi 进程随应用退出，无「老进程 + 新文件」窗口。

### 8. 行为变化（需在 UI 告知用户）

- **换 key 生效时机**：env 按子进程固定，换 key 需重启会话生效；设置保存时提示。
- **PF 模型页「已配置」口径改为自有账本**（pf-auth.json + models.json 引用）；auth.json 里用户给系统 pi 配的 key 不再显示在 PF 页面（互不归属）。
- **系统 pi / pi-web：零变化。**

### 9. UI 变化（settings - 模型）

- 状态文案：`已配置` → `已配置 · 系统凭据库` / `已配置 · 文件存储`（降级时）
- note：「密钥存入 Windows 凭据管理器 / macOS 钥匙串；pf-auth.json 只留变量名，auth.json 归 pi」

## 代码落点

| 文件 | 改动 |
|---|---|
| `crates/pi-link/src/credentials.rs`（新） | `trait SecretVault` + `KeyringVault` + 测试内存 Fake；`env_var_name()`、官方 env 名映射表（随 vendor pin）、引用识别、双文件迁移扫描 |
| `crates/pi-link/src/pf_auth.rs`（新） | pf-auth.json 读写（`write_json_private`） |
| `crates/pi-link/src/config.rs` | 现 auth.json 读写函数退役为迁移专用；新保存/断开走 pf-auth |
| `crates/pi-link/src/client.rs` | spawn 点读双账本收集引用 + `envs()` 注入 |
| `crates/pi-link/src/models_json.rs` | `set_api_key` 帮手（分流）；`rename_provider` 同步 vault 搬家 + 引用重写 |
| `crates/app/src/startup.rs` | 一次性收编（后台，先于 spawn） |
| `crates/app/src/settings/custom_models.rs` | `mj_save_provider` / `mj_add_provider` 接新帮手；回显改引用/徽标 |
| `crates/app/src/settings/models.rs` | 保存/断开/状态口径切 pf-auth；文案更新 |
| 会话层（spawn 调用方） | 缺钥提示透传 |

## 测试

- 单测（pi-link）：变量名净化/冲突哈希、injectAs 映射表、引用识别、迁移判定矩阵（明文→收编、`$OTHER`/`!`/oauth→跳过、auth.json 复制不删）、FakeVault 全流程、降级路径、改名联动。
- 集成：真实 keyring roundtrip 标 `#[ignore]`（本机手动跑）。
- pif-ui：保存后断言 pf-auth.json/models.json 含 `$PF_KEY_*` 且无明文、**auth.json 字节不变**；断开后条目消失；高级引用不被劫持。
- 手工验收：Windows 凭据管理器目视条目；macOS 钥匙串访问；系统 pi / pi-web 行为回归（装 PF 前后对比）；降级注入。

## 分期

- **M1（本设计落地范围）**：credentials/pf_auth 模块 + 保存/断开/改名切自有账本 + spawn 注入 + 一次性收编 + 降级 + UI 文案。
- **M2（可选）**：「复制到剪贴板」导出；存储方式设置项；保存后自动重启受影响会话；主密码保险库（Argon2id + AES-256-GCM，主密码可 DPAPI/钥匙串包装免输）——叠加层。

## 开放问题

1. 官方 env 名映射表与 vendor pin 的同步：pi 升级（bump vendor）时新增/改名 provider 的 env 名由 pi-link 符合性测试看护。
2. glm 等多 plan provider 的 env 歧义（如 ZAI 有 Global/China 两个变量名）：实现时按 catalog 条目定，表里允许一名多写。
3. keyring v3 在 GitHub Actions runner 的可用性（不阻塞 M1）。
