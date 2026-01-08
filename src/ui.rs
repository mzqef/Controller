use eframe::egui;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use log::info;
use std::time::{Duration, Instant};
use crate::core::events::AppEvent;
use tokio::sync::mpsc::Sender;
use tray_icon::menu::{MenuEvent, CheckMenuItem};

pub struct TrayHandler {
    pub icon: tray_icon::TrayIcon,
    pub enable_item: CheckMenuItem,
    pub enable_id: tray_icon::menu::MenuId,
    pub exit_id: tray_icon::menu::MenuId,
    pub show_log_id: tray_icon::menu::MenuId,
    pub tx: Sender<AppEvent>,
    pub custom_commands: std::collections::HashMap<tray_icon::menu::MenuId, String>,
}

#[allow(dead_code)]
pub enum UiEvent {
    ProcessingStarted,
    CopyPressed,
    SetProcessingEnabled(bool),
    ShowResult(String, String), // original, result
    StreamUpdate(String), // chunk
    StreamEnd(bool), // true = success, false = incomplete
    StreamError(String), // error message
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
    rx: Receiver<UiEvent>,
    app_tx: Sender<AppEvent>,
    text: String, // The full text received so far
    displayed_text: String, // The text currently shown (for typewriter effect)
    original_text: String, // This will be the content of the input box
    visible: bool,
    focus_grace_period: Option<Instant>,
    needs_resize: bool,
    state: AppState,
    tray_handler: Arc<Mutex<TrayHandler>>,
    last_type_time: Instant,
}

impl MyApp {
    pub fn new(cc: &eframe::CreationContext<'_>, rx: Receiver<UiEvent>, app_tx: Sender<AppEvent>, ctx_holder: Arc<Mutex<Option<egui::Context>>>, tray_handler: Arc<Mutex<TrayHandler>>) -> Self {
        info!("MyApp initialized");
        // Share the context
        *ctx_holder.lock().unwrap() = Some(cc.egui_ctx.clone());
        
        // Load fonts
        Self::configure_fonts(&cc.egui_ctx);
        
        // Apply Theme
        Self::apply_japanese_theme(&cc.egui_ctx);

        Self {
            rx,
            app_tx,
            text: String::new(),
            displayed_text: String::new(),
            original_text: String::new(),
            visible: false,
            focus_grace_period: None,
            needs_resize: false,
            state: AppState::Idle,
            tray_handler,
            last_type_time: Instant::now(),
        }
    }

    fn apply_japanese_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();
        
        // Japan 2046 Palette (Cyber-Japonism)
        let bg_color = egui::Color32::from_rgb(10, 10, 16); // Deep Cyber Night
        let panel_color = egui::Color32::from_rgb(15, 15, 20); // Slightly lighter
        let neon_cyan = egui::Color32::from_rgb(0, 243, 255); // Cyber Cyan
        let neon_pink = egui::Color32::from_rgb(255, 0, 60); // Cyber Pink
        let text_color = egui::Color32::from_rgb(220, 230, 255); // Cool White

        visuals.panel_fill = panel_color;
        visuals.window_fill = bg_color;
        visuals.override_text_color = Some(text_color);
        
        // Selection
        visuals.selection.bg_fill = neon_pink;
        visuals.selection.stroke = egui::Stroke::new(1.0, egui::Color32::WHITE);
        
