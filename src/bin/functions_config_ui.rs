#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Standalone Functions Configuration window.
//! Includes tabs for: Hotkeys and AI Actions (with JSON editing)
//! Spawned as a separate process from the tray menu.

use eframe::egui;
use IntelliBoard::core::config::{
    HotkeysConfig, HotkeyBinding, load_hotkeys_config, save_hotkeys_config,
    ActionsConfig, ActionDefinition, ActionLocalConfig, ActionRemoteConfig,
    load_actions_config, save_actions_config,
};
use std::time::Instant;

fn main() -> eframe::Result<()> {
    let hotkeys_config = load_hotkeys_config().unwrap_or_default();
    let actions_config = load_actions_config().unwrap_or_default();
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([900.0, 600.0])
            .with_position([1050.0, 150.0])
            .with_title("IntelliBoard Functions")
            .with_resizable(true)
            .with_active(true)
            .with_icon(IntelliBoard::ui::theme::load_egui_icon()),
        ..Default::default()
    };
    
    eframe::run_native(
        "Functions Configuration",
        options,
        Box::new(move |cc| {
            IntelliBoard::ui::theme::configure_fonts(&cc.egui_ctx);
            IntelliBoard::ui::theme::apply_theme(&cc.egui_ctx);
            Box::new(FunctionsConfigApp::new(hotkeys_config, actions_config))
        }),
    )
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Hotkeys,
    Actions,
    Settings,
}

struct FunctionsConfigApp {
    // State
    current_tab: Tab,
    save_feedback: Option<(Instant, bool, String)>,
    
    // Hotkeys tab
    hotkeys_config: HotkeysConfig,
    temp_key: String,
    available_keys: Vec<String>,
    
    // Actions tab - JSON editing (split Remote/Local)
    actions_config: ActionsConfig,
    selected_action_idx: Option<usize>,
    new_action_name: String,
    
    // JSON editor buffers
    remote_json_buffer: String,
    local_json_buffer: String,
    json_parse_error: Option<String>,
    
    // Editor state - Metadata (read-only id, editable description/hidden)
    edit_description: String,
    edit_hidden: bool,
    
    // Conflict detection
    file_last_modified: Option<std::time::SystemTime>,
    has_unsaved_changes: bool,
}

impl FunctionsConfigApp {
    fn new(hotkeys_config: HotkeysConfig, actions_config: ActionsConfig) -> Self {
        let available_keys: Vec<String> = ('A'..='Z')
            .map(|c| format!("Key{}", c))
            .chain(('0'..='9').map(|c| format!("Key{}", c)))
            .collect();
        
        // Get initial file modification time
        let file_last_modified = std::fs::metadata("config/actions.toml")
            .and_then(|m| m.modified())
            .ok();
        
        Self {
            current_tab: Tab::Hotkeys,
            save_feedback: None,
            hotkeys_config,
            temp_key: String::new(),
            available_keys,
            actions_config,
            selected_action_idx: None,
            new_action_name: String::new(),
            // JSON buffers
            remote_json_buffer: String::new(),
            local_json_buffer: String::new(),
            json_parse_error: None,
            // Metadata
            edit_description: String::new(),
            edit_hidden: false,
            file_last_modified,
            has_unsaved_changes: false,
        }
    }
    
    /// Load action fields into the JSON editor buffers
    fn load_action_into_editor(&mut self, idx: usize) {
        if let Some(action) = self.actions_config.actions.get(idx) {
            // Serialize remote config to JSON
            self.remote_json_buffer = action.remote.as_ref()
                .map(|r| serde_json::to_string_pretty(r).unwrap_or_else(|_| "{}".to_string()))
                .unwrap_or_else(|| "{}".to_string());
            
            // Serialize local config to JSON
            self.local_json_buffer = action.local.as_ref()
                .map(|l| serde_json::to_string_pretty(l).unwrap_or_else(|_| "{}".to_string()))
                .unwrap_or_else(|| "{}".to_string());
            
            // Metadata
            self.edit_description = action.description.clone();
            self.edit_hidden = action.hidden;
            self.json_parse_error = None;
        }
    }
    
