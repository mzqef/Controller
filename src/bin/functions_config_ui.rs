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

// -----------------------------------------------------------------------------
// Design tokens (re-exported from theme.rs).
//
// Previously this file declared its own SURFACE / BORDER / ACCENT / TEXT_MUTED /
// DANGER palette that diverged from the rest of the app. We now alias the
// canonical tokens so all windows share one design system. The local names are
// kept as private aliases to minimise churn in the rendering code below.
// -----------------------------------------------------------------------------
use IntelliBoard::ui::theme::{
    SPACE_2, SPACE_4, SPACE_5, SPACE_6, SPACE_7,
    RADIUS_LG, RADIUS_MD,
    TEXT_SM, TEXT_BASE, TEXT_MD, TEXT_LG, TEXT_XL, TEXT_2XL,
    STROKE_HAIRLINE,
    SURFACE_0, SURFACE_1, SURFACE_2, SURFACE_3,
    BORDER,
    ACCENT_BORDER, TEXT_MUTED, DANGER_TEXT,
    CTRL_H_MD, CTRL_H_LG,
};

// Legacy local aliases (kept so existing call sites compile; they now point at
// the canonical tokens, so the two windows can never drift again).
const SURFACE: egui::Color32 = SURFACE_2;
const SURFACE_ALT: egui::Color32 = SURFACE_3;
const ACCENT: egui::Color32 = ACCENT_BORDER;
const DANGER: egui::Color32 = DANGER_TEXT;

