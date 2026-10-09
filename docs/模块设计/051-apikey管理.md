# API Key 管理（系统安全存储 · pf-auth / providers 双账本）

模块代码：
apikeyManager

对应界面：
1、settings - 模型（provider 详情的 API Key 区、自定义 provider/添加 Provider 的 API Key 字段）

## 铁律

1. **PF 永不写 `~/.pi/agent/models.json`**——读只读（catalog 合并显示需要），零写入，连修复都不写（051 M1.1，用户令）。PF 对它造成的任何历史污染由用户手工清理。
2. **PF 永不写 `~/.pi/agent/auth.json`**——pi 的凭据文件归 pi；仅首启一次性**复制**收编（复制不删）。
3. 明文密钥唯一 rest 归宿 = OS 凭据库（Windows Credential Manager / macOS Keychain，`keyring` crate）。

## 核心架构（一句话）

**PF 的东西记 PF 的账本**：目录 provider 的 key 记 `~/.pi-flash/pf-auth.json`，自定义 provider 记 `~/.pi-flash/providers.json`，两份都是 PF 私有文件（0600）；pi 侧需要什么，spawn 时**注入**进去（env 变量 + 官方扩展注册），pi 的文件一个字节都不碰。

## 背景与事故记录

- 现状前身：PF 曾直接读写 auth.json（与 pi 共用凭据）；M1 初版还把 models.json 当"兜底通道"写入了 `providers.{p}.apiKey` 引用，导致两个真实事故：
  1. 双 pi 用户（系统 pi + PF 共用 agent 目录）的系统 pi 解析不了 `$PF_KEY_*` 引用，自定义 provider 直接不可用；
  2. apiKey-only 裸条目让 catalog provider（zai-coding-cn）被侧栏误判为自定义 provider，点「+ 模型」立即落盘 `{"id": ""}` 占位条目且 flush 无校验 → pi 严格校验（`id minLength 1`）→ 整个 models.json schema 报废。
- 结论（用户裁决）：明文本来就不该到处放；共享文件一个字节都不能写。功能存亡靠注入解决。

## pi 侧事实（vendored 源码核实）

- models.json 加载路径：`ModelRuntime.create({ modelsPath })` 可注入，但 **CLI 无 `--models-json` flag、无单独 env、RPC 消息不支持注册 provider**；
- `PI_CODING_AGENT_DIR` env 是全量隔离（auth/sessions/settings/skills 全跟走），不可用于只换 models.json；
- **官方注入通道：扩展 `-e file.js` 在 pi 进程内 `pi.registerProvider(name, config)`**（types.d.ts:1276 官方示例的 apiKey 就是 `"$PROXY_API_KEY"` 环境变量插值形态）；`-ne`（禁自动扩展）与显式 `-e` 可共存（resource-loader.js:403：noExtensions 时 extensionPaths = 显式路径集）；
- pi 自己的 provider→env 名映射 `getApiKeyEnvVars`（bundle chunk）是官方表，PF 照抄固化。

## 存储分工

| 内容 | 存哪 | 保护 | pi 怎么拿到 |
|---|---|---|---|
| 目录 provider key（deepseek/zai-coding-cn…） | `~/.pi-flash/pf-auth.json` | 凭据库（`$PF_KEY_*` 引用） | spawn 注入官方 env 名（表随 vendor pin） |
| 自定义 provider（含 key/模型） | `~/.pi-flash/providers.json` | 凭据库（同上） | spawn 挂 `-e pf-providers.mjs` → 进程内 `registerProvider` |
| 系统 pi 自己的 auth.json / models.json | `~/.pi/agent/`（pi 地盘） | 不属于 PF 管辖 | 系统 pi 自己读；PF 只读 models.json 做 catalog 合并 |

## 目标

1. 磁盘零明文（两份 PF 账本只放 `$PF_KEY_*` 变量名；降级模式除外，见「降级」）。
2. pi 的 auth.json / models.json **零写入**；系统 pi / pi-web 与 PF 互不影响。
3. 登录会话内无密码 UX；凭据库不可用自动降级（pf-auth.json / providers.json 明文 0600），功能不断。
4. 迁移全部**复制式**：只从 pi 文件往 PF 账本收编，绝不回写。

## 威胁模型

| 场景 | 效果 |
|---|---|
| 其他本地低权限用户读文件 | 防（ACL + 0600） |
| 磁盘失窃 / 备份 / 网盘 / 误提交 | **防**（凭据库不随文件走，文件里只有变量名） |
| 同用户恶意软件 / 内存扫描 | 不防（诚实边界） |

## 方案选型

OS 凭据库（选定）> 明文 0600（CLI 基线，rest 暴露）> 密码数据库（真增益仅当每次输密码且不缓存，免输则主密码又得靠 DPAPI/钥匙串包一层，绕回凭据库；留作 M2 可选叠加层）。

## 设计细节

### 1. pf-auth.json（目录 provider 账本）

```json
{ "deepseek": { "type": "api_key", "key": "$PF_KEY_DEEPSEEK", "injectAs": "DEEPSEEK_API_KEY", "mode": "vault" } }
```

- `injectAs` = pi 官方 env 名，逐字照抄 vendored `getApiKeyEnvVars` 表（含 zai→ZAI_API_KEY、zai-coding-cn→ZAI_CODING_CN_API_KEY 等 40 项）；查不到的 provider 记自身变量名——pi 不消费即不可用，**诚实降级，不再写 models.json 兜底**。
- `mode`：`vault`（凭据库）/ `ref`（用户手写 `$VAR`/`!cmd`，PF 不掺和）/ `file`（降级明文，PF 直接注入 env）。

### 2. providers.json（自定义 provider 账本）

