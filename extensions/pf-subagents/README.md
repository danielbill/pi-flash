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
- Per-lineage online quota that rejects instead of queueing (default from
  `<profiles dir>/config.json`, see **Module config** below).
- Depth limit from the same file, enforced before any resource is taken.
- **Capacity scope = lineage root.** Every delegation names its root (`rootId`,
  required — a missing one is rejected, never guessed), and quota is charged
  there, so one tree never holds more than `capacity.default` live descendants in
  total. This mirrors dsh's
  `resident.get(parent.id)?.pool ?? rootPool(parent)`. Pool sets live in one
  process-wide registry (`Symbol.for("pf-subagents.capacity.pools")`) because
  every child session mounts its own manager instance; tests clear it with
  `resetPoolRegistry()`. Pools are process-local: a child spawned in another OS
  process would count separately.
- **Lineage survives a reload.** The child's `subagent/child` marker carries
  `depth` + `rootId`, and `session_start` restores them (`restoreChildLineage`),
  so a reopened child still cannot out-delegate its depth or open a fresh pool.
- Profile documents: `~/.pi/agent/agents/<id>.md`, `.pi/agents/<id>.md` (project shadows global).
- Versioned descriptor custom entry (`subagent/descriptor`) + orphan reconciliation on `session_start`.
- Stable failure codes, see `src/errors.ts`.

## Not in M1

Background (`parent`) delivery notifications, codemode fan-out, worktree isolation,
external backends (ACP / Claude Code / Codex), aggregation panel.

## Module config

`<profiles dir>/config.json` (i.e. next to the profile `.md` files) is the **single
source of truth** for delegation limits and model defaults. It is internal: it is
not a tool parameter and never appears in a tool description, so a delegating
model cannot raise its own budget.

```json
{
  "capacity": { "default": 5, "min": 2, "max": 10 },
  "maxDepth": { "default": 1, "min": 1, "max": 3 },
  "maxTurns": 100,
  "model": "deepseek/deepseek-flash"
}
```

- `capacity` / `maxDepth` carry both the **value** and its **bounds**. Code only
  validates and clamps — there is no second, hardcoded ceiling. To allow more
  concurrency, raise `max` in the file.
- `default` must lie inside its own `min..max`; a self-inconsistent document is
  rejected with `MODULE_CONFIG_INVALID`.
- `maxTurns` is the turn budget for a child when neither the call nor the profile
  sets one (`0` = unlimited).
- `capacity` / `maxDepth` bounds are enforced by `src/module-config.ts`; the
  quoted values here are the ones shipped for PF (`pi-flash-dev/agents/config.json`).
- `model` is the `provider/model` fallback for profiles that name none.
- Unknown keys are ignored, so hosts can annotate the file (`version`, UI hints).
- Read fresh on every delegation entry: an edit applies without restarting the
  host. A missing or broken file falls back to conservative defaults **without**
  overwriting the limits currently in force.

## Test

```bash
node --test "test/*.test.ts"
```

No install step: Node 24 runs the TypeScript directly and the SDK is imported lazily,
so the suite runs without `@earendil-works/pi-coding-agent` resolving.