fn main() -> eframe::Result<()> {
    let hotkeys_config = load_hotkeys_config().unwrap_or_default();
    let actions_config = load_actions_config().unwrap_or_default();
    
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Window sized 1120×760. Position placed near the top-left so the
            // whole window fits on a 1920-wide screen (was 1000,130 → right
            // edge at 2120, off-screen). Centered would need monitor info at
            // build time, so we anchor near the left margin instead.
            .with_inner_size([1120.0, 760.0])
            .with_position([80.0, 80.0])
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
    edit_remote_model: String,
    edit_remote_prompt: String,
    edit_local_model: String,
    edit_local_prompt: String,
    
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
            current_tab: Tab::Actions,
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
            edit_remote_model: String::new(),
            edit_remote_prompt: String::new(),
            edit_local_model: String::new(),
            edit_local_prompt: String::new(),
            file_last_modified,
            has_unsaved_changes: false,
        }
    }
    
    fn optional_text(value: &str) -> Option<String> {
        if value.trim().is_empty() {
            None
        } else {
            Some(value.to_string())
        }
    }

    fn page_title(ui: &mut egui::Ui, title: &str) {
        // Use explicit TEXT_COLOR (same as body) + bold so the title has strong
        // contrast against the card background. Previously relied on the muted
        // default text color, which blended into the dark surface.
        ui.label(egui::RichText::new(title).size(TEXT_2XL).strong().color(egui::Color32::WHITE));
        ui.add_space(SPACE_5);
    }

    fn section(ui: &mut egui::Ui, title: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
        // Token-based card frame: elevated surface, rounded, hairline border.
        // The card fills the FULL available width without overflowing the right
        // edge. We capture card_width = available_width() OUTSIDE the Frame
        // (this is the width the whole Frame must occupy, including its own
        // inner margin). Inside the Frame, the inner content width is then
        // card_width minus the Frame's own inner margins on both sides, so the
        // Frame's outer rect equals exactly card_width — no overflow.
        let card_width = ui.available_width();
        // Frame inner margin is SPACE_5 on each side; subtract both so the
        // Frame's total outer width equals card_width.
        let inner_content_width = (card_width - 2.0 * SPACE_5).max(0.0);
        egui::Frame::none()
            .fill(SURFACE)
            .stroke(egui::Stroke::new(STROKE_HAIRLINE, BORDER))
            .rounding(egui::Rounding::same(RADIUS_LG))
            .inner_margin(egui::Margin::same(SPACE_5))
            .show(ui, |ui| {
                // Force the inner UI width so the Frame's outer rect = card_width.
                ui.set_min_width(inner_content_width);
                ui.set_max_width(inner_content_width);
                // Section title: explicit high-contrast color + bold.
                ui.label(egui::RichText::new(title).size(TEXT_MD).strong().color(egui::Color32::WHITE));
                ui.add_space(SPACE_4);
                add_contents(ui);
            });
    }

    fn nav_item(ui: &mut egui::Ui, selected: bool, label: &str) -> bool {
        let fill = if selected { SURFACE_ALT } else { egui::Color32::TRANSPARENT };
        let text = if selected {
            egui::RichText::new(label).strong().color(egui::Color32::WHITE).size(TEXT_BASE)
        } else {
            egui::RichText::new(label).size(TEXT_BASE)
        };
        ui.add_sized(
            [ui.available_width(), CTRL_H_LG],
            egui::Button::new(text)
                .fill(fill)
                .stroke(egui::Stroke::new(STROKE_HAIRLINE, if selected { ACCENT } else { BORDER }))
                .rounding(egui::Rounding::same(RADIUS_MD)),
        ).clicked()
    }

    fn text_row(ui: &mut egui::Ui, label: &str, value: &mut String, hint: &str, secret: bool) -> bool {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.add_sized([104.0, CTRL_H_MD], egui::Label::new(egui::RichText::new(label).color(TEXT_MUTED).size(TEXT_SM)));
            let mut edit = egui::TextEdit::singleline(value)
                .desired_width(ui.available_width())
                .hint_text(hint);
            if secret {
                edit = edit.password(true);
            }
            changed = ui.add(edit).changed();
        });
        changed
    }

    fn prompt_box(ui: &mut egui::Ui, label: &str, value: &mut String, rows: usize) -> bool {
        ui.label(egui::RichText::new(label).color(TEXT_MUTED).size(TEXT_SM));
        ui.add(
            egui::TextEdit::multiline(value)
                .desired_width(ui.available_width())
                .desired_rows(rows),
        ).changed()
    }
    
    /// Load action fields into the JSON editor buffers
    fn load_action_into_editor(&mut self, idx: usize) {
        if let Some(action) = self.actions_config.actions.get(idx) {
            let mut remote = action.remote.clone().unwrap_or_default();
            self.edit_remote_model = remote.model.take().unwrap_or_default();
            self.edit_remote_prompt = remote.prompt.take().unwrap_or_default();
            self.remote_json_buffer = serde_json::to_string_pretty(&remote).unwrap_or_else(|_| "{}".to_string());

            let mut local = action.local.clone().unwrap_or_default();
            self.edit_local_model = local.model.take().unwrap_or_default();
            self.edit_local_prompt = local.prompt.take().unwrap_or_default();
            self.local_json_buffer = serde_json::to_string_pretty(&local).unwrap_or_else(|_| "{}".to_string());
            
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
                (Ok(mut remote), Ok(mut local)) => {
                    if let Some(action) = self.actions_config.actions.get_mut(idx) {
                        remote.model = Self::optional_text(&self.edit_remote_model);
                        remote.prompt = Self::optional_text(&self.edit_remote_prompt);
                        local.model = Self::optional_text(&self.edit_local_model);
                        local.prompt = Self::optional_text(&self.edit_local_prompt);
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
    
    /// Reset all unsaved changes by reloading both configs from disk. This
    /// replaces the old Reset Hotkeys / Reset Actions buttons with a single
    /// "Reset" that discards every in-memory edit and reloads the on-disk state.
    fn reset_all(&mut self) {
        if let Ok(cfg) = load_actions_config() {
            self.actions_config = cfg;
        }
        if let Ok(cfg) = load_hotkeys_config() {
            self.hotkeys_config = cfg;
        }
        self.selected_action_idx = None;
        self.json_parse_error = None;
        self.has_unsaved_changes = false;
        self.file_last_modified = std::fs::metadata("config/actions.toml")
            .and_then(|m| m.modified())
            .ok();
        self.save_feedback = Some((Instant::now(), true, "Reverted to saved".to_string()));
    }

    fn render_hotkeys_tab(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing = egui::vec2(SPACE_4, SPACE_4);
        Self::page_title(ui, "Hotkeys");
        let action_labels = self.get_action_labels();

        let mut to_delete = None;
        let mut changed_any = false;
        // Current Shortcuts — the egui::Frame inside section() expands to fill
        // ui.available_width() by default, so the card is already full-width.
        // (A previous attempt wrapped this in allocate_ui(width, 0.0), but the
        // zero height confused the layout and did not actually widen anything.)
        Self::section(ui, "Current Shortcuts", |ui| {
            egui::Grid::new("hotkey_grid")
                .striped(true)
                .num_columns(4)
                .spacing([SPACE_5, SPACE_4])
                .min_col_width(96.0)
                .show(ui, |ui| {
                    ui.label(egui::RichText::new("Key").strong().color(TEXT_MUTED).size(TEXT_SM));
                    ui.label(egui::RichText::new("Action").strong().color(TEXT_MUTED).size(TEXT_SM));
                    ui.label(egui::RichText::new("Description").strong().color(TEXT_MUTED).size(TEXT_SM));
                    ui.label("");
                    ui.end_row();

                    for (idx, binding) in self.hotkeys_config.bindings.iter_mut().enumerate() {
                        let key_display = binding.key.strip_prefix("Key").unwrap_or(&binding.key);
                        ui.monospace(format!("Ctrl + {}", key_display));

                        let before = binding.action.clone();
                        let selected_label = action_labels.iter()
                            .find(|(action_id, _)| action_id == &binding.action)
                            .map(|(_, label)| label.clone())
                            .unwrap_or_else(|| binding.action.clone());
                        egui::ComboBox::from_id_source(format!("action_{}", idx))
                            .selected_text(selected_label)
                            .show_ui(ui, |ui| {
                                for (action_id, label) in &action_labels {
                                    ui.selectable_value(&mut binding.action, action_id.clone(), label);
                                }
                            });
                        if before != binding.action {
                            changed_any = true;
                        }

                        let description = self.actions_config.get_action(&binding.action)
                            .map(|a| a.description.as_str())
                            .unwrap_or("Unknown action");
                        ui.label(description);

                        if ui.small_button("Remove").clicked() {
                            to_delete = Some(idx);
                        }

                        ui.end_row();
                    }
                });
        });

        if let Some(idx) = to_delete {
            self.hotkeys_config.bindings.remove(idx);
            changed_any = true;
        }
        if changed_any {
            self.has_unsaved_changes = true;
        }

        ui.add_space(SPACE_5);
        Self::section(ui, "Add Shortcut", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new("Ctrl +").color(TEXT_MUTED).size(TEXT_SM));
                egui::ComboBox::from_id_source("new_hotkey_key")
                    .selected_text(self.temp_key.strip_prefix("Key").unwrap_or(&self.temp_key))
                    .show_ui(ui, |ui| {
                        for key in &self.available_keys {
                            let display = key.strip_prefix("Key").unwrap_or(key);
                            ui.selectable_value(&mut self.temp_key, key.clone(), display);
                        }
                    });

                for (action_id, label) in &action_labels {
                    if ui.button(label).clicked() && !self.temp_key.is_empty() {
                        self.add_hotkey_binding(action_id);
                        self.has_unsaved_changes = true;
                    }
                }
            });
        });
    }
    
    fn render_actions_tab(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing = egui::vec2(SPACE_4, SPACE_4);
        Self::page_title(ui, "AI Actions");
        let default_remote_model = self.actions_config.defaults.model.clone().unwrap_or_else(|| "qwen-max".to_string());
        let default_local_model = self.actions_config.defaults.local.as_ref()
            .and_then(|local| local.model.clone())
            .unwrap_or_else(|| "gemma4-it:e2b".to_string());
        let available_height = ui.available_height() - 10.0;
        let action_info: Vec<(usize, String, bool)> = self.actions_config.actions.iter()
            .enumerate()
            .map(|(idx, action)| (idx, action.label().to_string(), action.hidden))
            .collect();
        let current_selected = self.selected_action_idx;
        let mut new_selection = None;

        ui.horizontal(|ui| {
            // Left "Actions" list panel — shrunk 25% per design review (was 256px
            // → 192px) so the editor pane beside it gets more room.
            let actions_list_w = 192.0;
            ui.allocate_ui_with_layout(
                egui::vec2(actions_list_w, available_height),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    Self::section(ui, "Actions", |ui| {
                        egui::ScrollArea::vertical()
                            .id_source("actions_list_scroll")
                            .max_height((available_height - 160.0).max(160.0))
                            .show(ui, |ui| {
                                for (idx, label, hidden) in &action_info {
                                    let selected = current_selected == Some(*idx);
                                    let row_text = if *hidden { format!("{}  hidden", label) } else { label.clone() };
                                    let fill = if selected { SURFACE_ALT } else { egui::Color32::TRANSPARENT };
                                    if ui.add_sized(
                                        [ui.available_width(), CTRL_H_MD],
                                        egui::Button::new(egui::RichText::new(row_text).size(TEXT_SM))
                                            .fill(fill)
                                            .stroke(egui::Stroke::new(STROKE_HAIRLINE, if selected { ACCENT } else { BORDER }))
                                            .rounding(egui::Rounding::same(RADIUS_MD)),
                                    ).clicked() {
                                        new_selection = Some(*idx);
                                    }
                                }
                            });

                        ui.add_space(SPACE_4);
                        ui.separator();
                        ui.add_space(SPACE_4);
                        ui.label(egui::RichText::new("New action").color(TEXT_MUTED).size(TEXT_SM));
                        ui.add(egui::TextEdit::singleline(&mut self.new_action_name)
                            .hint_text("Action name")
                            .desired_width(ui.available_width()));
                        ui.horizontal(|ui| {
                            if ui.button("Add").clicked() && !self.new_action_name.is_empty() {
                                let id = self.new_action_name
                                    .to_lowercase()
                                    .chars()
                                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                                    .collect::<String>()
                                    .trim_matches('_')
                                    .to_string();
                                let mut new_action = ActionDefinition::new(&id, Some(&self.new_action_name));
                                new_action.description = "New custom action".to_string();
                                self.actions_config.actions.push(new_action);
                                let new_idx = self.actions_config.actions.len() - 1;
                                self.selected_action_idx = Some(new_idx);
                                self.load_action_into_editor(new_idx);
                                self.new_action_name.clear();
                                self.has_unsaved_changes = true;
                            }
                            // Delete moved to the action-detail header (top-right
                            // of the editor pane) — no longer mixed with Add.
                        });
                    });
                },
            );

            ui.add_space(SPACE_5);

            ui.allocate_ui_with_layout(
                // Right editor pane — narrowed by ~3 character widths (~24px)
                // so the Action / Remote Request / Local Request cards read as a
                // comfortable column rather than stretching edge-to-edge.
                egui::vec2((ui.available_width() - 24.0).max(280.0), available_height),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    if let Some(idx) = self.selected_action_idx {
                        let action_id = self.actions_config.actions.get(idx)
                            .map(|a| a.id.clone())
                            .unwrap_or_default();
                        let action_label = self.actions_config.actions.get(idx)
                            .map(|a| a.label().to_string())
                            .unwrap_or_default();

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(action_label).size(TEXT_LG).strong().color(egui::Color32::WHITE));
                            ui.label(egui::RichText::new(action_id).monospace().color(TEXT_MUTED).size(TEXT_SM));
                            // Delete moved here (action detail top-right) per
                            // design review. A right_to_left layout anchors it to
                            // the right edge of the editor pane; clicking it
                            // removes the selected action and clears the editor.
                            // Removed from the "New action" row below.
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                // Delete sized to match the Save / Reset buttons
                                // (96px wide, CTRL_H_MD tall) so it reads as a
                                // peer primary action rather than a tiny inline
                                // button.
                                let btn_w = 96.0;
                                if ui.add_sized(
                                    [btn_w, CTRL_H_MD],
                                    egui::Button::new(egui::RichText::new("Delete").color(DANGER).size(TEXT_SM)),
                                ).clicked() {
                                    if let Some(i) = self.selected_action_idx {
                                        self.actions_config.actions.remove(i);
                                        self.selected_action_idx = None;
                                        self.has_unsaved_changes = true;
                                    }
                                }
                            });
                        });

                        if let Some(ref error) = self.json_parse_error {
                            ui.label(egui::RichText::new(error).color(DANGER));
                        }

                        ui.add_space(SPACE_4);
                        egui::ScrollArea::vertical()
                            .id_source("action_editor_scroll")
                            // Leave room so the Advanced CollapsingHeader can
                            // expand without being clipped by the scroll cap
                            // (was the cause of "Advanced cannot expand").
                            .max_height((available_height - 24.0).max(200.0))
                            .show(ui, |ui| {
                                Self::section(ui, "Action", |ui| {
                                    if Self::text_row(ui, "Description", &mut self.edit_description, "Short label for this action", false) {
                                        self.has_unsaved_changes = true;
                                    }
                                    if ui.checkbox(&mut self.edit_hidden, "Hidden").changed() {
                                        self.has_unsaved_changes = true;
                                    }
                                });

                                ui.add_space(SPACE_5);
                                Self::section(ui, "Remote Request", |ui| {
                                    if Self::text_row(ui, "Model", &mut self.edit_remote_model, default_remote_model.as_str(), false) {
                                        self.has_unsaved_changes = true;
                                    }
                                    if Self::prompt_box(ui, "Prompt", &mut self.edit_remote_prompt, 5) {
                                        self.has_unsaved_changes = true;
                                    }
                                    egui::CollapsingHeader::new("Advanced")
                                        .id_source("advanced_remote")
                                        .default_open(false)
                                        .show(ui, |ui| {
                                            if ui.add(
                                                egui::TextEdit::multiline(&mut self.remote_json_buffer)
                                                    .font(egui::FontId::monospace(TEXT_SM))
                                                    .desired_width(ui.available_width())
                                                    .desired_rows(6)
                                                    .code_editor(),
                                            ).changed() {
                                                self.has_unsaved_changes = true;
                                            }
                                        });
                                });

                                ui.add_space(SPACE_5);
                                Self::section(ui, "Local Request", |ui| {
                                    if Self::text_row(ui, "Model", &mut self.edit_local_model, default_local_model.as_str(), false) {
                                        self.has_unsaved_changes = true;
                                    }
                                    if Self::prompt_box(ui, "Prompt", &mut self.edit_local_prompt, 4) {
                                        self.has_unsaved_changes = true;
                                    }
                                    egui::CollapsingHeader::new("Advanced")
                                        .id_source("advanced_local")
                                        .default_open(false)
                                        .show(ui, |ui| {
                                            if ui.add(
                                                egui::TextEdit::multiline(&mut self.local_json_buffer)
                                                    .font(egui::FontId::monospace(TEXT_SM))
                                                    .desired_width(ui.available_width())
                                                    .desired_rows(4)
                                                    .code_editor(),
                                            ).changed() {
                                                self.has_unsaved_changes = true;
                                            }
                                        });
                                });
                            });
                    } else {
                        Self::section(ui, "Action", |ui| {
                            ui.label(egui::RichText::new("Select an action from the list.").color(TEXT_MUTED));
                        });
                    }
                },
            );
        });

        if let Some(idx) = new_selection {
            if current_selected.is_some() && current_selected != Some(idx) {
                let _ = self.apply_editor_to_action();
            }
            self.selected_action_idx = Some(idx);
            self.load_action_into_editor(idx);
        }
    }
    
    fn render_settings_tab(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing = egui::vec2(SPACE_4, SPACE_4);
        Self::page_title(ui, "Settings");
        let mut changed = false;

        // All cards render full-width — egui::Frame inside section() expands to
        // ui.available_width() by default, so no manual width allocator needed.
        // (A previous allocate_ui(width, 0.0) wrapper had zero height and did
        // not actually widen anything.)
        {
            let defaults = &mut self.actions_config.defaults;

            Self::section(ui, "Remote Defaults", |ui| {
                let mut url = defaults.api_url.clone().unwrap_or_default();
                if Self::text_row(ui, "API URL", &mut url, "https://api.openai.com/v1/chat/completions", false) {
                    defaults.api_url = Self::optional_text(&url);
                    changed = true;
                }

                let mut model = defaults.model.clone().unwrap_or_default();
                if Self::text_row(ui, "Model", &mut model, "qwen-max", false) {
                    defaults.model = Self::optional_text(&model);
                    changed = true;
                }

                let mut key = defaults.api_key.clone().unwrap_or_default();
                if Self::text_row(ui, "API Key", &mut key, "${API_KEY}", true) {
                    defaults.api_key = Self::optional_text(&key);
                    changed = true;
                }
            });

            ui.add_space(SPACE_5);
            if defaults.local.is_none() {
                defaults.local = Some(ActionLocalConfig::default());
            }
            Self::section(ui, "Local Defaults", |ui| {
                if let Some(local) = &mut defaults.local {
                    let mut url = local.api_url.clone().unwrap_or_default();
                    if Self::text_row(ui, "API URL", &mut url, "http://127.0.0.1:52625/v1/chat/completions", false) {
                        local.api_url = Self::optional_text(&url);
                        changed = true;
                    }

                    let mut model = local.model.clone().unwrap_or_default();
                    if Self::text_row(ui, "Model", &mut model, "gemma4-it:e2b", false) {
                        local.model = Self::optional_text(&model);
                        changed = true;
                    }

                    let mut key = local.api_key.clone().unwrap_or_default();
                    if Self::text_row(ui, "API Key", &mut key, "${LOCAL_API_KEY}", true) {
                        local.api_key = Self::optional_text(&key);
                        changed = true;
                    }
                }
            });
        }

        ui.add_space(SPACE_5);
        Self::section(ui, "Behavior", |ui| {
            if ui.checkbox(&mut self.actions_config.force_local, "Use local model for every action").changed() {
                changed = true;
            }
        });

        ui.add_space(SPACE_5);
        Self::section(ui, "Export", |ui| {
            let mut path = self.actions_config.export_path.clone().unwrap_or_default();
            if Self::text_row(ui, "Path", &mut path, "Downloads", false) {
                self.actions_config.export_path = Self::optional_text(&path);
                changed = true;
            }
        });

        if changed {
            self.has_unsaved_changes = true;
        }
    }
}

