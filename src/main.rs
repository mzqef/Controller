#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod api;
mod ui;

use crate::core::clipboard::ClipboardManager;
use crate::core::config::{LlmConfig, CommandsConfig};
use crate::core::events::AppEvent;
use crate::core::actions::{Action, ActionHandler};
use crate::core::clipboard_listener;
use crate::core::graph_server;
use crate::core::memory::MemoryEvent;
use crate::core::memory_store::MemoryStore;
use crate::api::client::LlmClient;
use crate::ui::{MyApp, UiEvent, TrayHandler};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use log::{info, error, debug, warn};
use flexi_logger::{Logger, FileSpec, Criterion, Naming, Cleanup};
use clap::Parser;
use single_instance::SingleInstance;
use rdev::{EventType, Key};
use tray_icon::{TrayIconBuilder, menu::{Menu, MenuItem, CheckMenuItem, PredefinedMenuItem}};

#[derive(Parser, Debug)]
#[command(name = "controller")]
struct Opt {
    #[arg(short, long)]
    config: Option<String>,

    #[arg(long)]
    stop: bool,

    #[arg(long)]
    log: bool,
}

pub fn load_icon(r: u8, g: u8, b: u8) -> tray_icon::Icon {
    let width = 32;
    let height = 32;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..height {
        for _ in 0..width {
            rgba.extend_from_slice(&[r, g, b, 255]);
        }
    }
    tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to create icon")
}