        // Widgets
        visuals.widgets.noninteractive.bg_fill = panel_color;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, neon_cyan); // Neon borders
        
        visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(25, 25, 35);
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(35, 35, 50);
        visuals.widgets.active.bg_fill = neon_cyan;
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::BLACK);
        
        ctx.set_visuals(visuals);
    }

    fn configure_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();
        
        // Cross-platform font search list
        let font_candidates = [
            // Windows
            "c:\\Windows\\Fonts\\msyh.ttc", // Microsoft YaHei
            "c:\\Windows\\Fonts\\simhei.ttf", // SimHei
            "c:\\Windows\\Fonts\\msgothic.ttc", // MS Gothic
            // macOS
            "/System/Library/Fonts/PingFang.ttc",
            "/Library/Fonts/Arial Unicode.ttf",
            // Linux
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ];
        
        for path in font_candidates {
            if let Ok(font_data) = std::fs::read(path) {
                fonts.font_data.insert(
                    "cjk_font".to_owned(),
                    egui::FontData::from_owned(font_data),
                );

                // Put CJK font first (highest priority) for proportional text:
                fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "cjk_font".to_owned());

                // Put CJK font as last fallback for monospace:
                fonts.families.entry(egui::FontFamily::Monospace).or_default().push("cjk_font".to_owned());

                ctx.set_fonts(fonts);
                info!("Loaded CJK font from {}", path);
                return;
            }
        }
        log::warn!("Failed to load any CJK fonts. Characters may not display correctly.");
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll Tray Events
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            let handler = self.tray_handler.lock().unwrap();
            if event.id == handler.exit_id {
                info!("Tray Exit clicked");
                std::process::exit(0);
            } else if event.id == handler.enable_id {
                let enabled = handler.enable_item.is_checked();
                info!("Tray Enable toggled: {}", enabled);
                
                // Update Icon
                let new_icon = if enabled {
                    crate::load_icon(60, 24, 22) // Dark Red (Active)
                } else {
                    crate::load_icon(128, 128, 128) // Grey (Inactive)
                };
                let _ = handler.icon.set_icon(Some(new_icon));
                
                let _ = handler.tx.blocking_send(AppEvent::ToggleProcessing(enabled));
            } else if event.id == handler.show_log_id {
                info!("Tray Show Log clicked");
                let log_dir = std::path::Path::new("logs");
                if let Ok(entries) = std::fs::read_dir(log_dir) {
                    let mut logs: Vec<_> = entries
                        .filter_map(|e| e.ok())
                        .filter(|e| {
                            let name = e.file_name().to_string_lossy().to_string();
                            name.starts_with("controller") && name.ends_with(".log")
                        })
                        .collect();
                    
                    logs.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()).unwrap_or(std::time::SystemTime::UNIX_EPOCH));

                    if let Some(latest) = logs.last() {
                        let path = latest.path();
                        #[cfg(target_os = "windows")]
                        let _ = std::process::Command::new("notepad").arg(path).spawn();
                    }
                }
            } else if let Some(cmd) = handler.custom_commands.get(&event.id) {
                info!("Executing custom command: {}", cmd);
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("wt.exe")
                    .args(&["-p", "Windows PowerShell", "-d", ".", "powershell", "-Command", cmd])
                    .spawn();
            }
        }

        // Check for new events
        while let Ok(event) = self.rx.try_recv() {
            match event {
                UiEvent::ProcessingStarted => {
                    info!("UI received ProcessingStarted event.");
                    self.text.clear();
                    self.displayed_text.clear();
                    self.visible = true;
                    self.focus_grace_period = Some(Instant::now());
                    self.needs_resize = true;
                    self.state = AppState::Waiting;
                    
                    // Move window to a visible position and set fixed size
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([100.0, 100.0].into()));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([500.0, 700.0].into()));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    ctx.request_repaint();
                }
                UiEvent::CopyPressed => {
                    info!("UI received CopyPressed event.");
                    // Briefly show the UI to acknowledge the copy action when processing is disabled
                    self.visible = true;
                    self.focus_grace_period = Some(Instant::now());
                    self.needs_resize = true;
                    ctx.request_repaint();
                }
                UiEvent::SetProcessingEnabled(enabled) => {
                    info!("UI received SetProcessingEnabled: {}", enabled);
                    // Update tray menu check and icon
                    let handler = self.tray_handler.lock().unwrap();
                    // Update check menu item
                    let _ = handler.enable_item.set_checked(enabled);
                    // Update icon
                    let new_icon = if enabled {
                        crate::load_icon(60, 24, 22) // Dark Red (Active)
                    } else {
                        crate::load_icon(128, 128, 128) // Grey (Inactive)
                    };
                    let _ = handler.icon.set_icon(Some(new_icon));
                    ctx.request_repaint();
                }
                UiEvent::ShowResult(original, content) => {
                    info!("UI received content to show. Length: {}", content.len());
                    self.text = content.clone();
                    self.displayed_text = content; // Show immediately for non-streaming
                    self.original_text = original;
                    self.visible = true;
                    self.focus_grace_period = Some(Instant::now());
                    self.needs_resize = true;
                    self.state = AppState::Finished;
                    
                    // Move window to a visible position and set fixed size
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([100.0, 100.0].into()));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([500.0, 700.0].into()));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    ctx.request_repaint();
                }
                UiEvent::StreamUpdate(chunk) => {
                    self.text.push_str(&chunk);
                    // Don't update displayed_text here, let the typewriter effect handle it
                    
                    // If we were waiting, switch to streaming immediately to hide progress bar
                    if self.state == AppState::Waiting {
                         self.state = AppState::Streaming;
                         self.needs_resize = true; // Resize once when starting to stream
                    }
                    ctx.request_repaint();
                }
                UiEvent::StreamEnd(success) => {
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
                UiEvent::Quit => {
                    info!("UI received Quit event. Exiting...");
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // Typewriter Effect Logic
        if self.state == AppState::Streaming && self.displayed_text.len() < self.text.len() {
            let now = Instant::now();
            // Adjust speed: faster if backlog is large
            let backlog = self.text.len() - self.displayed_text.len();
            let speed_ms = if backlog > 50 { 5 } else if backlog > 20 { 10 } else { 20 };
            
            if now.duration_since(self.last_type_time).as_millis() > speed_ms {
                // Add next char(s)
                // Be careful with UTF-8 boundaries
                let remaining = &self.text[self.displayed_text.len()..];
                if let Some(c) = remaining.chars().next() {
                    self.displayed_text.push(c);
                    self.last_type_time = now;
                    self.needs_resize = true; // Resize as text grows
                }
            }
            // Always request repaint if we assume typewriter is active, to keep the loop running
            ctx.request_repaint();
        }

        // Ensure animation runs during Waiting state and we poll for new events during Streaming
        if self.state == AppState::Waiting || self.state == AppState::Streaming {
             ctx.request_repaint();
        }

        if !self.visible {
            // Move off-screen and keep loop alive
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([10000.0, 10000.0].into()));
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |_ui| {});
            return;
        }

        // Check focus status to auto-close
        if self.visible {
            let should_close = if let Some(start) = self.focus_grace_period {
                if start.elapsed() > Duration::from_millis(500) {
                    // Grace period over, check focus
                    !ctx.input(|i| i.focused)
                } else {
                    false // Still in grace period
                }
            } else {
                !ctx.input(|i| i.focused)
            };

            if should_close {
                 self.visible = false;
                 // Cancel processing if window closes/loses focus
                 let _ = self.app_tx.try_send(AppEvent::Cancel);
            }
        }
        
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
                                self.visible = false;
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

        // Close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.visible = false;
        }
    }
}
