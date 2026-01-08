#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod api;
mod ui;

use crate::core::clipboard::ClipboardManager;
use crate::core::config::{LlmConfig, CommandsConfig};
use crate::core::events::AppEvent;
use crate::core::actions::{Action, ActionHandler};
use crate::core::clipboard_listener;
use crate::api::client::LlmClient;
use crate::ui::{MyApp, UiEvent, TrayHandler};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use log::{info, error, debug};
use flexi_logger::{Logger, FileSpec, Criterion, Naming, Cleanup};
use config::Config;
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
        .with_tooltip("Controller")
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
    let ui_tx_clone = ui_tx.clone();
    
    // Bridge Streaming Events to UI
    let bridge_tx = ui_tx.clone();
    
    // Health check must be spawned inside a runtime, so we defer it to the runtime block below

    let action_handler = Arc::new(ActionHandler::new(clipboard.clone(), llm_client.clone(), Some(ui_tx.clone())));

    // Clipboard Listener
    let (cb_tx, mut cb_rx) = mpsc::channel(100);
    clipboard_listener::start_listener(cb_tx);

    let last_copy_time = Arc::new(Mutex::new(std::time::Instant::now()));
    let last_copy_time_input = last_copy_time.clone();

    // Key Input Listener
    let tx_input = tx.clone();
    std::thread::spawn(move || {
        let mut ctrl_pressed = false;
        
        let _ = rdev::listen(move |event| {
            match event.event_type {
                EventType::KeyPress(Key::ControlLeft) | EventType::KeyPress(Key::ControlRight) => ctrl_pressed = true,
                EventType::KeyRelease(Key::ControlLeft) | EventType::KeyRelease(Key::ControlRight) => ctrl_pressed = false,
                EventType::KeyPress(key) if ctrl_pressed => {
                    // Check if copy happened recently (e.g. < 2s)
                    let check_elapsed = {
                        match last_copy_time_input.lock() {
                            Ok(guard) => guard.elapsed() < Duration::from_millis(2000),
                            Err(_) => false,
                        }
                    };

                    if check_elapsed {
                        match key {
                            Key::KeyR => { let _ = tx_input.blocking_send(AppEvent::TriggerAction(Action::Format)); },
                            Key::KeyT => { let _ = tx_input.blocking_send(AppEvent::TriggerAction(Action::TranslateE2C)); },
                            Key::KeyY => { let _ = tx_input.blocking_send(AppEvent::TriggerAction(Action::TranslateC2E)); },
                            Key::KeyE => { let _ = tx_input.blocking_send(AppEvent::TriggerAction(Action::Explain)); },
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
            
            // Bridge task for streaming
            tokio::spawn(async move {
                while let Some(event) = stream_rx.recv().await {
                   let _ = bridge_tx.send(event);
                }
            });

            loop {
                tokio::select! {
                    _ = cb_rx.recv() => {
                        // Update timestamp
                        if let Ok(mut guard) = last_copy_time.lock() {
                            *guard = std::time::Instant::now();
                        }
                        debug!("Clipboard change detected");
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
                            _ => {}
                        }
                    }
                }
            }
        });
    });

    // Start UI
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_visible(false) // Start hidden
            .with_taskbar(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top(),
        ..Default::default()
    };

    eframe::run_native(
        "Controller",
        options,
        Box::new(move |cc| {
            let app = MyApp::new(cc, ui_rx, tx.clone(), gui_ctx, tray_handler);
            Box::new(app)
        }),
    ).map_err(|e| anyhow::anyhow!("Eframe error: {}", e))
}