    /// Apply JSON editor values back to the selected action
    fn apply_editor_to_action(&mut self) -> bool {
        if let Some(idx) = self.selected_action_idx {
            // Parse remote JSON
            let remote_result: Result<ActionRemoteConfig, _> = serde_json::from_str(&self.remote_json_buffer);
            let local_result: Result<ActionLocalConfig, _> = serde_json::from_str(&self.local_json_buffer);
            
            match (remote_result, local_result) {
                (Ok(remote), Ok(local)) => {
                    if let Some(action) = self.actions_config.actions.get_mut(idx) {
                        action.remote = Some(remote);
                        action.local = Some(local);
                        action.description = self.edit_description.clone();
                        action.hidden = self.edit_hidden;
                        self.json_parse_error = None;
                        self.has_unsaved_changes = true;
                        return true;
                    }
                }
                (Err(e), _) => {
                    self.json_parse_error = Some(format!("Remote JSON: {}", e));
                }
                (_, Err(e)) => {
                    self.json_parse_error = Some(format!("Local JSON: {}", e));
                }
            }
        }
        false
    }
    
    fn check_file_changed(&mut self) -> bool {
        if let Ok(meta) = std::fs::metadata("config/actions.toml") {
            if let Ok(modified) = meta.modified() {
                if let Some(last) = self.file_last_modified {
                    if modified > last {
                        return true;
                    }
                }
            }
        }
        false
    }
    
    fn get_action_labels(&self) -> Vec<(String, String)> {
        self.actions_config.actions.iter()
            .filter(|a| !a.hidden)
            .map(|a| (a.id.clone(), a.label().to_string()))
            .collect()
    }
    
    fn add_hotkey_binding(&mut self, action_id: &str) {
        if self.hotkeys_config.bindings.iter().any(|b| b.key == self.temp_key) {
            return; // Key already used
        }
        
        self.hotkeys_config.bindings.push(HotkeyBinding {
            key: self.temp_key.clone(),
            action: action_id.to_string(),
            modifiers: Some("Ctrl".to_string()),
        });
        self.temp_key.clear();
    }
    
    fn save_all(&mut self) {
        // Apply any pending editor changes first
        let _ = self.apply_editor_to_action();
        
        let mut errors = Vec::new();
        
        if let Err(e) = save_hotkeys_config(&self.hotkeys_config) {
            errors.push(format!("Hotkeys: {}", e));
        }
        
        if let Err(e) = save_actions_config(&self.actions_config) {
            errors.push(format!("Actions: {}", e));
        }
        
        if errors.is_empty() {
            self.save_feedback = Some((Instant::now(), true, "All settings saved!".to_string()));
            self.has_unsaved_changes = false;
            // Update file modification time
            self.file_last_modified = std::fs::metadata("config/actions.toml")
                .and_then(|m| m.modified())
                .ok();
        } else {
            self.save_feedback = Some((Instant::now(), false, errors.join("; ")));
        }
    }
    
    fn render_hotkeys_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Keyboard Shortcuts");
        ui.add_space(5.0);
        ui.label("All shortcuts use Ctrl + Key combination");
        ui.add_space(10.0);
        
        let action_labels = self.get_action_labels();
        
        // Table of current bindings
        egui::Grid::new("hotkey_grid")
            .striped(true)
            .spacing([10.0, 5.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Key").strong());
                ui.label(egui::RichText::new("Action").strong());
                ui.label(egui::RichText::new("Description").strong());
                ui.label(""); // Delete button
                ui.end_row();
                
                let mut to_delete = None;
                
                for (idx, binding) in self.hotkeys_config.bindings.iter_mut().enumerate() {
                    let key_display = binding.key.strip_prefix("Key").unwrap_or(&binding.key);
                    ui.label(format!("Ctrl + {}", key_display));
                    
                    // Action dropdown
                    egui::ComboBox::from_id_source(format!("action_{}", idx))
                        .selected_text(&binding.action)
                        .show_ui(ui, |ui| {
                            for (action_id, label) in &action_labels {
                                ui.selectable_value(&mut binding.action, action_id.clone(), label);
                            }
                        });
                    
                    // Description from action config
                    let description = self.actions_config.get_action(&binding.action)
                        .map(|a| a.description.as_str())
                        .unwrap_or("Unknown action");
                    ui.label(description);
                    
                    if ui.button("✖").clicked() {
                        to_delete = Some(idx);
                    }
                    
                    ui.end_row();
                }
                
                if let Some(idx) = to_delete {
                    self.hotkeys_config.bindings.remove(idx);
                }
            });
        
        ui.add_space(15.0);
        ui.separator();
        ui.add_space(10.0);
        
