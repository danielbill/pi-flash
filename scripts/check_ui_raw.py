#!/usr/bin/env python3
"""check_ui_raw.py —— 「页面禁手搓控件」守卫（UI组件规范 §10 / ui/kit 头注释）。

规则（2026-10-20 立，起因：040-1 扩展市场页面手搓按钮/分段控件，返工一整天）：

  在**同一个交互链**里出现**裸几何字面量** = 违规。
  - 交互链 = 从 `div(...)` / `h_flex()` / `v_flex()` 起，到 `;` 止的一整个表达式；
    链内出现 `.on_mouse_down(` / `.cursor_pointer(` / `.on_click(` 即「可交互控件」。
  - 裸几何字面量 = `.h(px(28.))` `.w(px(100.))` `.size(px(24.))` `.px(px(10.))`
    `.py/.p/.pt/.pb/.pl/.pr(px(N))` `.gap(px(N))` `.rounded(px(N))`
    （出现 `::` 的令牌引用如 `btn::MD_H` / `radius::CTRL` / `space::SP4` 一律合法）。

  合规写法 = 调 `crate::ui::kit::*` 里的组件，或至少用 tokens 常量。

逃生阀：链内任意行写 `ui-raw: ok <理由>` → 不计违规（仍计入「可见债务」统计）。

存量：`scripts/ui-raw-baseline.txt`（`path<TAB>count`）冻结既有命中，
**只拦新增**；`--update-baseline` 只能人手动跑（agent 不许自己刷基线，否则等于没规则）。
组件自身目录 `crates/app/src/ui/**` 不受此规则约束（它就是字面量的唯一合法产地）。

用法:
  python scripts/check_ui_raw.py                 # 检查（非零退出 = 违规）
  python scripts/check_ui_raw.py --update-baseline   # 重写基线（人工确认后）
  python scripts/check_ui_raw.py --list          # 打印全部命中与债务
"""

import os
import re
import sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..')
SCAN_ROOTS = ('crates/app/src',)
EXEMPT_PREFIXES = ('crates/app/src/ui/',)  # 组件产地：自己就是字面量的作者
BASELINE = os.path.join(ROOT, 'scripts', 'ui-raw-baseline.txt')

# 几何字面量：方法名( px( 数字 )，且括号里不是 :: 令牌引用
GEO = re.compile(
    r'\.(?:h|w|size|px|py|p|pt|pb|pl|pr|gap|gap_x|gap_y|rounded)\(\s*'
    r'(?:gpui::)?px\(\s*[0-9]'
)
INTERACT = re.compile(r'\.(?:on_mouse_down|on_click|cursor_pointer)\(')
HEAD = re.compile(r'\b(?:div|h_flex|v_flex)\(\s*\)')
ESCAPE = 'ui-raw: ok'


def chains(path):
    """切出「一条表达式」：起点 = 含 div()/h_flex()/v_flex() 的行，
    终点 = 以 ; 结尾的行（rustfmt 后链式调用每行一个方法，按行切足够稳）。"""
    lines = open(path, encoding='utf-8', errors='replace').read().splitlines()
    out, cur = [], []
    for i, line in enumerate(lines):
        text = line.strip()
        if not cur:
            if HEAD.search(text):
                cur = [(i + 1, line)]
                if text.endswith(';'):
                    out.append(cur)
                    cur = []
            continue
        cur.append((i + 1, line))
        if text.endswith(';'):
            out.append(cur)
            cur = []
    if cur:
        out.append(cur)
    return out


def scan():
    """返回 {path: (violations, escaped)}，path 相对仓库根、正斜杠。"""
    report = {}
    for root in SCAN_ROOTS:
        for dirpath, _dirs, names in os.walk(os.path.join(ROOT, root)):
            for name in names:
                if not name.endswith('.rs'):
                    continue
                full = os.path.join(dirpath, name)
                rel = os.path.relpath(full, ROOT).replace(os.sep, '/')
                if rel.startswith(EXEMPT_PREFIXES):
                    continue
                bad, esc = [], []
                for chain in chains(full):
                    body = '\n'.join(text for _ln, text in chain)
                    if not INTERACT.search(body) or not GEO.search(body):
                        continue
                    if ESCAPE in body:
                        esc.append(chain[0][0])
                    else:
                        bad.append(chain[0][0])
                if bad or esc:
                    report[rel] = (bad, esc)
    return report


