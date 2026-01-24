# Controller

Controller is a Rust desktop agent that enhances clipboard workflows using LLMs. It detects copied text (ligatures, OCR errors, math, etc.), sends it to configured LLMs for fixes/translations/explanations, and updates the clipboard with the result.

## Quick Workflow
1. Copy text to clipboard.
2. Press a hotkey within ~2s to trigger an action.
3. Controller processes the text and updates the clipboard (or shows a UI overlay for queries).

### Default Hotkeys

| Shortcut | Action |
|---:|:---|
| `Ctrl+R` | Format / Copy Check (fix ligatures, broken OCR, remove spurious line breaks) |
| `Ctrl+T` | Translate English → Chinese |
| `Ctrl+Y` | Translate Chinese → English |
| `Ctrl+E` | Explain (concise explanation) |

Note: Hotkeys only trigger if the copy event occurred recently (debounce ~2s). See `src/main.rs` for the exact timing logic.

## Configuration
- Primary LLM configuration: `config/llm.toml` (per-function sections: `copy_check`, `translate2c`, `translate2e`, `explain`, `user_query`, etc.).
- Additional commands: `config/commands.toml` (tray custom commands).
- Secrets: use `.env` or environment variables. Example:

```dotenv
API_KEY=sk-your-actual-key-here
```

`config/llm.toml` supports remote and local endpoints (the client automatically falls back to local when the remote API is unreachable). Use `${VAR_NAME}` in the toml to read env vars.

Configuration override behavior
- The canonical config files are in `config/` (tracked). At runtime the app will load `config/llm.toml` and then overlay an optional user config from `$XDG_CONFIG_HOME/Controller/llm.toml` (commonly `~/.config/Controller/llm.toml`). This allows machine-specific overrides without changing repo files.
- The build script copies `config/` into `target/<profile>/config` for convenience; these are build artifacts and should not be committed. See `.gitignore` for `target/**/config/`.

## Features
- Event-driven clipboard listener + global hotkeys (`rdev`).
- Fast debounce logic to avoid false triggers.
- Per-feature model/prompt configuration and hybrid remote/local fallback.
- Streaming-aware UI (`eframe`/`egui`) with a typewriter effect for streamed chunks.
- Tray integration for enabling/disabling processing, opening logs, and custom commands.

## Architecture (developer view)
- `src/main.rs`: app entry, single-instance guard (binds a marker socket), logging setup (`flexi_logger`), tray setup, and runtime bridge between async LLM and sync UI.
- `src/core/*`: core logic — `actions.rs` implements high-level `Action` handling; `detector.rs` contains regex/heuristics for deciding whether to send clipboard text to the LLM.
- `src/api/client.rs`: LLM client — resolves per-function configuration, does remote/local fallback, supports streaming, and emits UI streaming events.
- `src/ui.rs`: UI overlay and `UiEvent` enum; handles streaming, show-result, errors, and tray UI updates.

Important pattern: asynchronous producers (LLM client) send `UiEvent` over a `tokio::sync::mpsc` channel; `main.rs` runs a bridge task that forwards those events to a blocking `std::sync::mpsc` channel consumed by the UI. If you change `UiEvent`, update `src/ui.rs`, all producer send sites (`src/api/*`, `src/core/*`), and the bridge in `src/main.rs`.

## Developer Workflows & Commands
- Build & run (use Release for production-like behavior):

```bash
cargo run --release
```

- Stop running instance (signals single instance):

```bash
cargo run -- --stop
```

- Dump latest log (power user; PowerShell example):

```powershell
Get-Content (Get-ChildItem "logs\*.log" | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName -Tail 50
```

- `build.rs` copies `config/` into `target/<profile>/config` at build time — edit files in `config/` to change default runtime settings.

## Key Files (where to look first)
- `src/main.rs` — boot, tray, channels, and runtime bridge.
- `src/core/actions.rs` — how actions map to LLM calls and clipboard updates.
- `src/core/detector.rs` — heuristics that gate LLM calls (ligatures, broken words, math, skip URL/path/CJK cases).
- `src/api/client.rs` — HTTP client, streaming parsing, remote/local fallback.
- `src/ui.rs` — UI state machine and `UiEvent` handling.

## Testing & Debugging Tips
- Test clipboard flows in `--release` mode (hooks and global listeners behave consistently).
- When testing streaming, check that `LlmClient` received a `tokio` sender (see `set_ui_tx`) and that the bridge task exists in `main.rs` to forward events to the UI.
- Inspect logs in `logs/` for request/response bodies (client logs requests when debugging streaming issues).
- Be conservative when changing `build.rs` — it affects packaging and runtime config lookup.

## Contributing Notes for Agents / Bots
- Keep edits minimal and localized; prefer adding focused patches (update the flow, adjust prompts, or add a small UI control).
- Preserve the `Detector -> Processor -> LlmClient -> Clipboard` flow and the async->sync event bridge. When adding a `UiEvent` variant, update:
  1. `src/ui.rs` (enum)
 2. All `send` sites in `src/core/*` and `src/api/*`.
 3. The bridge forwarding code in `src/main.rs`.

---
If you'd like, I can also:
- Add `UiEvent` send-site references into this README, or
- Add a short PR-review checklist for UI/streaming changes.

