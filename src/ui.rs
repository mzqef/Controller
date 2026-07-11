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

use crate::ui::theme::{
    SPACE_2, SPACE_3, SPACE_4,
    RADIUS_MD, RADIUS_SM,
    TEXT_SM, TEXT_BASE, TEXT_MD,
    STROKE_HAIRLINE,
    SURFACE_2,
    BORDER,
    NEON_CYAN, NEON_PINK, DANGER_TEXT, SUCCESS, WARN, TEXT_COLOR, TEXT_MUTED,
    CTRL_H_SM,
};

// Result window geometry (kept here for clarity; was hardcoded inline before).
const RESULT_W: f32 = 520.0;
const RESULT_H: f32 = 700.0;
const RESULT_X: f32 = 1350.0;
const RESULT_Y: f32 = 250.0;
const BAR_W: f32 = 300.0;
const BAR_H: f32 = 56.0;

pub struct TrayHandler {
    pub icon: tray_icon::TrayIcon,
    pub enable_item: CheckMenuItem,
    pub enable_id: tray_icon::menu::MenuId,
    pub local_mode_item: CheckMenuItem,
    pub local_mode_id: tray_icon::menu::MenuId,
    pub exit_id: tray_icon::menu::MenuId,
    pub show_log_id: tray_icon::menu::MenuId,
    pub hotkey_config_id: tray_icon::menu::MenuId,
    pub tx: Sender<AppEvent>,
    pub custom_commands: std::collections::HashMap<tray_icon::menu::MenuId, String>,
}

pub enum UiEvent {
    /// Processing started with action label for display (e.g., "Translation", "Image OCR")
    /// and the original input text (so the result window can show the correct input
    /// immediately on popup, instead of stale content from the previous call).
    ProcessingStarted(String, String),
    ShowResult(String, String), // original, result
    StreamUpdate(String), // chunk
    StreamEnd(bool), // true = success, false = incomplete
    StreamError(String), // error message
    ShowMemoryGraph,
    ShowHotkeyConfig,
    /// Pop up the floating action toolbar near the selection. Carries the
    /// currently-selected text and the screen position (x, y) where the
    /// toolbar should anchor.
    ///
    /// TRIGGER POLICY: this event is sent ONLY from the clipboard-copy path
    /// in `main.rs` (after a stable, non-programmatic copy of meaningful
    /// text). There is no separate "mouse-selection-release" trigger — the
    /// toolbar appears after the user *copies* text, not after they merely
    /// select it.
    ShowActionToolbar { text: String, x: i32, y: i32 },
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

/// Floating selection toolbar state — pops up near the caret/cursor when the
/// user copies text, offering clickable AI functions as an alternative to the
/// hotkey path.
struct ToolbarState {
    visible: bool,
    text: String,
    /// Anchor position (mouse release point) in physical screen pixels.
    pos: (i32, i32),
    /// Last known toolbar window rect in physical screen pixels, updated each
    /// frame after positioning. Used for the hover-keep / leave-hide test.
    rect: Option<egui::Rect>,
    /// When the toolbar was first shown (for the hard timeout).
    shown_at: Instant,
    /// Last time the cursor was confirmed to be inside the keep-zone (toolbar
    /// rect ∪ anchor grace area). When the cursor leaves, we hide promptly.
    last_in_zone: Instant,
    /// Latest snapshot of available actions (id, label) for rendering buttons.
    actions: Vec<(String, String)>,
}

impl Default for ToolbarState {
    fn default() -> Self {
        Self {
            visible: false,
            text: String::new(),
            pos: (300, 300),
            rect: None,
            shown_at: Instant::now() - std::time::Duration::from_secs(60),
            last_in_zone: Instant::now() - std::time::Duration::from_secs(60),
            actions: Vec::new(),
        }
    }
}

/// Absolute hard timeout: even if the cursor never leaves, hide after this long
/// so the toolbar can't linger forever.
const TOOLBAR_TIMEOUT_SECS: u64 = 12;
/// Grace radius (px) around the anchor point that still counts as "staying".
/// Lets the user drift the mouse a little without dismissing the popup.
const ANCHOR_GRACE_PX: f32 = 36.0;
/// How long the cursor may be outside the keep-zone before we hide. Small but
/// non-zero to avoid flicker on rapid mouse jitter.
const LEAVE_GRACE_MS: u64 = 120;

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
    /// Floating action toolbar state (pops up after a copy).
    toolbar: ToolbarState,
    /// Shared actions config so the toolbar can render the current function list.
    shared_actions: Arc<std::sync::RwLock<crate::core::config::ActionsConfig>>,
    /// Previous-frame left-mouse-button state, used for press→release
    /// (click) edge detection in the result-window "click outside to
    /// close" logic. See `check_click_outside_to_close`.
    was_left_mouse_down: bool,
}