def load_baseline():
    base = {}
    if not os.path.exists(BASELINE):
        return base
    for raw in open(BASELINE, encoding='utf-8').read().splitlines():
        raw = raw.strip()
        if not raw or raw.startswith('#'):
            continue
        parts = raw.split()
        if len(parts) == 2 and parts[1].isdigit():
            base[parts[0]] = int(parts[1])
    return base


def write_baseline(report):
    lines = [
        '# 「页面禁手搓控件」存量基线（scripts/check_ui_raw.py）',
        '# 格式: <相对路径> <命中数>；只允许人工 --update-baseline 重写。',
        '# 目标：逐模块把这些数字清零（先搬 ui/kit，再删行）。',
    ]
    for path in sorted(report):
        lines.append(f'{path} {len(report[path][0])}')
    open(BASELINE, 'w', encoding='utf-8', newline='\n').write('\n'.join(lines) + '\n')


def main():
    # CI runner（windows-latest）stdout 默认 cp1252：下面的中文 OK/违规行会
    # UnicodeEncodeError 直接炸守卫。统一强制 UTF-8（本地 UTF-8 终端不受影响）。
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, 'reconfigure'):
            stream.reconfigure(encoding='utf-8', errors='replace')

    report = scan()
    debt = sum(len(v[1]) for v in report.values())

    if '--update-baseline' in sys.argv:
        write_baseline(report)
        print(f'baseline 已更新：{len(report)} 个文件 / '
              f'{sum(len(v[0]) for v in report.values())} 处命中（另有 {debt} 处已声明豁免）')
        return 0

    if '--list' in sys.argv:
        for path in sorted(report):
            bad, esc = report[path]
            if bad:
                print(f'{len(bad):3d} {path}  行 {", ".join(map(str, bad))}')
            if esc:
                print(f'  ok {path}  行 {", ".join(map(str, esc))}（{ESCAPE} 声明）')
        print(f'\n合计 {sum(len(v[0]) for v in report.values())} 处违规 / {debt} 处豁免')
        return 1 if sum(len(v[0]) for v in report.values()) else 0

    base = load_baseline()
    # 基线卫生：键指向不存在的文件 = 文件拆分/删除后基线未重写（0.5.0 发版实录：
    # 41 处旧债挂死路径 + 78 处新路径无档 → 42 文件批发假红，靠 worktree 对账人工
    # 裁定）。死路径当场硬失败，把漂移前移到拆分现场，不再攒到发版日结算。
    dead = [p for p in base if not os.path.exists(os.path.join(ROOT, p))]
    if dead:
        print('VIOLATION(基线卫生): %d 条基线指向已不存在的文件——'
              '拆分/删除后须人工 --update-baseline 重写：' % len(dead))
        for p in sorted(dead):
            print(f'    {p}  {base[p]}')
        return 1
    violations, cleared = [], []
    for path, (bad, _esc) in sorted(report.items()):
        allowed = base.get(path, 0)
        if len(bad) > allowed:
            violations.append((path, allowed, len(bad), bad[allowed:]))
        elif len(bad) < allowed:
            cleared.append((path, allowed, len(bad)))

    if cleared:
        print('提示: 以下文件命中数已低于基线，可下调 baseline（别让债务藏在数字里）：')
        for path, was, now in cleared:
            print(f'    {path}: {was} → {now}')

    if violations:
        print(f'VIOLATION(ui-raw): {len(violations)} 个文件新增手搓控件（交互链里的裸几何字面量）：')
        for path, allowed, now, lines in violations:
            print(f'    {path}  基线 {allowed} / 现在 {now}  行 {", ".join(map(str, lines))}')
        print('  修法：改用 crate::ui::kit::* 的组件，或至少换成 ui::tokens 常量；')
        print('  确属特殊控件 → 该链内写 `// ui-raw: ok <理由>`，并在 §10 表里登记尺寸。')
        print('  **不要**自己跑 --update-baseline 掩盖。')
        print(f'  （另有 {debt} 处已声明豁免的债务，--list 可看）')
        return 1

    new_files = [p for p in report if p not in base]
    if new_files:
        print(f'VIOLATION(ui-raw): 新文件带手搓控件：{new_files}')
        return 1

    print(f'ok（基线内 {sum(v[0].__len__() for v in report.values())} 处存量，'
          f'豁免 {debt} 处）')
    return 0


if __name__ == '__main__':
    sys.exit(main())
