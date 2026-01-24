#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Standalone hotkey configuration window.
//! Spawned as a separate process from the tray menu.

use eframe::egui;
use IntelliBoard::core::config::{HotkeysConfig, HotkeyBinding, load_hotkeys_config, save_hotkeys_config};
use std::time::Instant;

fn main() -> eframe::Result<()> {
    let config = load_hotkeys_config().unwrap_or_default();
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([550.0, 450.0])
            .with_position([1350.0, 300.0])
            .with_title("IntelliBoard - Hotkey Configuration")
            .with_resizable(true)
            .with_active(true)
            .with_icon(IntelliBoard::ui::theme::load_egui_icon()),
        ..Default::default()
    };
    
    eframe::run_native(
        "Hotkey Configuration",
        options,
        Box::new(move |cc| {
            IntelliBoard::ui::theme::configure_fonts(&cc.egui_ctx);
            IntelliBoard::ui::theme::apply_theme(&cc.egui_ctx);
            Box::new(HotkeyConfigApp::new(config))
        }),
    )
}

struct HotkeyConfigApp {
    config: HotkeysConfig,
    temp_key: String,
    available_keys: Vec<String>,
    available_actions: Vec<String>,
    save_feedback: Option<(Instant, bool)>,
}

impl HotkeyConfigApp {
    fn new(config: HotkeysConfig) -> Self {
        let available_keys: Vec<String> = ('A'..='Z')
            .map(|c| format!("Key{}", c))
            .chain(('0'..='9').map(|c| format!("Key{}", c)))
            .collect();
        
        let available_actions = vec![
            "Format".to_string(),
            "TranslateE2C".to_string(),
            "TranslateC2E".to_string(),
            "Explain".to_string(),
        ];
        
        Self {
            config,
            temp_key: String::new(),
            available_keys,
            available_actions,
            save_feedback: None,
        }
    }
    
    fn add_binding(&mut self, action: &str) {
        if self.config.bindings.iter().any(|b| b.key == self.temp_key) {
            return; // Key already used
        }
        
        self.config.bindings.push(HotkeyBinding {
            key: self.temp_key.clone(),
            action: action.to_string(),
            modifiers: Some("Ctrl".to_string()),
        });
        self.temp_key.clear();
    }
}

impl eframe::App for HotkeyConfigApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Configure Keyboard Shortcuts");
            ui.add_space(10.0);
            
            ui.label("All shortcuts use Ctrl + Key combination");
            ui.add_space(10.0);
            
            // Table header
            egui::Grid::new("hotkey_grid")
                .striped(true)
                .spacing([10.0, 5.0])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Key").strong());
                    ui.label(egui::RichText::new("Action").strong());
                    ui.label(egui::RichText::new("Description").strong());
                    ui.label(""); // For delete button
                    ui.end_row();
                    
                    let mut to_delete = None;
                    
                    for (idx, binding) in self.config.bindings.iter_mut().enumerate() {
                        // Key column
                        let key_display = binding.key.strip_prefix("Key").unwrap_or(&binding.key);
                        ui.label(format!("Ctrl + {}", key_display));
                        
                        // Action dropdown
                        egui::ComboBox::from_id_source(format!("action_{}", idx))
                            .selected_text(&binding.action)
                            .show_ui(ui, |ui| {
                                for action in &self.available_actions {
                                    ui.selectable_value(&mut binding.action, action.clone(), action);
                                }
                            });
                        
                        // Description
                        let description = match binding.action.as_str() {
                            "Format" => "Fix PDF text and format math",
                            "TranslateE2C" => "Translate English to Chinese",
                            "TranslateC2E" => "Translate Chinese to English",
                            "Explain" => "Explain selected text",
                            _ => "Unknown action",
                        };
                        ui.label(description);
                        
                        // Delete button
                        if ui.button("✖").clicked() {
                            to_delete = Some(idx);
                        }
                        
                        ui.end_row();
                    }
                    
                    if let Some(idx) = to_delete {
                        self.config.bindings.remove(idx);
                    }
                });
            
            ui.add_space(15.0);
            ui.separator();
            ui.add_space(10.0);
            
            // Add new binding section
            ui.heading("Add New Hotkey");
            ui.horizontal(|ui| {
                ui.label("Ctrl +");
                
                egui::ComboBox::from_label("Key")
                    .selected_text(self.temp_key.strip_prefix("Key").unwrap_or(&self.temp_key))
                    .show_ui(ui, |ui| {
                        for key in &self.available_keys {
                            let display = key.strip_prefix("Key").unwrap_or(key);
                            ui.selectable_value(&mut self.temp_key, key.clone(), display);
                        }
                    });
                
                if ui.button("Add Format").clicked() && !self.temp_key.is_empty() {
                    self.add_binding("Format");
                }
                if ui.button("Add TranslateE2C").clicked() && !self.temp_key.is_empty() {
                    self.add_binding("TranslateE2C");
                }
                if ui.button("Add TranslateC2E").clicked() && !self.temp_key.is_empty() {
                    self.add_binding("TranslateC2E");
                }
                if ui.button("Add Explain").clicked() && !self.temp_key.is_empty() {
                    self.add_binding("Explain");
                }
            });
            
            ui.add_space(15.0);
            ui.separator();
            ui.add_space(10.0);
            
            // Save/Reset buttons
            ui.horizontal(|ui| {
                if ui.button("💾 Save & Apply").clicked() {
                    match save_hotkeys_config(&self.config) {
                        Ok(_) => {
                            self.save_feedback = Some((Instant::now(), true));
                        }
                        Err(_) => {
                            self.save_feedback = Some((Instant::now(), false));
                        }
                    }
                }
                
                // Show save feedback
                if let Some((time, success)) = &self.save_feedback {
                    if time.elapsed().as_secs() < 3 {
                        if *success {
                            ui.label(egui::RichText::new("✓ Saved! Restart app to apply.").color(egui::Color32::from_rgb(0, 255, 136)));
                        } else {
                            ui.label(egui::RichText::new("✗ Failed to save").color(egui::Color32::from_rgb(255, 85, 85)));
                        }
                    }
                }
                
                if ui.button("Reset to Defaults").clicked() {
                    self.config = HotkeysConfig::default();
                }
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
            
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(5.0);
            
            // Note about hot-reload
            ui.label(egui::RichText::new("Note: Changes take effect immediately for the running app.").italics().weak());
        });
        
        // Close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
