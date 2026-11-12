# Changelog

All notable changes to released versions live here; see `docs/CHANGELOG规范.md` for the format.

## [0.5.1] - 2026-11-12

Highlights of this release:
1. Unified input menu: the + button opens one menu aggregating skills & commands, models, files and sessions, with universal search
2. Editor input assistance aligned with Zed: bracket/quote autoclose & paired deletion, auto-surround, list continuation, paste re-indent, syntax-aware indentation
3. A batch of editor selection/keyboard and rendering fixes

### Install & Update

```bash
npm install -g pi-flash@latest   # same command for first install and update
pi-flash
```

Requires Node.js ≥ 22.19. Alternatively download the zip portable build, extract it and double-click `pi-flash.exe`. Windows x64 only.

### New
- **Unified input menu**: the "+" button in the composer toolbar opens one unified menu with tabs aggregating `/` skills & commands, `$` models, `@` project files and `&` sessions; the universal search at the top matches all categories at once; with no prefix typed, "+" pins straight to the skills page; the "open file" button invokes the system file picker and inserts the pick as @path; the attachment button becomes "+" (image attachments now go through Ctrl+V paste). (73a0c790, f4b9130d)
- **Session reference chip**: a session path inserted via `@` now folds into an "icon + session name" chip instead of a long path; session cards gain an "@ to input box" button for one-click referencing. (4496e0e4, 559588ad)
- **Input assistance (aligned with Zed, code-editor mode)**:
  - Autoclose family: typing a bracket/quote with an empty selection inserts the closing pair, typing the same closer overtypes instead of inserting, backspace deletes the pair; typing an opener with a selection auto-wraps it (brackets/quotes/`*`/`~`); Enter inside a bracket pair creates the three-line structure;
  - List & comment continuation: Markdown `-` `*` `+` list continuation, `1.` increments, task item continuation, Enter on an empty item exits the list; comment continuation (`//`, `#`, …);
  - Paste enhancements: multi-line paste re-indents to the cursor line's indentation, pasting a URL over a Markdown selection becomes `[selection](url)`, backspace in an indented region steps back one tab stop; copy/cut with an empty selection takes the whole line;
  - Syntax-aware indentation: a line-start closing bracket aligns with its opening line after Enter; indent-aware newlines. (dd113e6c, 007274cc, f90938f7, 005c399a)

### Bug fixes
- Editor key hijacking: stale message selections no longer hijack Ctrl+C (selection copy takes priority); Ctrl+←/→ restores word-wise movement; Tab restores block indent for multi-line selections. (fca63183)
- Under soft wrap, Shift+↑/↓ extends the selection line by visual line while keeping the column, instead of jumping by paragraph. (9b4b7b5b)
- Single tilde `~x~` in Markdown is no longer rendered as strikethrough. (e210f7cf)
- Mermaid fences inside agent finalization blocks render as diagrams again. (5c94d7ac)
- The `@` menu treated CJK directory names like "内存" as whitespace and went dead; quoted directory drilling left one stray quote per level; menu items are now a fixed 30px tall. (d25fc137, 3cde3071, 5b449583)
- Mojibake for Chinese file names in the zip portable build (0.5.0: "启动说明.txt" extracted as garbage names) — entry names forced to UTF-8 with triple defense plus a hard packaging gate. (0ad4a686)

### Improvements
- Input menu visual polish: font sizes aligned to panel font slots, dimmer hints, larger + button with a symmetric hover base, tab bar flush with the search bar, toolbar background corners unified at 4px, over-long paths clipped inside item boxes. (cb99b6aa, be97266b, 6eb0f577, 20797d93, 8287ff05, a48c8e48, b1d486d8, 1dec3ae3, acbf4d83, ee2070fd)
## [0.5.0] - 2026-11-11

Highlights of this release:
1. New async subagent system
2. New plan mode
3. New self-debugging tools

### Install & Update

```bash
npm install -g pi-flash@latest   # Same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19. A portable zip is also available: download, unzip, and double-click `pi-flash.exe`. Windows x64 only.

### New
- **Async subagent system (044)**: async subagent task dispatching, a redesigned navigation panel + sub-session observer tab, `resumeFrom` cold-resume continuation; the concurrency-capacity source of truth moved to PF's own `agents/config.json` (2–10, default 3, depth fixed at 1); the subagent profile directory moved to `<PF data dir>/agents` (overridable per project via `.pi-flash/agents`, 96bb7e82); delete confirmation + background recycle-bin cleanup; finer-grained error codes (DEPTH_LIMIT / NOT_RESUMABLE, etc.). (96bb7e82)
- **Plan mode (031)**: toggle with `/plan` + composer indicator; one-click approve/reject on plan proposal cards; the `plan_mode.ts` P0 gate script ships with the package.
- **PFContext session-context foundation (001, new crate `pf-context`)**: settings-snapshot single writer + atomic writes + watcher, `ext-context.json` projected to extensions, SQLite WAL domain store, `pfctx-manager` daemon + rotating pfctx.log.
- **PFContext-based self-debugging tools**.
- **PF plugin host extracted into its own crate (`pf-plugin-host`, 090-4)**.
- Session input persists across restarts: drafts/attachments are stored per project and survive reopening; the new-session page keeps a stable per-project draft.

### Bug fixes
- **Streaming output stalls**: event-pump batching + notify throttling; implicitly installing a disabled plugin no longer blocks the first frame (streaming at ~116 t/s).
- Input lag: spinner `repeat()` animation is now frame-limited (with_fps).
- AskCard: pressing Enter no longer skips two questions at once; after finishing, a broken window no longer pops up again.
- The subagent observer tab no longer squeezes out the main session tab; the third nav row / hover menu no longer goes dead (the descriptor dropped `parentToolCallId`).
- The taskbar icon could not be clicked after being updated; startup hangs (wasm signature verification moved off the main thread).

### Improvements
- Session bottom status bar: the per-plugin status visibility toggle moved into Settings›Interface (034 wrap-up); optimistic sending (optimistic_send).
- The session-flow subagent card now matches the session-list style: status marker + redesigned completion notice (044-0 acceptance ⑤).
- Performance and tidy-up: oversized files such as the three market pages were all split to ≤500 lines (zero behavior change).
## [0.4.3] - 2026-11-05

Highlights of this release:
0. A bottom status bar for the session area: extension status (usage, MCP, etc.) is now fixed in place, clickable, and per-plugin visibility is toggleable
1. Model pricing in the model detail dialog: official prices pulled from models.dev, with manual override
2. When a background session is waiting for your answer, its session row shows an orange-dot badge plus a toast
3. A batch of interaction fixes and performance work

### Install & Update

```bash
npm install -g pi-flash@latest   # Same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19. A portable zip is also available: download, unzip, and double-click `pi-flash.exe`. Windows x64 only.

### New
- **Bottom status bar for the session area**: session status set via ctx.ui.setStatus moves from a bare one-liner above the input box into a designed slot at the bottom of the session area; multiple entries are separated by `|` with overflow ellipsized on a single line; clickable items jump straight to their target (usage → /usage, MCP → Settings›MCP), and command-type entries are disabled while running with a tooltip; the `[...]` menu at the right edge toggles per-plugin status visibility (takes effect live, persisted); a master switch sits right of MCP in the composer toolbar (on by default). (949a2adc, c01ab9cf)
- **Model pricing management**: the model detail dialog gains four price fields (input/output/cache read/cache write, $/MTok) — filled automatically from the official models.dev price source (raw + jsdelivr dual channel, 7-day cache, falls back to stale cache offline); catalog prices only fill empty/zero slots; a hand-entered value is an explicit override, clearing it falls back to the catalog price; the dialog is correspondingly taller (500×600). (98260a32, 6a74c513, cbe86e71, 903393f8)
- **Background session waiting-for-you notice** (pi-web notification parity): when a background session's extension dialog is waiting for your answer, its session row shows an orange-dot badge plus a toast "Background session "X" is waiting for your answer", reminded once per request. (d599bdf2)
- **Session-list phase row**: a running session shows "Working + elapsed timer", a waiting one shows "Waiting"; the running spinner is now an 8-spoke ring (ZCode-style starburst). (00df351a, a67734c0, eaabe8f7)