impl MyApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        rx: flume::Receiver<UiEvent>,
        app_tx: Sender<AppEvent>,
        ctx_holder: Arc<Mutex<Option<egui::Context>>>,
        tray_handler: Arc<Mutex<TrayHandler>>,
        process_manager: Arc<crate::core::process_manager::ProcessManager>,
        shared_actions: Arc<std::sync::RwLock<crate::core::config::ActionsConfig>>,
    ) -> Self {
        info!("MyApp initialized");
        *ctx_holder.lock().unwrap() = Some(cc.egui_ctx.clone());
        
        theme::configure_fonts(&cc.egui_ctx);
        theme::apply_theme(&cc.egui_ctx);

        // Build initial toolbar action list from config (visible, non-translation-text
        // entries — translation/vision actions are still listed; user can click).
        let actions = shared_actions
            .read()
            .map(|cfg| {
                cfg.visible_actions()
                    .iter()
                    .map(|a| (a.id.clone(), a.label().to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

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
            toolbar: ToolbarState { actions, ..Default::default() },
            shared_actions,
            was_left_mouse_down: false,
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
            } else if event.id == handler.local_mode_id {
                let use_local = handler.local_mode_item.is_checked();
                info!("Tray Local Mode toggled: {}", use_local);
                let _ = handler.tx.try_send(AppEvent::ToggleLocalMode(use_local));
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
                        {
                            let _ = std::process::Command::new("notepad").arg(&path).spawn();
                        }
                        #[cfg(not(target_os = "windows"))]
                        {
                            // Try common Linux/macOS editors / openers.
                            for opener in &[
                                "xdg-open", // Linux default
                                "open",     // macOS
                                "geany",
                                "gedit",
                                "kate",
                                "nano",
                            ] {
                                if which(opener).is_some() {
                                    // Terminal-based editors need a terminal host.
                                    if opener == &"nano" {
                                        let _ = std::process::Command::new("xterm")
                                            .arg("-e")
                                            .arg(opener)
                                            .arg(&path)
                                            .spawn();
                                    } else {
                                        let _ = std::process::Command::new(opener)
                                            .arg(&path)
                                            .spawn();
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
            } else if event.id == handler.hotkey_config_id {
                info!("Tray Configure Hotkeys clicked");
                let _ = handler.tx.try_send(AppEvent::ShowHotkeyConfig);
            } else if let Some(cmd) = handler.custom_commands.get(&event.id) {
                info!("Executing custom command: {}", cmd);
                #[cfg(target_os = "windows")]
                {
                    let _ = std::process::Command::new("wt.exe")
                        .args(&["-p", "Windows PowerShell", "-d", ".", "powershell", "-Command", cmd])
                        .spawn();
                }
                #[cfg(not(target_os = "windows"))]
                {
                    // On Linux/macOS, run the command in an available terminal
                    // emulator so the user sees output. Falls back to a plain
                    // shell if no terminal is found.
                    let term = find_terminal_emulator();
                    if let Some(term) = term {
                        let _ = std::process::Command::new(&term)
                            .arg("-e")
                            .arg("sh")
                            .arg("-c")
                            .arg(cmd)
                            .spawn();
                    } else {
                        // No terminal found — run directly (output goes nowhere
                        // but at least the command executes).
                        let _ = std::process::Command::new("sh")
                            .arg("-c")
                            .arg(cmd)
                            .spawn();
                    }
                }
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
                UiEvent::ProcessingStarted(action_label, original_input) => {
                    info!("UI received ProcessingStarted event: {}", action_label);
                    self.text.clear();
                    self.displayed_text.clear();
                    self.current_action_label = action_label;
                    // Show the correct input immediately so the result window
                    // does not display stale content from the previous call.
                    self.original_text = original_input;
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
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([RESULT_X, RESULT_Y].into()));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([RESULT_W, RESULT_H].into()));
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
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([RESULT_X, RESULT_Y].into()));
                        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([RESULT_W, RESULT_H].into()));
                        // Bring forward once so the result window appears above the
                        // previously-focused app. WindowLevel will already be Normal
                        // here (Streaming != Waiting), so without this Focus() the
                        // newly-expanded window could land behind the foreground app.
                        if std::env::var_os("IntelliBoard_NO_UI_FOCUS").is_none() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                        }
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
                UiEvent::ShowActionToolbar { text, x, y } => {
                    info!("Showing action toolbar at ({},{}), text len={}", x, y, text.len());
                    // Refresh action list from current config in case it changed
                    if let Ok(cfg) = self.shared_actions.read() {
                        self.toolbar.actions = cfg
                            .visible_actions()
                            .iter()
                            .map(|a| (a.id.clone(), a.label().to_string()))
                            .collect();
                    }
                    self.toolbar.text = text;
                    self.toolbar.pos = (x, y);
                    self.toolbar.rect = None;
                    self.toolbar.visible = true;
                    self.toolbar.shown_at = Instant::now();
                    self.toolbar.last_in_zone = Instant::now();
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

        // Window level (Z-order) dispatch.
        //
        // The same egui viewport is reused for three different surfaces. They
        // get different Z-order behaviour depending on what's currently shown:
        //
        //   - Result window (Finished / Error / Incomplete / Streaming with
        //     `self.visible`) : Normal level. It's brought forward by an
        //     explicit `ViewportCommand::Focus` at the moment the result
        //     arrives (see ShowResult above), so it pops to the front once and
        //     then lets the user raise other windows above it. This matches the
        //     UX expectation: it doesn't *stay* on top forever.
        //   - Status bar (Waiting) and selection toolbar : AlwaysOnTop. These
        //     are short-lived overlays that must sit on top of the focused app
        //     until they dismiss themselves.
        //
        // Sending the command every frame is cheap (egui coalesces identical
        // commands) and lets transitions happen immediately on state change.
        if std::env::var_os("IntelliBoard_NO_TOPMOST").is_none() {
            // Only request topmost for surfaces that genuinely need it. The
            // result window falls back to Normal so it can drop behind other
            // windows after the initial Focus().
            let want_topmost = (self.state == AppState::Waiting)
                || (self.toolbar.visible && self.state == AppState::Idle);
            let target_level = if want_topmost {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(target_level));
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

        // ----- Floating Action Toolbar -----
        // Pops up right where the user finished a text selection (mouse release
        // point). Sticks to the selection/mouse: stays open while the cursor
        // hovers the toolbar OR lingers near the anchor; hides the moment the
        // cursor moves away.
        if self.toolbar.visible && self.state == AppState::Idle {
            // Read the global cursor every frame so we can react even while the
            // egui window itself has no focus / no pointer events.
            let cursor = crate::platform::get_cursor_pos();
            let now = Instant::now();

            // Hard timeout — never linger forever, even if hovered.
            if now.duration_since(self.toolbar.shown_at).as_secs() >= TOOLBAR_TIMEOUT_SECS {
                info!("Toolbar hard timeout, hiding");
                self.toolbar.visible = false;
            }

            // Determine whether the cursor is in the keep-zone.
            let in_zone = match (cursor, self.toolbar.rect) {
                (Some((cx, cy)), Some(rect)) => {
                    let in_toolbar = rect.contains(egui::pos2(cx as f32, cy as f32));
                    // Grace circle around the anchor (the mouse-release point),
                    // so the user can move from selection → toolbar without the
                    // popup dismissing itself.
                    let dx = cx as f32 - self.toolbar.pos.0 as f32;
                    let dy = cy as f32 - self.toolbar.pos.1 as f32;
                    let near_anchor = (dx * dx + dy * dy).sqrt() <= ANCHOR_GRACE_PX;
                    in_toolbar || near_anchor
                }
                _ => true, // can't read cursor yet — assume inside (don't flicker on show)
            };

            if in_zone {
                self.toolbar.last_in_zone = now;
            } else if now.duration_since(self.toolbar.last_in_zone).as_millis()
                >= LEAVE_GRACE_MS as u128
            {
                info!("Cursor left toolbar zone, hiding");
                self.toolbar.visible = false;
            }

            // Keep repainting so the leave/timeout checks actually fire on time.
            ctx.request_repaint();
        }

        if self.toolbar.visible && self.state == AppState::Idle {
            // Vertical menu: one function per row, width auto-fitted to the
            // longest label so nothing gets truncated, generous row spacing so
            // buttons don't feel crammed together.
            let pad = 6.0;
            let row_gap = 4.0;
            let font_size = 12.0;
            let row_h = font_size + 10.0; // text + vertical breathing room
            let count = self.toolbar.actions.len().max(1) as f32;

            // Auto-fit width: measure the longest label (approx char width ≈ 0.6 * size)
            // and add left padding + right slack. This avoids both truncation and
            // a window that's too wide for short labels.
            let max_label_chars = self
                .toolbar
                .actions
                .iter()
                .map(|(_, l)| l.chars().count())
                .max()
                .unwrap_or(8) as f32;
            let btn_w = (max_label_chars * font_size * 0.65 + 16.0).max(80.0);
            let tb_w = btn_w + pad * 2.0;
            let tb_h = row_h * count + row_gap * (count - 1.0) + pad * 2.0;

            // Screen size in physical pixels (same space as the mouse hook).
            let (screen_w_phys, screen_h_phys) = crate::platform::get_primary_monitor_size();

            // egui's ViewportCommand::OuterPosition works in LOGICAL points, but
            // the mouse hook gives us PHYSICAL pixels. On a 150% scaled display a
            // logical coordinate of 1000 maps to 1500 physical pixels, so we must
            // divide physical pixel coordinates by pixels_per_point before handing
            // them to OuterPosition. Otherwise the window lands far from the mouse.
            let ppp = ctx.input(|i| i.pixels_per_point()).max(0.0001);

            let offset_x = 8.0; // physical px
            let offset_y = 14.0; // physical px

            // Target physical-pixel position for the window's top-left.
            let x_phys = (self.toolbar.pos.0 as f32 + offset_x)
                .min(screen_w_phys as f32 - tb_w * ppp - 4.0)
                .max(4.0);
            let y_phys = (self.toolbar.pos.1 as f32 + offset_y)
                .min(screen_h_phys as f32 - tb_h * ppp - 4.0)
                .max(4.0);

            // Convert to logical points for egui.
            let x_pos = x_phys / ppp;
            let y_pos = y_phys / ppp;

            // One-shot diagnostic so we can see the math at runtime.
            if std::env::var_os("IntelliBoard_DIAG_TOOLBAR").is_some() {
                log::info!(
                    "[diag] toolbar anchor phys=({}, {}) screen_phys=({}, {}) ppp={} -> window logical=({:.0}, {:.0}) size=({:.0}x{:.0})",
                    self.toolbar.pos.0, self.toolbar.pos.1,
                    screen_w_phys, screen_h_phys, ppp,
                    x_pos, y_pos, tb_w, tb_h
                );
            }

            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([x_pos, y_pos].into()));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([tb_w, tb_h].into()));

            // Record the window rect in PHYSICAL pixels so the keep-zone test
            // (which compares against get_cursor_pos physical pixels) matches.
            self.toolbar.rect = Some(egui::Rect::from_min_size(
                egui::pos2(x_phys, y_phys),
                egui::vec2(tb_w * ppp, tb_h * ppp),
            ));

            let frame = egui::Frame::default()
                .fill(crate::ui::theme::SURFACE_3)
                .stroke(egui::Stroke::new(STROKE_HAIRLINE, BORDER))
                .rounding(egui::Rounding::same(RADIUS_MD))
                .inner_margin(egui::Margin::same(pad));

            egui::CentralPanel::default()
                .frame(frame)
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = row_gap;
                    ui.vertical(|ui| {
                        for (id, label) in self.toolbar.actions.clone() {
                            // Each row: full-width SelectableLabel with left
                            // alignment (SelectableLabel is left-aligned by
                            // default when given the full available width).
                            let resp = ui.add_sized(
                                [btn_w, row_h],
                                egui::SelectableLabel::new(
                                    false,
                                    egui::RichText::new(&label)
                                        .color(TEXT_COLOR)
                                        .size(font_size),
                                ),
                            );

                            if resp.clicked() {
                                info!("Toolbar button clicked: {}", id);
                                let trigger_text = self.toolbar.text.clone();
                                if let Some(action) = crate::core::actions::Action::from_name(&id) {
                                    // Ensure the clipboard holds the selected text so the
                                    // action handler reads exactly what the user selected.
                                    let _ = self.app_tx.try_send(AppEvent::SetClipboard(trigger_text));
                                    let _ = self.app_tx.try_send(AppEvent::TriggerAction(action));
                                }
                                self.toolbar.visible = false;
                            }
                        }
                    });
                });

            // Escape closes the toolbar
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.toolbar.visible = false;
            }

            ctx.request_repaint();
            return;
        }

        // During WAITING ONLY, show small indicator bar at bottom
        // Once streaming starts, switch to the full result window
        let is_waiting = self.state == AppState::Waiting;
        
        if is_waiting {
            // Show small processing indicator bar at bottom-center of screen.
            // Uses the token-based indicator frame: SURFACE_3 fill, accent ring,
            // rounded corners, consistent padding. Previously a flat black rect
            // with raw neon and sharp 4px corners — felt cheap.
            let bar_width = BAR_W;
            let bar_height = BAR_H;
            
            // Get primary monitor size (fallback to common resolution)
            let (screen_width, screen_height) = ctx.input(|i| {
                i.viewport().monitor_size.map(|s| (s.x, s.y)).unwrap_or((1920.0, 1080.0))
            });
            
            let x_pos = (screen_width - bar_width) / 2.0;
            let y_pos = screen_height - bar_height - 60.0; // ~2cm (60px) from bottom
            
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([x_pos, y_pos].into()));
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([bar_width, bar_height].into()));
            
            // Token-based indicator frame (replaces ad-hoc Frame::default)
            let frame = crate::ui::theme::indicator_frame();
            
            egui::CentralPanel::default()
                .frame(frame)
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.x = SPACE_4;
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
                                .color(NEON_CYAN)
                                .size(TEXT_MD)
                                .monospace()
                        );
                        
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Stop button — sized control, soft danger text.
                            if ui.add_sized(
                                [CTRL_H_SM, CTRL_H_SM],
                                egui::Button::new(
                                    egui::RichText::new("✕")
                                        .color(DANGER_TEXT)
                                        .size(TEXT_MD)
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::new(STROKE_HAIRLINE, BORDER))
                                .rounding(egui::Rounding::same(RADIUS_MD)),
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
            // Shrink to 1×1 instead of moving far off-screen, so next time the
            // toolbar or result window positions itself the OS doesn't clamp the
            // window from (10000,10000) to a random screen edge before our
            // set_outer_position takes hold.
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([1.0, 1.0].into()));
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |_ui| {});
            return;
        }

        // Click-outside-to-close: detect a left-button click (press that was
        // down last frame and is released this frame) whose release point lies
        // outside the result window. egui only sees clicks inside its own
        // window, so we poll the global OS mouse state and compare against the
        // window's outer rect in screen coordinates.
        //
        // Only active for the terminal result states (not while Waiting —
        // that shows the small status bar, handled above — and not while the
        // selection toolbar is up, which has its own leave-to-hide logic).
        let down_now = crate::platform::is_left_mouse_down();
        let is_click = self.was_left_mouse_down && !down_now;
        self.was_left_mouse_down = down_now;
        if is_click {
            let inside_result = ctx.input(|i| i.viewport().outer_rect)
                .map(|r| {
                    if let Some((mx, my)) = crate::platform::get_cursor_pos() {
                        // outer_rect is in physical pixels, same as the cursor.
                        r.contains(egui::pos2(mx as f32, my as f32))
                    } else {
                        // Can't read cursor — assume inside to avoid a
                        // spurious close on the very first frame.
                        true
                    }
                })
                .unwrap_or(true);
            if !inside_result {
                info!("Click outside result window detected — closing result window");
                self.visible = false;
                self.state = AppState::Idle;
            }
        }

        // Result window - show full size popup for Finished/Error/Incomplete.
        // Move window to right side, proper size.
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition([RESULT_X, RESULT_Y].into()));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize([RESULT_W, RESULT_H].into()));
        
        // Token-based popup frame: rounded, soft accent ring (not raw neon),
        // generous padding, elevated surface (SURFACE_3). Previously this was
        // sharp 0px corners with a raw 2px cyan border on flat black — that read
        // as low-quality. The rounded accent ring + elevated fill gives depth.
        let frame = crate::ui::theme::popup_frame();

        egui::CentralPanel::default()
            .frame(frame)
            .show(ctx, |ui| {
                let available_height = ui.available_height();
                let input_height = available_height * 0.34;
                let output_height = available_height - input_height - SPACE_4 * 2.0 - 1.0;

                // Input Area (top portion)
                ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), input_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                    // Header bar: full-width row holding the status indicator on the
                    // left and the close button on the right. This is a dedicated row
                    // so it can never overlap the input text below it (was the cause
                    // of the status pill sitting on top of the selected text).
                    ui.horizontal(|ui| {
                        // Status Indicator — status-tinted label (left).
                        match self.state {
                            AppState::Waiting => {
                                ui.label(egui::RichText::new("待機").size(TEXT_MD).color(NEON_CYAN));
                            }
                            AppState::Streaming => {
                                ui.label(egui::RichText::new("受信").size(TEXT_MD).color(NEON_CYAN));
                            }
                            AppState::Finished => {
                                ui.label(egui::RichText::new("终章").size(TEXT_MD).color(SUCCESS));
                            }
                            AppState::Incomplete => {
                                ui.label(egui::RichText::new("中断").size(TEXT_MD).color(WARN));
                            }
                            AppState::Error => {
                                ui.label(egui::RichText::new("エラー").size(TEXT_MD).color(NEON_PINK));
                            }
                            AppState::Idle => {}
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            // Close button — sized control, danger-tinted, rounded.
                            if ui.add_sized(
                                [CTRL_H_SM, CTRL_H_SM],
                                egui::Button::new(egui::RichText::new("✕").color(NEON_PINK).size(TEXT_MD))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::new(STROKE_HAIRLINE, NEON_PINK.linear_multiply(0.6)))
                                    .rounding(egui::Rounding::same(RADIUS_MD)),
                            ).clicked() {
                                info!("User clicked close button");
                                let _ = self.app_tx.try_send(AppEvent::Cancel);
                                self.visible = false;
                                self.state = AppState::Idle;
                            }
                        });
                    });

                    ui.add_space(SPACE_2);

                    // Input text content — full width, below the header bar.
                    if self.state == AppState::Waiting {
                        // Hide input text during processing, show placeholder.
                        ui.label(
                            egui::RichText::new("INPUT LOCKED // 入力ロック中")
                                .color(TEXT_MUTED)
                                .size(TEXT_SM)
                                .monospace()
                        );
                    } else {
                        egui::ScrollArea::vertical()
                            .id_source("input_scroll")
                            .max_height((input_height - 40.0).max(10.0))
                            .show(ui, |ui| {
                                let response = ui.add(
                                    egui::TextEdit::multiline(&mut self.original_text)
                                        .desired_width(ui.available_width())
                                        .font(egui::FontId::proportional(TEXT_BASE))
                                        .text_color(TEXT_COLOR)
                                        .frame(false)
                                );

                                // Handle Enter to submit (Shift+Enter for newline)
                                if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift) {
                                    if self.original_text.ends_with('\n') {
                                        self.original_text.pop();
                                    }
                                    
                                    info!("User submitted query: {}", self.original_text);
                                    self.text.clear();
                                    self.displayed_text.clear();
                                    self.state = AppState::Waiting;
                                    
                                    let _ = self.app_tx.try_send(AppEvent::UserQuery(self.original_text.clone()));
                                }
                            });
                    }
                });
            
                ui.add_space(SPACE_4);
                // Soft hairline divider instead of a heavy separator.
                let (div_rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
                ui.painter().line_segment(
                    [div_rect.left_center(), div_rect.right_center()],
                    egui::Stroke::new(STROKE_HAIRLINE, BORDER),
                );
                ui.add_space(SPACE_4);

                // Output Area (remaining height)
                ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), output_height), egui::Layout::top_down(egui::Align::Min), |ui| {
                    egui::ScrollArea::vertical()
                        .id_source("output_scroll")
                        .show(ui, |ui| {
                            if self.state == AppState::Waiting {
                                ctx.request_repaint();
                                ui.vertical_centered(|ui| {
                                    ui.add_space(SPACE_4);
                                    ui.label(egui::RichText::new("SYSTEM PROCESSING // 解析中").monospace().color(NEON_CYAN).size(TEXT_BASE)); 
                                    ui.add_space(SPACE_3);
                                    
                                    // Indeterminate scanner bar — rounded track + rounded moving
                                    // segment, replacing the old sharp black/neon rectangles.
                                    let time = ctx.input(|i| i.time);
                                    let progress = (time * 1.5 % 1.0) as f32;
                                    
                                    let bar_height = 16.0;
                                    let width = ui.available_width() * 0.9;
                                    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, bar_height), egui::Sense::hover());
                                    
                                    // Track (elevated surface, rounded)
                                    ui.painter().rect_filled(rect, egui::Rounding::same(RADIUS_MD), SURFACE_2);
                                    
                                    // Moving scanner segment (rounded)
                                    let bar_width = width * 0.3;
                                    let x_pos = rect.left() + (width - bar_width) * progress;
                                    let bar_rect = egui::Rect::from_min_size(
                                        egui::pos2(x_pos, rect.top()),
                                        egui::vec2(bar_width, bar_height)
                                    );
                                    ui.painter().rect_filled(bar_rect, egui::Rounding::same(RADIUS_MD), NEON_PINK);
                                    ui.add_space(SPACE_4);
                                });
                            } else {
                                ui.label(
                                    egui::RichText::new(&self.displayed_text)
                                        .color(TEXT_COLOR)
                                        .size(TEXT_BASE)
                                );
                            }
                        });
                });
        
            
                // Footer (optional, maybe just space)
                ui.add_space(SPACE_2);
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

