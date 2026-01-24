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
use tokio::sync::mpsc;
use log::{info, error, debug};
use flexi_logger::{Logger, FileSpec, Criterion, Naming, Cleanup};
use clap::Parser;
use single_instance::SingleInstance;
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem, CheckMenuItem, PredefinedMenuItem}};

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

fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

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

    // Setup Tray
    let tray_menu = Menu::new();
    let show_log_i = MenuItem::new("Show Log", true, None);
    let show_log_id = show_log_i.id().clone();
    let hotkey_config_i = MenuItem::new("Configuration", true, None);
    let hotkey_config_id = hotkey_config_i.id().clone();
    let enable_i = CheckMenuItem::new("Enable Processing", true, true, None);
    let enable_id = enable_i.id().clone();
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
    
    // Use Mutex to allow mutation of LlmClient if we weren't using Arc. 
    // Since we are using Arc, we can't easily mutate.
    // However, we want to inject UI TX into LLM client for streaming.
    // Let's modify LlmClient to be capable of receiving the UI TX.
    
    // We need to pass the tx to the client. But the tx is created here.
    // We'll wrap LlmClient in Arc<Mutex<>> or just configure it before wrapping in Arc if possible.
    // Actually, simpler: construct client with ui_tx option.
    let mut raw_llm_client = LlmClient::new(actions_config.clone());
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
            let ipc_server = GraphIpcServer::new(memory_store.clone(), 12345);
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
                                // Check IME composition state (Windows-specific)
                                #[cfg(windows)]
                                let ime_composing = IntelliBoard::platform::is_ime_composing();
                                #[cfg(not(windows))]
                                let ime_composing = false;
                                
                                // Apply time+content debouncing for IME compatibility
                                if let Some(wait_ms) = clipboard.should_debounce(&text, ime_composing) {
                                    if std::env::var_os("IntelliBoard_DIAG_CLIPBOARD").is_some() {
                                        log::debug!("[diag] debouncing clipboard for {}ms (ime={})", wait_ms, ime_composing);
                                    }
                                    // Schedule a delayed re-check by sleeping briefly and continuing
                                    // The next clipboard event or timeout will re-trigger processing
                                    tokio::time::sleep(tokio::time::Duration::from_millis(wait_ms.min(50))).await;
                                    continue;
                                }
                                
                                // Content is stable - process it
                                clipboard.mark_processed(&text);
                                
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
                             AppEvent::Cancel => {
                                info!("Cancel requested - stopping in-flight request");
                                cancel_flag_inner.store(true, std::sync::atomic::Ordering::SeqCst);
                            },
                            AppEvent::ToggleProcessing(enabled) => {
                                // processor.set_enabled(enabled); // We removed processor
                                info!("Processing enabled: {}", enabled);
                            },
                            AppEvent::ShowMemoryGraph => {
                                let _ = ui_tx_for_events.send(UiEvent::ShowMemoryGraph);
                            }
                            AppEvent::ShowHotkeyConfig => {
                                let _ = ui_tx_for_events.send(UiEvent::ShowHotkeyConfig);
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
    let viewport = {
        let builder = eframe::egui::ViewportBuilder::default()
            .with_visible(false) // Start hidden
            .with_taskbar(false)
            .with_decorations(false)
            .with_transparent(true);

        if std::env::var_os("IntelliBoard_NO_TOPMOST").is_some() {
            builder
        } else {
            builder.with_always_on_top()
        }
    };

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let pm_for_cleanup = process_manager.clone();
    let result = eframe::run_native(
        "IntelliBoard",
        options,
        Box::new(move |cc| {
            let app = MyApp::new(cc, ui_rx, tx.clone(), gui_ctx, tray_handler, process_manager);
            Box::new(app)
        }),
    );
    
    // Clean up child processes on exit
    info!("Main app exiting, killing child processes...");
    pm_for_cleanup.kill_all();
    
    result.map_err(|e| anyhow::anyhow!("Eframe error: {}", e))
}
