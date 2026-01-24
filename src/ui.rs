pub mod memory_graph;
pub mod hotkey_config;
pub mod theme;

use eframe::egui;
use std::sync::{Arc, Mutex};
use log::info;
use std::time::Instant;
use crate::core::events::AppEvent;
use tokio::sync::mpsc::Sender;
use tray_icon::menu::{MenuEvent, CheckMenuItem};
use tray_icon::TrayIconEvent;

pub struct TrayHandler {
    pub icon: tray_icon::TrayIcon,
    pub enable_item: CheckMenuItem,
    pub enable_id: tray_icon::menu::MenuId,
    pub exit_id: tray_icon::menu::MenuId,
    pub show_log_id: tray_icon::menu::MenuId,
    pub hotkey_config_id: tray_icon::menu::MenuId,
    pub tx: Sender<AppEvent>,
    pub custom_commands: std::collections::HashMap<tray_icon::menu::MenuId, String>,
}

pub enum UiEvent {
    /// Processing started with action label for display (e.g., "Translation", "Image OCR")
    ProcessingStarted(String),
    ShowResult(String, String), // original, result
    StreamUpdate(String), // chunk
    StreamEnd(bool), // true = success, false = incomplete
    StreamError(String), // error message
    ShowMemoryGraph,
    ShowHotkeyConfig,
    Quit,
}

#[derive(PartialEq)]
enum AppState {
    Idle,
    Waiting,
    Streaming,
    Finished,
    Error,
    Incomplete,
}

pub struct MyApp {
    rx: flume::Receiver<UiEvent>,
    app_tx: Sender<AppEvent>,
    text: String,
    displayed_text: String,
    original_text: String,
    visible: bool,
    needs_resize: bool,
    state: AppState,
    tray_handler: Arc<Mutex<TrayHandler>>,
    last_type_time: Instant,
    process_manager: Arc<crate::core::process_manager::ProcessManager>,
    /// Current action label for processing indicator
    current_action_label: String,
}