// ---------------------------------------------------------------------------
// Platform helpers for opening logs / running custom commands.
// ---------------------------------------------------------------------------

/// Look up an executable by name in `$PATH`. Returns its full path if found.
fn which(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let candidate = dir.join(name);
            let candidate = if cfg!(windows) {
                // On Windows, also try with .exe extension.
                if candidate.extension().is_none() {
                    let with_exe = candidate.with_extension("exe");
                    if with_exe.is_file() {
                        return Some(with_exe);
                    }
                }
                candidate
            } else {
                candidate
            };
            if candidate.is_file() {
                Some(candidate)
            } else {
                None
            }
        })
    })
}

/// Find an available terminal emulator on Linux/macOS.
/// Returns the binary name if one is found in `$PATH`.
fn find_terminal_emulator() -> Option<&'static str> {
    const TERMINALS: &[&str] = &[
        "x-terminal-emulator", // Debian / Ubuntu
        "gnome-terminal",
        "konsole",
        "xfce4-terminal",
        "alacritty",
        "kitty",
        "terminator",
        "tilix",
        "xterm", // ubiquitous fallback
        "wezterm",
        "foot", // Wayland
        // macOS uses Terminal.app via `open`, handled separately.
    ];

    for term in TERMINALS {
        if which(term).is_some() {
            return Some(term);
        }
    }
    None
}
