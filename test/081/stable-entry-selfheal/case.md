# 081 稳定入口自愈：`payload\` 刷新 / 冗余清理 / pi 指针收敛

## 背景

081 布局里应用内更新只落 `payload-<版本>\`，稳定入口 `payload\` 只由 npm 薄壳
维护，而薄壳 `replacePayload`（bin/lib.js:482）先 `rmSync` 整目录再逐文件
rename——非原子，任一文件撞锁（PF 运行中 / Defender 扫描 / node.exe 侧车）
即半途而废，留下空/半残稳定入口且**无人补齐**（薄壳 `payloadOk` 只看版本化
载荷，有 newer 就短路）。后果实证（2026-11-04 本机）：`payload\` 自 10-19 起
为空 → `stable_entry_exe()` 返回 None → win_pin 自愈全灭 → 开始菜单/任务栏
死链（同步实录：auth.json 全路径标记死链 = bead pi-flash-vpj1）。

**修复设计（bead pi-flash-ab0c 定稿）**：迁移时机 = **新版首启**（T2 老进程
退出释放镜像锁后，新版跑在自己的版本目录里——此时稳定入口必然无锁）。不做
T1 落位时复制：pin 启动的常见路径下老版本就跑在 `payload\` 里，T1 时必被锁。

## 三件套（`crates/app/src/services/updater/payload.rs`）

1. **`refresh_stable_entry`**：启动后台把稳定入口刷到最新完整载荷——
   - 源 = 版本最高的完整版本化载荷；已是该版本 → 幂等零动作；稳定入口更新
     → 不降级；
   - 占用探针：目标 exe 写打开失败（运行中镜像锁）→ 整体跳过，下次启动重试；
   - 非 exe 先拷、exe 最后拷、`.payload-version` 完整走完才写——失败混合态里
     旧 exe 仍可启动（交接兜底），无戳 = 下次视为未完成重来；
   - 挂载点：`handoff_if_newer` 的后台线程，**先于** `cleanup_older`；交接场
     景跳过（新版首启会做同一件事）；debug 构建不碰安装树（win_pin 同纪律）。
2. **`cleanup_older` 扩判据**：稳定入口完整时，版本 ≤ 稳定入口戳的版本化目
   录都是冗余副本，一并删——稳态归一为一份。rename 占用探针保护运行中目录。
3. **`repoint_pi_shim_to_stable`**：自愈完成后 `pi-target.json` 重钉稳定入口
   （版本化目录会被清理删掉，钉版本目录迟早悬空）；同线程顺序 = 自愈 → 重
   钉 → 清理。T1（`stage_update_in`）仍钉 staged 新目录，保「交接后敲 `pi`
   即新内核」，首启后收敛。

### 顺带修复

- `fetch.rs` `stage_update_in` 的 pi 指针 cli 路径漏了 `vendor/pi` 段——
  `target.join(NODE_MODULES_REL)` 拼出 `<payload>\node_modules\…`（不存在），
  release 用户应用内更新后全局 `pi` 的 cli 指针一直悬空。补
  `join(VENDOR_DIR_NAME)`。

## T1 预拷优化（同日追加，bead ab0c 讨论）

首启自愈复制 1.5 万文件 ~25s，其中老进程锁得住的只有它自己的运行镜像——
vendor 等万计文件在 T1 时无主。优化：**落位后趁热预拷，首启只收尾**。

- `precopy_stable_entry`（T1，`stage_update_in` 写完载荷戳后起后台线程，
  release only）：逐文件复制 `target → payload\`，失败（锁定/IO 错）记账
  不中断；
  - **无一失败**（老进程跑在版本化目录的常见情形）→ 直接写戳完成迁移 +
    指针收敛稳定入口 → 首启零复制；
  - 有剩余（老进程跑在稳定入口，pi-flash.exe 必锁）→ 写 `.payload-stage`
    半程标记（首行版本 + 待补相对路径清单）；
- `refresh_stable_entry` 升级收尾模式：读到同版本标记 → 只按清单补拷（实际
  = pi-flash.exe 一个大文件，秒级）→ 写戳清标记；无标记/版本对不上 → 全量
  兜底；树遍历失败 → 不写戳不写标记（完整度未知，清单不可信），下次全量；
- 进程中途死掉 → 无戳无标记 → 首启全量兜底，永不劣化。

新增单测：`precopy_direct_migration_when_target_idle` /
`precopy_marker_then_refresh_finishes`（真镜像锁全链：预拷 → 标记 → 收尾）/
`refresh_marker_version_mismatch_falls_back_to_full`。

## 单测（`payload::tests`，app crate）

- `stable_entry_version_requires_stamp_and_exe`：缺戳/缺 exe/非版本戳都不算
  完整（半残不冒充）。
- `refresh_copies_newest_and_stamps_last`：半残稳定入口补齐 + 戳最后写。
- `refresh_noop_when_already_current`：幂等零动作。
- `refresh_never_downgrades`：稳定入口更新于版本化目录 → 跳过。
- `refresh_skips_when_target_locked`（windows）：真镜像锁（cmd.exe 复制成
  pi-flash.exe 运行）→ 整体跳过、旧内容原封不动。std 全共享模式模拟不出镜
  像锁，必须真跑。
- `refresh_incomplete_writes_no_stamp`：单文件失败 → 不写戳，其余文件照拷。
- `cleanup_deletes_redundant_versioned_dirs`：≤ 稳定入口戳的版本化目录删除、
  me 保护、稳定入口本体不受影响。
- `cleanup_keeps_everything_without_stable_stamp`：无稳定入口时扩展判据不生效。

## 生命周期验证矩阵（设计推演，随发版人工复核）

| 场景 | 行为 |
|---|---|
| 正常更新（pin 启动 → 更新 → 点重启） | T2 交接进 `payload-<新>\`；首启自愈刷 `payload\`；重钉指针；三次启动内版本目录清理，稳态一份 |
| npm install 半途炸（本机 10-19 实录） | `payload\` 空 → 下次任意启动（版本化目录里）自愈补齐 |
| 更新期间用户秒点 pin | 旧稳定 exe 仍在 → 交接兜底到新版 |
| 多实例（一个跑稳定入口一个跑版本目录） | 探针/占用保护，跳过重试 |
| 绿色包 | 目录名剥版本段为自身 → 同路径守卫跳过，零接触 |

## 人工验收（发版包，非 debug）

1. 装 0.4.x → 应用内更新到新版 → 点重启 → 确认 `payload\` 已是新版、
   `pi-target.json` 钉 `payload\`、`pi` 命令可用；
2. 手动清空 `payload\` → 任意方式重启 → 确认补齐；
3. 任务栏 pin 在两次升级之间始终可点（零交接或多一跳，均可启动）。