impl MyApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rx: flume::Receiver<UiEvent>,
        app_tx: Sender<AppEvent>,
        ctx_holder: Arc<Mutex<Option<egui::Context>>>,
        tray_handler: Arc<Mutex<TrayHandler>>,
        process_manager: Arc<crate::core::process_manager::ProcessManager>,
    ) -> Self {
        info!("MyApp initialized");
        *ctx_holder.lock().unwrap() = Some(cc.egui_ctx.clone());
        
        theme::configure_fonts(&cc.egui_ctx);
        theme::apply_theme(&cc.egui_ctx);

        Self {
            rx,
            app_tx,
            text: String::new(),
            displayed_text: String::new(),
            original_text: String::new(),
            visible: false,
            needs_resize: false,
            state: AppState::Idle,
            tray_handler,
            last_type_time: Instant::now(),
            process_manager,
            current_action_label: String::new(),
        }
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll Tray Events
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            let handler = self.tray_handler.lock().unwrap();
            if event.id == handler.exit_id {
                info!("Tray Exit clicked");
                // Kill all child processes before exiting
                self.process_manager.kill_all();
                std::process::exit(0);
            } else if event.id == handler.enable_id {
                let enabled = handler.enable_item.is_checked();
                info!("Tray Enable toggled: {}", enabled);
                
                // Update Icon
                let new_icon = if enabled {
                    crate::load_tray_icon_active()
                } else {
                    crate::load_tray_icon_inactive()
                };
                let _ = handler.icon.set_icon(Some(new_icon));

                let _ = handler.tx.try_send(AppEvent::ToggleProcessing(enabled));
            } else if event.id == handler.show_log_id {
                info!("Tray Show Log clicked");
                let log_dir = std::path::Path::new("logs");
                if let Ok(entries) = std::fs::read_dir(log_dir) {
                    let mut logs: Vec<_> = entries
                        .filter_map(|e| e.ok())
                        .filter(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            name.starts_with("IntelliBoard") && name.ends_with(".log")
                        })
                        .collect();
                    
                    logs.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(std::time::SystemTime::UNIX_EPOCH));

                    if let Some(latest) = logs.last() {
                        let path = latest.path();
                        #[cfg(target_os = "windows")]
                        let _ = std::process::Command::new("notepad").arg(path).spawn();
                    }
                }
            } else if event.id == handler.hotkey_config_id {
                info!("Tray Configure Hotkeys clicked");
                let _ = handler.tx.try_send(AppEvent::ShowHotkeyConfig);
            } else if let Some(cmd) = handler.custom_commands.get(&event.id) {
                info!("Executing custom command: {}", cmd);
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("wt.exe")
                    .args(&["-p", "Windows PowerShell", "-d", ".", "powershell", "-Command", cmd])
                    .spawn();
            }
        }

        // Handle tray icon left-click (opens Memory Graph)
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match event {
                TrayIconEvent::Click {
                    button: tray_icon::MouseButton::Left,
                    button_state: tray_icon::MouseButtonState::Up,
                    ..
                } => {
                    info!("Tray left-click: opening Memory Graph");
                    let _ = self.app_tx.try_send(AppEvent::ShowMemoryGraph);
                    ctx.request_repaint();
                }
                _ => {}
            }
        }

        // Check for new events
        while let Ok(event) = self.rx.try_recv() {
            match event {
                UiEvent::ProcessingStarted(action_label) => {
                    info!("UI received ProcessingStarted event: {}", action_label);
                    self.text.clear();
                    self.displayed_text.clear();
                    self.current_action_label = action_label;
                    self.visible = true;
                    self.needs_resize = true;
                    self.state = AppState::Waiting;

                    if std::env::var_os("IntelliBoard_DIAG_UI").is_some() {
                        info!("[diag] UI show: ProcessingStarted");
                    }
                    
                    // Don't position/focus here - handled in processing bar section
                    ctx.request_repaint();
                }
                UiEvent::ShowResult(original, content) => {
                    info!("UI received ShowResult. Length: {}", content.len());
                    self.original_text = original;
                    
                    // If we were streaming, text is already populated via StreamUpdate
                    // Just let the typewriter continue - don't change state or text
                    if self.state != AppState::Streaming {
                        // No streaming happened (very fast or non-streaming response)
                        self.text = content.clone();
                        self.displayed_text = content;
                        self.state = AppState::Finished;
                    }
                    // If streaming, stay in Streaming state - typewriter will transition to Finished
                    
                    self.visible = true;
                    self.needs_resize = true;

                    if std::env::var_os("IntelliBoard_DIAG_UI").is_some() {
                        info!("[diag] UI show: ShowResult");
                    }
                    
                    // Move window to right side, lower position
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([1350.0, 250.0].into()));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([500.0, 700.0].into()));
                    if std::env::var_os("IntelliBoard_NO_UI_FOCUS").is_none() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    } else if std::env::var_os("IntelliBoard_DIAG_UI").is_some() {
                        info!("[diag] UI focus suppressed by IntelliBoard_NO_UI_FOCUS");
                    }
                    ctx.request_repaint();
                }
                UiEvent::StreamUpdate(chunk) => {
                    info!("UI StreamUpdate: +{} bytes, total text={}, displayed={}", 
                          chunk.len(), self.text.len() + chunk.len(), self.displayed_text.len());
                    self.text.push_str(&chunk);
                    // Don't update displayed_text here, let the typewriter effect handle it
                    
                    // If we were waiting, switch to streaming and show result window
                    if self.state == AppState::Waiting {
                        info!("Transitioning Waiting -> Streaming");
                        self.state = AppState::Streaming;
                        self.needs_resize = true;
                        
                        // Move to result window position/size
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([1350.0, 250.0].into()));
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([500.0, 700.0].into()));
                    }
                    ctx.request_repaint();
                }
                UiEvent::StreamEnd(success) => {
                    info!("UI StreamEnd: success={}, text={}, displayed={}", 
                          success, self.text.len(), self.displayed_text.len());
                    self.state = if success { AppState::Finished } else { AppState::Incomplete };
                    // Ensure all text is shown at the end
                    self.displayed_text = self.text.clone();
                    self.needs_resize = true;
                    ctx.request_repaint();
                }
                UiEvent::StreamError(msg) => {
                    self.text.push_str(&format!("\n[Error: {}]", msg));
                    self.displayed_text = self.text.clone(); // Show error immediately
                    self.state = AppState::Error;
                    self.needs_resize = true;
                    ctx.request_repaint();
                }