        // Add new binding
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
            
            ui.label("→");
            
            for (action_id, label) in &action_labels {
                if ui.button(label).clicked() && !self.temp_key.is_empty() {
                    self.add_hotkey_binding(action_id);
                }
            }
        });
    }
    
    fn render_actions_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("AI Actions");
        ui.label("Select an action to edit its JSON configuration");
        ui.add_space(5.0);
        
        let available_height = ui.available_height() - 10.0;
        
        // Use columns for side-by-side layout with independent scrolling
        ui.columns(2, |columns| {
            // LEFT COLUMN: Action list
            columns[0].set_min_width(180.0);
            columns[0].set_max_width(200.0);
            columns[0].label(egui::RichText::new("Actions").strong());
            
            let action_info: Vec<(usize, String)> = self.actions_config.actions.iter()
                .enumerate()
                .map(|(idx, action)| {
                    let label = if action.hidden {
                        format!("{} (hidden)", action.label())
                    } else {
                        action.label().to_string()
                    };
                    (idx, label)
                })
                .collect();
            
            let mut new_selection = None;
            let current_selected = self.selected_action_idx;
            
            egui::ScrollArea::vertical()
                .id_source("actions_list_scroll")
                .max_height(available_height - 150.0)
                .show(&mut columns[0], |ui| {
                    for (idx, label) in &action_info {
                        if ui.selectable_label(current_selected == Some(*idx), label).clicked() {
                            new_selection = Some(*idx);
                        }
                    }
                });
            
            if let Some(idx) = new_selection {
                if current_selected.is_some() && current_selected != Some(idx) {
                    let _ = self.apply_editor_to_action();
                }
                self.selected_action_idx = Some(idx);
                self.load_action_into_editor(idx);
            }
            
            columns[0].add_space(10.0);
            columns[0].separator();
            columns[0].label("New Action Name:");
            columns[0].add(egui::TextEdit::singleline(&mut self.new_action_name)
                .hint_text("My Action")
                .desired_width(150.0));
            if columns[0].button("➕ Add Action").clicked() && !self.new_action_name.is_empty() {
                // Slugify the name to create the id
                let id = self.new_action_name
                    .to_lowercase()
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                    .collect::<String>()
                    .trim_matches('_')
                    .to_string();
                
                let mut new_action = ActionDefinition::new(&id, Some(&self.new_action_name));
                new_action.description = "New custom action".to_string();
                // new_action already has default remote/local from new()
                
                self.actions_config.actions.push(new_action);
                let new_idx = self.actions_config.actions.len() - 1;
                self.selected_action_idx = Some(new_idx);
                self.load_action_into_editor(new_idx);
                self.new_action_name.clear();
                self.has_unsaved_changes = true;
            }
            if self.selected_action_idx.is_some() && columns[0].button("🗑 Delete Selected").clicked() {
                if let Some(idx) = self.selected_action_idx {
                    self.actions_config.actions.remove(idx);
                    self.selected_action_idx = None;
                    self.has_unsaved_changes = true;
                }
            }
            
            // RIGHT COLUMN: Dual JSON Editors (Remote/Local)
            if let Some(idx) = self.selected_action_idx {
                let action_id = self.actions_config.actions.get(idx)
                    .map(|a| a.id.clone())
                    .unwrap_or_default();
                let action_label = self.actions_config.actions.get(idx)
                    .map(|a| a.label().to_string())
                    .unwrap_or_default();
                
                columns[1].horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Editing: {} ({})", action_label, action_id)).strong());
                    if ui.button("Apply").clicked() {
                        if self.apply_editor_to_action() {
                            self.save_feedback = Some((Instant::now(), true, "Changes applied!".to_string()));
                        }
                    }
                    if ui.button("Revert").clicked() {
                        self.load_action_into_editor(idx);
                    }
                });
                
                // Show parse error if any
                if let Some(ref error) = self.json_parse_error {
                    columns[1].label(egui::RichText::new(format!("⚠ {}", error))
                        .color(egui::Color32::from_rgb(255, 100, 100)));
                }
                
                let editor_height = available_height - 60.0;
                
                egui::ScrollArea::vertical()
                    .id_source("action_editor_scroll")
                    .max_height(editor_height)
                    .show(&mut columns[1], |ui| {
                        let field_width = ui.available_width() - 10.0;
                        
                        // Metadata section
                        ui.add_space(5.0);
                        ui.horizontal(|ui| {
                            ui.label("Description:");
                            if ui.add(egui::TextEdit::singleline(&mut self.edit_description)
                                .desired_width(field_width - 100.0)).changed() {
                                self.has_unsaved_changes = true;
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut self.edit_hidden, "Hidden (exclude from hotkey dropdown)").changed() {
                                self.has_unsaved_changes = true;
                            }
                        });
                        
                        ui.add_space(10.0);
                        ui.separator();
                        
                        // REMOTE JSON SECTION
                        ui.add_space(5.0);
                        ui.label(egui::RichText::new("☁ Remote API Configuration (JSON)").strong());
                        ui.label(egui::RichText::new("Fields: api_url, api_key, model, prompt, temperature, is_translation, is_vision, min_pixels, max_pixels").weak().small());
                        ui.add_space(3.0);
                        
                        if ui.add(
                            egui::TextEdit::multiline(&mut self.remote_json_buffer)
                                .font(egui::FontId::monospace(12.0))
                                .desired_width(field_width)
                                .desired_rows(8)
                                .code_editor()
                        ).changed() {
                            self.has_unsaved_changes = true;
                        }
                        
                        ui.add_space(10.0);
                        ui.separator();
                        
                        // LOCAL JSON SECTION
                        ui.add_space(5.0);
                        ui.label(egui::RichText::new("💻 Local LLM Configuration (JSON)").strong());
                        ui.label(egui::RichText::new("Fields: api_url, model, prompt, + any extra params").weak().small());
                        ui.add_space(3.0);
                        
                        if ui.add(
                            egui::TextEdit::multiline(&mut self.local_json_buffer)
                                .font(egui::FontId::monospace(12.0))
                                .desired_width(field_width)
                                .desired_rows(6)
                                .code_editor()
                        ).changed() {
                            self.has_unsaved_changes = true;
                        }
                        
                        ui.add_space(5.0);
                        ui.label(egui::RichText::new("Tip: Add custom fields for extra API parameters. Empty fields use global defaults.").weak().italics().small());
                    });
            } else {
                columns[1].label("← Select an action to edit");
            }
        });
    }
    
    fn render_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Global API Settings");
        ui.add_space(5.0);
        ui.label("Default endpoints and credentials used by all actions (can be overridden per-action)");
        ui.add_space(15.0);
        
        let defaults = &mut self.actions_config.defaults;
        let available_width = ui.available_width();
        
        // Remote API section
        ui.label(egui::RichText::new("☁ Remote API").strong());
        ui.add_space(5.0);
        
        ui.horizontal(|ui| {
            ui.label("API URL:");
            let mut url = defaults.api_url.clone().unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut url)
                .desired_width(available_width - 80.0)
                .hint_text("https://api.openai.com/v1/chat/completions")).changed() {
                defaults.api_url = if url.is_empty() { None } else { Some(url) };
                self.has_unsaved_changes = true;
            }
        });
        
        ui.add_space(5.0);
        ui.horizontal(|ui| {
            ui.label("API Key:");
            let mut key = defaults.api_key.clone().unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut key)
                .password(true)
                .desired_width(available_width - 80.0)
                .hint_text("sk-... or ${ENV_VAR}")).changed() {
                defaults.api_key = if key.is_empty() { None } else { Some(key) };
                self.has_unsaved_changes = true;
            }
        });
        
        ui.add_space(20.0);
        ui.separator();
        ui.add_space(10.0);
        
        // Local API section
        ui.label(egui::RichText::new("💻 Local LLM").strong());
        ui.add_space(5.0);
        
        // Initialize local config if None
        if defaults.local.is_none() {
            defaults.local = Some(ActionLocalConfig::default());
        }
        
        if let Some(local) = &mut defaults.local {
            ui.horizontal(|ui| {
                ui.label("API URL:");
                let mut url = local.api_url.clone().unwrap_or_default();
                if ui.add(egui::TextEdit::singleline(&mut url)
                    .desired_width(available_width - 80.0)
                    .hint_text("http://localhost:8000/v1/chat/completions")).changed() {
                    local.api_url = if url.is_empty() { None } else { Some(url) };
                    self.has_unsaved_changes = true;
                }
            });
        }
        
        ui.add_space(20.0);
        ui.separator();
        ui.add_space(10.0);
        
        // Force local toggle
        if ui.checkbox(&mut self.actions_config.force_local, "Force Local Mode (always use local LLM, ignore remote)").changed() {
            self.has_unsaved_changes = true;
        }
        
        ui.add_space(20.0);
        ui.separator();
        ui.add_space(10.0);
        
        // Export path section
        ui.label(egui::RichText::new("📁 Export Settings").strong());
        ui.add_space(5.0);
        
        ui.horizontal(|ui| {
            ui.label("Export Path:");
            let mut path = self.actions_config.export_path.clone().unwrap_or_default();
            if ui.add(egui::TextEdit::singleline(&mut path)
                .desired_width(available_width - 100.0)
                .hint_text("Leave empty for Downloads folder")).changed() {
                self.actions_config.export_path = if path.is_empty() { None } else { Some(path) };
                self.has_unsaved_changes = true;
            }
        });
        ui.label(egui::RichText::new("Directory for exporting Memory Graph data").weak().italics().small());
    }
}

