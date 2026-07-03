# IntelliBoard Architecture

This document provides a comprehensive overview of IntelliBoard's architecture, including data flow diagrams, component relationships, and the complete project structure.

## Table of Contents

- [High-Level Overview](#high-level-overview)
- [Data Flow](#data-flow)
- [Event System](#event-system)
- [Component Architecture](#component-architecture)
- [IPC Architecture](#ipc-architecture)
- [Project Structure](#project-structure)
- [Module Dependency Graph](#module-dependency-graph)

---

## High-Level Overview

IntelliBoard is a Rust desktop agent that enhances clipboard workflows using LLMs. The application runs as a system tray icon with a low-level keyboard hook that intercepts hotkeys only when clipboard content is available for processing.

```mermaid
flowchart LR
    subgraph Input
        A[📋 Clipboard] --> B[Detector]
        K[⌨️ Keyboard Hook] --> C{Recent Copy?}
    end
    
    subgraph Processing
        C -->|Yes| D[Action Handler]
        C -->|No| E[Pass Through]
        B --> D
        D --> F[LLM Client]
        F -->|Remote| G[Cloud API]
        F -->|Fallback| H[Local LLM]
    end
    
    subgraph Output
        G --> I[📋 Write Clipboard]
        H --> I
        G --> J[🖥️ UI Overlay]
        H --> J
    end
```

---

## Data Flow

### Clipboard Processing Pipeline

```mermaid
sequenceDiagram
    participant User
    participant Clipboard
    participant Hook as Keyboard Hook
    participant Detector
    participant Action as Action Handler
    participant LLM as LLM Client
    participant UI as UI Overlay
    
    User->>Clipboard: Copy text (Ctrl+C)
    Clipboard->>Detector: Content changed
    Detector->>Detector: Debounce (150-250ms)
    
    User->>Hook: Press hotkey (e.g., Ctrl+T)
    Hook->>Hook: Check last_copy_time < 2s
    Hook->>Action: Dispatch action
    
    Action->>LLM: Send text + prompt
    
    alt Remote API Available
        LLM->>LLM: Stream response
        LLM-->>UI: StreamUpdate chunks
    else Remote Unavailable
        LLM->>LLM: Fallback to local
        LLM-->>UI: StreamUpdate chunks
    end
    
    LLM->>Clipboard: Write result
    LLM->>UI: StreamEnd
    UI->>User: Show result overlay
```

### Memory System Flow

```mermaid
sequenceDiagram
    participant Clipboard
    participant MemStore as MemoryStore
    participant IPC as IPC Server
    participant GraphUI as Memory Graph UI
    
    Clipboard->>MemStore: add_clipboard(text)
    MemStore->>MemStore: Dedupe check
    MemStore->>MemStore: Persist to SQLite
    MemStore->>IPC: bump_revision()
    IPC->>GraphUI: Push DataChanged
    GraphUI->>IPC: GetSnapshot
    IPC->>GraphUI: Snapshot{items, edges}
    GraphUI->>GraphUI: Render graph
```

---

## Event System

IntelliBoard uses a multi-channel event system for communication between components.

### Event Types

```mermaid
classDiagram
    class AppEvent {
        <<enumeration>>
        TriggerAction(Action)
        UserQuery(String)
        ToggleProcessing(bool)
        Cancel
        ShowMemoryGraph
        ShowHotkeyConfig
        ConfigChanged(ConfigChange)
    }
    
    class UiEvent {
        <<enumeration>>
        ProcessingStarted(String, String)
        ShowResult(String, String)
        StreamUpdate(String)
        StreamEnd(bool)
        StreamError(String)
        ShowMemoryGraph
        ShowHotkeyConfig
        Quit
    }
    
    class MemoryEvent {
        <<enumeration>>
        ClipboardAdded(String)
        ActionResult(ActionType, String, Uuid)
    }
    
    class GraphRequest {
        <<enumeration>>
        GetSnapshot
        UpdateNodePosition
        UpdateItemTitle
        PromoteItem
        AddUserEdge
        DeleteItem
        ClearAllPositions
    }
```

### Channel Architecture

```mermaid
flowchart TB
    subgraph "Main Thread"
        TrayMenu[Tray Menu]
        EguiLoop[eframe/egui Loop]
    end
    
    subgraph "Tokio Runtime"
        AppEventRx[AppEvent Receiver]
        ActionHandler[Action Handler]
        LlmClient[LLM Client]
        IpcServer[IPC Server]
    end
    
    subgraph "Hook Thread"
        KeyboardHook[WH_KEYBOARD_LL Hook]
        ClipboardListener[Clipboard Listener]
    end
    
    subgraph "Child Processes"
        MemoryGraphUI[memory_graph_ui.exe]
        ConfigUI[functions_config_ui.exe]
    end
    
    KeyboardHook -->|"mpsc<AppEvent>"| AppEventRx
    ClipboardListener -->|"mpsc<AppEvent>"| AppEventRx
    TrayMenu -->|"mpsc<AppEvent>"| AppEventRx
    
    AppEventRx --> ActionHandler
    ActionHandler --> LlmClient
    LlmClient -->|"mpsc<UiEvent>"| EguiLoop
    
    IpcServer <-->|"TCP JSON"| MemoryGraphUI
    ConfigUI -->|"File Watch"| AppEventRx
```

---

## Component Architecture

### Core Components

| Component | File | Responsibility |
|-----------|------|----------------|
| **Entry Point** | `main.rs` | Single-instance guard, tray setup, runtime bridge |
| **UI Overlay** | `ui.rs` | eframe app, UiEvent state machine, result display |
| **Action Handler** | `core/actions.rs` | Orchestrates Detector → LLM → Clipboard flow |
| **LLM Client** | `api/client.rs` | Remote/local API calls, streaming, fallback |
| **Detector** | `core/detector.rs` | Regex/heuristics for text classification |
| **Clipboard** | `core/clipboard.rs` | arboard wrapper, IME-aware debouncing |
| **Memory Store** | `core/memory_store.rs` | SQLite-backed clipboard history |
| **Config** | `core/config.rs` | TOML loading, hot-reload, user overrides |
| **IPC Server** | `core/ipc_server.rs` | TCP server for Memory Graph UI |
| **Process Manager** | `core/process_manager.rs` | Child process spawning/cleanup |

### Platform Layer

```mermaid
flowchart TB
    subgraph "Platform Abstraction"
        PlatformMod[platform/mod.rs]
    end
    
    subgraph "Windows"
        WinHook[windows.rs]
        WinHook --> |WH_KEYBOARD_LL| SetWindowsHookEx
        WinHook --> |Copy-gated blocking| last_copy_time
    end
    
    subgraph "Unix"
        UnixHook[unix.rs]
        UnixHook --> |rdev grab| GlobalHotkey
    end
    
    PlatformMod --> WinHook
    PlatformMod --> UnixHook
```

---

## IPC Architecture

### Memory Graph Communication

```mermaid
flowchart LR
    subgraph "Main Process"
        MemStore[(MemoryStore<br/>SQLite)]
        IpcSrv[IPC Server<br/>:12345]
        Notifier[broadcast::Sender]
    end
    
    subgraph "Graph UI Process"
        GraphView[MemoryGraphView]
        TcpClient[TCP Client]
    end
    
    MemStore <--> IpcSrv
    IpcSrv <-->|"JSON over TCP"| TcpClient
    TcpClient <--> GraphView
    
    Notifier -->|"DataChanged push"| IpcSrv
    IpcSrv -->|"Push notification"| TcpClient
```

### IPC Protocol

| Request | Response | Description |
|---------|----------|-------------|
| `GetSnapshot` | `Snapshot{items, edges}` | Fetch all memory items and edges |
| `UpdateNodePosition{id, x, y}` | `Snapshot` | Move node, persist position |
| `UpdateItemTitle{id, title}` | `Snapshot` | Rename item |
| `PromoteItem{id, target_type}` | `Snapshot` | Promote to Mid/Long-term |
| `AddUserEdge{source, target}` | `Snapshot` | Create user-defined link |
| `DeleteItem{id}` | `Snapshot` | Delete item and orphan edges |
| `ClearAllPositions` | `Snapshot` | Reset all node positions (Auto Align) |

---

## Project Structure

```
IntelliBoard/
├── 📄 Cargo.toml              # Dependencies and build config
├── 📄 build.rs                # Icon embedding, config copy
├── 📄 README.md               # User documentation
├── 📄 LICENSE                 # License file
├── 📄 DISTRIBUTION.md         # Distribution guidelines
│
├── 📁 config/                 # Default configuration files
│   ├── actions.toml           # AI action definitions
│   ├── hotkeys.toml           # Hotkey bindings
│   └── commands.toml          # Custom tray commands
│
├── 📁 docs/                   # Documentation
│   └── ARCHITECTURE.md        # This file
│
├── 📁 resources/              # Static resources
│   └── icon.png               # Application icon
│
├── 📁 src/
│   ├── 📄 main.rs             # Entry point, tray, runtime bridge
│   ├── 📄 lib.rs              # Library crate root
│   ├── 📄 startup.rs          # Platform-agnostic initialization
│   ├── 📄 ui.rs               # Main UI overlay (eframe app)
│   │
│   ├── 📁 api/                # External API clients
│   │   ├── mod.rs
│   │   └── client.rs          # LLM client with streaming
│   │
│   ├── 📁 core/               # Core business logic
│   │   ├── mod.rs
│   │   ├── actions.rs         # Action handler orchestration
│   │   ├── clipboard.rs       # Clipboard with debouncing
│   │   ├── clipboard_listener.rs  # arboard integration
│   │   ├── config.rs          # Config types and loaders
│   │   ├── config_watcher.rs  # Hot-reload via notify
│   │   ├── detector.rs        # Text classification heuristics
│   │   ├── events.rs          # AppEvent, MemoryEvent enums
│   │   ├── ipc_messages.rs    # GraphRequest/Response types
│   │   ├── ipc_server.rs      # TCP server for Graph UI
│   │   ├── memory.rs          # MemoryItem, MemoryEdge types
│   │   ├── memory_store.rs    # SQLite persistence layer
│   │   └── process_manager.rs # Child process management
│   │
│   ├── 📁 platform/           # Platform-specific code
│   │   ├── mod.rs             # Conditional compilation
│   │   ├── windows.rs         # WH_KEYBOARD_LL hook
│   │   └── unix.rs            # rdev-based hotkeys
│   │
│   ├── 📁 ui/                 # UI components
│   │   ├── memory_graph.rs    # Visual graph rendering
│   │   ├── hotkey_config.rs   # (Legacy) hotkey editor
│   │   └── theme.rs           # "Japan 2046" theme
│   │
│   └── 📁 bin/                # Standalone executables
│       ├── functions_config_ui.rs  # Config editor window
│       ├── hotkey_config_ui.rs     # (Legacy) hotkey editor
│       └── memory_graph_ui.rs      # Memory graph viewer
│
├── 📁 logs/                   # Runtime log files
├── 📁 tests/                  # Test suite
│   └── integration/           # Integration tests (TODO)
│
└── 📁 target/                 # Build output
    └── release/
        ├── IntelliBoard.exe
        ├── functions_config_ui.exe
        └── memory_graph_ui.exe
```

---

## Module Dependency Graph

```mermaid
flowchart TB
    subgraph "Binaries"
        MainBin[main.rs]
        FuncConfigBin[functions_config_ui]
        MemGraphBin[memory_graph_ui]
    end
    
    subgraph "Library Crate"
        Lib[lib.rs]
        
        subgraph "Core"
            Actions[actions]
            Clipboard[clipboard]
            Config[config]
            ConfigWatcher[config_watcher]
            Detector[detector]
            Events[events]
            IpcMsg[ipc_messages]
            IpcSrv[ipc_server]
            Memory[memory]
            MemStore[memory_store]
            ProcMgr[process_manager]
        end
        
        subgraph "API"
            LlmClient[client]
        end
        
        subgraph "UI"
            MemGraph[memory_graph]
            Theme[theme]
        end
        
        subgraph "Platform"
            Windows[windows]
            Unix[unix]
        end
    end
    
    MainBin --> Lib
    FuncConfigBin --> Lib
    MemGraphBin --> Lib
    
    Lib --> Core
    Lib --> API
    Lib --> UI
    Lib --> Platform
    
    Actions --> LlmClient
    Actions --> Clipboard
    Actions --> Detector
    Actions --> Config
    
    IpcSrv --> MemStore
    IpcSrv --> IpcMsg
    
    MemStore --> Memory
    
    MemGraph --> IpcMsg
    MemGraph --> Memory
```

---

## Key Patterns

### Shared State

| Pattern | Usage | Location |
|---------|-------|----------|
| `Arc<Mutex<T>>` | Tray handler, UI state | `ui.rs`, `main.rs` |
| `Arc<RwLock<T>>` | Hot-reloadable configs | `config.rs` |
| `Arc<AtomicBool>` | Processing flags | `actions.rs` |
| `Arc<MemoryStore>` | Clipboard history | `memory_store.rs` |

### Async Patterns

| Pattern | Usage | Location |
|---------|-------|----------|
| `tokio::spawn` | Background tasks | `main.rs`, `ipc_server.rs` |
| `tokio::select!` | Event multiplexing | `main.rs`, `handle_client` |
| `tokio::sync::mpsc` | Cross-thread messaging | UI events, app events |
| `tokio::sync::broadcast` | Push notifications | IPC server |
| `tokio::sync::OnceCell` | Lazy initialization | LLM client |

### Error Handling

| Pattern | Usage |
|---------|-------|
| `anyhow::Result` | Top-level error propagation |
| `log::error!` + continue | Non-fatal errors in loops |
| Lock poison recovery | `RwLock`/`Mutex` in `memory_store.rs` |

---

## Configuration Hot-Reload

```mermaid
sequenceDiagram
    participant User
    participant File as TOML File
    participant Watcher as ConfigWatcher
    participant Main as Main Loop
    participant Config as Config State
    
    User->>File: Edit and save
    File->>Watcher: notify::Event
    Watcher->>Watcher: Debounce (100ms)
    Watcher->>Main: ConfigChanged(Actions/Hotkeys)
    Main->>Config: Reload from file
    Config->>Config: Merge user overrides
    Main->>Main: Update Arc<RwLock<Config>>
```

---

## See Also

- [README.md](../README.md) - User documentation
- [.github/copilot-instructions.md](../.github/copilot-instructions.md) - AI coding agent guidelines
- [config/actions.toml](../config/actions.toml) - Action configuration reference
