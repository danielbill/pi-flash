# 042-1 发版门禁：pif-secret.exe 随包分发

## 背景

auth.json 标记条目 `!'…/pif-secret.exe' get <provider>` 是 pi 从系统凭据库取回
key 的唯一桥（pi 是 node 进程，无 keyring 能力；042-1 退役 env 注入后只剩
`!cmd` 一条路）。`helper_exe()`（pi-link/src/auth_json.rs:27）按 current_exe
同目录写绝对路径——机制正确，断点在打包：release.sh 曾漏复制
`target/release/pif-secret.exe`，用户机上 pi 执行标记命令失败 → provider 判未配置。

## 用例

### TC1 组装清单含 helper

- 操作：跑 `scripts/release.sh`（或手工执行第 3 步组装片段到临时目录）。
- 断言：
  - `dist/pi-flash/pif-secret.exe` 存在，且与 `dist/pi-flash/pi-flash.exe` 同目录；
  - `target/release/pif-secret.exe` 缺失时脚本以非零码中止（`set -e` + 显式 guard）。

### TC2 包内取回链路（本机发版前必测）

- 操作：`cargo build --release -p app` → 模拟组装（两 exe + node.exe + vendor/pi
  拷入独立目录，如 `tmp/pkgtest/`）→ 包内启动 pi-flash → 设置-模型页存测试 key。
- 断言：
  1. `~/.pi/agent/auth.json` 标记路径指向**包内目录**（如 `tmp/pkgtest/pif-secret.exe`），
     不得指向 `target/debug`；
  2. 包内手动执行 `./pif-secret.exe get <provider>` stdout 输出 key 本身（exit 0）；
  3. 用该 provider 发消息端到端可对话（pi → 标记 → 包内 helper → 凭据管理器全链通）。

### TC3 反向复现（测试方法自证）

- 操作：把包内 `pif-secret.exe` 临时改名再发消息。
- 断言：该 provider 判为未配置；改回名字后恢复。测毕清除测试 key 与 auth.json 条目。

### TC4 落位覆盖（npm 三形态同目录性）

- 断言（由整包复制结构保证，抽查即可）：绿色包解压根、npm `payload/`、应用内更新
  `payload-<版本>/` 三种落位下，pif-secret.exe 都与各自目录里的 pi-flash.exe 相邻；
  同目录覆盖升级（payload/）后旧标记路径自动重新有效。
