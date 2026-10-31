# pf-subagents

Subagent delegation for Pi. Call mechanics follow [dsh-subagent](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/subagent);
execution stays pi-native — the child is a real pi session and the result is a real tool result.

Design: `docs/模块设计/045-subagent管理.md`. Plan: `045-1-subagent开发计划.md`.
Tests: `045-2-subagent测试设计.md`.

## What M1 ships

- `agent` tool (`namespace: subagents`), caller delivery: the call awaits the child.
- Providers: `spawn` (fresh child context), `fork` (inherits completed history).
- Activation lifecycle: reserve identity and capacity → publish only after the child
  accepted the task → settle after cleanup → close admission on settle.
- Per-parent online quota that rejects instead of queueing (default 8,
  `PI_SUBAGENTS_CAPACITY`).
- Depth limit (default 1, `PI_SUBAGENTS_MAX_DEPTH`), enforced before any resource is taken.
- Profile documents: `~/.pi/agent/agents/<id>.md`, `.pi/agents/<id>.md` (project shadows global).
- Versioned descriptor custom entry (`subagent/descriptor`) + orphan reconciliation on `session_start`.
- Stable failure codes, see `src/errors.ts`.

## Not in M1

Background (`parent`) delivery notifications, codemode fan-out, worktree isolation,
external backends (ACP / Claude Code / Codex), aggregation panel.

## Test

```bash
node --test "test/*.test.ts"
```

No install step: Node 24 runs the TypeScript directly and the SDK is imported lazily,
so the suite runs without `@earendil-works/pi-coding-agent` resolving.