impl eframe::App for FunctionsConfigApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Check for external file changes
        let file_changed_externally = self.check_file_changed();

        if file_changed_externally && self.has_unsaved_changes {
            egui::Window::new("Config File Changed")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.add_space(SPACE_2);
                    ui.label(egui::RichText::new("The config file was modified externally.").size(TEXT_BASE));
                    ui.add_space(SPACE_5);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = SPACE_4;
                        if ui.button("Reload").clicked() {
                            if let Ok(cfg) = load_actions_config() {
                                self.actions_config = cfg;
                                self.selected_action_idx = None;
                                self.has_unsaved_changes = false;
                            }
                            self.file_last_modified = std::fs::metadata("config/actions.toml")
                                .and_then(|m| m.modified()).ok();
                        }
                        if ui.button("Keep Editing").clicked() {
                            self.file_last_modified = std::fs::metadata("config/actions.toml")
                                .and_then(|m| m.modified()).ok();
                        }
                    });
                });
        }

        egui::SidePanel::left("navigation")
            .exact_width(208.0)
            .frame(
                egui::Frame::none()
                    .fill(SURFACE_1)
                    .stroke(egui::Stroke::new(STROKE_HAIRLINE, BORDER))
                    .inner_margin(egui::Margin::same(SPACE_6)),
            )
            .show(ctx, |ui| {
                ui.label(egui::RichText::new("IntelliBoard").size(TEXT_XL).strong());
                ui.label(egui::RichText::new("Functions").color(TEXT_MUTED).size(TEXT_SM));
                ui.add_space(SPACE_6);

                if Self::nav_item(ui, self.current_tab == Tab::Actions, "AI Actions") {
                    self.current_tab = Tab::Actions;
                }
                if Self::nav_item(ui, self.current_tab == Tab::Hotkeys, "Hotkeys") {
                    self.current_tab = Tab::Hotkeys;
                }
                if Self::nav_item(ui, self.current_tab == Tab::Settings, "Settings") {
                    self.current_tab = Tab::Settings;
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    if self.has_unsaved_changes {
                        ui.label(egui::RichText::new("Unsaved changes").color(egui::Color32::from_rgb(255, 200, 80)).size(TEXT_SM));
                    } else {
                        ui.label(egui::RichText::new("Saved").color(TEXT_MUTED).size(TEXT_SM));
                    }
                });
            });

        egui::TopBottomPanel::bottom("actions")
            .frame(
                egui::Frame::none()
                    .fill(SURFACE_1)
                    .stroke(egui::Stroke::new(STROKE_HAIRLINE, BORDER))
                    .inner_margin(egui::Margin::symmetric(SPACE_6, SPACE_5)),
            )
            .show(ctx, |ui| {
                // Simplified bottom bar: only Save + Reset, both right-aligned.
                // Removed: Reset Hotkeys, Reset Actions, Close (the window's own
                // title-bar close button and Escape still close the window).
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_4;

                    // Save / Reset given a comfortable minimum width (was 0.0 =
                    // auto-sized to text only, which felt too narrow).
                    let btn_w = 96.0;
                    if ui.add_sized(
                        [btn_w, CTRL_H_MD],
                        egui::Button::new(egui::RichText::new("Save").size(TEXT_SM).strong().color(egui::Color32::BLACK))
                            .fill(ACCENT),
                    ).clicked() {
                        self.save_all();
                    }

                    if ui.add_sized(
                        [btn_w, CTRL_H_MD],
                        egui::Button::new(egui::RichText::new("Reset").size(TEXT_SM)),
                    ).clicked() {
                        self.reset_all();
                    }

                    // Status feedback sits to the LEFT of the buttons (still
                    // inside the right_to_left layout, so it renders before them).
                    if let Some((time, success, msg)) = &self.save_feedback {
                        if time.elapsed().as_secs() < 4 {
                            let color = if *success { ACCENT } else { DANGER };
                            ui.label(egui::RichText::new(msg).color(color).size(TEXT_SM));
                        }
                    }
                });
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(SURFACE_0)
                    .inner_margin(egui::Margin::same(SPACE_7)),
            )
            .show(ctx, |ui| match self.current_tab {
                Tab::Actions => self.render_actions_tab(ui),
                Tab::Hotkeys => {
                    // auto_shrink(false) + max_width forces the scroll area (and
                    // therefore the section cards inside) to fill the full panel
                    // width. Default auto_shrink([true;2]) collapses the width to
                    // the content, which made Hotkeys cards narrower than Actions.
                    let w = ui.available_width();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .max_width(w)
                        .show(ui, |ui| {
                            ui.set_width(w);
                            self.render_hotkeys_tab(ui)
                        });
                }
                Tab::Settings => {
                    let w = ui.available_width();
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .max_width(w)
                        .show(ui, |ui| {
                            ui.set_width(w);
                            self.render_settings_tab(ui)
                        });
                }
            });
        
        // Close on Escape
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
