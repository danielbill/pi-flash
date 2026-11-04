# 042-1 发版门禁：pif-secret.exe 随包分发

## 背景

auth.json 标记条目 `!'…/pif-secret.exe' get <provider>` 是 pi 从系统凭据库取回
key 的唯一桥（pi 是 node 进程，无 keyring 能力；042-1 退役 env 注入后只剩
`!cmd` 一条路）。`helper_exe()`（pi-link/src/auth_json.rs:27）按 current_exe
同目录写绝对路径——机制正确，断点在打包：release.sh 曾漏复制
`target/release/pif-secret.exe`，用户机上 pi 执行标记命令失败 → provider 判未配置。

**本机开发（cargo run）保存 key 看到 target/debug 路径是预期行为**——写的是
「当前运行 exe 所在目录」。验证修复必须从**包内 exe** 启动并保存。

## 自动化：`scripts/secret-e2e.sh`

全程隔离目录 + pif-ui 自动化，不碰真实 `~/.pi` / `~/.pi-flash`，不抢屏幕。
**须在 Git Bash、仓库根目录执行**（bash 脚本 + `VAR=… cmd` / `$(…)` 语法，cmd.exe 跑不了）。

```bash
cargo build && cargo build --release -p app && cargo build --release -p pi-link --bin pif-secret
scripts/secret-e2e.sh                                   # 假 key：TC1/2a/2b/2c + TC3 helper 缺失分支
E2E_SECRET_KEY="$(target/debug/pif-secret.exe get zai-coding-cn)" scripts/secret-e2e.sh \
  --provider e2e-zai --base https://open.bigmodel.cn/api/coding/paas/v4 --model glm-4.6
                                                         # 含 TC2d / TC3 真对话断言（复用本机已存的 zai key）
```

夹具 `tmp/secret-e2e/`：`pkg/`（release 两 exe + node.exe + vendor/pi，与 release.sh
第 3 步同构）、`pf/`（PI_FLASH_DIR）、`agent/`（PI_CODING_AGENT_DIR，PF 与 pi 子进程
同一 env）。`--keep` 保留夹具。

| 用例 | 断言 |
|---|---|
| TC1 组装 | `pkg/pif-secret.exe` 与 `pkg/pi-flash.exe` 同目录在位 |
| TC2a 存 key | `settings.open → mj_new → mj_form → mj_save` 真实保存链路；`mj_providers` 含商、`error` 为 null |
| TC2b 标记 | 隔离 `agent/auth.json` 该商 key：PATH 模式（042-2 起，包内 release 实例启动即注册 pif-secret 进用户级 PATH）== **裸命令** `!pif-secret get <商>`；未就绪回落 `!'<pkg 正斜杠绝对路径>/pif-secret.exe' get <商>`。两种形态都不含 `target/debug`、无明文 key |
| TC2c 取回 | `pkg/pif-secret.exe get <商>` stdout == 保存值、exit 0；换 cwd 一致 |
| TC2d 端到端（真 key） | `session.new → session.send` 出现 assistant 回复、无凭据类错误 |
| TC3 反向 | 藏起 helper → 标记命令不可执行；（真 key）重启实例发消息失败；放回 → 取回恢复、（真 key）对话恢复 |
| 清理 | `settings.mj_delete` 真实删除链路：auth.json 标记 + 凭据库条目同步清 |

pi 对 `!cmd` 结果按进程缓存，TC3 每次切换 helper 状态都重启实例。
「真回复」= assistant 且 text 非空：pi 请求失败/重试会落 **text 为空的 assistant
条目**（helper 缺失时实测 4 条），只看 role 会误判成功。

## 人工抽查（TC4 落位覆盖）

由整包复制结构保证，发版后抽查即可：绿色包解压根、npm `payload/`、应用内更新
`payload-<版本>/` 三种落位下，pif-secret.exe 都与各自目录里的 pi-flash.exe 相邻；
同目录覆盖升级（payload/）后旧标记路径自动重新有效。
