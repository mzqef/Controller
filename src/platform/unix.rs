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

/// Focus a window by its title using `wmctrl -a <title>` on X11.
/// Returns `false` if wmctrl is unavailable or the window was not found.
/// On Wayland, wmctrl is typically non-functional — returns `false`.
pub fn focus_window_by_title(title: &str) -> bool {
    let result = std::process::Command::new("wmctrl")
        .args(&["-a", title])
        .status();
    match result {
        Ok(status) if status.success() => {
            debug!("Focused existing window via wmctrl: {}", title);
            true
        }
        _ => {
            // Fallback: try xdotool search by name
            let result = std::process::Command::new("xdotool")
                .args(&["search", "--name", title, "windowactivate"])
                .status();
            matches!(result, Ok(s) if s.success())
        }
    }
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
    // On X11 we can try xdotool / xsel for cursor; on Wayland this is restricted.
    // Prefer mouse cursor so the toolbar appears near the user's pointer.
    if let Some((x, y)) = get_cursor_pos() {
        return (x + 12, y + 14);
    }
    (300, 300)
}

/// Selection-detection mouse hook on Unix using rdev::grab.
///
/// rdev::grab can capture mouse events on X11 (requires `/dev/uinput` write
/// access, or on some setups the X11 record extension). When a drag selection
/// ends (left button released after movement), we emit
/// `AppEvent::SelectionAt { x, y }`.
///
/// On Wayland or systems without grab permission, this will log an error and
/// return `Ok` with a dummy handle — the toolbar can still be triggered via
/// the clipboard-copy path (which is the primary trigger today).
pub fn init_selection_mouse_hook(
    tx: tokio::sync::mpsc::Sender<crate::core::events::AppEvent>,
) -> Result<Arc<()>, String> {
    use std::sync::atomic::AtomicBool;
    static DRAGGING: AtomicBool = AtomicBool::new(false);
    static LAST_X: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
    static LAST_Y: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);

    std::thread::spawn(move || {
        let callback = move |event: Event| -> Option<Event> {
            match event.event_type {
                EventType::ButtonPress(button) => {
                    if matches!(button, rdev::MouseButton::Left) {
                        DRAGGING.store(false, Ordering::Relaxed);
                    }
                    Some(event)
                }
                EventType::MouseMove { x, y } => {
                    // Any movement with left button (tracked separately) marks a drag.
                    // rdev doesn't give us simultaneous button state on move events
                    // on all platforms, so we just record the latest position.
                    LAST_X.store(x as i32, Ordering::Relaxed);
                    LAST_Y.store(y as i32, Ordering::Relaxed);
                    DRAGGING.store(true, Ordering::Relaxed);
                    Some(event)
                }
                EventType::ButtonRelease(button) => {
                    if matches!(button, rdev::MouseButton::Left) {
                        let was_drag = DRAGGING.swap(false, Ordering::Relaxed);
                        if was_drag {
                            let x = LAST_X.load(Ordering::Relaxed);
                            let y = LAST_Y.load(Ordering::Relaxed);
                            let _ = tx.try_send(crate::core::events::AppEvent::SelectionAt {
                                x,
                                y,
                            });
                        }
                    }
                    Some(event)
                }
                _ => Some(event),
            }
        };

        if let Err(e) = rdev::grab(callback) {
            error!("Failed to grab mouse events for selection detection: {:?}", e);
        }
    });

    Ok(Arc::new(()))
}

/// Synthesize Ctrl+C for grabbing a selection on Linux/macOS.
/// Uses `xdotool key ctrl+c` on X11. On Wayland this is typically not
/// possible from another process, so we fall back to a no-op (the user
/// must already have copied the text for the toolbar to appear).
pub fn send_copy_shortcut() {
    // xdotool is the standard X11 automation tool.
    let result = std::process::Command::new("xdotool")
        .args(&["key", "ctrl+c"])
        .spawn();
    if let Err(e) = result {
        log::debug!("send_copy_shortcut: xdotool not available ({})", e);
    }
}

/// Global cursor position via `xdotool getmouselocation` on X11.
/// Returns `None` on Wayland or when xdotool is unavailable.
pub fn get_cursor_pos() -> Option<(i32, i32)> {
    let output = std::process::Command::new("xdotool")
        .args(&["getmouselocation", "--shell"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut x: Option<i32> = None;
    let mut y: Option<i32> = None;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("X=") {
            x = rest.trim().parse().ok();
        } else if let Some(rest) = line.strip_prefix("Y=") {
            y = rest.trim().parse().ok();
        }
    }
    match (x, y) {
        (Some(x), Some(y)) => Some((x, y)),
        _ => None,
    }
}

/// Global left-mouse-button state via `xdotool getmouselocation --shell`.
/// Parses the `BUTTON` field; returns `false` when xdotool is unavailable
/// (e.g. Wayland), which disables "click outside to close" for the result
/// window — the window can still be closed via its close button or Escape.
pub fn is_left_mouse_down() -> bool {
    let output = match std::process::Command::new("xdotool")
        .args(&["getmouselocation", "--shell"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return false,
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    // xdotool --shell emits BUTTON=<n> only when a button is held.
    // Button 1 = left. Absence of BUTTON or BUTTON=0 means no button held.
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("BUTTON=") {
            let val: i32 = rest.trim().parse().unwrap_or(0);
            return val == 1;
        }
    }
    false
}

/// Primary monitor size via `xdotool getdisplaygeometry` on X11.
/// Returns a sensible fallback (1920×1080) on Wayland or failure.
pub fn get_primary_monitor_size() -> (i32, i32) {
    let output = match std::process::Command::new("xdotool")
        .args(&["getdisplaygeometry"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return (1920, 1080),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let parts: Vec<&str> = stdout.split_whitespace().collect();
    if parts.len() >= 2 {
        let w = parts[0].parse().unwrap_or(1920);
        let h = parts[1].parse().unwrap_or(1080);
        return (w, h);
    }
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
