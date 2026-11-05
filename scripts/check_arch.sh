#!/usr/bin/env bash
# check_arch.sh — ARCHITECTURE.md §8 守护规则
# 用法: scripts/check_arch.sh [--update-baseline]   (非零退出 = 违规)
# 口径: 行数红线按"非测试行数"计——#[cfg(test)] 标注的块(含 mod tests;
#       指向的独立测试文件)不计入,测试体量不惩罚生产代码。
#       函数行数仅量顶层 fn(视图函数在本仓均为顶层自由函数,impl 方法不量)。
# §1/§2 超线存量冻结在 scripts/arch-lines-baseline.txt（ui-raw 基线同款口径）：
#       新增超线 / 存量继续增长 = 违规；基线只许人工 `--update-baseline`
#       重写，agent 禁刷基线（刷基线 = 等于没规则）。

set -uo pipefail
cd "$(dirname "$0")/.."
[[ $# -eq 0 || "${1:-}" == "--update-baseline" ]] || {
  echo "用法: scripts/check_arch.sh [--update-baseline]"; exit 1;
}

LIMIT_FILE=1500        # 单文件非测试行数上限
LIMIT_RENDER=300       # 单个顶层视图函数行数上限

fail=0

echo "== 1. 单文件非测试行数 ≤ $LIMIT_FILE（存量冻结 arch-lines-baseline.txt）=="
echo "== 2. 顶层视图函数行数 ≤ $LIMIT_RENDER（同上基线，只拦新增/增长）=="
python - "$LIMIT_FILE" "$LIMIT_RENDER" "${1:-}" <<'PYEOF'
import os, re, sys

limit_file, limit_render = int(sys.argv[1]), int(sys.argv[2])
files = []
for root in ('crates/app/src', 'crates/pi-link/src'):
    for dirpath, _dirs, names in os.walk(root):
        for name in names:
            if name.endswith('.rs'):
                files.append(os.path.join(dirpath, name).replace('\\', '/'))
# 视图函数行数只查 app(规则本意是 render/视图函数;pi-link 解析函数不在此列)
fn_roots = [f for f in files if f.startswith('crates/app/src')]

excluded_files = set()

def scan(path):
    """返回 (行列表, 每行是否属测试区)。#[cfg(test)] 块整体算测试区;
    `#[cfg(test)] mod x;` 指向的独立文件也整体算测试区。"""
    lines = open(path, encoding='utf-8', errors='replace').read().splitlines()
    flags = [False] * len(lines)
    i, n = 0, len(lines)
    while i < n:
        if lines[i].strip() == '#[cfg(test)]':
            j = i + 1
            item = lines[j] if j < n else ''
            m = re.match(r'\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;', item)
            if m:
                base = os.path.dirname(path)
                for cand in (os.path.join(base, m.group(1) + '.rs'),
                             os.path.join(base, m.group(1), 'mod.rs')):
                    cand = cand.replace('\\', '/')
                    if cand in files:
                        excluded_files.add(cand)
                for k in range(i, min(j + 1, n)):
                    flags[k] = True
                i = j + 1
                continue
            depth, started, k = 0, False, j
            while k < n:
                depth += lines[k].count('{') - lines[k].count('}')
                if '{' in lines[k]:
                    started = True
                if started and depth <= 0:
                    break
                k += 1
            for k2 in range(i, min(k + 1, n)):
                flags[k2] = True
            i = k + 1
            continue
        i += 1
    return lines, flags

# --- 存量基线（ui-raw 同款口径：冻结存量，只拦新增/增长）---
BASELINE = 'scripts/arch-lines-baseline.txt'
UPDATE = len(sys.argv) > 3 and sys.argv[3] == '--update-baseline'

def load_baseline():
    base = {}
    if os.path.isfile(BASELINE):
        for ln in open(BASELINE, encoding='utf-8'):
            parts = ln.split()
            if len(parts) == 3 and parts[0] in ('FILE', 'FN'):
                base[parts[0] + ' ' + parts[1]] = int(parts[2])
    return base

over_file, over_fn = {}, {}
fn_set = set(fn_roots)
for path in sorted(set(files) - excluded_files):
    lines, flags = scan(path)
    non_test = sum(1 for f in flags if not f)
    if non_test > limit_file:
        over_file[path] = non_test
    if path not in fn_set:
        continue
    # 顶层 fn 行数(跳过测试区内的 fn)
    i, n = 0, len(lines)
    while i < n:
        m = re.match(r'^(?:pub(?:\(crate\))? )?fn (\w+)\(', lines[i])
        if m and not flags[i]:
            depth, started, j = 0, False, i
            while j < n:
                depth += lines[j].count('{') - lines[j].count('}')
                if '{' in lines[j]:
                    started = True
                if started and depth <= 0:
                    break
                j += 1
            j = min(j, n - 1)
            cnt = sum(1 for k in range(i, j + 1) if not flags[k])
            if cnt > limit_render:
                over_fn[f"{path}::{m.group(1)}"] = cnt
            i = j
        i += 1

if UPDATE:
    with open(BASELINE, 'w', encoding='utf-8', newline='\n') as f:
        f.write('# 「行数红线」存量基线（scripts/check_arch.sh §1/§2）\n')
        f.write('# 格式: FILE <相对路径> <非测试行数> / FN <相对路径>::<fn名> <非测试行数>\n')
        f.write('# 只允许人工 `scripts/check_arch.sh --update-baseline` 重写（agent 禁刷基线）。\n')
        f.write('# 目标：逐条清偿（拆分降数），存量只许降不许涨。\n')
        for path, cnt in sorted(over_file.items()):
            f.write(f'FILE {path} {cnt}\n')
        for q, cnt in sorted(over_fn.items()):
            f.write(f'FN {q} {cnt}\n')
    print(f'baseline rewritten: FILE×{len(over_file)} + FN×{len(over_fn)} -> {BASELINE}')
    sys.exit(0)

base = load_baseline()
bad_file, bad_fn = [], []
for path, cnt in sorted(over_file.items()):
    b = base.get('FILE ' + path)
    if b is None:
        bad_file.append(f"{path} ({cnt} non-test lines) —— 新增超线：先拆分或口径复核（确属存量 → 人工 --update-baseline）")
    elif cnt > b:
        bad_file.append(f"{path} ({cnt} non-test lines, 基线 {b}) —— 超线存量增长：先拆分清偿")
for q, cnt in sorted(over_fn.items()):
    b = base.get('FN ' + q)
    if b is None:
        bad_fn.append(f"{q} ({cnt} non-test lines) —— 新增超线：先拆分或口径复核（确属存量 → 人工 --update-baseline）")
    elif cnt > b:
        bad_fn.append(f"{q} ({cnt} non-test lines, 基线 {b}) —— 超线存量增长：先拆分清偿")

for b in bad_file:
    print("VIOLATION(file):", b)
for b in bad_fn:
    print("VIOLATION(fn):", b)
sys.exit(1 if (bad_file or bad_fn) else 0)
PYEOF
[ $? -ne 0 ] && fail=1

echo
echo "== 3. 禁止 on_key_down 字符匹配输入 =="
if grep -rn "chars().count() == 1" crates/app/src --include='*.rs' | grep -v test; then
  echo "VIOLATION: on_key_down 手搓字符输入"
  fail=1
else
  echo "ok"
fi

echo
echo "== 4. 页面禁手搓控件（交互链里的裸几何字面量；存量走 scripts/ui-raw-baseline.txt）=="
if python scripts/check_ui_raw.py; then
  :
else
  echo "VIOLATION: 新增手搓控件（改用 crate::ui::kit::* / ui::tokens；特殊件写 ui-raw: ok 并在 §10 登记）"
  fail=1
fi

echo
echo "== 5. build 无新警告 / test 全绿 =="
# 构建失败也要红（不能只数 warning：error 行不匹配 ^warning，曾把编译
# 损坏静默成 "app warnings: 0" + "tests ok"，0.1.10 发版被 release 构建
# 拦下才发现）
if cargo build -p app --quiet 2>&1 | tee /tmp/build_app | grep -c "^warning" > /tmp/warn_app; then
  :
fi
if grep -q "^error" /tmp/build_app; then
  echo "VIOLATION: app build failed（末 15 行：）"
  tail -15 /tmp/build_app
  fail=1
fi
echo "app warnings: $(cat /tmp/warn_app) (基线对比交由 CI;本地以不新增为准)"
# 退出码为准（编译失败/测试失败都非 0）——只 grep 输出里的 FAILED 会漏掉
# 编译损坏（error 输出里没有 FAILED 字样）
if cargo test --quiet > /tmp/test_out 2>&1; then
  echo "tests ok"
else
  echo "VIOLATION: tests failed（编译或运行，末 20 行：）"
  tail -20 /tmp/test_out
  fail=1
fi

echo
if [ "$fail" -ne 0 ]; then
  echo "check_arch: FAIL"
else
  echo "check_arch: OK"
fi
exit $fail
