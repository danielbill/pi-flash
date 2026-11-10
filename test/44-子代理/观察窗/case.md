# 44-子代理 · 观察窗（子会话打开路由）

对象：PF app 侧子会话打开链路（`crates/app/src/session/pool/open.rs`）。
2026-11-10「挤占主会话 tab」返工后的固定口径：**身份由调用意图携带**
（`open_subagent_session_window` 唯一入口），不在打开层用文件嗅探反推。
`open_session` 只留防呆闸（marker 命中即改道观察窗）。

## W1 卡片点击开观察窗，不抢固定段

- 步骤：主会话委派 ≥2 个子代理；逐个点击转录里的子代理卡。
- 期望：每次都在自由区**末尾**新增任务名 tab（bot 图标），固定段主会话
  tab 恒在第一位；重复点同一张卡只激活不重复 push。

## W2 文件缺失/半写时不误判

- 步骤：委派后**立刻**点卡（子会话文件未落盘或只有头部条目）；以及构造
  头部 64KB 窗口切在多字节字符中间的会话文件（回归测试
  `pi-link::sessions::info::is_subagent_session_survives_multibyte_head_cut`）。
- 期望：两种情况都开只读观察窗（identity 意图直达，不嗅探）；永不出现
  「按普通会话抢占固定段」。自动：上述回归测试。

## W3 观察窗 tab 激活与关闭

- 步骤：观察窗与主会话互切；点 tab × 关闭；右键批量关。
- 期望：互切无损；关前台观察窗回主会话；观察窗永不写 last-open、
  不进最近列表、不 spawn pi 进程。

## W4 未知入口改道

- 步骤：会话查询命中子会话 → 点结果跳转；`session.open` 自动化 op 传
  子会话路径；（历史污染场景）last-open 指向子会话后切项目/重启。
- 期望：三条路全部落只读观察窗（防呆闸 + SearchHit.subagent +
  `startup::restorable_session` 共享闸），重启不恢复成子会话。