// use crate::core::ipc_server; // Removed misplaced import

// ...

                UiEvent::ShowMemoryGraph => {
                    info!("Launching Memory Graph UI process...");
                    
                    #[cfg(feature = "debug-mode")]
                    let port = 12345;
                    #[cfg(not(feature = "debug-mode"))]
                    let port = 12345; // TODO: Configurable port?
                    
                    match crate::core::process_manager::build_memory_graph_command(port) {
                        Ok(cmd) => {
                            if let Err(e) = self.process_manager.spawn_or_focus(
                                "memory_graph",
                                "IntelliBoard Memory Graph",
                                cmd
                            ) {
                                log::error!("Failed to spawn/focus Memory Graph: {}", e);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to build Memory Graph command: {}", e);
                        }
                    }
                }
                UiEvent::ShowHotkeyConfig => {
                    info!("Spawning Functions Configuration window...");
                    match crate::core::process_manager::build_functions_config_command() {
                        Ok(cmd) => {
                            if let Err(e) = self.process_manager.spawn_or_focus(
                                "functions_config",
                                "IntelliBoard Functions",
                                cmd
                            ) {
                                log::error!("Failed to spawn/focus Functions Config: {}", e);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to build Functions Config command: {}", e);
                        }
                    }
                }
                UiEvent::Quit => {
                    info!("UI received Quit event. Exiting...");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // Typewriter Effect Logic - fast streaming display
        // Goal: Display new content quickly while maintaining smooth animation feel
        if self.state == AppState::Streaming && self.displayed_text.len() < self.text.len() {
            let backlog = self.text.len() - self.displayed_text.len();
            
            // Always make progress - show at least 1 char per frame
            // Scale up when backlog is large to catch up quickly
            let chars_to_add = if backlog > 100 {
                backlog // Show everything immediately - way behind
            } else if backlog > 20 {
                10.max(backlog / 3) // Fast catch-up
            } else {
                3.max(backlog) // Always show at least the backlog or 3 chars
            };
            
            // Add characters, respecting UTF-8 boundaries
            let remaining = &self.text[self.displayed_text.len()..];
            let mut added = 0;
            for c in remaining.chars().take(chars_to_add) {
                self.displayed_text.push(c);
                added += 1;
            }
            
            if added > 0 {
                self.needs_resize = true;
            }
            
            // Request repaint immediately to keep animation smooth
            ctx.request_repaint();
        } else if self.state == AppState::Streaming && self.displayed_text.len() >= self.text.len() && !self.text.is_empty() {
            // Typewriter caught up and we have content - stay in streaming until StreamEnd
            // Just keep requesting repaints to be ready for more content
            ctx.request_repaint();
        }

        // Ensure animation runs during Waiting state and we poll for new events during Streaming
        if self.state == AppState::Waiting || self.state == AppState::Streaming {
             ctx.request_repaint();
        }

        // During WAITING ONLY, show small indicator bar at bottom
        // Once streaming starts, switch to the full result window
        let is_waiting = self.state == AppState::Waiting;
        
        if is_waiting {
            // Show small processing indicator bar at bottom-center of screen
            // Size: ~280x50 pixels, always on top
            // Use monitor info for positioning - assume 1920x1080 as fallback
            let bar_width = 280.0;
            let bar_height = 50.0;
            
            // Get primary monitor size (fallback to common resolution)
            let (screen_width, screen_height) = ctx.input(|i| {
                i.viewport().monitor_size.map(|s| (s.x, s.y)).unwrap_or((1920.0, 1080.0))
            });
            
            let x_pos = (screen_width - bar_width) / 2.0;
            let y_pos = screen_height - bar_height - 60.0; // ~2cm (60px) from bottom
            
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([x_pos, y_pos].into()));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([bar_width, bar_height].into()));
            
            // Processing bar frame
            let frame = egui::Frame::default()
                .fill(egui::Color32::from_rgb(10, 10, 16))
                .rounding(4.0)
                .stroke(egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 243, 255)))
                .inner_margin(8.0);
            
            egui::CentralPanel::default()
                .frame(frame)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        // Animated spinner/progress indicator
                        let time = ctx.input(|i| i.time);
                        let phase = (time * 2.0 % 1.0) as f32;
                        let dots = match ((phase * 4.0) as usize) % 4 {
                            0 => "   ",
                            1 => ".  ",
                            2 => ".. ",
                            _ => "...",
                        };
                        
                        // Action label
                        let label_text = if self.current_action_label.is_empty() {
                            "Processing".to_string()
                        } else {
                            self.current_action_label.clone()
                        };
                        
                        ui.label(
                            egui::RichText::new(format!("{}{}", label_text, dots))
                                .color(egui::Color32::from_rgb(0, 243, 255))
                                .size(14.0)
                                .monospace()
                        );
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Stop button
                            if ui.button(
                                egui::RichText::new("✕")
                                    .color(egui::Color32::from_rgb(255, 100, 100))
                                    .size(14.0)
                            ).clicked() {
                                info!("User clicked stop button");
                                let _ = self.app_tx.try_send(AppEvent::Cancel);
                                self.visible = false;
                                self.state = AppState::Idle;
                            }
                        });
                    });
                });
            
            ctx.request_repaint();
            return;
        }

        if !self.visible {
            // Move off-screen and keep loop alive
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([10000.0, 10000.0].into()));
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |_ui| {});
            return;
        }

        // NOTE: Focus-based auto-close removed - window stays open until user explicitly closes it
        // or a new action is triggered (which cancels the old one and shows new UI)
        
        // Result window - show full size popup for Finished/Error/Incomplete
        // Move window to right side, proper size
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([1350.0, 250.0].into()));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([500.0, 700.0].into()));
        
        // Custom Frame for Japan 2046 look
        let frame = egui::Frame::default()
            .fill(egui::Color32::from_rgb(10, 10, 16)) // Deep Night
            .rounding(0.0) // Sharp corners for tech look
            .stroke(egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 243, 255))) // Neon Cyan Border
            .inner_margin(20.0);

        egui::CentralPanel::default()
            .frame(frame)
            .show(ctx, |ui| {
                let available_height = ui.available_height();
                let input_height = available_height * 0.33;
                let output_height = available_height * 0.66;

                // Input Area (Top 1/3)
                ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), input_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.horizontal_top(|ui| {
                        let available_width = ui.available_width() - 30.0; // Reserve space for close button
                        
                        ui.vertical(|ui| {
                            ui.set_max_width(available_width);
                            
                            if self.state == AppState::Waiting {
                                // Hide input text during processing, show placeholder
                                ui.label(
                                    egui::RichText::new("INPUT LOCKED // 入力ロック中")
                                        .color(egui::Color32::from_rgb(100, 100, 120))
                                        .monospace()
                                );
                            } else {
                                egui::ScrollArea::vertical()
                                    .id_source("input_scroll")
                                    .max_height((input_height - 30.0).max(10.0)) // Reserve space for header/buttons, ensure non-negative
                                    .show(ui, |ui| {
                                        let response = ui.add(
                                            egui::TextEdit::multiline(&mut self.original_text)
                                                .desired_width(available_width)
                                                .font(egui::FontId::proportional(14.0))
                                                .text_color(egui::Color32::from_rgb(220, 230, 255)) // Cool White
                                                .frame(false) // Minimalist look
                                        );

                                        // Handle Enter to submit (Shift+Enter for newline)
                                        if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) {
                                            // Remove the newline that might have been added
                                            if self.original_text.ends_with('\n') {
                                                self.original_text.pop();
                                            }
                                            
                                            info!("User submitted query: {}", self.original_text);
                                            self.text.clear(); // Clear previous result
                                            self.displayed_text.clear();
                                            self.state = AppState::Waiting;
                                            
                                            // Send event to backend
                                            let _ = self.app_tx.try_send(AppEvent::UserQuery(self.original_text.clone()));
                                        }
                                    });
                            }
                        });

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
                            if ui.button(egui::RichText::new("❌").color(egui::Color32::from_rgb(255, 0, 60))).clicked() {
                                info!("User clicked close button");
                                let _ = self.app_tx.try_send(AppEvent::Cancel);
                                self.visible = false;
                                self.state = AppState::Idle;
                            }

                            // Status Indicator
                            let status_size = 14.0;
                            match self.state {
                                AppState::Waiting => {
                                    ui.label(egui::RichText::new("待機").size(status_size).color(egui::Color32::from_rgb(0, 243, 255))); 
                                }
                                AppState::Streaming => {
                                    ui.label(egui::RichText::new("受信").size(status_size).color(egui::Color32::from_rgb(0, 243, 255)));
                                }
                                AppState::Finished => {
                                    ui.label(egui::RichText::new("终章").size(status_size).color(egui::Color32::from_rgb(0, 255, 65))); // Matrix Green
                                }
                                AppState::Incomplete => {
                                    ui.label(egui::RichText::new("中断").size(status_size).color(egui::Color32::from_rgb(255, 200, 0))); // Gold
                                }
                                AppState::Error => {
                                    ui.label(egui::RichText::new("エラー").size(status_size).color(egui::Color32::from_rgb(255, 0, 60))); // Red
                                }
                                AppState::Idle => {}
                            }
                        });
                    });
                });
            
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                // Output Area (Remaining 2/3)
                ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), output_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                    egui::ScrollArea::vertical()
                        .id_source("output_scroll")
                        .show(ui, |ui| {
                            if self.state == AppState::Waiting {
                                // Request repaint for animation
                                ctx.request_repaint();
                                ui.vertical_centered(|ui| {
                                    ui.add_space(10.0);
                                    // Cyberpunk loading text
                                    ui.label(egui::RichText::new("SYSTEM PROCESSING // 解析中").monospace().color(egui::Color32::from_rgb(0, 243, 255))); 
                                    ui.add_space(5.0);
                                    
                                    // Indeterminate progress bar
                                    let time = ctx.input(|i| i.time);
                                    let progress = (time * 1.5 % 1.0) as f32;
                                    
                                    let bar_height = 20.0; // Fixed height ~0.7cm
                                    let width = ui.available_width() * 0.9;
                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, bar_height), egui::Sense::hover());
                                    
                                    // Background line
                                    ui.painter().rect_filled(rect, 0.0, egui::Color32::from_rgb(30, 30, 40));
                                    
                                    // Moving scanner line
                                    let bar_width = width * 0.3;
                                    let x_pos = rect.left() + (width - bar_width) * progress;
                                    let bar_rect = egui::Rect::from_min_size(
                                        egui::pos2(x_pos, rect.top()),
                                        egui::vec2(bar_width, bar_height)
                                    );
                                    
                                    ui.painter().rect_filled(bar_rect, 0.0, egui::Color32::from_rgb(255, 0, 60));
                                    ui.add_space(10.0);
                                });
                            } else {
                                ui.label(
                                    egui::RichText::new(&self.displayed_text)
                                        .color(egui::Color32::from_rgb(220, 230, 255)) // Cool White
                                        .size(14.0)
                                );
                            }
                        });
                });
        

            
                // Footer (optional, maybe just space)
                ui.add_space(5.0);
            });

        // Close main UI on Escape (Memory Graph escape is handled earlier)
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            info!("User pressed Escape - closing window");
            let _ = self.app_tx.try_send(AppEvent::Cancel);
            self.visible = false;
            self.state = AppState::Idle;
        }
    }
}