- schema 沿用 models.json 的 providers 形状（编辑器缓冲零改动成本）；
- **落盘净化**：空 id / 缺 id 的模型条目、空模型数组、无有效内容（含 apiKey-only 裸条目）的 provider，写前一律丢弃——schema 报废事故根治点；
- 自定义 key 走同款分流：明文 → 凭据库 + 引用；高级引用原样；库失败降级明文。

### 3. 扩展注入（pf-providers.mjs）

- `~/.pi-flash/pf-providers.mjs` 由 PF 生成（版本化，内容变化即重写）：

```js
import { readFileSync } from "node:fs";
const providers = (() => { try { return JSON.parse(readFileSync(new URL("./providers.json", import.meta.url), "utf8")).providers ?? {}; } catch { return {}; } })();
export default function (pi) {
  for (const [id, cfg] of Object.entries(providers)) {
    try { pi.registerProvider(id, cfg); } catch (e) { console.error(...); }
  }
}
```

- client.rs spawn：账本非空且模板存在 → `cmd.arg("-e").arg(ext)`；与 `-ne` 隔离兼容（显式 `-e` 照常加载）；
- spawn 同时注入 env（`credentials::spawn_env_at`）：pf-auth（injectAs / 降级明文）+ providers.json（`$PF_KEY_*` 引用）→ 凭据库解出 → `cmd.envs(...)`；解不出的引用跳过，不阻塞 spawn。

### 4. 凭据库条目规范

- service `PiFlash`；条目名 = provider id 原样；Windows Credential Manager / macOS Keychain（keyring v3，`windows-native`/`apple-native` features）。

### 5. 变量名规范

- `PF_KEY_<大写净化provider>`，非 `[A-Za-z0-9]` 字符净化为 `_` 且整体追加 6 位 FNV 短哈希（`my-gw` → `PF_KEY_MY_GW_XXXXXX`，`a-b`/`a.b` 不冲突）；纯字母数字 id 保持干净（`deepseek` → `PF_KEY_DEEPSEEK`）。
- `PF_KEY_` 前缀 PF 保留；用户手写的其他 `$VAR` / `!cmd` 原样落盘、不注入不迁移。

### 6. 保存 / 替换 / 断开 / 改名

- 保存（目录 → pf-auth；自定义 → providers.json 缓冲，净化落盘）：`$`/`!` 高级引用原样；明文 → vault + 引用；vault 失败 → 降级明文 + 错误条提示。
- 断开：删自己账本的条目 + vault delete（幂等）。**不碰 pi 文件。**
- 自定义 provider 改名：vault 条目跟名搬家。
- 显示：不回显明文；状态文案 `已配置 · 系统凭据库` / `已配置 · 文件存储`。

### 7. 迁移（全部复制式，一次性，先于任何 spawn）

- auth.json 明文 api_key → vault + pf-auth（复制不删；oauth/引用跳过；已有条目跳过）；
- pi models.json 有真自定义内容（有 baseUrl 或有非空 id 模型）的条目 → providers.json（复制不删；apiKey-only 垃圾跳过；复制后明文立即凭据库化）；标记文件 `~/.pi-flash/providers-migrated` 防重跑；
- models.json / auth.json **字节不动**，单测断言。

### 8. 行为变化（需在 UI 告知用户）

- 换 key 需重启会话生效（env/扩展按子进程固定）。
- PF 注册的内容只服务 PF 会话（用户裁决的架构）：系统 pi 想用同款 provider 就写它自己的 models.json。
- OAuth 归 pi：PF 模型页不再显示/管理 auth.json 里的 OAuth 登录态。

## 代码落点

| 文件 | 职责 |
|---|---|
| `pi-link/src/credentials.rs` | SecretVault trait + KeyringVault/MemVault/FailVault；`$PF_KEY_*` 识别、env_var_name、官方 env 表（照抄 getApiKeyEnvVars）、spawn_env_at |
| `pi-link/src/pf_auth.rs` | pf-auth.json 账本 + 目录 key 保存/断开/收编 |
| `pi-link/src/pf_providers.rs` | providers.json 账本（净化落盘）+ key 分流 + 扩展模板 + models.json 复制迁移 |
| `pi-link/src/models_json.rs` | **只读视图 + 纯 JSON 手术**（编辑器缓冲用），无任何 I/O |
| `pi-link/src/client.rs` | spawn：envs 注入 + `-e` 扩展挂载 |
| `pi-link/src/catalog.rs` | disk_models 合并 pf 账本（只读，PF 自定义模型免会话即可见） |
| `app/src/startup.rs` | boot：扩展模板 → 复制迁移 → auth 收编 |
| `app/src/settings/custom_models.rs`、`models.rs` | 编辑器/侧栏/状态切自有账本 |

## 测试

- pi-link 123+：引用识别、变量名/冲突哈希、官方表（zai-coding-cn 等）、分流路由、降级、断开、收编（复制不删、字节不变断言）、**sanitize（空 id 永不落盘）**、复制迁移（筛选/幂等/字节不变/标记）、spawn_env 合并跳过、扩展模板稳定性。
- 手工验收：models.json 前后 diff 字节不变；PF 新建自定义 provider → 只出现 providers.json + 会话内可用；系统 pi 不受影响；降级注入。

## 分期

- **M1/M1.1（已落地）**：上述全部。
- **M2（可选）**：剪贴板导出；存储方式设置项；保存后自动重启受影响会话；主密码保险库（Argon2id + AES-256-GCM 叠加层）；models.json 只读条目的「导入到 PF」按钮。

## 开放问题

1. 扩展注册的 provider 在 `get_available_models` 的呈现与 catalog 缓存合并的时序（实现已留 catalog 合并路径，验收确认）。
2. keyring v3 在 GitHub Actions runner 的可用性（集成测试 `#[ignore]`，不阻塞）。
