# Controller Copilot Instructions

Short, focused guidance for AI coding agents working in this repo. Keep changes small, preserve existing patterns, and reference the files linked below.

## Quick Context (why this structure)
- Desktop Rust agent that intercepts clipboard events, detects malformed text, uses an LLM to produce fixes, and writes back to the clipboard. UI is minimal and runs alongside a background service.

## Service Boundaries & Key Files
- `src/main.rs`: app entry — single-instance guard (TCP 18432), logging setup (`flexi_logger`), tray + global hooks (`rdev`).
- `src/core/processor.rs`: orchestrates Detector → LlmClient (`src/api/client.rs`) → ClipboardManager. Look here for `PendingFix` handling and processing flags (`Arc<AtomicBool>`).
- `src/core/detector.rs`: heuristics and regexes that determine whether text should be sent to the LLM (ligatures, broken words, math, URL/path skipping).
- `src/ui.rs`: `eframe`/`egui` UI and tray integration; theme applied via `apply_japanese_theme` and UI state machine (`Idle`, `Waiting`, `Streaming`, `Finished`, `Error`, `Incomplete`).
- `src/api/client.rs`: LLM communication, supports streaming updates (emits `UiEvent::StreamUpdate`).

## Runtime & Developer Workflows (concrete commands)
- Build & run locally (use Release): `cargo run --release` (release runtime expected for hooks and packaging).
- Stop running instance: `cargo run -- --stop` (app listens on TCP 18432 for control commands).
- Dump latest logs: `cargo run -- --log` or inspect `logs/` for the most recent `.log` file.
- `build.rs` copies `config/` into `target/<profile>/config` during build — editing configs in `config/` is the main way to change defaults.

## Concurrency & Messaging Patterns (what to reuse)
- `tokio` runtime for async flows.
- Cross-component messaging: `tokio::sync::mpsc` for UI events (`UiEvent`), `tokio::sync::watch` for short-lived shared state like `PendingFix`.
- Shared flags use `Arc<AtomicBool>`; UI state uses `Arc<Mutex<T>>` — follow these patterns when adding new shared state.

## Integration Points & External Dependencies
- Clipboard: `arboard` (cross-platform) + `rdev` for global input events; changes here affect core detection and testing.
- Tray & UI: `tray-icon` alongside `eframe` — tray events are handled separately from the eframe update loop.
- HTTP/LLM: `reqwest` is configured for long timeouts and streaming; follow existing client patterns for retries/timeouts.

## Project-specific Conventions
- Prefer minimal, focused edits. Avoid broad architectural refactors unless requested.
- Preserve the `Detector -> Processor -> LlmClient -> Clipboard` flow and `PendingFix` semantics when adding processing features.
- UI changes: use `apply_japanese_theme` and existing color constants; prefer adding small controls to `src/ui.rs` rather than large reworks.
- Log usage: use `log::{info, error}` and avoid panics in main loops; surface errors to `logs/` only.

## Useful Code Examples (search these when coding)
- Event names & flow: see `src/core/events.rs` for `AppEvent` / `UiEvent` definitions.
- Detection logic: `src/core/detector.rs`.
- LLM client patterns: `src/api/client.rs` (streaming, timeout behavior).

### UiEvent examples & lifecycle
- `UiEvent` enum is declared in `src/ui.rs` and contains these variants: `ProcessingStarted`, `CopyPressed`, `SetProcessingEnabled(bool)`, `ShowResult(String, String)`, `StreamUpdate(String)`, `StreamEnd(bool)`, `StreamError(String)`, `Quit` — see [src/ui.rs](src/ui.rs#L21).
- Common send sites (examples):
	- Backend signals start: `ui_tx.send(UiEvent::ProcessingStarted)` — example in [src/core/actions.rs](src/core/actions.rs#L72).
	- Backend returns final result: `ui_tx.send(UiEvent::ShowResult(original, processed))` — example in [src/core/actions.rs](src/core/actions.rs#L96).
	- Backend reports an error: `ui_tx.send(UiEvent::StreamError(e.to_string()))` — example in [src/core/actions.rs](src/core/actions.rs#L99).
	- LLM streaming chunks: `tx.try_send(crate::ui::UiEvent::StreamUpdate(content))` inside the streaming loop — see [src/api/client.rs](src/api/client.rs#L400).
- Channel wiring pattern to preserve when modifying events:
	1. UI uses a blocking std channel: `let (ui_tx, ui_rx) = std::sync::mpsc::channel::<UiEvent>();` — [src/main.rs](src/main.rs#L104).
	2. Async producers (LLM client) use `tokio::sync::mpsc::Sender<UiEvent>` for non-blocking streaming; `LlmClient::set_ui_tx` accepts that channel — [src/api/client.rs](src/api/client.rs#L73).
	3. `main.rs` creates a bridge task that receives from the `tokio` channel and forwards events to the std `ui_tx` for the UI loop — see the bridge in [src/main.rs](src/main.rs#L171-L200).
	- When adding or renaming `UiEvent` variants, update `src/ui.rs`, all send sites in `src/core/*` and `src/api/*`, and the bridge in `src/main.rs`.


## Editing & Testing Tips
- When testing clipboard behavior locally, run in release mode and use a second terminal to inspect logs: `Get-Content (Get-ChildItem logs\\*.log | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName -Tail 50`.
- Be conservative editing `build.rs` because it handles packaging of `config/` into `target/`.

---
If any section is unclear or you want more detail (examples of `UiEvent` payloads or `PendingFix` lifecycle), tell me which area to expand. 