### Bug fixes
- AskCard: the preview area is now full-height with scrolling for long content (the old 240px cap truncated it); custom answer rows with empty descriptions keep their height, rows align, and a selected row can be deselected. (e6e2c809, 75ba968d)
- Tool/thinking pill popup menus: unified anchor positioning (dropping the right-aligned special case), uniform 10px padding, row spacing pinned back — no more squeezing. (6e0a5d2e, 2b45bd38, 97df83b5)
- Editor: selecting inline code in a message no longer triggers a file open; directories are no longer misread as files with "Access denied". (dd3c9ae3)

### Improvements
- Performance: fixed an 11x models.json disk-read storm per keystroke in the model detail dialog; settings-page listing dropped from 3-5ms to 1.5ms per frame with a real directory. (f0582e40, 5ae62a58)
- Session-list row height 32 → 34px for a larger hit target; the accent bar is separated from the row body with a gap between adjacent bars. (fd8992c8, 7e710d03)
## [0.4.2] - 2026-11-04

Highlights of this release:
0. Fixed saved API keys becoming unretrievable after the 0.4.0→0.4.1 upgrade (credential markers de-pathed)
1. 8 built-in slash commands added: /compact etc. are now visible end-to-end, with acceptance feedback and a before/after token comparison on completion
2. Extension UI now matches pi-web: status bar, window title, keyboard navigation for select dialogs, multi-line editor
3. After install, the bundled pi kernel registers as the system-wide `pi` command — typing `pi` in a terminal now runs the 1.5 kernel
4. Faster first launch after upgrades, with self-healing for a broken stable entry

### Install & Update

```bash
npm install -g pi-flash@latest   # Same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19. A portable zip is also available: download, unzip, and double-click `pi-flash.exe`. Windows x64 only.

### New
- **8 built-in slash commands implemented client-side** (pi-web handleBuiltinSlashCommand parity): built-in commands like /compact and /clear now appear in the slash menu (they used to be invisible among extension commands), and their execution is visible end-to-end — acceptance shows a toast, completion shows a before/after token comparison, and failures show pi's rejection reason. (9f079fba, 598f4d41, 0df04c62, 95414e7a)
- **Extension UI surface, pi-web parity as a batch**:
  - ctx.ui.setStatus: session status set by an extension now renders in the UI (ExtensionStatusBar parity). (4d43815b)
  - ctx.ui.setTitle: maps to the OS window title (document.title parity). (6db66241)
  - Blocking select dialogs get keyboard navigation: ↑↓ moves the highlight (wrapping), Enter selects. (d17a0666)
  - Editor dialogs switched to a multi-line input with Enter to submit; prefill supports real newlines. (0388a9c4)
- **The bundled pi kernel registers as the system-wide `pi`**: installs a pi.exe forwarding shim plus a user-Path first segment — no uninstalls, no edits, pure shadowing. Typing `pi` in any terminal/script runs the vendored 1.5 kernel; after an upgrade handoff it serves the new kernel immediately. (7199f22f)

### Bug fixes
- **Saved API keys all became unretrievable after the 0.4.0→0.4.1 upgrade**: auth.json marker entries hard-coded the old install path, which broke the moment the app moved; markers are now de-pathed and pif-secret lands in the user-level PATH directory, so retrieval survives upgrades and moves. (6210558a)
- In-flight extension dialogs lingered forever after the pi process died: the pump tail clears the queue and the state bubbles up, so dialogs exit with the process. (663e1e8a)
- An unknown extension UI method (e.g. after a pi upgrade) could block an extension forever: unknown methods now answer cancelled immediately, with a trace. (bdac103a)
- ANSI escape sequences in extension widget/status text were rendered raw: they are now stripped uniformly (PlainTextTheme parity). (6cb0294c)

### Improvements
- **Faster first launch after upgrades**: stable-entry precopy — files the old process doesn't lock are copied as soon as they land, so the new first launch only finishes one big file (pi-flash.exe); in the common case, zero copying. (9d505b59)
- **Stable-entry self-healing, three pieces**: taskbar dead links caused by the npm shell's payload placement failing halfway are auto-repaired on new-version first launch; redundant versioned directories are cleaned up into one; the global pi pointer re-pins to the stable entry. (4dc4dd40)
- Final feedback for user actions now goes through the unified notify_toast channel. (bc0aa429)
## [0.4.1] - 2026-11-03

Highlights of this release:
0. Fixed sessions freezing after running extension commands (issue #7)
1. Fixed duplicate tray icons (double WeChat) after installing a plugin
2. Fixed MCP server changes not taking effect while the session is idle
3. Session rows now show a pin badge, capped at 5 badges per row

### Install & Update

```bash
npm install -g pi-flash@latest   # Same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19. A portable zip is also available: download, unzip, and double-click `pi-flash.exe`. Windows x64 only.

### New
- Session-row badge assembly: pinned sessions show a pin badge pinned to the far right, plugin badges follow in registration order, capped at 5 per row (rightmost kept). (031887ec)

