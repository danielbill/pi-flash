# Changelog

All notable changes to released versions live here; see `docs/CHANGELOG规范.md` for the format.

## [0.1.0] - 2026-10-10

First official public release of pi-flash: a fast desktop shell for the pi coding agent, built with Rust + GPUI.

### Install & Update

```bash
npm install -g pi-flash@latest   # first install; re-run the same command to update
pi-flash
```

Requires Node.js ≥ 22.19. 
Or download the green zip, unzip and run `pi-flash.exe` directly (no Node needed).

### New
- Session core: streaming rendering, multi-turn chat, interruption, branching & full history
- Input & editing: Markdown source/preview toggle, auto-saved pasted images, built-in terminal, file tree & git panel
- Extensions: models / skills / plugins / MCP (pinned pi ships with the app, never reads the system PATH)
- Settings: UI / theme / languages / remote control (WeChat)
- Distribution: one-command npm install & update; macOS arm64 build attached to the Release by CI

### Bug fixes
- File tree and the "available models" list in Settings could not scroll: the scroll handle was rebuilt every frame, resetting the offset; handles are now reused by id

### Improvements
- Completion sound replaced with the original pi-web tone (WAV synthesized on the Rust side instead of the system beep)
