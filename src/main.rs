#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]


use IntelliBoard::core::clipboard::ClipboardManager;
use IntelliBoard::core::config::{ActionsConfig, CommandsConfig};
use IntelliBoard::core::config_watcher::{ConfigWatcher, ConfigChange};
use IntelliBoard::core::events::AppEvent;
use IntelliBoard::core::actions::{Action, ActionHandler};
use IntelliBoard::core::clipboard_listener;
use IntelliBoard::core::graph_server;
use IntelliBoard::core::memory::MemoryEvent;
use IntelliBoard::core::memory_store::MemoryStore;
use IntelliBoard::core::ipc_server::GraphIpcServer;
use IntelliBoard::core::process_manager::ProcessManager;
use IntelliBoard::api::client::LlmClient;
use IntelliBoard::ui::{MyApp, UiEvent, TrayHandler};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;
use std::io::{Read, Write};
use tokio::sync::mpsc;
use log::{info, error, debug};
use flexi_logger::{Logger, FileSpec, Criterion, Naming, Cleanup};
use clap::Parser;
use single_instance::SingleInstance;
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem, CheckMenuItem, PredefinedMenuItem}};

const CONTROL_ADDR: &str = "127.0.0.1:18432";

#[derive(Parser, Debug)]
#[command(name = "IntelliBoard")]
struct Opt {
    #[arg(short, long)]
    config: Option<String>,

    #[arg(long)]
    stop: bool,

    #[arg(long)]
    log: bool,
}

fn send_stop_command() -> anyhow::Result<()> {
    match std::net::TcpStream::connect(CONTROL_ADDR) {
        Ok(mut stream) => {
            stream.write_all(b"stop\n")?;
            println!("Stop command sent to IntelliBoard.");
        }
        Err(e) => {
            println!("No running IntelliBoard control server found at {}: {}", CONTROL_ADDR, e);
        }
    }
    Ok(())
}

fn start_control_server(ui_tx: flume::Sender<UiEvent>) {
    std::thread::spawn(move || {
        let listener = match std::net::TcpListener::bind(CONTROL_ADDR) {
            Ok(listener) => listener,
            Err(e) => {
                log::error!("Failed to bind control server at {}: {}", CONTROL_ADDR, e);
                return;
            }
        };

        info!("Control server listening on {}", CONTROL_ADDR);
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };

            let mut command = String::new();
            if stream.read_to_string(&mut command).is_ok() && command.trim().eq_ignore_ascii_case("stop") {
                info!("Stop command received");
                let _ = ui_tx.send(UiEvent::Quit);
                break;
            }
        }
    });
}

fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

    if opt.stop {
        return send_stop_command();
    }

    let instance = SingleInstance::new("IntelliBoard_app").map_err(|e| anyhow::anyhow!("Failed to check single instance: {}", e))?;
    if !instance.is_single() {
        if opt.log {
            // Log checking logic omitted for brevity, assuming user knows how to check logs
            println!("IntelliBoard is already running.");
            return Ok(());
        } else {
            eprintln!("IntelliBoard is already running.");
            return Ok(());
        }
    }

    Logger::try_with_str("info")?
        .log_to_file(FileSpec::default().directory("logs").basename("IntelliBoard"))
        .write_mode(flexi_logger::WriteMode::Direct)
        .format(flexi_logger::opt_format)
        .rotate(Criterion::Size(10 * 1024 * 1024), Naming::Timestamps, Cleanup::KeepLogFiles(3))
        .start()?;

    std::panic::set_hook(Box::new(|info| {
        error!("Panic occurred: {:?}", info);
    }));

    info!("Starting IntelliBoard...");
    
    // Load .env file if present
    match dotenv::dotenv() {
        Ok(path) => info!("Loaded .env from: {:?}", path),
        Err(e) => info!("Could not load .env file: {} (using system env vars)", e),
    }

    // Verify critical environment variables
    if std::env::var("API_KEY").is_ok() {
        info!("API_KEY found in environment.");
    } else {
        log::warn!("API_KEY not found in environment. Remote features may fail.");
    }
    
    // Load Actions configuration (replaces LLM config with dynamic actions)
    let actions_config: ActionsConfig = match IntelliBoard::core::config::load_actions_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            log::warn!("Failed to load actions config: {}, using defaults", e);
            ActionsConfig::default()
        }
    };

    let cmd_config: Option<CommandsConfig> = match IntelliBoard::core::config::load_commands_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            log::warn!("Failed to load commands config: {}", e);
            None
        }
    };
    
    let hotkeys_config = match IntelliBoard::core::config::load_hotkeys_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            log::warn!("Failed to load hotkeys config: {}", e);
            IntelliBoard::core::config::HotkeysConfig::default()
        }
    };
    
    // Wrap hotkeys in Arc<RwLock> for hot-reload support
    let shared_hotkeys = std::sync::Arc::new(std::sync::RwLock::new(hotkeys_config));
    
    // Wrap actions config in Arc<RwLock> for hot-reload
    let shared_actions = std::sync::Arc::new(std::sync::RwLock::new(actions_config.clone()));

    // Channels
    let (ui_tx, ui_rx) = flume::unbounded::<UiEvent>();
    let (tx, mut rx) = mpsc::channel::<AppEvent>(100);
    let (graph_tx, graph_rx) = mpsc::channel::<MemoryEvent>(256);
    start_control_server(ui_tx.clone());
    
    // Config watcher for hot-reload
    let config_watcher = match ConfigWatcher::new("config") {
        Ok(w) => Some(w),
        Err(e) => {
            log::warn!("Failed to start config watcher: {}", e);
            None
        }
    };
    
    // Forward config changes to main event loop
    if let Some(watcher) = config_watcher {
        let tx_config = tx.clone();
        std::thread::spawn(move || {
            loop {
                if let Some(change) = watcher.try_recv() {
                    let _ = tx_config.blocking_send(AppEvent::ConfigChanged(change));
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });
    }

    // Enable dark mode for native Win32 context menus (tray right-click, etc.)
    // Must be called before TrayIconBuilder::build() to take effect.
    #[cfg(windows)]
    IntelliBoard::platform::enable_dark_mode();

    // Setup Tray
    let tray_menu = Menu::new();
    let show_log_i = MenuItem::new("Show Log", true, None);
    let show_log_id = show_log_i.id().clone();
    let hotkey_config_i = MenuItem::new("Configuration", true, None);
    let hotkey_config_id = hotkey_config_i.id().clone();
    let enable_i = CheckMenuItem::new("Enable Processing", true, true, None);
    let enable_id = enable_i.id().clone();
    let local_mode_i = CheckMenuItem::new("Use Local Model", true, false, None);
    let local_mode_id = local_mode_i.id().clone();
    let exit_i = MenuItem::new("Exit", true, None);
    let exit_id = exit_i.id().clone();

    let mut custom_commands_map = std::collections::HashMap::new();
    if let Some(cfg) = &cmd_config {
        if !cfg.commands.is_empty() {
            let _ = tray_menu.append(&PredefinedMenuItem::separator());
            let mut sorted_commands: Vec<_> = cfg.commands.iter().collect();
            sorted_commands.sort_by_key(|(k, _)| *k);
            for (name, cmd) in sorted_commands {
                let item = MenuItem::new(name, true, None);
                custom_commands_map.insert(item.id().clone(), cmd.clone());
                tray_menu.append(&item).unwrap();
            }
            let _ = tray_menu.append(&PredefinedMenuItem::separator());
        }
    }

    tray_menu.append(&show_log_i).unwrap();
    tray_menu.append(&hotkey_config_i).unwrap();
    tray_menu.append(&enable_i).unwrap();
    tray_menu.append(&local_mode_i).unwrap();
    tray_menu.append(&exit_i).unwrap();

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_menu_on_left_click(false)
        .with_tooltip("IntelliBoard - Left-click for Memory Graph")
        .with_icon(IntelliBoard::load_tray_icon_active())
        .build()?;

    let tray_handler = Arc::new(Mutex::new(TrayHandler {
        icon: tray_icon,
        enable_item: enable_i,
        enable_id,
        local_mode_item: local_mode_i,
        local_mode_id,
        show_log_id,
        hotkey_config_id,
        exit_id,
        tx: tx.clone(),
        custom_commands: custom_commands_map,
    }));

     let gui_ctx = Arc::new(Mutex::new(None::<eframe::egui::Context>));

    // Core Components
    let clipboard = Arc::new(ClipboardManager::new().expect("Failed to init clipboard"));
    
    // Memory Store
    let memory_store = Arc::new(MemoryStore::new().expect("Failed to init memory store"));
    
    // Process Manager for child windows
    let process_manager = Arc::new(ProcessManager::new());
    
    let mut raw_llm_client = LlmClient::new(shared_actions.clone());
    // Use flume channel - works in both sync and async contexts
    raw_llm_client.set_ui_tx(ui_tx.clone());
    
    // Share the egui context so LlmClient can trigger repaints during streaming
    raw_llm_client.set_egui_ctx(gui_ctx.clone());
    
    // Cancel flag for stopping in-flight requests
    let cancel_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    raw_llm_client.set_cancel_flag(cancel_flag.clone());
    
    let llm_client = Arc::new(raw_llm_client);
    
    // UI event sender for non-streaming UI commands (e.g., show Memory Graph)
    let ui_tx_for_events = ui_tx.clone();

    let action_handler = Arc::new(ActionHandler::new(
        clipboard.clone(),
        llm_client.clone(),
        Some(ui_tx.clone()),
        Some(graph_tx.clone()),
        Some(memory_store.clone()),
    ));

    // Clipboard Listener
    let (cb_tx, mut cb_rx) = mpsc::channel(100);
    clipboard_listener::start_listener(cb_tx);

    let last_copy_time = Arc::new(Mutex::new(std::time::Instant::now()));

    // Diagnostics (used to correlate rare/IME-related issues)
    let diag_start = Instant::now();
    let diag_seq = Arc::new(AtomicU64::new(0));

    // Initialize platform-specific hotkey system
    let hotkey_system = match IntelliBoard::startup::init_hotkey_system(
        tx.clone(),
        shared_hotkeys.clone(),
        last_copy_time.clone(),
    ) {
        Ok(handle) => Some(handle),
        Err(e) => {
            log::error!("Failed to initialize hotkey system: {}", e);
            None
        }
    };
    
    // Clone shared configs for async block
    let shared_actions_inner = shared_actions.clone();
    let shared_hotkeys_inner = shared_hotkeys.clone();
    let hotkey_system_inner = hotkey_system.clone();
    let cancel_flag_inner = cancel_flag.clone();

    // Main Tokio Loop
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            info!("Event loop started");
            llm_client.spawn_health_check();

            // IPC Server for Graph UI (create first to get notifier)
            // Attach the LLM client so Auto Connect can call the AI model.
            let ipc_server = GraphIpcServer::new(memory_store.clone(), 12345)
                .with_llm_client(llm_client.clone());
            let graph_notifier = ipc_server.get_notifier();
            
            // Graph server task (receives MemoryEvent messages and updates MemoryStore)
            // Pass the notifier so it can push updates to connected graph UIs
            tokio::spawn(graph_server::run(memory_store.clone(), graph_rx, graph_notifier));

            // Start IPC Server
            tokio::spawn(async move {
                ipc_server.run().await;
            });

            loop {
                tokio::select! {
                    _ = cb_rx.recv() => {
                        // Update timestamp IMMEDIATELY for hotkey gating
                        // This must happen for ALL clipboard changes (text or image)
                        // before any other processing or filtering
                        if let Ok(mut guard) = last_copy_time.lock() {
                            *guard = std::time::Instant::now();
                        }
                        
                        // Store clipboard content to memory (skip programmatic writes)
                        // with IME-aware debouncing to handle Pinyin/other IME composition
                        if let Ok(text) = clipboard.get_text() {
                            if clipboard.should_ignore_clipboard_text(&text) {
                                if std::env::var_os("IntelliBoard_DIAG_CLIPBOARD").is_some() {
                                    log::debug!("[diag] ignored programmatic clipboard write");
                                }
                            } else {
                                // Apply time+content debouncing for IME compatibility.
                                //
                                // BUG FIX (selection toolbar never showed up): the previous
                                // implementation did `sleep(...); continue;` after a debounce
                                // hit. `continue` returns to the `select!`, which only re-enters
                                // this branch when ANOTHER clipboard event arrives. On Windows
                                // `clipboard_master` fires exactly once per copy, so the debounced
                                // content was never re-checked and the floating toolbar / memory
                                // write never fired for that copy.
                                //
                                // Now we loop locally: sleep, re-read the clipboard, and re-evaluate
                                // `should_debounce` until the content is stable (or it changes and
                                // we re-stabilize on the new content). We also bail out if the
                                // clipboard text drifts away from what we're waiting on, so a
                                // mid-debounce copy of different content resets cleanly via the
                                // outer loop's next event.
                                let mut current_text = text.clone();
                                loop {
                                    #[cfg(windows)]
                                    let ime_composing = IntelliBoard::platform::is_ime_composing();
                                    #[cfg(not(windows))]
                                    let ime_composing = false;

                                    match clipboard.should_debounce(&current_text, ime_composing) {
                                        None => break, // stable — fall through to processing
                                        Some(wait_ms) => {
                                            if std::env::var_os("IntelliBoard_DIAG_CLIPBOARD").is_some() {
                                                log::debug!("[diag] debouncing clipboard for {}ms (ime={})", wait_ms, ime_composing);
                                            }
                                            tokio::time::sleep(tokio::time::Duration::from_millis(wait_ms.min(50))).await;
                                            // Re-read: if the clipboard changed to something else,
                                            // abandon this stabilization and let the next clipboard
                                            // event handle the new content.
                                            match clipboard.get_text() {
                                                Ok(t) if t == current_text => continue,
                                                Ok(_) => {
                                                    if std::env::var_os("IntelliBoard_DIAG_CLIPBOARD").is_some() {
                                                        log::debug!("[diag] clipboard content changed mid-debounce; aborting this stabilization");
                                                    }
                                                    // The outer loop will pick it up on the next event.
                                                    // We break out without processing so we don't fire
                                                    // the toolbar on stale content.
                                                    // Skip the post-loop processing for this event.
                                                    // The fresh clipboard event will deliver it.
                                                    current_text.clear();
                                                    break;
                                                }
                                                Err(_) => break,
                                            }
                                        }
                                    }
                                }

                                if current_text.is_empty() {
                                    // Aborted mid-debounce (content changed underneath us).
                                    // The new content will arrive as its own clipboard event.
                                    continue;
                                }

                                // Content is stable - process it.
                                // Use the stabilized snapshot so we don't race with another copy.
                                let text = current_text;
                                clipboard.mark_processed(&text);
                                
                                // Pop up the floating action toolbar near the selection
                                // (only if it's meaningful text; skip very short / whitespace)
                                let trimmed = text.trim();
                                if trimmed.len() >= 2 && !trimmed.chars().all(|c| c.is_whitespace()) {
                                    let (px, py) = IntelliBoard::platform::get_selection_popup_pos();
                                    let _ = ui_tx.send(UiEvent::ShowActionToolbar {
                                        text: text.clone(),
                                        x: px,
                                        y: py,
                                    });
                                }
                                
                                if let Err(e) = graph_tx.send(MemoryEvent::AddClipboard(text)).await {
                                    error!("Failed to enqueue clipboard memory event: {}", e);
                                }
                            }
                        }
                        let now_ms = diag_start.elapsed().as_millis();
                        let seq = diag_seq.fetch_add(1, Ordering::Relaxed) + 1;
                        debug!("[diag #{seq} @ {now_ms}ms] Clipboard change detected");
                    }
                    Some(event) = rx.recv() => {
                        debug!("Processing event: {:?}", event);
                        match event {
                            AppEvent::TriggerAction(action) => {
                                // Cancel any in-flight request before starting new one
                                cancel_flag_inner.store(true, std::sync::atomic::Ordering::SeqCst);
                                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                                cancel_flag_inner.store(false, std::sync::atomic::Ordering::SeqCst);
                                
                                // Copy-gating is handled in the keyboard hook callback
                                // If we receive this event, the hook already verified a recent copy
                                let handler = action_handler.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handler.handle(action).await {
                                        // Don't log "Cancelled" as an error - it's intentional
                                        if !e.to_string().contains("Cancelled") {
                                            error!("Action failed: {}", e);
                                        }
                                    }
                                });
                            },
                            AppEvent::UserQuery(query) => {
                                // Cancel any in-flight request before starting new one
                                cancel_flag_inner.store(true, std::sync::atomic::Ordering::SeqCst);
                                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                                cancel_flag_inner.store(false, std::sync::atomic::Ordering::SeqCst);
                                
                                info!("Processing user query: {}", query);
                                let handler = action_handler.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handler.handle(Action::UserQuery(query)).await {
                                        if !e.to_string().contains("Cancelled") {
                                            error!("User query failed: {}", e);
                                        }
                                    }
                                });
                            },
                            AppEvent::SetClipboard(text) => {
                                // Toolbar wants the clipboard to hold the selected text
                                // before the action runs. Mark it programmatic so it
                                // doesn't get re-stored into memory, then let the
                                // TriggerAction that follows pick it up.
                                if let Err(e) = clipboard.set_text_programmatic(&text) {
                                    error!("SetClipboard failed: {}", e);
                                }
                            },
                             AppEvent::Cancel => {
                                info!("Cancel requested - stopping in-flight request");
                                cancel_flag_inner.store(true, std::sync::atomic::Ordering::SeqCst);
                            },
                            AppEvent::ToggleProcessing(enabled) => {
                                // processor.set_enabled(enabled); // We removed processor
                                info!("Processing enabled: {}", enabled);
                            },
                            AppEvent::ToggleLocalMode(use_local) => {
                                info!("Local mode toggled: {}", use_local);
                                llm_client.set_force_local(use_local);
                            },
                            AppEvent::ShowMemoryGraph => {
                                let _ = ui_tx_for_events.send(UiEvent::ShowMemoryGraph);
                            }
                            AppEvent::ShowHotkeyConfig => {
                                let _ = ui_tx_for_events.send(UiEvent::ShowHotkeyConfig);
                            }
                            // SelectionAt is reserved for a future mouse-hook-driven
                            // toolbar trigger (see AppEvent::SelectionAt docs). The
                            // toolbar is currently driven by the clipboard-change
                            // path below, so we just log here to keep the match
                            // exhaustive and forward-compatible.
                            AppEvent::SelectionAt { x, y } => {
                                debug!("SelectionAt received ({}, {}) — no mouse-hook handler wired up; toolbar is driven by clipboard change", x, y);
                            }
                            AppEvent::ConfigChanged(change) => {
                                info!("Config file changed: {:?}", change);
                                match change {
                                    ConfigChange::ActionsConfig => {
                                        match IntelliBoard::core::config::load_actions_config() {
                                            Ok(cfg) => {
                                                if let Ok(mut guard) = shared_actions_inner.write() {
                                                    *guard = cfg;
                                                    info!("Actions config reloaded");
                                                }
                                            }
                                            Err(e) => error!("Failed to reload actions config: {}", e),
                                        }
                                    }
                                    ConfigChange::HotkeysConfig => {
                                        match IntelliBoard::core::config::load_hotkeys_config() {
                                            Ok(cfg) => {
                                                if let Ok(mut guard) = shared_hotkeys_inner.write() {
                                                    *guard = cfg;
                                                    info!("Hotkeys config reloaded");
                                                }
                                                // Reinstall hotkey system with new config
                                                if let Some(ref hs) = hotkey_system_inner {
                                                    if let Err(e) = hs.reinstall() {
                                                        error!("Failed to reinstall hotkey system: {}", e);
                                                    } else {
                                                        info!("Hotkey system reinstalled with updated config");
                                                    }
                                                }
                                            }
                                            Err(e) => error!("Failed to reload hotkeys config: {}", e),
                                        }
                                    }
                                    _ => debug!("Ignoring config change: {:?}", change),
                                }
                            }
                        }
                    }
                }
            }
        });
    });

    // Start UI
    //
    // Topmost policy: we do NOT enable global always-on-top here. The viewport
    // is reused for three different surfaces (status bar, result window,
    // selection toolbar) and they have different Z-ordering needs:
    //   - status bar / toolbar : AlwaysOnTop (must overlay whatever the user
    //                            is doing until the result arrives).
    //   - result window        : Normal level, brought forward with Focus()
    //                            so it appears above the focused window at the
    //                            moment of completion but lets the user click
    //                            other apps above it afterwards.
    //
    // WindowLevel is dispatched each frame from `update()` via
    // `ViewportCommand::WindowLevel`.
    let viewport = eframe::egui::ViewportBuilder::default()
        .with_visible(false) // Start hidden
        .with_taskbar(false)
        .with_decorations(false)
        .with_transparent(true);

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let pm_for_cleanup = process_manager.clone();
    let result = eframe::run_native(
        "IntelliBoard",
        options,
        Box::new(move |cc| {
            let app = MyApp::new(cc, ui_rx, tx.clone(), gui_ctx, tray_handler, process_manager, shared_actions.clone());
            Box::new(app)
        }),
    );
    
    // Clean up child processes on exit
    info!("Main app exiting, killing child processes...");
    pm_for_cleanup.kill_all();
    
    result.map_err(|e| anyhow::anyhow!("Eframe error: {}", e))
}
