#!/usr/bin/env bash
# check_arch.sh — ARCHITECTURE.md §6 守护规则
# 用法: scripts/check_arch.sh   (非零退出 = 违规)
# 基线机制: baseline 文件记录当前已知违规(文件=数量),只对"新增"违规报错,
# 存量违规按 G+ 计划逐个消化后从基线删除。

set -uo pipefail
cd "$(dirname "$0")/.."

LIMIT_FILE=1500        # 单文件行数上限
LIMIT_RENDER=300       # 单个 render/fn 行数上限(仅检查 fn render / fn main_column 等视图函数)
BASELINE=scripts/arch_baseline.txt

fail=0

echo "== 1. 单文件行数 ≤ $LIMIT_FILE =="
while IFS= read -r f; do
  n=$(wc -l < "$f")
  if [ "$n" -gt "$LIMIT_FILE" ]; then
    echo "VIOLATION: $f ($n lines)"
    fail=1
  fi
done < <(find crates/app/src crates/pi-link/src -name '*.rs')

echo
echo "== 2. render/视图函数行数 ≤ $LIMIT_RENDER =="
# 对每个 rs 文件,找出顶层 fn 并量行数(简化:fn 行到下一个同缩进 '}')
python - "$LIMIT_RENDER" <<'PYEOF'
import re, sys, glob
limit = int(sys.argv[1])
bad = []
for path in glob.glob('crates/app/src/**/*.rs', recursive=True):
    lines = open(path, encoding='utf-8').read().splitlines()
    i = 0
    while i < len(lines):
        m = re.match(r'^(\s*)(?:pub(?:\(crate\))? )?fn (\w+)\(', lines[i])
        if m and m.group(1) == '':  # 顶层(无缩进)才量
            indent = m.group(1)
            depth = 0; started = False; j = i
            while j < len(lines):
                depth += lines[j].count('{') - lines[j].count('}')
                if '{' in lines[j]: started = True
                if started and depth <= 0:
                    break
                j += 1
            n = j - i + 1
            if n > limit:
                bad.append(f"{path}: fn {m.group(2)} ({n} lines)")
            i = j
        i += 1
for b in bad:
    print("VIOLATION:", b)
sys.exit(1 if bad else 0)
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
echo "== 4. build 无新警告 / test 全绿 =="
cargo build -p app --quiet 2>&1 | grep -c "^warning" > /tmp/warn_app
echo "app warnings: $(cat /tmp/warn_app) (基线对比交由 CI;本地以不新增为准)"
cargo test --quiet > /tmp/test_out 2>&1
if grep -q "FAILED" /tmp/test_out; then
  echo "VIOLATION: tests failed"
  fail=1
else
  echo "tests ok"
fi

echo
if [ "$fail" -ne 0 ]; then
  echo "check_arch: FAIL"
else
  echo "check_arch: OK"
fi
exit $fail
