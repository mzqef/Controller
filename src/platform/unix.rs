//! Unix/Linux/macOS platform functionality using rdev::grab

use crate::core::actions::Action;
use crate::core::config::HotkeysConfig;
use crate::core::events::AppEvent;
use log::{debug, error, warn};
use rdev::{Event, EventType, Key};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;
use tokio::sync::mpsc;

/// Stub for Unix/macOS - focus by window title not implemented
pub fn focus_window_by_title(title: &str) -> bool {
    warn!("focus_window_by_title not implemented on this platform: {}", title);
    false
}

/// Stub for Unix/macOS - IME composition detection not implemented
pub fn is_ime_composing() -> bool {
    // On Unix/macOS, we don't have a simple way to detect IME composition
    // The debounce logic in clipboard.rs will still help, just without IME-specific timing
    false
}

/// Stub for Unix/macOS - caret/cursor position for selection popup.
/// Returns a fixed position; full implementation would use X11/Wayland APIs.
pub fn get_selection_popup_pos() -> (i32, i32) {
    (300, 300)
}

/// Stub for Unix/macOS - selection-detection mouse hook.
/// rdev::grab already captures mouse events on some platforms, but wiring the
/// drag-selection detection here is left as a TODO. The toolbar can still be
/// triggered via the hotkey path.
pub fn init_selection_mouse_hook(_tx: tokio::sync::mpsc::Sender<crate::core::events::AppEvent>) -> Result<Arc<()>, String> {
    warn!("init_selection_mouse_hook not implemented on Unix/macOS");
    Ok(Arc::new(()))
}

/// Stub for Unix/macOS - synthesized Ctrl+C for grabbing a selection.
/// A real implementation would use xdotool / CGEvent. No-op here.
pub fn send_copy_shortcut() {}

/// Stub for Unix/macOS - global cursor position. Returns `None`; full
/// implementation would query X11 / Wayland.
pub fn get_cursor_pos() -> Option<(i32, i32)> {
    None
}

/// Stub for Unix/macOS - primary monitor size. Returns a fallback; full
/// implementation would query X11 / Wayland.
pub fn get_primary_monitor_size() -> (i32, i32) {
    (1920, 1080)
}

/// Initialize platform-specific hotkey system using rdev::grab
/// 
/// Note: On Unix, the grab callback reads from shared_hotkeys on each event,
/// so config changes are picked up automatically without needing to reinstall.
pub fn init_hotkey_system(
    tx: mpsc::Sender<AppEvent>,
    shared_hotkeys: Arc<RwLock<HotkeysConfig>>,
) -> Result<Arc<()>, String> {
    // Spawn thread that uses rdev::grab to intercept and block hotkeys
    std::thread::spawn(move || {
        let mut ctrl_pressed = false;
        let diag_start = Instant::now();
        let diag_seq = Arc::new(AtomicU64::new(0));

        let callback = move |event: Event| -> Option<Event> {
            let diag_enabled = std::env::var_os("IntelliBoard_DIAG_KEYS").is_some();
            let now_ms = diag_start.elapsed().as_millis();
            let seq = diag_seq.fetch_add(1, Ordering::Relaxed) + 1;

            if diag_enabled {
                debug!(
                    "[diag #{seq} @ {now_ms}ms] rdev event: type={:?} name={:?}",
                    event.event_type, event.name
                );
            }

            match event.event_type {
                EventType::KeyPress(Key::ControlLeft) | EventType::KeyPress(Key::ControlRight) => {
                    ctrl_pressed = true;
                    Some(event) // Pass through
                }
                EventType::KeyRelease(Key::ControlLeft) | EventType::KeyRelease(Key::ControlRight) => {
                    ctrl_pressed = false;
                    Some(event) // Pass through
                }
                EventType::KeyPress(key) if ctrl_pressed => {
                    // Read current hotkeys
                    let bindings = match shared_hotkeys.read() {
                        Ok(cfg) => cfg.bindings.clone(),
                        Err(_) => return Some(event), // Pass through on error
                    };

                    let key_str = format!("{:?}", key);
                    for binding in &bindings {
                        if binding.key == key_str {
                            let action = Action::from_name(&binding.action);
                            if let Some(act) = action {
                                let _ = tx.try_send(AppEvent::TriggerAction(act));
                            }
                            // Block the event by returning None
                            debug!("Blocked hotkey: Ctrl+{}", key_str);
                            return None;
                        }
                    }

                    // Not a registered hotkey, pass through
                    Some(event)
                }
                _ => Some(event), // Pass through all other events
            }
        };

        // Use grab instead of listen - this allows blocking events
        if let Err(e) = rdev::grab(callback) {
            error!("Failed to grab keyboard events: {:?}", e);
        }
    });

    // Return a dummy handle since we don't need to manage the grab thread
    Ok(Arc::new(()))
}
