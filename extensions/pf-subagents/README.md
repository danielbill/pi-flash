# pf-subagents

Subagent delegation for Pi. Call mechanics follow [dsh-subagent](https://github.com/deepseek-ai/deepseek-harness/tree/master/packages/subagent);
execution stays pi-native — the child is a real pi session and the settlement is a real session message.

Design: `docs/模块设计/44-子代理/044-5-subagent管理.md` (dispatch/settlement contract in §4).
Tests: `44-子代理/044-4-subagent验收清单.md`. Parity with dsh: [`PARITY.md`](./PARITY.md).

## What M1 ships

- `agent` tool (`namespace: subagents`) dispatches and returns the child id immediately —
  a delegation never parks the parent's turn, so the user can always talk to it. When the
  child settles the notice (`subagent-completion`) enters the parent session — idle parents
  are woken, streaming parents get it steered into the current turn (044-5 §4).
- `blocking: true` is the opt-in for programmatic callers (code-mode fan-out) that must
  read `structuredContent.text` inline; it parks the turn for the child's whole run.
- Providers: `spawn` (fresh child context), `fork` (inherits completed history).
- Activation lifecycle: reserve identity and capacity → publish only after the child
  accepted the task → settle after cleanup → close admission on settle.
- Per-session online quota that rejects instead of queueing (default from
  `<profiles dir>/config.json`, see **Module config** below).
- Depth limit from the same file, enforced before any resource is taken.
- **Capacity scope = the delegating session.** dsh charges a whole tree to its
  root pool via `resident.get(parent.id)?.pool ?? rootPool(parent)`; that needs the
  parent to be a live registration in one process-wide manager. Here one pi
  process serves one PF session (`crates/app/src/agent_session.rs:39`) and child
  sessions only mount extensions named by their profile, so a child has no `agent`
  tool and depth is capped at 1 — per-session and per-root are the same bucket.
  `PARITY.md` records the exact precondition that would have to change.
- **Depth survives a reload.** The child's `subagent/child` marker carries
  `depth`, and `session_start` restores it (`restoreChildDepth`) — the same role
  dsh's agent header plays in `depth.ts`.
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