### Bug fixes
- **Sessions freezing when running extension commands (issue #7), fixed as a batch**:
  - The waiting row kept spinning forever ("Waiting for model...") after an extension command consumed the prompt. (2813b991)
  - Multiple blocking extension dialogs covered each other, permanently blocking earlier requests; they now queue and are answered in order, with the queue count shown in the dialog header. (a7830a15)
  - "Running extension command..." lingered for seconds after closing the dialog, and finishing a command wiped the whole screen: settlement now matches pi-web, bubbles are removed based on disk truth, and Stop cancels in-flight dialogs. (c9e48c95)
  - Local-queue receipts swallowed the handled settlement signal, leaving the state stuck forever (root cause); extension dialogs redesigned as a 600×600 dual-pane scrolling shell so long content no longer pushes the close button out of view. (b9e2edce)
- During slash-command execution the waiting row now shows "Running extension command..." instead of the misleading "Waiting for model...". (f07c911f)
- Duplicate function-bar icons (double WeChat) after installing a plugin: runtime entry uniqueness enforced, and ghost double-loading from legacy flat-layout residue eradicated. (2fee18f1, 512d43ae)
- Tools no longer go missing after unloading an extension mid-session and remounting it — direct tools are restored per the transcript ledger. (608b6f19)
- MCP server changes (toggle/remove/bulk/save/rename) did not apply to already-open sessions — a live process never re-read mcp.json on its own; changes now mark the session for rebind. (31aa0200)
- Session-row pin badge flickered on hover. (1ff354cf)
- Provider picker layout: fixed 896px content width centered, card grid left-aligned with group titles. (21af6cbd, ae5e293b)
## [0.4.0] - 2026-11-03

Highlights:
0. Upgraded the bundled pi kernel to 1.5
1. Fixed the 0.3.1 bug where built-in providers could not list their models directly
2. Added custom providers and models; auto-fill now sources model parameters from OpenRouter instead of models.dev
3. Breaking upgrade of the plugin framework with new plugin lifecycle management — installed plugins must be upgraded along with PF
4. New "WeChat Bridge" plugin on the plugin side, now listed in the PF internal plugin center
5. Quick Panel upgraded to 0.1.5
6. Restart recovery now restores the whole workspace — every file open before closing is reopened — instead of just the session

### Install & Update

```bash
npm install -g pi-flash@latest   # same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19. A portable zip is also available: unzip and double-click `pi-flash.exe`. Windows x64 only.

### New
- Custom providers and models
- Plugin updates: market cards and the "Installed" page compare versions and show an "Update" button; disable → overwrite → reload in one step. Plugin data directories now live outside the version layer so updates keep data (old layouts migrate automatically on first launch). (aed83a83, c4107496, 3d03df58)
- Plugin manifests support an `engines.pf` minimum version: installs are refused on older PF, and already-installed incompatible plugins show a toast at startup. (1652bd1a)
- Startup restores the tab strip: file / terminal tabs survive restarts. (124ac458, 7907e2bb)
- Plugin contract extensions (for plugin developers): top-bar menu items generated from `contributes.topbar_menu` in the manifest, `dock=tab` full-page plugin tabs, session-slot claiming, canvas primitives and component state broadcast, lifecycle v5 (unload). (06ea39aa, 05cb4665, 81eb7ce8, 7210fde3, 79f997c3)
- Market card preview images support the manifest `icon` field. (2fcd4c84)

### Bug fixes

- **Credential helper `pif-secret.exe` now ships in the package**: the 0.3.1 package was missing it, so API keys saved via Settings might not be retrievable after upgrading; this release bundles it and adds an end-to-end automated guard. (aed927e8, 3edbbd64)
- Built-in providers fetch their model catalog again right after saving an API key (root-caused a 27-provider whitelist gap), with a static catalog fallback (42 providers / 1,599 models) — save the key and the models are there. (e34cb0b1, 567fbb8f, 97cb77f8)
- Custom providers: models found by the speed test but chat unresponsive, "available models 0/0", "+ Model" not persisted, key blank after refresh, renaming during speed test creating duplicate providers, `/v4`-style version segments wrongly appended with `/v1`. (aafca8f4, 6a3373ad, 08a8e995, 66520120)
- Settings › Models heavily reworked; `models.json` = user selection: the input-panel model menu and the Settings list now share one source; fixed built-in providers lingering in the left list after removal, static model toggles of built-in providers not responding, and custom models losing parameters on off/on round-trips. (4ea0d903, 3a2f2042, 9907daf0, 993e3a10, d93a2df0, 253ca038)
- Model details: clicking a row opened a different model, names mixed up, read-only inputs not refreshing, stale values after closing, auto-fill failing on aliases and `-latest` names. (e7e23ea5, 044188a8, 9c716320, 67486028, 628ab27f, 57ec2029)
- Reasoning level was locked to off. (3f2d74ab)
- Creating a provider occasionally stalled for tens of seconds while reading the registry. (b20632df)
- Providers registered by extensions leaked into the custom-provider list. (0e1f0740, bd747d61)
- Market: blank window when filtering the pi native market by type (~20s → 0), honest page counts, filtering now re-queries; version badge wrapping to the next line; disabled plugins showing the bare package name in the Installed list. (1eff6ceb, 23d46f6d, 5dbc7fd6, 6bc461d8, 00177d60)
- Plugins: disabling a plugin no longer kills the pi process immediately; bound sessions are no longer silently dropped by idle reclamation; stale mount sources after disable/uninstall are invalidated; duplicate wasm load spawns; crash in the context-menu command round-trip. (d5d9e20c, 04ba4d3e, e0cd9e22, 791e2952, 9aff8784)
- In-progress sessions are kept when switching projects. (9219d225)
- New-session model menu and selection: opens with ✓ on the default model row, clicking a row now checks it, selection and sending recover after pi exits; without a starred model the draft pill follows pi's default. (a09686ca, 136ce674, 89633090)
- Models enabled for a custom provider were not persisted (probed models errored in chat and the detail page fell back to read-only); a `models.json` missing the cost field made pi reject the whole file (custom providers vanishing, sends spinning); deleting a provider/model left stale enabledModels entries. (9c6240a1, 0619cb24, 295c5c83)
- Primary button weight unified with the other buttons (CJK fallback scaling made buttons in the same row uneven). (30d3ef3b)

### Improvements

- Bundled pi upgraded 1.4.0 → 1.5.0: zero RPC wire changes, transparent to sessions and extensions. (5f754ed0)
- Extension update detection v2: latest npm versions are fetched via direct registry calls; a failed check no longer disturbs the UI (details only, trace in dev builds). (6aa10bb6, 177118e6)
- Model editor slimmed down to a centered dialog: model ID + auto-fill / context window + max output / capability checkboxes, with [Test] [Remove] [Save] at the bottom. (08cba470, 5fc78269, d8852a28)
- API Type removed: a provider only needs baseUrl + API key; the wire format is inferred from the baseUrl. (2b6ce8df)
- Model toggles no longer enforce "keep at least one". (d3a5c543)
- The built-in WeChat panel is retired; the pf-wxbot plugin takes over fully. (000b8de7, 2f6304e1)
- Removed the Ctrl+Shift+F project-search shortcut. (6985add5)
- The subagents extension is renamed pi-subagents → pf-subagents to avoid the same-named community package on the pi market. (f848fdfa)
- Startup timing probes: `PI_FLASH_PERF=1` prints per-stage durations. (b9f60d4a)
## [0.3.1] - 2026-10-25

### Install & update

```bash
npm install -g pi-flash@latest
pi-flash
```

Requires Node.js ≥ 22.19. Or download the zip portable build and double-click `pi-flash.exe`. Windows x64 only.

### Fixed

- Fixed PF plugin center error: plugins installed from the plugin center failed to load in 0.3.0.
## [0.3.0] - 2026-10-24

This release updates the bundled pi core to 1.4;
ships the "Pi-Flash Plugins" market along with the first "Quick Panel" plugin;
plus a batch of stability and UI fixes.

### Install & Update

```bash
npm install -g pi-flash@latest   # same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19.
You can also download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- Pi-Flash plugin market (090-4): a new "Pi-Flash Plugins" page in the top-bar menu — browse and install Pi-Flash plugins; "All plugins | Installed" tab switch; the Installed tab shows details, uninstall confirmation and an inline enable/disable switch (a disabled plugin disappears from the status bar / file tree / distribution immediately). (e3d62a2, 1454b4d, d6293d3, 9545e84, 1f73360, e1311a5, ff98d1a)
- New "Quick Panel" feature

### Bug fixes
- The PF icon pinned to the taskbar broke after updates: stable AUMID + Start Menu entry with dead-link self-healing. (efa5394)
- Model list silently not refreshing after saving an API key: empty-catalog placeholder + a persistent fetch button + a Failed probe state. (7a7b2dd)
- Two code-block fixes in sessions: stale syntax highlighting after switching themes; add the missing 1px code frame. (be6b17b, ad94775)
- Status-bar tab hover darkened the active card in light theme. (936c3b7)
- Scrollbar cross-talk between the two left-panel areas: dragging one scrollbar moved the other. (4ee6512)
- Gap drift between the "Settings → Extensions" submenu and its parent menu (when long labels widen the card). (72fe285)

### Improvements
- Bundled pi 1.3.0 → 1.4.0. (7a838ff)
- Divider specification unified: 1px resting line, 4px accent hover/drag overlay line (one shared implementation behind the content-area left edge / left dock split / plugin panel), hover hit thickness settled at 12px. (c451b62, e58755c, c8743ec)
## [0.2.3] - 2026-10-22

This release ships the Pi Extension Market: browse, search and install pi extensions without leaving PF;
it also fixes markdown table narrow-column wrapping and revamps the input panel's MCP capsule.

### Install & Update

```bash
npm install -g pi-flash@latest   # same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19.
You can also download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New

- Pi Extension Market (040-2): a port of pi.dev's Package Catalog page — browse and search pi extensions on npm, sort by most downloads / recently updated / name, with paged browsing; the entry lives in the top-bar "Menu". (768da6e, 5f3f634, c16607a)

### Bug fixes

- Markdown tables in sessions could wrap long tokens in narrow columns: column widths were purely proportional with no minimum, so a wide column squeezed narrow ones until a number like 002068 broke into 00206 / 8; table columns now have a pixel floor (minimum width) plus a safety margin absorbing layout-vs-paint stepping differences. (7c4f13e, 53b3e65)

### Improvements

- Input-panel MCP capsule revamped: the "MCP" label is replaced by a server icon + enabled count (e.g. [2]), matching the extensions button; it stays visible even when the count is 0. (52ccd42)
## [0.2.2] - 2026-10-21

I'm quite fond of this release myself —
1. Added a Run action for runnable files in the file tree: bat/ps1/py/js scripts can now be launched right from the file context menu. Super handy, haha — PF has truly become my project manager.
2. Search has been greatly improved.

### Install & Update

```bash
npm install -g pi-flash@latest   # same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19.
You can also download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New

- Project search overhaul (083): file name + full-text content search, low-latency with no index. The results panel mirrors Zed: rounded file-header cards, group headers, right-aligned line numbers with dynamic column width, match highlight background; landing page for the empty state, focus on open, scrollbar added; the old search dialog is retired. (bb00f49, ca86852, f54db1d, afd6179, 6fcbe25, 37b8d89, 3c1beb7, 1cc44db, e9f3474, 7d4f788)
- Session search rebuild (013): a query panel sharing the file-search layout (search only, no replace); upgraded to full cross-project search — sessions from all projects are searchable, with results grouped per session showing the owning project and session name (absolute-time subtitles). (66c3832, 6c3fdfb, e68722c, fec5ffd, d60635e)
- File-tree context-menu Run (021): run script files in a new built-in terminal tab with one click. An extension recipe table covers exe/bat/ps1/js/py/sh; Python resolves via project venv → py.exe → PATH (skipping WindowsApps fake aliases), sh finds bash; menu items show the interpreter name and are greyed out with a reason when resolution fails. (2a8d285)
- Model picker, four fixes: menu widened 40px (model names no longer crowd the star); scrollbar appears when content overflows; ↑/↓ selection auto-scrolls into view; starred default model pinned to top. (295d149)
- The file-tree "search files" tooltip now shows a keybinding chip (Ctrl+L). (4d8de11)

### Bug fixes

- Renamed sessions reverted to their old sidebar title: rename entries were dropped by the 1MB tail-window scan (15MB sessions always hit; 4 of 8 renamed sessions on real machines). Name scanning now rides the streaming message-count scan, covering the entire session file, matching pi's own semantics. (d3cbc89)
- Editor: with terminal/search tabs still open, closing the last document tab now returns to the chat page instead of a blank placeholder. (d2c2258)
- Chat "scroll to bottom" floating button: ghost shadow removed and enlarged (32→36px) for easier clicking. (9124b14)

### Improvements

- The "auto-hide actions" toggle now also covers the two maximize buttons in the panel control bar and file-tree action bar. (e3831f7)
- Session panel search entry demoted to a small icon merged into the right-side button group, subject to auto-hide. (1b95258, d051780)
- Theme "厨房笔记" (Kitchen Notes) renamed to "秋叶" (Autumn Leaves). (af81ffc)
## [0.2.1] - 2026-10-20

This release focuses on crash fixes and performance:
the Chinese markdown editor "process vanishes instantly" crash is fixed;
crashes caused by typing `exit` in the terminal are gone;
the session send-queue system ships — "Queue / Steer" modes configurable while the agent runs;
memory usage optimizations;
and lag when opening very large file directories is fixed.

### Install & Update

```bash
npm install -g pi-flash@latest   # same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19.
A zip portable build is also available: download, unzip, double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New

- Session send-queue: messages sent while the agent is running no longer "always cut in line" — by default they enter a local queue and go out automatically when the current turn ends; the queue card sits above the input panel and supports drag-to-reorder, pencil recall-to-edit, "Send now" interrupt, and trash delete; sending is blocked while the buffer already holds a queued message; the stop button recalls queued messages too; ESC cancels the queue; WeChat remote sends go through the same funnel. Settings → Other adds "Send behavior while running" with two modes: Queue (default) / Steer. (aa5c4fc, 551fa2b, edc23ce, 8f6a1cf, 14e6059, 3367977)
- "Steer / Send now" now means interrupt: the current reply is aborted immediately and re-issued as a new instruction; the half-finished reply stays on screen and the new message continues with that context. (edc23ce)
- Infrequently used icons in the session / file-tree action bars auto-hide and reveal on hover; Settings → Interface adds an "Auto-hide icons" switch (on by default; turn it off to keep icons always visible, effective immediately). (98cbd72, 4bf8673)

### Bug fixes

- Chinese markdown editing killed the process instantly: the window vanished with no error while editing Chinese articles (three real-machine crashes in a row; autosave saved the content) — the vendored editor's tree-sitter highlight callback sliced raw bytes, and a dirty offset landing inside a 3-byte Han character panicked across an `extern "C"` boundary (cannot unwind → abort); the root cause was the IME composition update computing its selection base from the wrong end of the replaced range. Fixed with a char-boundary guard in the callback plus the corrected selection base. (6e9af0e)
- Interrupts while running: an interjection during thinking dragged the whole message screen into the lower half (interrupts now only retire the tail-follow anchor); pinned-to-top is now decided by "was it a real interrupt"; echoes no longer double-post; stray stream_cut fragments merge back into the old reply. (92ff894, 0343a22)
- The thinking-level capsule kept showing the old level after switching (on real machines it always showed high). (3ae6f31)
- Terminal: painting right after the terminal area shrank read stale row counts and crashed; a bare `exit` + Enter no longer kills the parent shell — it now closes the terminal tab. (faf8987, c4d0aa1)
- Image bubbles no longer show the `[Image: original …]` coordinate caption. (5947554)
- File tree froze when expanding a repo opened via a subdirectory: git status / numstat paths are now anchored to the repo root. (0eefea6)
- The context-usage popup is 40px wider (420 → 460), so the bottom three buttons no longer overflow in English. (2a73342)
- Extension detail status no longer prints raw update-check error strings; the "Extensions" settings nav item is back (it was hidden when the market page shipped). (bd21660, 6d26f83)
- Idle sends no longer wrongly enter the send queue; the "back to latest" button in the chat list no longer fails while tail-following. (b2a56a3, 6a1dfb1)

### Improvements

- Memory usage: decoded-bitmap cache entries are now evicted lazily (root fix for image-heavy long sessions where memory only grew); idle reclamation now retires runtime entities too. (1f66410, b655602)
- Completed Traditional Chinese / English translations for the model page and "Add Provider" texts. (2042423, 271b653, 2cb104a)
- Shortcuts page simplified: inline explanatory notes removed, leaving titles and keycaps. (af86124, 65fe2da)
- Removed the "Reset" button from the session content width setting. (d64e64e)
## [0.2.0] - 2026-10-18

This release focuses on: performance — the giant-session lag is fixed at its root (a 688-message session drops from O(turns) to O(viewport) per frame, composer typing no longer repaints the whole window per keystroke, and the send freeze is gone); three major features — session pinning, Pi extension update detection, and session auto-cleanup; free tab drag & drop on the topbar (Zed-style); and mid-session RPC stops are no longer silent.

### Install & Update

```bash
npm install -g pi-flash@latest   # same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip portable build, extract and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

**On 0.1.11 or later, the in-app updater works** (Settings → Check for updates). **If you are still on 0.1.10 or earlier, you must use the npm command above** (the old in-app updater is broken and cannot install new builds).

### New
- Session pinning (020): hover a session row for the pin slot, or use the pin button on the right-click card; pinned sessions form a group at the top (folder-bookmark header); the original project group filters by the pin list so entries never show twice. (c1f811f, d68112d, 42eaaf9, f652ad0)
- Pi extension update detection & updates (082): the extensions page detects available updates and updates all in one click; the detail card's [Update] button only appears when an update exists. (4fe2aeb, f3e860c, 02bb266)
- Session auto-cleanup (052): a day-based stepper in Settings → Other (0/5/…/100, default 0 = off); a background task moves expired sessions to the recycle bin. (609924e, d5451eb, 15799c7)
- Adjustable session content width: one content column for the message list and the composer capsule (capped at 1200 / step 20). (041bdee, a258cd0)
- Free tab drag & drop on the topbar (Zed pane parity): file tabs reorder freely (the pinned session-tab segment stays fixed); the tab strip follows the active tab; the tab area stretches to 20px left of the minimize button; drag ghosts carry file-type icons and grab-hand cursors. (4b6596a, 46497cc, ac3b2ef, b2a5996, 801db0a, c5a394d)
- Panic logs written to disk (diag): crashes leave evidence behind. (e06c777)
- All UI strings moved into the i18n tables: zero hardcoded strings across 26 files. (1702292)

### Fixes
- Giant-session lag fixed at the root: a 688-message session drops from O(turns) to O(viewport) per frame; composer typing removes the per-keystroke O(n) passes and redundant per-frame work and no longer repaints the whole window, with the input crash fixed; the send freeze (two O(session-size) scans on the main thread) is gone; input history capped. (89ac76e, 3b326ec, e06c777, 3540725)
- RPC: stopping a session mid-run is no longer silent — EOF now completes turn-final semantics and session events are parsed in full. (775150e)
- Single-semantics regression: current project ≡ the active session's project; removed a vestigial send-gate check. (ff29615, 12544df)
- Registered projects with zero sessions no longer create empty groups — no "no sessions" placeholder projects in the panel. (2d1a5c0)
- Root cause of "sticky" tab drags: non-unique tab element ids (ported from zed pane.rs). (258185a, bf58439)
- Shortcuts page restored to the centered index layout (2 groups, 9 entries) with its tab moved after "About"; a batch of acceptance rework on the new-session page (notification strip bottom-anchoring, divider, card layout, zero-width extension card deformation, etc.). (f85eb3a, 928763d, b8f962b, 3eef399, 4a3ef19, 294a712)
- Minimum window height relaxed to 600 (restore size and minimum size split into four constants). (1cfbd1b)
## [0.1.13] - 2026-10-17

This release focuses on: a reworked startup targeting — the app opens on the "most recently active project" and no longer fabricates project state; the "update handover pollutes the project registry with the payload directory" issue exposed by real-machine testing of 0.1.12 is fixed for good; a new "latte" theme; two hard bugs fixed (project-switch crash and duplicate queued sends); file-tree and dock state now survive restarts.

### Install & Update

```bash
npm install -g pi-flash@latest   # same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip portable build, extract and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

**On 0.1.11 or later, the in-app updater works** (Settings → Check for updates). **If you are still on 0.1.10 or earlier, you must use the npm command above** (the old in-app updater is broken and cannot install new builds).

### New
- New "latte" theme (Catppuccin Latte port, accent aligned to the Obsidian misty-violet #5F5FAB); nord-light removed; unknown theme names now fall back to the first registry theme instead of a blank screen. (479b0d2, 606359f, e3916de)
- File-tree state survives restarts: expand/collapse memory, Pin favorites view restored per project. (23a1f8a)
- Dock maximized state survives restarts: a maximized file tree / session list stays maximized after reopening. (038d5dc)
- Settings group-header master toggles get a "partially selected" intermediate state (skills / extensions / MCP). (f1ed223)

### Fixes
- Project-switch crash: Pin favorites follow the project switch, and gitignore matching no longer panics on non-prefix paths. (77aa69e)
- Duplicate queued sends eradicated: sends now go through prompt+streamingBehavior, so a stuck queue entry is never replayed. (37919a1)
- Startup targeting rework: the app opens on the "most recently active project" instead of the globally last session (cwd is a session attribute; the app never fabricates projects); root cause of "every startup lands in the payload directory" found during real-machine testing of 0.1.12 — the cwd inherited by the update handover polluted the project registry — is fixed. (8cb95b4, 81aff88, 0be0ae6, f02bbf8, 452c91a)
- Two dead switches on the skills page; SKILL.md frontmatter no longer grows an extra line on every write. (5944907)
- Skill paths no longer carry the Windows `\\?\` verbatim prefix. (8aff9a2)
- Navigation panel: flyout panel bottom edge aligns with the input panel baseline (no longer sticks to the window bottom when dragged down); flyout card area and summary bar background now match the function panel. (516371c, 3a03336)
## [0.1.12] - 2026-10-17

This release focuses on: the PF project registry (opened projects are registered automatically, zero-session projects show up in both panels); keyboard shortcuts fixes round 3 & 4 (consecutive Ctrl+←/→ switching, terminal no longer swallows the keys, window control buttons fixed); a batch of session bubble scroll and file-tree interaction fixes; no more stray terminal window on Windows startup.

### Install & Update

```bash
npm install -g pi-flash@latest   # same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip portable build, extract and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

**On 0.1.11 or later, the in-app updater works** (Settings → Check for updates): the new "versioned directory extraction + startup handover" chain took over in 0.1.11, and this release is its first real-machine upgrade target. **If you are still on 0.1.10 or earlier, you must use the npm command above** (the old in-app updater is broken and cannot install new builds).

### New
- PF project registry: opened projects are registered automatically (`~/.pi-flash/projects.json`); zero-session projects now appear in the function-panel project list and the session directory — projects no longer depend on having sessions to be visible. (ee82d0c)

### Fixes
- Shortcuts (082 rounds 3/4): Ctrl+←/→ reworked to the Zed Pane model — ancestor-context anchoring + focusable anchor, so consecutive switching no longer stalls; tab switching is now shell-exclusive (the terminal no longer swallows the keys, Markdown preview is keyboard-reachable); window control buttons (minimize/maximize/close) fixed (track_focus anchor moved down to content-main). (8af1b26, 16fdfba, 47cbfcd)
- Session scrolling batch: very tall user bubbles were unscrollable (gpui List wheel registration timing); scrolling inside a bubble to its edge no longer bleeds through and moves the session list in the same frame. (65d03ec, a238cb3)
- File tree batch: inline create/rename caret no longer blinks off (initial focus now goes through InputState::focus); newly created entries and the committed row scroll into view. (5dbb6e8, 8e92ec1)
- Windows startup no longer pops a stray terminal window. (3be6deb)
- Tools capsule menu now right-anchored, right edge flush with the button. (f96ca70)

### Improvements
- Deleting large directories in the file tree moved to a background thread (trash on background_spawn) — no more main-thread stalls. (ff89733)
- Settings → Interface: removed the "Markdown WYSIWYG" toggle. (2732556)
## [0.1.11] - 2026-10-17

This release focuses on: a reworked in-app update staging — versioned payload directory + startup handover (powershell swap removed), fixing the root cause of the 0.1.10 endless redownload loop; two new themes ("kitchen" and "pine"); faster session open and project switching; Ctrl+L global file search on the shortcuts page.

### Install & Update

```bash
npm install -g pi-flash@latest   # same command for first install and updates
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip portable build, extract and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

**In-app update is broken on 0.1.10 and earlier — please update via the npm command above (or re-download the zip)**: the updater shipped with those versions uses a flawed whole-directory swap and cannot install new builds (the 0.1.10 endless redownload loop was one symptom); only from 0.1.11 does the new "versioned directory extraction + startup handover" chain take over, and in-app updates work again from this release on. Zip portable users should likewise reinstall via npm or re-download the zip instead of relying on the old in-app updater.

### New
- In-app update rework: after download the payload is extracted into a versioned directory; at startup a self-verifying handover (directory name version must equal the process version, preventing spawn storms) launches the new build, and the powershell swap is gone entirely; the 0.1.10 endless redownload root cause (the swap racing into antivirus scanning) is fixed for good; machines upgrading from older builds clean up legacy leftovers at startup; extraction cost is paid once at install time, and stampless half-extracted payloads are removed; the real-machine update pipeline is scripted into tests (with three failure injections)
- Two new themes: "kitchen" (MiMo 014 cream-paper letterpress feel) and "pine" (Pine Forest Green port); kitchen moves to the front of the theme list, ayu-light removed — light themes back to one row of 5
- Shortcuts page: Ctrl+L global file search
- The compaction locked-state hint now shows a spinner up front

### Fixes
- Shortcuts page vertical centering actually works (centering moved up to the mc-body flex layer)
- Session toolbar icons 13→16 spec size, aligned with the left control bar small icons
- Detail cards / search popups no longer tick seconds (time_ago minimum granularity is now minutes)
- Turn-summary user lines no longer cut in half on the third line (line_clamp(3))
- Compaction locked-state hint gets a 12px left inset, aligned with the placeholder
- Pine accent toned down (2ecc40 → 29a83c)

### Improvements
- Deep session open-time optimization: leaf full-text reread and on-disk counting fully backgrounded
- Project-switch git refresh backgrounded: main thread 190-313ms → 106-126ms
## [0.1.10] - 2026-10-16

This release focuses on: the keyboard shortcuts system (082), file-tree Pin favorites and cross-project switching, drag-to-reorder project tabs, a consolidated usage panel; the npm shell now pre-fetches the real latest version before downloading the payload (registry mirror lag no longer ships old versions); built-in pi upgraded to 1.3.0. If you are still on ≤ 0.1.8, please reinstall via npm or the zip — the in-app update chain has been fixed since 0.1.9.

- **Shortcuts (082)**: Alt+1 / Alt+2 maximize the left/right sub-panel; Ctrl+←/→ switch to the adjacent tab; F2 renames files in the file tree; a new "Shortcuts" settings page lists every binding in a centered index layout
- **File tree**: Pin favorites — toolbar button + right-click pin/unpin, favorites as a standalone view, persisted per project; project dropdown in the title for cross-project switching (↑↓/Enter keyboard navigable); activating a file tab syncs the tree to that project
- **Project drag & drop**: drag-to-reorder project tabs (order persisted; tabs collapse while dragging); hand-grab cursors drawn in-house (vendored gpui now implements Windows OpenHand/ClosedHand)
- **Usage panel**: system prompt / session tools entries moved into the panel (equally spaced with compact on one row); the compact button gets a double-arrow icon with tri-state coloring
- **Other features**: session summary bar now shows four icon+count stats (messages/user/assistant/tools); new "session directory" toolbar button (opens the project's session group in Explorer); Enter is the only send key and all sends go through the queue; "free only" filter for the model list; click Markdown preview to return to editing; bolder, faster-blinking editor caret
- **Auto-update chain**: the npm shell now resolves the true latest version from the GitHub Releases API before building payload URLs — mirror lag behind the shell version no longer installs outdated payloads; end-to-end pipeline tests for the updater are now part of the regular gates (every 0.1.6/0.1.8 incident is pinned; no more relying on real users as the test bench)
- **Built-in pi 1.2.0 → 1.3.0**: wire protocol verified byte-for-byte; behavior changes recorded in pi-rpc.md (model switch drops other-model thinking, resume keeps tools, two tool-arg parsing fixes, wider retry list)
- **Fixes**: editor jumping to the bottom when clicking a visible line; file tabs keep a stable open order (MRU reordering removed); pending_echoes no longer accumulates (64-entry dead-letter cap); usage-panel cache hit rate display ×100 corrected (97.2% showed as 1.0%); shortcut page modifier keycap colors fixed (swapped mix_rgb args); project/session context cards widened to 338px with full path wrapping, delete consolidated into the detail card
## [0.1.9] - 2026-10-15

The ask_user_question full-survey card lands; in-app updates get the swap dead-loop fix and faster extraction; the WeChat bridge no longer auto-starts (pre-pluginization).

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

**On ≤ 0.1.8, prefer the npm command above or a fresh zip download this time**: the staging/extraction code that runs during an in-app update belongs to the OLD build (the fixes ship in this one); the command-line/zip path bypasses that entirely. From 0.1.9 on, in-app updates run the new logic.

### New
- ask_user_question survey card: multi-question surveys from the agent now render as one tabbed card — switch tabs freely (custom answers follow along), answered questions go ●→✓, confirm is gated by [n/]N (grey until complete), with a Tab/Enter/number/Esc keyboard model and an × in the corner to cancel the whole survey; a selected single-choice option with a preview upgrades to a side preview pane, and custom answers to single-choice go back as plain text. Confirming auto-answers the queued requests silently and clears itself when the stream ends. (e5fd391, 277fcab, b8fc012, bc03faf, 6a065b0, 8fb285f)

### Bug fixes
- Update staging dead loop (observed on 0.1.8): clicking "restart to update" flashed console windows forever without starting — when the swap was blocked by orphaned processes the staging marker survived, so the relaunched old build saw a valid staged update and started another round; the swap now clears this app's orphaned sidecars first and consumes the marker on failure (retried on next launch), crash-recovery semantics unchanged. (6126664)
- Context usage popup cache-hit rate displayed ×100 (97.2% showed as 1.0%). (a00be40)

### Improvements
- In-app update experience: after download completes the pill switches to "Extracting & installing…" (no more frozen-looking wait); extraction is now parallel — the same package measured 26.9s → 10.1s. (02cd7a1)
- The WeChat bridge no longer auto-starts with the app (the "another poller holds the lock" error on multi-instance is gone), as a prelude to pluginization; see module doc 060 in the repo for how to re-enable. (b1b5ac0)
## [0.1.8] - 2026-10-15

Text selection & copy in the message area and drag-and-drop moves in the file tree land, plus a batch of interaction fixes around composer scrolling, the session scroll area and in-place file renaming.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- Message text selection (phase 1): finalized agent message blocks now render through TextView with mouse selection & copy, plus a right-click Copy menu; clickable file paths get link styling (link color + underline, visible on light themes); a batch of table rendering fixes (column widths weighted by display width, column drift, in-cell wrapping, bold headers). (eea59ce, dfa56dd, ef2d66c, 2b075ba, b013e6d, 585c594, 08d7051, 9c0bee6, 144fdd7)
- File tree drag-and-drop moves: drag a file/folder onto a target folder to move it; hovering a collapsed folder auto-expands it after 500ms (ported from Zed's project_panel DnD). (e695eab)
- Settings' document-font sizes gain 16/18/20/22 (other slots stay 15/16/17/18); all font-size slots now clamp to 10–30. (907a8b7)

### Bug fixes
- Composer multi-line cursor-movement jitter: in short viewports the scroll position flip-flopped on every keypress and the line count alternated 3↔4 (the vendor scroll_to top/bottom safe areas overlapped and never converged); also fixes the first line being clipped out of view when the cursor sits on the bottom line. (79910c6)
- Session scroll wheel failure: with short content a small scroll jumped straight to the bottom and the scroll margin collapsed to zero; the wheel now only pauses the bottom-pinning — scrolling back to the bottom re-arms follow, and reading history no longer gets dragged along by streaming output. (386896b)
- New sessions created inside a collapsed project group vanished: binding a session now auto-expands its group (fork and WeChat-created sessions take the same path). (26057ea)
- Four in-place file-rename visual fixes: 1px jitter (the row border is now always mounted as a transparent placeholder), the text jumping up 1px, the font/size changing mid-rename (follows the panel font slot), and the text being indented 4px too far. (b0b6224, b686892, fec33d0, 08f1c31, 09a8437)
- The editor body font now follows the settings' document-font slot (was hardcoded JetBrains Mono); line height aligns with Zed's Comfortable (1.618, was a fixed 1.25rem — too dense). (d5eddae, 0323612)

### Improvements
- The session-row detail card is now summoned by right-click: the hover auto-popup mechanism was removed entirely (the interaction couldn't be smoothed out — owner's call); right-click opens the card, clicking outside dismisses it, and the card never sits under the cursor. (64ab3f4)
- The composer plug (extensions) icon went 13→16 (the standard middle size). (0a1dd18, 2d3a36e)
## [0.1.7] - 2026-10-15

The left dock becomes an integrated sessions + file-tree panel (draggable divider, mutually-collapsing maximize), the composer gains a right-click menu; a self-swap fix ends the update dead loop for zip / direct-launch users; dot files are visible in the file tree by default; a batch of file-search and icon-alignment fixes.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- Integrated left dock: sessions (top) + file tree (bottom) in one panel — drag the divider to resize (double-click resets, ratio persisted), a single maximize button collapses the other side; the session control bar is three-left two-right (Projects / Sessions / Search + list toggle / maximize), and the status bar keeps a single "Projects" tab. The project-row hover buttons come along: the trash can opens the delete confirmation directly, + becomes a compact new-session entry (the ⋯ menu is gone). (b864750, 6391e36, 57207f6)
- Composer right-click menu: undo / redo / select all + cut / copy / paste / delete (pasted images are attachment-ized first, same path as Ctrl+V; no shortcut column — the system handles those). (4046977)

### Bug fixes
- Update staging dead loop (observed on 0.1.6): users who double-click the payload exe directly, or run the zip build, could never apply a downloaded update ("Please restart to switch to x.x.x" forever) — the exe now self-swaps: on launch it detects a fresh staging mark, swaps it in and starts the new build; the "download complete" notice is now clickable to restart into the update immediately, so an update completes under every launch method. (e6a0d15)
- Dot files visible in the file tree: .gitignore / .beads and friends were hidden by a "dot-prefixed means hidden" rule mislabeled as Zed semantics; now only Zed's default scan exclusions (.git / .DS_Store etc.) stay out of the tree, gitignored entries still render dimmed. (ba65eb6)
- File-search panel batch: fixed the broken row file icons (asset path was double-prefixed); the panel widens to 700 (92% in small windows), row content aligns with the input text, over-long paths wrap, and the alignment baseline moved to the input box's left edge. (15c9fba, d0a7916, f5e1af1)
- Project-row hover button column alignment: the 10px double-inset that shifted the whole group left is offset back — trash ↔ list-toggle and new-session ↔ maximize now share x per column. (b516dfa)

### Improvements
- Icon & chrome alignment batch (owner-decided): removed icon_hover's +2px draw inflation (the root cause of "icons look random" — same nominal size painted 2px larger than icon()), all sites back to the design-spec size tiers; the control bar's right pair aligns column-wise with the file-tree action bar (26×26 / 2 gap); the "Sessions" icon gets an 18→19 optical compensation; the status-bar settings button / projects tab move up to the large icon tier; the project-row path hover bar is turned off. (ed419c0, 8ec6754, edf5267, 9257cd5)
## [0.1.6] - 2026-10-14

An editor open-capability batch: images open directly, legacy encodings like GBK/UTF-16 open and save; a tab context menu and a changelog entry point; fixes for frozen large single-line files and CJK crashes; scrolling feel aligned with Zed.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- Images open directly: the open route sniffs file headers (previously any file containing NUL bytes went gray-screen, so png and friends never got rendered); png/jpg/gif/webp/bmp etc. render in an image tab, and with tiff/ico/pnm added the set matches Zed's eight formats; svg opens as source (preview stays a separate action for later). (f8046cd, b8ba388)
- Legacy encodings open and save: opening text now runs an encoding-detection pipeline (BOM / UTF-16 NUL distribution / chardetng sniffing + magic header table, ported line-by-line from Zed's file_content), so GBK and UTF-16LE files like .reg open, and saving writes back in the original encoding, round-trip-safe. (f8046cd)
- Tab context menu: Close / Close Others / Close Left / Close Right / Close All (same as Zed); batch-closing folds multiple dirty files into a single confirmation (Save & Close / Close Without Saving applies to the whole batch); the first session tab can never be closed. (11331e6)
- Click the version number on the new-session page to open the changelog: the dimmed idle version label is clickable — it reads the local cache first and fetches by tag if missing; a failed fetch now times out at 15s and shows "暂时无法连接 GitHub" (can't reach GitHub right now), and clicking again retries, instead of waiting 30s and landing on a misleading "no changelog". (e423e6b, 4a480f6)

### Bug fixes
- Large single-line files froze: opening minified frontend bundles (e.g. echarts.min.js, a 940k-character last line) froze for minutes per frame — syntax highlighting and text shaping are now clipped to the viewport byte window, back to milliseconds. (1d30077)
- Crashes and misplaced highlights on CJK files: after viewport clipping, style spans no longer covered the whole visible text, so files containing Chinese were sliced mid-multibyte-character and crashed outright (0xc0000409), while ASCII files got silently misaligned highlights — default-style fillers before and after the window restore the tiling invariant. (17138f2)
- Markdown soft-wrap regression: turning WYSIWYG off used to drag soft-wrap down with it; the two are decoupled and source editing wraps again (consistent with the preview state). (b654ae8)
- First-launch-after-update popup accounting: dev/smoke builds no longer write last_seen — previously a single run polluted the ledger and swallowed the real update's first-launch popup. (dab6fe7)

### Improvements
- Scrolling feel aligned with Zed: per-frame tree-sitter re-queries and per-capture heap allocations inside the viewport are gone (verified equal by a differential test), the double frame per wheel tick is eliminated, and line heights snap to whole device pixels (fixing sub-pixel shimmer at 1.5× scaling); app-side overhead for large-file scrolling is essentially zero. (35836ff)
## [0.1.5] - 2026-10-13

In-app updates gain mirror failover and a download percentage, the context usage popup gets a redesign, plus a batch of chrome alignment tweaks.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- In-app update mirror failover: payload downloads speed-test the direct connection (abandoned after a 3s average below 128KB/s) and walk through mirrors until one hits; checksums are pinned to the official hash over a direct connection to prevent cross-packed downloads. `PI_FLASH_MIRROR` / `PI_FLASH_MIRRORS` share the installer's rules. (6734db6)
- In-app update download percentage: the version slot now refreshes in place with NN% (omitted when the server sends no content-length). (8e82205)
- Context usage popup redesign: a title line (used / window · %) with an accent progress bar, a per-category breakdown row (7-bucket estimate, descending color dots) and the cache-hit rate, in a 420px two-column grid with thousand-separated token counts. (bf03158, c17247c, 6b6c827, a618b1c)

### Bug fixes
- Phantom keybinding hints: the tooltips for Ctrl+P / N / K could show fake keys like Win+P (gpui's cmd is a platform modifier); the cmd-* twin bindings were removed. (d63039e)

### Improvements
- A batch of chrome alignment (owner-decided): topbar and statusbar are now equal height at 40px (window control buttons follow at 42×40); bar tabs fit the bar height; the statusbar settings button's right edge aligns with the list right line and the first tab card's left edge sits on the icon alignment line; the icon alignment line collapses into a single token (11→12px); composer right-group button spacing matches the left group (15px). (d2760c5, c100b22, 61981c5, 418512c, d00de94, e015300, 189008a)
## [0.1.4] - 2026-10-13

A UI polish batch: bundled pi upgraded to 1.2.0, global shortcuts and keybinding hints, a set of completion-menu / cross-project fixes, and several owner-decided UI tweaks.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- Global shortcuts: Ctrl+P open project, Ctrl+N new session, Ctrl+K search sessions — available anywhere in the app. (ec16a1e)
- Keybinding hints: sidebar header and project-row buttons now have tooltips with a keybinding chip that follows rebinds automatically; tooltip delay shortened to 350ms. (ec16a1e, 670869a, c55c993)
- Binary file handling: unparseable files open a tab with a gray page showing the detected type plus an "open with default app" fallback (Zed-style). (25e9965)
- Markdown WYSIWYG toggle: new switch on the Settings > Interface page, off by default. (bcb8e04)
- Session rename box upgraded: three auto-growing lines, selects all on open with the cursor anchored at the start, so typing replaces the whole name. (04cbbb2)

### Bug fixes
- @ / slash completion menu: menu items were not clickable (clicking did nothing and the raw query was sent, losing the directory prefix), and Enter accepted item 0 regardless of the selection. Both fixed. (7913e03, 3fe693c)
- Cross-project session switching: opening a session from another project left the file tree / changes panel on the previous project (empty tree, wrong changes); ghost-tab clicks and the automation entry are covered too. (5d708ff, 1e46aaa)
- Session rename box opened blank (text scrolled horizontally off-screen); the first frame now shows the full text selected. (72e0dfc)
- About page author name went missing (a call site dropped in the semantic-key migration). (63516a3)

### Improvements
- Bundled pi upgraded from 1.1.0 to 1.2.0, with all protocol review points passing. (3695715)
- Panels decoupled from the content area: switching the left dock no longer swaps the content view — switching to the file tree on a chat page no longer replaces the conversation with a document. (a7eca88)
- A batch of UI tweaks (owner-decided): composer extensions button becomes a plug icon + [n] count; dock collapse button aligned with the list icon line; terminal icon unified to the boxed style; "more" icon switched to a bare lucide ellipsis; eye/search toolbar buttons brought up to spec; v5 wordmark enlarged and the welcome logo +20%. (1a2121a, b7e7a71, 0583b77, 7c2024b, 9c03115, 3d3b656, dd985f6, f84e8c5, 91c51d0, c0ab7fa)
## [0.1.3] - 2026-10-13

Instant sidebar feedback + a faster download path: ghost tabs appear the moment you send, and slow downloads switch mirrors automatically.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- Sidebar ghost tabs: the moment a new session's first message is sent, a tab appears in the sidebar immediately (spinner + first-line preview) and turns into the real tab once the session file lands; rejected messages, dead processes or no session file remove the placeholder instead of leaving a tab that never becomes real. (5914900)
- Download speed-test with automatic mirror fallback: payload downloads for install/update measure the direct connection and switch through the gh-proxy.com / ghfast.top / ghproxy.net mirrors when slow; the download line refreshes percentage and speed in place, with a 30s no-activity timeout. `PI_FLASH_MIRROR` still pins a single source. (8b2048e)

### Bug fixes
- Startup restore: closing the app without sending anything no longer loses the last session (the "phantom path" — pi reporting the session file during construction before it exists on disk — is no longer bound). (5914900)

### Improvements
- Download residue cleanup: partial downloads (payload.tmp-* / update.tmp-*) are removed on launch and before downloads based on whether the owning process is alive; stale leftovers are always cleaned. Checksums come from the official source over a direct connection while the zip goes through mirrors, preventing cross-packed downloads. (8b2048e)
## [0.1.2] - 2026-10-12

A terminal & file-editing fix batch: copy/paste entries for the terminal, a set of field-reported UI fixes, and HTML files now open as source in the editor.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19.
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.
Windows x64 only.

### New
- Terminal copy/paste entries: a context menu (copy / paste / select all) plus Windows-style keys — Ctrl+C copies when there is a selection (still sends ^C interrupt without one), Ctrl+Shift+C / Ctrl+Insert to copy, Shift+Insert to paste, Ctrl+Shift+A to select all. (9daa12c)
- Terminal selection: double-click selects a word, triple-click a whole (logical) line, and dragging past the terminal edge auto-scrolls. (1c68a5b)
- Terminal editing with a selection: Backspace / Delete removes the whole selection at once, and pasting replaces it. (3afb8a3)
- New "auto-collapse after" setting: 1 / 3 / 7-day options; project groups inactive beyond the threshold start collapsed. (e03659e)

### Bug fixes
- Terminal: with multiple terminals open, switching tabs no longer sends keystrokes to the background terminal; clicking a tab focuses it, no extra click on the terminal needed. (80ede11)
- Terminal: "Open in terminal" no longer switches the left dock to the file tree; a project can have multiple terminals instead of jumping to the existing one. (73173bd)
- Sessions: switching to or creating a session brings the content area back to the chat view instead of staying on a terminal / file tab. (badaed6)
- Sessions: a new session appears in the sidebar as soon as its first message is sent, not after the whole turn finishes. (23506b7)
- Sessions: tightened end-of-turn notification dots — an errored turn no longer shows the "completed" blue dot, and a single tool error no longer marks the whole turn red. (faee211)
- Session list: titles showing "(empty)" — when a long system-prompt line or a huge paste pushed the first user message out of the index, the first 50 characters are now recovered as the preview. (fff7f8a)
- Sessions: during the auto-retry backoff window (2–8s) the UI wrongly showed idle; it now stays "running" and the status line reports "error, retrying in Ns (attempt x/y)". (0747542)
- Markdown rendering: inline `code`, bold and links inside list items were dropped entirely (only punctuation remained). (cd7528c)
- Editor: syntax highlighting for large HTML files (an 118KB design file in testing) went from ~8s per frame to smooth. (382ab0b)
- Images: result / large image previews scale proportionally, no more vertical padding. (05317a0)
- Settings: the "show thinking" description showed Chinese on Traditional Chinese / English UI. (0f50b2c)
- Project card: clicking outside the card during rename now cancels it instead of forcing a rename. (2bc46de)
- The taskbar icon had a ring of padding around it; the .ico was regenerated full-bleed at all nine sizes. (7ad6c63)

### Improvements
- HTML files now open as source in the editor (syntax highlighting, editable, autosave), with "Open in browser" kept as the first action in the context menu. (8fd3bcf, e3e3dd5)
- The completion sound is now an original warm marimba two-tone (A4→E5). (4cba884)
- New i18n missing-translation guard: UI copy without a Traditional Chinese / English translation now fails tests instead of silently falling back to Chinese. (ed2e126)

## [0.1.1] - 2026-10-12

First official public release of pi-flash: a fast pi coding agent desktop client built with Rust + GPUI.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19. 
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.

### New
- Agent sessions
- File management and editing
- Pi configuration: models / skills / extensions / MCP
- Other settings

### Bug fixes
- The npm package installs correctly: the thin shell fetches the payload from the GitHub Release by its own version and verifies SHA-256
## [0.1.0] - 2026-10-12

First official public release of pi-flash: a fast pi coding agent desktop client built with Rust + GPUI.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19. 
Or download the zip, unzip it and double-click `pi-flash.exe` — no Node needed.

### New
- Agent sessions
- File management and editing
- Pi configuration: models / skills / extensions / MCP
- Other settings
