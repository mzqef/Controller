use eframe::egui;
use crate::core::config::{HotkeysConfig, HotkeyBinding, save_hotkeys_config};
use log::{info, error};
use std::sync::{Arc, RwLock};

pub struct HotkeyConfigWindow {
    config: HotkeysConfig,
    temp_key: String,
    available_keys: Vec<String>,
    available_actions: Vec<String>,
    shared_hotkeys: Arc<RwLock<HotkeysConfig>>,
    save_feedback: Option<(std::time::Instant, bool)>, // (time, success)
}

impl HotkeyConfigWindow {
    pub fn new(config: HotkeysConfig, shared_hotkeys: Arc<RwLock<HotkeysConfig>>) -> Self {
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
            shared_hotkeys,
            save_feedback: None,
        }
    }
    
    pub fn get_config(&self) -> &HotkeysConfig {
        &self.config
    }
    
    pub fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut open = true;
        let mut should_close = false;
        
        egui::Window::new("Hotkey Configuration")
            .open(&mut open)
            .default_width(500.0)
            .resizable(true)
            .show(ctx, |ui| {
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
                    if ui.button("💾 Save").clicked() {
                        match save_hotkeys_config(&self.config) {
                            Ok(_) => {
                                info!("Hotkey configuration saved successfully");
                                // Hot-reload: update the shared config
                                if let Ok(mut guard) = self.shared_hotkeys.write() {
                                    *guard = self.config.clone();
                                    info!("Hotkeys hot-reloaded (no restart needed)");
                                }
                                self.save_feedback = Some((std::time::Instant::now(), true));
                            }
                            Err(e) => {
                                error!("Failed to save hotkey configuration: {}", e);
                                self.save_feedback = Some((std::time::Instant::now(), false));
                            }
                        }
                    }
                    
                    // Show save feedback
                    if let Some((time, success)) = &self.save_feedback {
                        if time.elapsed().as_secs() < 3 {
                            if *success {
                                ui.label(egui::RichText::new("✓ Applied!").color(egui::Color32::from_rgb(0, 255, 136)));
                            } else {
                                ui.label(egui::RichText::new("✗ Failed").color(egui::Color32::from_rgb(255, 85, 85)));
                            }
                        }
                    }
                    
                    if ui.button("Reset to Defaults").clicked() {
                        self.config = HotkeysConfig::default();
                    }
                    
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            should_close = true;
                        }
                    });
                });
            });
        
        if should_close {
            open = false;
        }
        open
    }
    
    fn add_binding(&mut self, action: &str) {
        // Check if key is already used
        if self.config.bindings.iter().any(|b| b.key == self.temp_key) {
            info!("Key {} is already bound", self.temp_key);
            return;
        }
        
        self.config.bindings.push(HotkeyBinding {
            key: self.temp_key.clone(),
            action: action.to_string(),
            modifiers: Some("Ctrl".to_string()),
        });
        self.temp_key.clear();
    }
}