impl eframe::App for FunctionsConfigApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for external file changes
        let file_changed_externally = self.check_file_changed();
        
        egui::CentralPanel::default().show(ctx, |ui| {
            // Show conflict warning if file changed externally while we have unsaved changes
            if file_changed_externally && self.has_unsaved_changes {
                egui::Window::new("⚠ Config File Changed")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .show(ctx, |ui| {
                        ui.label("The config file was modified externally.");
                        ui.label("You have unsaved changes. What would you like to do?");
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("Reload from file (lose changes)").clicked() {
                                if let Ok(cfg) = load_actions_config() {
                                    self.actions_config = cfg;
                                    self.selected_action_idx = None;
                                    self.has_unsaved_changes = false;
                                }
                                self.file_last_modified = std::fs::metadata("config/actions.toml")
                                    .and_then(|m| m.modified()).ok();
                            }
                            if ui.button("Keep my changes").clicked() {
                                self.file_last_modified = std::fs::metadata("config/actions.toml")
                                    .and_then(|m| m.modified()).ok();
                            }
                        });
                    });
            }
            
            // Tab bar
            ui.horizontal(|ui| {
                if ui.selectable_label(self.current_tab == Tab::Hotkeys, "🎹 Hotkeys").clicked() {
                    self.current_tab = Tab::Hotkeys;
                }
                if ui.selectable_label(self.current_tab == Tab::Actions, "🤖 AI Actions").clicked() {
                    self.current_tab = Tab::Actions;
                }
                if ui.selectable_label(self.current_tab == Tab::Settings, "⚙ Settings").clicked() {
                    self.current_tab = Tab::Settings;
                }
                
                // Show unsaved indicator
                if self.has_unsaved_changes {
                    ui.label(egui::RichText::new("●").color(egui::Color32::from_rgb(255, 200, 0)));
                }
            });
            
            ui.separator();
            ui.add_space(10.0);
            
            // Tab content
            match self.current_tab {
                Tab::Actions => self.render_actions_tab(ui),
                Tab::Hotkeys | Tab::Settings => {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        match self.current_tab {
                            Tab::Hotkeys => self.render_hotkeys_tab(ui),
                            Tab::Settings => self.render_settings_tab(ui),
                            _ => {}
                        }
                    });
                }
            }
            
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(5.0);
            
            // Bottom bar: Save/Reset/Close
            ui.horizontal(|ui| {
                if ui.button("💾 Save All").clicked() {
                    self.save_all();
                }
                
                // Show save feedback
                if let Some((time, success, msg)) = &self.save_feedback {
                    if time.elapsed().as_secs() < 4 {
                        if *success {
                            ui.label(egui::RichText::new(format!("✓ {}", msg)).color(egui::Color32::from_rgb(0, 255, 136)));
                        } else {
                            ui.label(egui::RichText::new(format!("✗ {}", msg)).color(egui::Color32::from_rgb(255, 85, 85)));
                        }
                    }
                }
                
                if ui.button("Reset Hotkeys").clicked() {
                    self.hotkeys_config = HotkeysConfig::default();
                    self.has_unsaved_changes = true;
                }
                
                if ui.button("Reset Actions").clicked() {
                    self.actions_config = ActionsConfig::default();
                    self.selected_action_idx = None;
                    self.has_unsaved_changes = true;
                }
                
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
            
            ui.add_space(5.0);
            ui.label(egui::RichText::new("Changes are auto-synced when saved. Hot-reload enabled.").italics().weak());
        });
        
        // Close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