fn main() -> anyhow::Result<()> {
    let opt = Opt::parse();

    let instance = SingleInstance::new("controller_app").map_err(|e| anyhow::anyhow!("Failed to check single instance: {}", e))?;
    if !instance.is_single() {
        if opt.log {
            // Log checking logic omitted for brevity, assuming user knows how to check logs
            println!("Controller is already running.");
            return Ok(());
        } else {
            eprintln!("Controller is already running.");
            return Ok(());
        }
    }

    Logger::try_with_str("info")?
        .log_to_file(FileSpec::default().directory("logs").basename("controller"))
        .write_mode(flexi_logger::WriteMode::Direct)
        .format(flexi_logger::opt_format)
        .rotate(Criterion::Size(10 * 1024 * 1024), Naming::Timestamps, Cleanup::KeepLogFiles(3))
        .start()?;

    std::panic::set_hook(Box::new(|info| {
        error!("Panic occurred: {:?}", info);
    }));

    info!("Starting Controller...");
    
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
    
    // Load LLM and commands configuration using overlay loader (repo defaults + XDG overrides)
    let llm_config: LlmConfig = match crate::core::config::load_llm_config() {
        Ok(cfg) => cfg,
        Err(e) => return Err(anyhow::anyhow!("Failed to load llm config: {}", e)),
    };

    let cmd_config: Option<CommandsConfig> = match crate::core::config::load_commands_config() {
        Ok(cfg) => cfg,
        Err(e) => {
            log::warn!("Failed to load commands config: {}", e);
            None
        }
    };

    // Channels
    let (ui_tx, ui_rx) = std::sync::mpsc::channel::<UiEvent>();
    let (tx, mut rx) = mpsc::channel::<AppEvent>(100);
    let (graph_tx, graph_rx) = mpsc::channel::<MemoryEvent>(256);

    // Setup Tray
    let tray_menu = Menu::new();
    let show_log_i = MenuItem::new("Show Log", true, None);
    let show_log_id = show_log_i.id().clone();
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
    tray_menu.append(&enable_i).unwrap();
    tray_menu.append(&exit_i).unwrap();

    let tray_icon = TrayIconBuilder::new()
        .with_menu(Box::new(tray_menu))
        .with_menu_on_left_click(false)
        .with_tooltip("Controller - Left-click for Memory Graph")
        .with_icon(load_icon(60, 24, 22))
        .build()?;

    let tray_handler = Arc::new(Mutex::new(TrayHandler {
        icon: tray_icon,
        enable_item: enable_i,
        enable_id,
        show_log_id,
        exit_id,
        tx: tx.clone(),
        custom_commands: custom_commands_map,
    }));

     let gui_ctx = Arc::new(Mutex::new(None::<eframe::egui::Context>));

    // Core Components
    let clipboard = Arc::new(ClipboardManager::new().expect("Failed to init clipboard"));
    
    // Memory Store
    let memory_store = Arc::new(MemoryStore::new().expect("Failed to init memory store"));
    
    // Use Mutex to allow mutation of LlmClient if we weren't using Arc. 
    // Since we are using Arc, we can't easily mutate.
    // However, we want to inject UI TX into LLM client for streaming.
    // Let's modify LlmClient to be capable of receiving the UI TX.
    
    // We need to pass the tx to the client. But the tx is created here.
    // We'll wrap LlmClient in Arc<Mutex<>> or just configure it before wrapping in Arc if possible.
    // Actually, simpler: construct client with ui_tx option.
    let mut raw_llm_client = LlmClient::new(llm_config.clone());
    // We need to convert std::sync::mpsc to tokio::sync::mpsc if client wants that?
    // Client wants tokio::sync::mpsc for async. But ui_tx is std::sync::mpsc::channel from main.
    // wait, UI rx is std::mpsc. egui needs std/winit event loop compatibility usually.
    // But async tasks want tokio channel.
    // We need a bridge or just use tokio channel for UI events if possible, OR keep std channel and have blocking send?
    // Blocking send in async code is bad.
    // Solution: Create a specific tokio channel for streaming updates, and a bridge task.
    let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel::<UiEvent>(100);
    raw_llm_client.set_ui_tx(stream_tx);
    
    let llm_client = Arc::new(raw_llm_client);
    
    // Bridge Streaming Events to UI
    let bridge_tx = ui_tx.clone();
    // UI event sender for non-streaming UI commands (e.g., show Memory Graph)
    let ui_tx_for_events = ui_tx.clone();
    
    // Clone memory_store for the eframe closure (before it moves into tokio loop)
    let memory_store_for_ui = memory_store.clone();
    
    // Health check must be spawned inside a runtime, so we defer it to the runtime block below

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
    let last_copy_time_input = last_copy_time.clone();

    // Diagnostics (used to correlate rare/IME-related issues)
    let diag_start = Instant::now();
    let diag_seq = Arc::new(AtomicU64::new(0));

    // Key Input Listener
    let tx_input = tx.clone();
    let diag_seq_input = diag_seq.clone();
    let diag_start_input = diag_start;
    std::thread::spawn(move || {
        let mut ctrl_pressed = false;
        
        let _ = rdev::listen(move |event| {
            let diag_enabled = std::env::var_os("CONTROLLER_DIAG_KEYS").is_some();
            let now_ms = diag_start_input.elapsed().as_millis();
            let seq = diag_seq_input.fetch_add(1, Ordering::Relaxed) + 1;

            if diag_enabled {
                debug!(
                    "[diag #{seq} @ {now_ms}ms] rdev event: type={:?} name={:?} time={:?}",
                    event.event_type,
                    event.name,
                    event.time
                );
            }

            match event.event_type {
                EventType::KeyPress(Key::ControlLeft) | EventType::KeyPress(Key::ControlRight) => ctrl_pressed = true,
                EventType::KeyRelease(Key::ControlLeft) | EventType::KeyRelease(Key::ControlRight) => ctrl_pressed = false,
                EventType::KeyPress(key) if ctrl_pressed => {
                    // Heuristic: on Windows, IME composition may surface as an unknown key.
                    // If this matches VK_PROCESSKEY (0xE5/229), avoid triggering hotkeys during composition.
                    #[cfg(target_os = "windows")]
                    if let Key::Unknown(raw) = key {
                        const VK_PROCESSKEY: u32 = 0xE5;
                        if raw == VK_PROCESSKEY {
                            if diag_enabled {
                                debug!("[diag #{seq} @ {now_ms}ms] IME composition suspected (Key::Unknown(VK_PROCESSKEY)); skipping hotkeys");
                            }
                            return;
                        }
                    }

                    // Check if copy happened recently (e.g. < 2s)
                    let check_elapsed = {
                        let lock_start = Instant::now();
                        match last_copy_time_input.lock() {
                            Ok(guard) => {
                                let lock_dur = lock_start.elapsed();
                                if diag_enabled && lock_dur > Duration::from_millis(1) {
                                    warn!("[diag #{seq} @ {now_ms}ms] last_copy_time lock took {:?}", lock_dur);
                                }
                                guard.elapsed() < Duration::from_millis(2000)
                            }
                            Err(_) => false,
                        }
                    };

                    if diag_enabled {
                        debug!("[diag #{seq} @ {now_ms}ms] ctrl+{:?} check_elapsed={}", key, check_elapsed);
                    }

                    if check_elapsed {
                        match key {
                            Key::KeyR => { let _ = tx_input.try_send(AppEvent::TriggerAction(Action::Format)); },
                            Key::KeyT => { let _ = tx_input.try_send(AppEvent::TriggerAction(Action::TranslateE2C)); },
                            Key::KeyY => { let _ = tx_input.try_send(AppEvent::TriggerAction(Action::TranslateC2E)); },
                            Key::KeyE => { let _ = tx_input.try_send(AppEvent::TriggerAction(Action::Explain)); },
                            _ => {}
                        }
                    }
                }
                _ => {}
            }
        });
    });

    // Main Tokio Loop
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async move {
            info!("Event loop started");
            llm_client.spawn_health_check();

            // Graph server task (receives MemoryEvent messages and updates MemoryStore)
            tokio::spawn(graph_server::run(memory_store.clone(), graph_rx));
            
            // Bridge task for streaming
            tokio::spawn(async move {
                while let Some(event) = stream_rx.recv().await {
                   let _ = bridge_tx.send(event);
                }
            });

            loop {
                tokio::select! {
                    _ = cb_rx.recv() => {
                        // Store clipboard content to memory (skip programmatic writes)
                        if let Ok(text) = clipboard.get_text() {
                            if clipboard.should_ignore_clipboard_text(&text) {
                                if std::env::var_os("CONTROLLER_DIAG_CLIPBOARD").is_some() {
                                    log::debug!("[diag] ignored programmatic clipboard write");
                                }
                            } else {
                                // Update timestamp (used for hotkey gating)
                                if let Ok(mut guard) = last_copy_time.lock() {
                                    *guard = std::time::Instant::now();
                                }
                                let _ = graph_tx.try_send(MemoryEvent::AddClipboard(text));
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
                                let handler = action_handler.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handler.handle(action).await {
                                        error!("Action failed: {}", e);
                                    }
                                });
                            },
                            AppEvent::UserQuery(query) => {
                                info!("Processing user query: {}", query);
                                let handler = action_handler.clone();
                                tokio::spawn(async move {
                                    if let Err(e) = handler.handle(Action::UserQuery(query)).await {
                                        error!("User query failed: {}", e);
                                    }
                                });
                            },
                             AppEvent::Cancel => {
                                // Simple cancel logic would need task tracking, omitting for simplicity or implementing basic cancellation
                                info!("Cancel requested");
                            },
                            AppEvent::ToggleProcessing(enabled) => {
                                // processor.set_enabled(enabled); // We removed processor
                                info!("Processing enabled: {}", enabled);
                            },
                            AppEvent::ShowMemoryGraph => {
                                let _ = ui_tx_for_events.send(UiEvent::ShowMemoryGraph);
                            }
                            _ => {}
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

        if std::env::var_os("CONTROLLER_NO_TOPMOST").is_some() {
            builder
        } else {
            builder.with_always_on_top()
        }
    };

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        "Controller",
        options,
        Box::new(move |cc| {
            let app = MyApp::new(cc, ui_rx, tx.clone(), gui_ctx, tray_handler, memory_store_for_ui);
            Box::new(app)
        }),
    ).map_err(|e| anyhow::anyhow!("Eframe error: {}", e))
}
