#!/usr/bin/env python3
"""clean-cache.py —— 磁盘垃圾回收（target 过期产物 + tmp 一次性试验目录）。

起因（2026-10-20）：D 盘 86% 满，`target/` 14.4G + `tmp/` 5.8G 全是可再生成物，
每次都要 agent 手工 du 一遍再逐个判断。本脚本把那次判断固化成规则。

## 规则（全部幂等、可重复跑）

**A. cargo 过期产物（`target/<profile>/deps`）**
  cargo 从不清旧哈希：同一个 crate 换 feature/源码状态就多一份
  `<crate>-<16位hash>.<ext>`，旧的那份再也不会被用到，但 exe+pdb 动辄 200MB。
  - 默认（安全档）：**只动 `exe pdb`**，同组留 1 份。这两类是**最终产物**：
    一个 bin 同时只跑一份哈希，旧哈希的铁定是废弃构建（探针/测试 bin 秒级
    重建），删了不影响下次 `cargo build`。
  - `--aggressive`（激进档，可选）：`rlib rmeta lib dll exp` → 留 `--keep-libs`
    份（默认 2）。注意：这些对每个 crate 是**多 bin 共用**的（同一个
    `libgpui_component-<hash>.dll` 服务 pi-flash 和 pif-ui），mtime 分不出
    活/死，激进档**可能让下次构建重编几个 crate**（实测 
    gpui/gpui-component 家族重编约 2~3 分钟）——不会坏，只是慢。
  - 只删 **mtime 早于 `--min-age-days`** 的（默认 3 天），正在进行中的构建不会被碰。
  - `.d` 依赖清单文件太小，不参与。

**B. incremental 目录（`target/<profile>/incremental/<session>`）**
  cargo 的增量编译会话按哈希建子目录，旧会话永不回收（`tmp/target-ui` 里
  1.6G 全是这个）。子目录整棵早于 `--min-age-days` 就删。

**C. tmp/ 一次性试验目录**
  `tmp/` 已 gitignore，是纯草稿场：UI 自动化实例（`ui-test-*` `ui-think*`）、
  解包基线、老二进制、一次性脚本。规则 = **早于 `--tmp-age-days`（默认 3 天）
  且不在白名单**的条目整棵删。
  白名单：`屏幕截图/`（AGENTS 约定截图目录）、`sound/`（sound.rs 注释引用的
  试听稿）、`release-notes-*.md`、任意 `*.md`。

**D. 其它 cargo target 目录** —— `target-check/`、`tmp/target-ui/` 这类
  独立 CARGO_TARGET_DIR，早于 `--min-age-days` 就整棵删（下次自动重建）。

## 安全阀

- **默认真删**；只打印将删什么、能省多少用 `--dry-run`。
- 删除清单逐条写入 `target/clean-cache.log`（dry-run 写 `.dry.log`）。
- `--trash`：tmp 部分改走回收站（PowerShell Shell.Application），防手滑；
  注意回收站仍在同一块盘上，**不清空回收站不返还空间**。

## 用法

```bash
python scripts/clean-cache.py                  # 真清（安全档）
python scripts/clean-cache.py --dry-run        # 体检：看看能省多少
python scripts/clean-cache.py --tmp-only       # 只清 tmp（最安全）
python scripts/clean-cache.py --target-only    # 只清 target
python scripts/clean-cache.py --aggressive     # 连过期 rlib/rmeta 一起清
python scripts/clean-cache.py --min-age-days 7 --tmp-age-days 7
```

删完如果下次构建明显变慢，说明删到了活产物（cargo 会自己重编，不会坏），
把 `--min-age-days` / `--keep` 调大即可。
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import time
from collections import defaultdict

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = os.path.join(ROOT, "target")
TMP = os.path.join(ROOT, "tmp")

# profile 目录名（cargo 标准三档 + 自定义 target 目录用的 debug/release）
PROFILES = ("debug", "release")

# 同 crate 同扩展名分组；lib 前缀也算一组
FAMILY_RE = re.compile(r"^(lib)?(.+)-([0-9a-f]{16})\.([A-Za-z0-9_]+)$")

# 扩展名 → 保留份数。安全档只管「最终产物」(exe/pdb)；其余（rlib/rmeta/lib/dll/exp）
# 是 crate 级、多 bin 共用的中间产物，活/死分不清，只在 --aggressive 下清。
KEEP_SAFE = {"exe": 1, "pdb": 1}
KEEP_LIB = {"rlib": 2, "rmeta": 2, "lib": 2, "dll": 2, "exp": 2}   # 值被 --keep-libs 覆盖
SKIP_EXT = {"d"}                        # 依赖清单，体积可忽略

# tmp/ 白名单（名字精确匹配 或 前缀匹配 或 后缀匹配）
TMP_KEEP_NAMES = {"屏幕截图", "sound", ".gitkeep", "desktop.ini"}
TMP_KEEP_PREFIX = ("release-notes",)
TMP_KEEP_SUFFIX = (".md",)

DAY = 86400


# ---------------------------------------------------------------- 体积统计

def path_size(p: str) -> int:
    if os.path.islink(p) or os.path.isfile(p):
        try:
            return os.path.getsize(p)
        except OSError:
            return 0
    total = 0
    for dirpath, _dirnames, filenames in os.walk(p):
        for f in filenames:
            try:
                total += os.path.getsize(os.path.join(dirpath, f))
            except OSError:
                pass
    return total


def mtime(p: str) -> float:
    try:
        return os.path.getmtime(p)
    except OSError:
        return time.time()


def human(n: float) -> str:
    for unit in ("B", "K", "M", "G", "T"):
        if abs(n) < 1024 or unit == "T":
            return f"{n:.1f}{unit}".replace(".0" + unit, unit)
        n /= 1024
    return f"{n:.1f}T"


def free_space(path: str) -> int:
    try:
        return shutil.disk_usage(path).free
    except OSError:
        return 0


# ---------------------------------------------------------------- 回收站

def send_to_recycle_bin(paths: list[str]) -> None:
    """走 PowerShell Shell.Application 把条目挪进回收站（Windows only）。"""
    if not paths:
        return
    if sys.platform != "win32":
        print("  ! --trash 仅 Windows 有效，回退为直接删除")
        for p in paths:
            remove(p)
        return
    quoted = ",".join("'" + p.replace("'", "''") + "'" for p in paths)
    ps = (
        "$si = New-Object -ComObject Shell.Application;"
        f"foreach ($p in @({quoted})) {{"
        "  $i = Split-Path $p -Leaf; $d = Split-Path $p -Parent;"
        "  $f = $si.Namespace($d).ParseName($i);"
        "  if ($f) { $f.InvokeVerb('delete') }"
        "}"
    )
    subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", ps],
        check=False, capture_output=True,
    )


# ---------------------------------------------------------------- 删除执行

class Cleaner:
    def __init__(self, apply: bool, log) -> None:
        self.apply = apply
        self.log = log
        self.items: list[tuple[str, int, str]] = []   # (path, size, why)

    def plan(self, path: str, why: str) -> None:
        size = path_size(path)
        if size <= 0:
            return
        self.items.append((path, size, why))
        self.log.write(("DEL " if self.apply else "DRY ") + f"{path}\t{size}\t{why}\n")


def remove(p: str) -> None:
    try:
        if os.path.isdir(p) and not os.path.islink(p):
            shutil.rmtree(p, ignore_errors=True)
        else:
            os.unlink(p)
    except OSError as e:
        print(f"  ! 删不掉 {p}: {e}")


# ---------------------------------------------------------------- A + B：target

def scan_target(cl: Cleaner, target_dir: str, min_age_days: float,
                aggressive: bool, keep_libs: int) -> None:
    if not os.path.isdir(target_dir):
        return
    cutoff = time.time() - min_age_days * DAY
    keep_map = dict(KEEP_SAFE)
    if aggressive:
        keep_map.update({k: keep_libs for k in KEEP_LIB})
    for prof in PROFILES:
        deps = os.path.join(target_dir, prof, "deps")
        if not os.path.isdir(deps):
            continue
        families: dict[tuple[str, str, str], list[str]] = defaultdict(list)
        for name in os.listdir(deps):
            m = FAMILY_RE.match(name)
            if not m:
                continue
            ext = m.group(4)
            if ext in SKIP_EXT or ext not in keep_map:
                continue
            p = os.path.join(deps, name)
            if os.path.isfile(p):
                families[((m.group(1) or ""), m.group(2), ext)].append(p)
        for key, paths in families.items():
            keep = keep_map[key[2]]
            if len(paths) <= keep:
                continue
            paths.sort(key=mtime)                     # 最旧在前
            for p in paths[:-keep]:
                if mtime(p) < cutoff:                 # 只动"老"的
                    cl.plan(p, f"{prof}/deps 过期哈希 ({key[1]}.{key[2]})")

        inc = os.path.join(target_dir, prof, "incremental")
        if os.path.isdir(inc):
            for name in os.listdir(inc):
                p = os.path.join(inc, name)
                if os.path.isdir(p) and mtime(p) < cutoff:
                    cl.plan(p, f"{prof}/incremental 旧会话")


def scan_stray_targets(cl: Cleaner, min_age_days: float) -> None:
    """target-check/、tmp/target-ui/ 这类旁挂 CARGO_TARGET_DIR。"""
    cutoff = time.time() - min_age_days * DAY
    cands = [os.path.join(ROOT, "target-check"), os.path.join(TMP, "target-ui"),
             os.path.join(ROOT, "target-auto")]
    for p in cands:
        if os.path.isdir(p) and mtime(p) < cutoff:
            cl.plan(p, "旁挂 cargo target 目录（下次自动重建）")


# ---------------------------------------------------------------- C：tmp

def tmp_keep(name: str) -> bool:
    if name in TMP_KEEP_NAMES:
        return True
    if name.startswith(TMP_KEEP_PREFIX):
        return True
    if name.endswith(TMP_KEEP_SUFFIX):
        return True
    return False


def scan_tmp(cl: Cleaner, tmp_dir: str, age_days: float) -> None:
    if not os.path.isdir(tmp_dir):
        return
    cutoff = time.time() - age_days * DAY
    for name in sorted(os.listdir(tmp_dir)):
        if tmp_keep(name):
            continue
        p = os.path.join(tmp_dir, name)
        if mtime(p) < cutoff:
            cl.plan(p, "tmp 一次性草稿")


# ---------------------------------------------------------------- main

def main() -> int:
    ap = argparse.ArgumentParser(
        description="清理 target 过期产物与 tmp 一次性草稿（默认真删，--dry-run 只体检）",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__.split("## 用法")[-1],
    )
    ap.add_argument("--apply", action="store_true", default=True,
                    help="真删（默认行为；保留该参数仅为兼容旧用法）")
    ap.add_argument("--dry-run", action="store_true",
                    help="只体检不删（覆盖默认的 apply 行为）")
    ap.add_argument("--min-age-days", type=float, default=3.0,
                    help="target 产物的最小年龄（天），默认 3")
    ap.add_argument("--tmp-age-days", type=float, default=3.0,
                    help="tmp 条目的最小年龄（天），默认 3")
    ap.add_argument("--target-only", action="store_true", help="只清 target")
    ap.add_argument("--tmp-only", action="store_true", help="只清 tmp")
    ap.add_argument("--no-stray", action="store_true",
                    help="不动 target-check / tmp/target-ui 这类旁挂 target 目录")
    ap.add_argument("--aggressive", action="store_true",
                    help="连过期的 rlib/rmeta/lib/dll/exp 也清（下次构建可能重编几个 crate）")
    ap.add_argument("--keep-libs", type=int, default=2,
                    help="--aggressive 下每个 crate 保留几份中间产物，默认 2")
    ap.add_argument("--trash", action="store_true",
                    help="tmp 部分走回收站（同盘仍占空间，需清空回收站才返还）")
    args = ap.parse_args()

    if args.target_only and args.tmp_only:
        ap.error("--target-only 与 --tmp-only 不能同时给")
    if args.dry_run:
        args.apply = False
    do_target = not args.tmp_only
    do_tmp = not args.target_only

    os.makedirs(TARGET, exist_ok=True)
    log_path = os.path.join(TARGET, "clean-cache.log" if args.apply else "clean-cache.dry.log")
    free_before = free_space(ROOT)

    with open(log_path, "a", encoding="utf-8") as log:
        log.write(f"\n=== {time.strftime('%Y-%m-%d %H:%M:%S')} "
                  f"{'APPLY' if args.apply else 'DRY-RUN'} "
                  f"{'AGGRESSIVE' if args.aggressive else 'safe'} ===\n")

        target_cl = Cleaner(args.apply, log)
        tmp_cl = Cleaner(args.apply, log)

        if do_target:
            scan_target(target_cl, TARGET, args.min_age_days,
                        args.aggressive, args.keep_libs)
            if not args.no_stray:
                scan_stray_targets(target_cl, args.min_age_days)
        if do_tmp:
            scan_tmp(tmp_cl, TMP, args.tmp_age_days)

    # ---- 报告
    def section(title: str, cl: Cleaner) -> int:
        if not cl.items:
            print(f"{title}: 干净，无需处理")
            return 0
        total = sum(s for _p, s, _w in cl.items)
        biggest = sorted(cl.items, key=lambda t: -t[1])[:5]
        print(f"{title}: {len(cl.items)} 项 / {human(total)}")
        for p, s, why in biggest:
            rel = os.path.relpath(p, ROOT).replace("\\", "/")
            print(f"    {human(s):>8}  {rel}   ({why})")
        if len(cl.items) > len(biggest):
            print(f"    …… 其余 {len(cl.items) - len(biggest)} 项见日志")
        return total

    print(f"\npi-flash 磁盘回收  [{os.path.basename(log_path)}]")
    print("-" * 62)
    total = 0
    if do_target:
        total += section("target 过期产物", target_cl)
    if do_tmp:
        total += section("tmp 草稿", tmp_cl)
    print("-" * 62)

    if not args.apply:
        print(f"预计可回收 {human(total)}  → 加 --dry-run 才是体检模式；去掉即真清（本次未删）")
    else:
        if do_tmp and args.trash:
            send_to_recycle_bin([p for p, _s, _w in tmp_cl.items])
            if do_target:
                for p, _s, _w in target_cl.items:
                    remove(p)
        else:
            for p, _s, _w in target_cl.items + tmp_cl.items:
                remove(p)
        free_after = free_space(ROOT)
        print(f"已回收 {human(total)}；可用空间 {human(free_before)} → {human(free_after)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
