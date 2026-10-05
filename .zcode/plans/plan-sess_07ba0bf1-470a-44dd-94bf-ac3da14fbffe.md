# 滚屏算法重构:抽独立模块 + 发送帧翻页修正

## 诊断(为什么现在钉不上顶)

钉顶完全依赖列表尾部的 spacer 垫片把「贴底跟随位」垫到用户消息顶。spacer 高度 = 视口高 − 锚下内容,而锚下内容用 `bounds_for_item` 取自**上一帧**布局——发送帧刚 splice 进来的用户消息是 Unmeasured,`bounds_for_item` 返回 None → `spacer_ready=false` → 垫片不挂载 → gpui Bottom 对齐铁律(list.rs:674-716:内容填不满视口就强制 `logical_scroll_top=None` 贴底)生效 → 用户气泡停在屏幕底部,历史消息还在屏上。发送路径从头到尾没调过 `scroll_to`,修正只能等下一个 pi 事件,甚至整轮不落位。

## 新架构:新增 `crates/app/src/session/chat_list.rs`

滚屏算法全部收进 `ChatList` 结构体(runtime 只剩 `pager: ChatList` 一个字段):

```rust
pub(crate) struct ChatList {
    pub state: ListState,                  // gpui 列表(Bottom 对齐)
    at_bottom: Rc<Cell<bool>>,             // scroll handler 维护
    anchor: Rc<Cell<Option<usize>>>,       // 钉顶锚点(用户消息条目 ix)
    spacer_px: Cell<f32>,                  // 渲染闭包读
    listed: Listed { msgs, phase, spacer } // splice 记账
}
```

函数级 API(每个决策一个具名函数):
- `page_turn(ix, msgs, phase)` — **发送帧翻页**(核心修正,见下)
- `sync(msgs, phase)` — 原 notify_list 的 splice/reset 编排,内部拆成 `grow_msgs` / `toggle_phase` / `mount_or_unmount_spacer` / `reset_if_mismatch` / `remeasure_tail` 私有小函数
- `settle_spacer()` — 精确垫片:bounds 可测时 spacer = 视口 − 锚下内容,然后逻辑位回 None(贴底胶水无缝接棒)
- `release()` — 锚点退役(AgentEnd/快照重建/发送失败)
- `reload(msgs)` — 会话切换/tail 预渲染:reset + 计数同步(替换现在裸调 `list.reset` 导致计数失配的隐患)
- `jump_to_bottom()` / `reveal(ix)` / `is_at_bottom()` / `anchor_active()` / `spacer_px()` / `scroll_top_ix()`

## 翻页算法(修正核心)

**发送帧**(不等回显、不等 bounds):
1. `anchor = Some(ix)`
2. 挂载 spacer,高度 = 视口高(过估,保证锚下内容 ≥ 视口)
3. `state.scroll_to(ListOffset { item_ix: ix, offset_in_item: 0 })` 硬置顶——逻辑位 Some,直接绕开 Bottom 贴底铁律
4. `sync(...)` 落账

**下一帧**(回显/任意事件):用户消息已测量 → `settle_spacer` 算出精确高度 → 逻辑位回 None,贴底胶水在同一几何下接棒,流式期间随内容增长自然下滑跟随(现状设计保留,只是首帧不再失位)。

**其它语义**:
- 滚轮上翻(锚点期)→ `release()`:尊重用户滚动,锚点退役、「回到最新」按钮出现(scroll handler 内建)
- AgentSettled/AgentEnd/快照重建/prompt 失败 → `release()`
- steer(流式中再次发言)同样走 `page_turn`,再次翻页

## 逐文件改动

| 文件 | 改动 |
|---|---|
| `session/chat_list.rs` | **新增**:结构体 + 上述函数 + 内联 `#[cfg(test)]` |
| `session/runtime.rs` | 字段 `list/list_at_bottom/prompt_anchor/prompt_spacer_px/listed_*` → `pager: ChatList`;`notify_list`→`pager.sync`;send_input/回显路径→`page_turn`;5 处 `prompt_anchor.set(None)`→`release()`;`scroll_to_bottom`→`jump_to_bottom`;`locate_message`/`apply_pending_locate`→`reveal` |
| `session/mod.rs` | 渲染闭包读 `pager.anchor_active()/spacer_px()`;nav gutter 读 `pager.scroll_top_ix()`、点击跳转走 `pager.reveal`;声明模块 |
| `session/input.rs` | `list_at_bottom.get()` → `pager.is_at_bottom()` |
| `actions_sessions.rs:73` / `actions_panels.rs:341` / `main.rs:614` | `r.list.reset(n)` → `r.pager.reload(n)` |

## 测试与验证

- `chat_list.rs` 单测(沿 app crate 惯例,纯记账逻辑,ListState 的 splice/reset/item_count 不需要窗口):msgs 增长 / phase 开关 / spacer 挂卸 / reset 后计数同步 / page_turn 后条目数 = msgs+phase+spacer / scroll_to 越界钳制
- `cargo check -p app` + `cargo test -p app` 全绿
- 手动验证清单:长历史会话发送→立即翻页置顶;空会话首条→置顶;流式中 steer→再次翻页;滚轮上翻→按钮出现;点按钮→回底;agent 结束→贴底显示结尾

beads 记一条 refactor issue,完工关闭。不改 pi-link、不动协议层。