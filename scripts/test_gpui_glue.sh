#!/usr/bin/env bash
# 跑 vendor/gpui 里的滚屏几何锁定测试（elements::list::test）。
#
# 为什么单独一个脚本：vendor/gpui 不是 workspace 成员——它的 examples 进了
# members 会让根目录 `cargo test` 连示例一起编（示例依赖 inspector/reqwest
# 等，编不过）。这里临时挂上成员、只跑 --lib，trap 保证还原 Cargo.toml/lock。
#
# 锁的是什么：贴底胶水（Bottom 对齐、logical_scroll_top=None）把末条底边钉在
# `viewport.bottom − padding.bottom`。pi-flash 的翻页垫片高度公式
#   spacer = (视口高 − padding.top − padding.bottom) − 锚下内容高
# 就是这个恒等式的逆解——列表内边距一改，钉顶位置立刻偏 157px（消息滚出屏顶）。
# 另锁 `measured_height_in`：区间高度和，未测量条目按 0 计（胶水态下量锚点）。
set -euo pipefail
cd "$(dirname "$0")/.."

cp Cargo.toml Cargo.toml.gluebak
cp Cargo.lock Cargo.lock.gluebak
restore() {
  mv Cargo.toml.gluebak Cargo.toml
  mv Cargo.lock.gluebak Cargo.lock
}
trap restore EXIT

python - <<'PY'
p = 'Cargo.toml'
s = open(p, encoding='utf-8', newline='').read()
s = s.replace(
    'members = ["crates/pi-link", "crates/app"]',
    'members = ["crates/pi-link", "crates/app", "vendor/gpui"]',
)
open(p, 'w', encoding='utf-8', newline='').write(s)
PY

cargo test -p gpui --lib -- elements::list
