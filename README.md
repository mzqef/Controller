# IntelliBoard

[![Rust](https://img.shields.io/badge/Rust-1.70%2B-orange)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20%7C%20macOS-blue)]()
[![License](https://img.shields.io/badge/License-MIT-green)](LICENSE)

IntelliBoard is a Rust desktop agent that enhances clipboard workflows using LLMs. It detects copied text (ligatures, OCR errors, math, etc.), sends it to configured LLMs for fixes/translations/explanations, and updates the clipboard with the result.

## Table of Contents

- [Quick Start](#quick-start)
- [Installation](#installation)
- [Features](#features)
- [Configuration Guide](#configuration-guide)
- [Architecture](#architecture)
- [Developer Guide](#developer-guide)
- [Troubleshooting](#troubleshooting)
- [Contributing](#contributing)

---

## Quick Start

### Workflow
1. Copy text to clipboard.
2. Press a hotkey within ~2s to trigger an action.
3. IntelliBoard processes the text and updates the clipboard (or shows a UI overlay for queries).

**Key behavior**: Hotkeys are only intercepted when you've copied text recently (within 2 seconds). Otherwise, they pass through to other applications — so `Ctrl+T` opens a new browser tab normally when you haven't just copied something.

### Default Hotkeys

| Shortcut | Action |
|---:|:---|
| `Ctrl+R` | Format (fix ligatures, broken OCR, remove spurious line breaks) |
| `Ctrl+T` | Translate English → Chinese |
| `Ctrl+Y` | Translate Chinese → English |
| `Ctrl+E` | Explain (concise explanation) |
| `Ctrl+O` | Image OCR (extract text from clipboard image) |

---

## Installation

### System Requirements

| Platform | Requirements |
|----------|--------------|
| **Windows** | Windows 10/11, MSVC toolchain |
| **Linux** | libxdo-dev, libxcb-*, libgtk-3-dev |
| **macOS** | Xcode Command Line Tools |

### Prerequisites

- **Rust 1.70+** with edition 2021 support
- **API Key** for your LLM provider (e.g., OpenAI, Alibaba Cloud DashScope)

### Build from Source

```bash
# Clone the repository
git clone https://github.com/mzqef/IntelliBoard.git
cd IntelliBoard

# Build in release mode
cargo build --release

# The executable will be at:
# Windows: target/release/IntelliBoard.exe
# Linux/macOS: target/release/IntelliBoard
```

### Linux Dependencies

```bash
# Ubuntu/Debian
sudo apt install libxdo-dev libxcb-shape0-dev libxcb-xfixes0-dev libgtk-3-dev

# Fedora
sudo dnf install libxdo-devel libxcb-devel gtk3-devel
```

### Environment Setup

Create a `.env` file in the project root or set environment variables:

```bash
# Required: Your LLM API key
export API_KEY="sk-your-api-key-here"
```

### Running

```bash
# Start IntelliBoard (runs as system tray application)
./target/release/IntelliBoard

# Stop running instance
./target/release/IntelliBoard --stop

# Show latest log file
./target/release/IntelliBoard --log
```

---

## Features

| Feature | Description |
|---------|-------------|
| **Low-level keyboard hook** | Uses `SetWindowsHookEx(WH_KEYBOARD_LL)` on Windows — only intercepts when you have recently copied text |
| **IME-aware debouncing** | Time + content stability checks prevent false triggers during Pinyin/Japanese IME composition |
| **Per-action configuration** | Each AI function can use different models, prompts, and API endpoints |
| **Remote/local fallback** | Automatically switches to local LLM when remote API is unreachable |
| **Vision/OCR support** | Extract text from clipboard images using vision-language models (qwen-vl-ocr) |
| **Streaming UI** | Real-time typewriter effect for LLM responses |
| **Non-blocking processing** | Small processing indicator bar that doesn't steal focus |
| **Memory Graph** | Visual clipboard history with relationships (left-click tray icon) |
| **Tray integration** | Enable/disable processing, open logs, launch config UI, custom commands |
| **Hot-reload** | Config changes are applied automatically without restarting |
| **Export path setting** | Configure where Memory Graph exports are saved |

---

## Configuration Guide

IntelliBoard uses a modular configuration system with **hot-reload** — changes are applied automatically without restarting.

### Configuration Files

| File | Purpose |
|------|---------|
| `config/actions.toml` | AI action definitions (prompts, models, API settings) |
| `config/hotkeys.toml` | Hotkey bindings (key + modifier → action mapping) |
| `config/commands.toml` | Custom tray menu commands |
| `.env` | Secrets (API keys) |

### Editing Options

#### Option 1: Configuration UI (Recommended)
1. Right-click the tray icon → **"Configuration"**
2. Use the **Hotkeys** tab to add/modify key bindings
3. Use the **AI Actions** tab to edit prompts and model settings
4. Use the **Settings** tab for API endpoints and export path
5. Click **Save All** to apply changes

#### Option 2: Direct File Editing
Edit the TOML files directly in `config/`. Changes are detected and applied automatically.

### Adding a Custom AI Action

1. Open `config/actions.toml` and add:

```toml
[[actions]]
id = "summarize"
label = "Summarize"
description = "Summarize long text in bullet points"

[actions.remote]
prompt = """Summarize the following text in 3-5 bullet points.
Be concise and capture the key points.
Return ONLY the bullet points."""
temperature = 0.3

[actions.local]
prompt = "Summarize in bullet points:"
```

2. Bind it to a hotkey in `config/hotkeys.toml`:

```toml
[[bindings]]
key = "KeyS"
action = "summarize"
modifiers = "Ctrl"
```

### Adding a Custom Hotkey

```toml
[[bindings]]
key = "KeyC"              # KeyA-KeyZ, Digit0-Digit9, F1-F12
action = "code_review"    # Must match an action's id field
modifiers = "Ctrl+Shift"  # Ctrl, Ctrl+Shift, Ctrl+Alt, Alt
```

### User Override Directory

Your personal config overrides are stored separately from the repo:

| Platform | Location |
|----------|----------|
| Windows | `%APPDATA%\IntelliBoard\` |
| Linux | `~/.config/IntelliBoard/` |
| macOS | `~/Library/Application Support/IntelliBoard/` |

Files here overlay the defaults in `config/`. The Config UI saves to this location.

### Environment Variables

```dotenv
API_KEY=sk-your-actual-key-here
```

Reference in TOML with `${VAR_NAME}` syntax:
```toml
api_key = "${API_KEY}"
```

### Extensibility: Extra API Parameters

Any extra fields in `[actions.remote]` or `[actions.local]` are passed through to the API request:

```toml
[actions.remote]
model = "qwen-mt-flash"
is_translation = true
source_lang = "auto"
target_lang = "Chinese"
# These extra fields are passed directly to the API:
style = "formal"
domain = "technical"
```

### Settings Tab Options

| Setting | Description |
|---------|-------------|
| Remote API URL | Default API endpoint for all actions |
| Remote API Key | API key (supports `${ENV_VAR}` syntax) |
| Local LLM URL | Fallback local LLM endpoint |
| Force Local Mode | Always use local LLM, ignore remote |
| Export Path | Directory for Memory Graph exports |

---

## Architecture

For detailed architecture documentation, see [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md).

### Quick Overview

```
┌─────────────┐     ┌──────────┐     ┌───────────┐     ┌──────────┐
│  Clipboard  │────▶│ Detector │────▶│ LLM Client│────▶│ UI Overlay│
└─────────────┘     └──────────┘     └───────────┘     └──────────┘
                          │                │
                          ▼                ▼
                    ┌──────────┐     ┌───────────┐
                    │ Memory   │     │ Clipboard │
                    │ Store    │     │ Write     │
                    └──────────┘     └───────────┘
```

### Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | App entry, single-instance guard, tray setup, runtime bridge |
| `src/platform/windows.rs` | Low-level keyboard hook with copy-gated blocking |
| `src/core/actions.rs` | High-level action handling and LLM calls |
| `src/core/detector.rs` | Regex/heuristics for text classification |
| `src/api/client.rs` | LLM client with remote/local fallback, streaming |
| `src/ui.rs` | UI overlay and UiEvent state machine |
| `src/core/memory_store.rs` | SQLite-backed clipboard history |

### Standalone UI Binaries

| Binary | Purpose |
|--------|---------|
| `functions_config_ui` | Hotkeys + Actions + Settings configuration editor |
| `memory_graph_ui` | Visual memory graph viewer |

---

## Developer Guide

### Commands

```bash
# Build & run (use Release for production-like behavior)
cargo run --release

# Stop running instance
cargo run -- --stop

# Build all binaries
cargo build --release --bins

# Run tests
cargo test

# Generate documentation
cargo doc --open
```

### Log Inspection

```powershell
# PowerShell: View latest log
Get-Content (Get-ChildItem "logs\*.log" | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName -Tail 50
```

```bash
# Bash: View latest log
tail -50 $(ls -t logs/*.log | head -1)
```

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `API_KEY` | LLM API key |
| `IntelliBoard_DIAG_CLIPBOARD` | Set to `1` for detailed clipboard event logging |

### Testing Tips

- Test clipboard flows in `--release` mode (hooks behave consistently)
- Set `IntelliBoard_DIAG_CLIPBOARD=1` for detailed clipboard event logging
- Inspect logs in `logs/` for request/response bodies
- When testing streaming, verify `LlmClient` received a UI sender via `set_ui_tx`

---

## Troubleshooting

### Common Issues

| Issue | Solution |
|-------|----------|
| **Hotkeys don't work** | Ensure you copied text within the last 2 seconds before pressing the hotkey |
| **"Failed to connect to graph server"** | Make sure the main IntelliBoard process is running |
| **API errors** | Check your API key in `.env` or config; verify network connectivity |
| **IME conflicts** | IntelliBoard uses debouncing to avoid IME interference; wait for composition to complete |
| **Multiple instances** | IntelliBoard uses a single-instance guard on TCP port 18432 |

### Checking Logs

Logs are stored in the `logs/` directory with timestamps. Check the most recent log for errors:

```powershell
# Windows PowerShell
Get-Content (Get-ChildItem "logs\*.log" | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName -Tail 100
```

### Resetting Configuration

Delete user override files to reset to defaults:

```powershell
# Windows
Remove-Item "$env:APPDATA\IntelliBoard\*.toml"

# Linux/macOS
rm ~/.config/IntelliBoard/*.toml
```

---

## Contributing

- Keep edits minimal and localized
- Preserve the `Clipboard → ActionHandler → LlmClient → UI` flow
- Follow existing patterns for shared state (`Arc<Mutex>`, `Arc<RwLock>`)
- When adding a `UiEvent` variant, update `src/ui.rs`, all send sites, and the bridge in `main.rs`

### Code Style

- Use `rustfmt` for formatting
- Use `clippy` for linting: `cargo clippy --all-targets`
- Add `///` documentation comments to public items

### Pull Request Guidelines

1. Create a feature branch from `main`
2. Write tests for new functionality
3. Ensure `cargo test` and `cargo clippy` pass
4. Update documentation if needed
5. Keep PRs focused on a single concern

---

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

---

For detailed implementation guidance, see:
- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) - Technical architecture
- [.github/copilot-instructions.md](.github/copilot-instructions.md) - AI coding agent guidelines

