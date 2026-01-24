//! Windows-specific platform functionality
//! 
//! Uses SetWindowsHookEx(WH_KEYBOARD_LL) for hotkey detection without consuming keys.
//! This allows system shortcuts like Ctrl+W to pass through to other applications.

use crate::core::actions::Action;
use crate::core::config::HotkeysConfig;
use crate::core::events::AppEvent;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, RwLock, Mutex};
use std::cell::RefCell;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use winapi::shared::minwindef::{LPARAM, WPARAM, LRESULT};
use winapi::um::winuser::{
    FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
    SetWindowsHookExW, UnhookWindowsHookEx, CallNextHookEx, GetMessageW,
    PostThreadMessageW, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN, WM_QUIT,
    KBDLLHOOKSTRUCT, MSG, GetAsyncKeyState, VK_CONTROL, VK_SHIFT, VK_MENU,
    VK_F1, VK_F2, VK_F3, VK_F4, VK_F5, VK_F6, VK_F7, VK_F8, VK_F9, VK_F10, VK_F11, VK_F12,
};
use winapi::um::libloaderapi::GetModuleHandleW;

thread_local! {
    static HOOK_STATE: RefCell<Option<HookCallbackState>> = RefCell::new(None);
}

/// State needed by the hook callback (must be thread-local because callback is static)
struct HookCallbackState {
    tx: mpsc::Sender<AppEvent>,
    hotkey_map: HashMap<(u32, u32), String>, // (modifiers_mask, vk_code) -> action_name
    last_copy_time: Arc<Mutex<Instant>>, // Shared copy timestamp for gating
}

/// Duration threshold: only block hotkeys if copy occurred within this window
const COPY_GATE_DURATION: Duration = Duration::from_secs(2);

/// Modifier key flags (matching our usage pattern)
const MOD_CTRL: u32 = 0x0001;
const MOD_SHIFT: u32 = 0x0002;
const MOD_ALT: u32 = 0x0004;

/// Low-level keyboard hook manager
/// 
/// Unlike RegisterHotKey, this approach:
/// 1. Inspects keys without consuming them (Ctrl+W works everywhere)
/// 2. Allows dynamic hotkey updates via reinstall_hook()
/// 3. Can detect any key combination, not just system hotkeys
pub struct LowLevelKeyboardHook {
    tx: mpsc::Sender<AppEvent>,
    shared_hotkeys: Arc<RwLock<HotkeysConfig>>,
    last_copy_time: Arc<Mutex<Instant>>, // Shared with main for copy-gated blocking
    hook_handle: Arc<parking_lot::Mutex<Option<usize>>>, // Store as usize for Send/Sync
    thread_id: Arc<AtomicU32>,
    stop_flag: Arc<AtomicBool>,
    thread_handle: Arc<parking_lot::Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl LowLevelKeyboardHook {
    pub fn new(
        tx: mpsc::Sender<AppEvent>,
        shared_hotkeys: Arc<RwLock<HotkeysConfig>>,
        last_copy_time: Arc<Mutex<Instant>>,
    ) -> Self {
        Self {
            tx,
            shared_hotkeys,
            last_copy_time,
            hook_handle: Arc::new(parking_lot::Mutex::new(None)),
            thread_id: Arc::new(AtomicU32::new(0)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            thread_handle: Arc::new(parking_lot::Mutex::new(None)),
        }
    }

    /// Build the hotkey lookup map from current config
    fn build_hotkey_map(&self) -> HashMap<(u32, u32), String> {
        let mut map = HashMap::new();
        
        let bindings = match self.shared_hotkeys.read() {
            Ok(cfg) => cfg.bindings.clone(),
            Err(e) => {
                error!("Failed to read hotkeys config: {}", e);
                return map;
            }
        };

        for binding in &bindings {
            if let Some(vk_code) = key_str_to_vk(&binding.key) {
                // Parse modifiers from binding (default to Ctrl only for backwards compat)
                let modifiers = parse_modifiers(&binding.modifiers.as_deref().unwrap_or("Ctrl"));
                map.insert((modifiers, vk_code), binding.action.clone());
                info!("Mapped hotkey: modifiers={:#x} vk={:#x} -> {}", modifiers, vk_code, binding.action);
            } else {
                warn!("Unknown key in hotkey config: {}", binding.key);
            }
        }

        map
    }

    /// Install the keyboard hook and start the message pump thread
    pub fn install(&self) -> Result<(), String> {
        self.stop_flag.store(false, Ordering::SeqCst);
        
        let tx = self.tx.clone();
        let hotkey_map = self.build_hotkey_map();
        let last_copy_time = self.last_copy_time.clone();
        let hook_handle = self.hook_handle.clone();
        let thread_id_storage = self.thread_id.clone();
        let stop_flag = self.stop_flag.clone();

        info!("Installing low-level keyboard hook with {} hotkeys", hotkey_map.len());

        let handle = std::thread::spawn(move || {
            // Store current thread ID for PostThreadMessage
            let current_thread_id = unsafe { winapi::um::processthreadsapi::GetCurrentThreadId() };
            thread_id_storage.store(current_thread_id, Ordering::SeqCst);

            // Initialize thread-local state for hook callback
            HOOK_STATE.with(|state| {
                *state.borrow_mut() = Some(HookCallbackState {
                    tx: tx.clone(),
                    hotkey_map,
                    last_copy_time,
                });
            });

            // Install the hook
            let hook = unsafe {
                SetWindowsHookExW(
                    WH_KEYBOARD_LL,
                    Some(low_level_keyboard_proc),
                    GetModuleHandleW(std::ptr::null()),
                    0, // 0 = all threads (required for WH_KEYBOARD_LL)
                )
            };

            if hook.is_null() {
                error!("SetWindowsHookExW failed");
                return;
            }

            info!("Low-level keyboard hook installed successfully");
            
            // Store hook handle for later cleanup (cast to usize for Send/Sync)
            {
                let mut guard = hook_handle.lock();
                *guard = Some(hook as usize);
            }

            // Message pump - required for low-level hooks to work
            unsafe {
                let mut msg: MSG = std::mem::zeroed();
                while !stop_flag.load(Ordering::Relaxed) {
                    // GetMessageW blocks until a message arrives or WM_QUIT
                    let result = GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0);
                    if result <= 0 {
                        // 0 = WM_QUIT, -1 = error
                        break;
                    }
                    // We don't need to dispatch messages, just pump them
                }

                // Cleanup
                UnhookWindowsHookEx(hook);
                info!("Low-level keyboard hook uninstalled");
            }

            // Clear thread-local state
            HOOK_STATE.with(|state| {
                *state.borrow_mut() = None;
            });
        });

        // Store thread handle
        {
            let mut guard = self.thread_handle.lock();
            *guard = Some(handle);
        }

        // Wait briefly for hook installation
        std::thread::sleep(std::time::Duration::from_millis(50));

        Ok(())
    }

    /// Reinstall the hook with updated hotkey configuration
    /// Call this after hotkeys config is reloaded
    pub fn reinstall(&self) -> Result<(), String> {
        info!("Reinstalling keyboard hook with updated config");
        self.uninstall();
        // Brief pause to ensure cleanup completes
        std::thread::sleep(std::time::Duration::from_millis(100));
        self.install()
    }

    /// Uninstall the hook and stop the message pump thread
    pub fn uninstall(&self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        
        // Send WM_QUIT to wake up the message pump
        let thread_id = self.thread_id.load(Ordering::SeqCst);
        if thread_id != 0 {
            unsafe {
                PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
            }
        }

        // Wait for thread to finish
        if let Some(handle) = self.thread_handle.lock().take() {
            let _ = handle.join();
        }

        // Clear hook handle
        {
            let mut guard = self.hook_handle.lock();
            *guard = None;
        }
    }
}

impl Drop for LowLevelKeyboardHook {
    fn drop(&mut self) {
        self.uninstall();
    }
}

/// The actual hook callback - must be extern "system" and static
/// 
/// CRITICAL: This function must be FAST (< 300ms) or Windows will remove the hook.
/// We only check modifier state and do a hashmap lookup, then try_send to a channel.
unsafe extern "system" fn low_level_keyboard_proc(
    code: i32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    // Always call next hook first for non-zero codes
    if code < 0 {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }

    // Only process key down events
    if wparam as u32 != WM_KEYDOWN && wparam as u32 != WM_SYSKEYDOWN {
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }

    let kb_struct = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    let vk_code = kb_struct.vkCode;

    // Skip modifier keys themselves
    if vk_code == VK_CONTROL as u32 || vk_code == VK_SHIFT as u32 || vk_code == VK_MENU as u32 
       || vk_code == 0xA0 || vk_code == 0xA1 || vk_code == 0xA2 || vk_code == 0xA3 
       || vk_code == 0xA4 || vk_code == 0xA5 { // L/R variants
        return unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) };
    }

    // Check current modifier state
    let ctrl_pressed = unsafe { GetAsyncKeyState(VK_CONTROL) < 0 };
    let shift_pressed = unsafe { GetAsyncKeyState(VK_SHIFT) < 0 };
    let alt_pressed = unsafe { GetAsyncKeyState(VK_MENU) < 0 };

    // Build modifier mask
    let mut modifiers: u32 = 0;
    if ctrl_pressed { modifiers |= MOD_CTRL; }
    if shift_pressed { modifiers |= MOD_SHIFT; }
    if alt_pressed { modifiers |= MOD_ALT; }

    // Look up in thread-local hotkey map and check if we should block the key
    let mut should_block = false;
    HOOK_STATE.with(|state| {
        if let Some(ref hook_state) = *state.borrow() {
            if let Some(action_name) = hook_state.hotkey_map.get(&(modifiers, vk_code)) {
                // Check if copy occurred recently (within gating window)
                let copy_is_recent = hook_state.last_copy_time
                    .lock()
                    .map(|guard| guard.elapsed() < COPY_GATE_DURATION)
                    .unwrap_or(false);
                
                if copy_is_recent {
                    debug!("Hotkey matched (copy recent): modifiers={:#x} vk={:#x} -> {}", modifiers, vk_code, action_name);
                    
                    if let Some(action) = Action::from_name(action_name) {
                        // Non-blocking send - if channel is full, we drop the event
                        let _ = hook_state.tx.try_send(AppEvent::TriggerAction(action));
                        should_block = true;
                    }
                } else {
                    debug!("Hotkey matched but no recent copy - passing through: modifiers={:#x} vk={:#x} -> {}", modifiers, vk_code, action_name);
                }
            }
        }
    });

    // Block matched hotkeys from reaching other apps ONLY if we're handling them
    // Otherwise pass through so other applications can use the key combo
    if should_block {
        return 1;
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Parse modifier string (e.g., "Ctrl+Shift") into bitmask
fn parse_modifiers(modifiers_str: &str) -> u32 {
    let mut mask = 0u32;
    let lower = modifiers_str.to_lowercase();
    
    if lower.contains("ctrl") || lower.contains("control") {
        mask |= MOD_CTRL;
    }
    if lower.contains("shift") {
        mask |= MOD_SHIFT;
    }
    if lower.contains("alt") {
        mask |= MOD_ALT;
    }
    
    // Default to Ctrl if nothing specified
    if mask == 0 {
        mask = MOD_CTRL;
    }
    
    mask
}

/// Convert key string (from config) to Windows virtual key code
fn key_str_to_vk(key_str: &str) -> Option<u32> {
    match key_str {
        // Letters
        "KeyA" => Some(0x41),
        "KeyB" => Some(0x42),
        "KeyC" => Some(0x43),
        "KeyD" => Some(0x44),
        "KeyE" => Some(0x45),
        "KeyF" => Some(0x46),
        "KeyG" => Some(0x47),
        "KeyH" => Some(0x48),
        "KeyI" => Some(0x49),
        "KeyJ" => Some(0x4A),
        "KeyK" => Some(0x4B),
        "KeyL" => Some(0x4C),
        "KeyM" => Some(0x4D),
        "KeyN" => Some(0x4E),
        "KeyO" => Some(0x4F),
        "KeyP" => Some(0x50),
        "KeyQ" => Some(0x51),
        "KeyR" => Some(0x52),
        "KeyS" => Some(0x53),
        "KeyT" => Some(0x54),
        "KeyU" => Some(0x55),
        "KeyV" => Some(0x56),
        "KeyW" => Some(0x57),
        "KeyX" => Some(0x58),
        "KeyY" => Some(0x59),
        "KeyZ" => Some(0x5A),
        
        // Function keys
        "F1" => Some(VK_F1 as u32),
        "F2" => Some(VK_F2 as u32),
        "F3" => Some(VK_F3 as u32),
        "F4" => Some(VK_F4 as u32),
        "F5" => Some(VK_F5 as u32),
        "F6" => Some(VK_F6 as u32),
        "F7" => Some(VK_F7 as u32),
        "F8" => Some(VK_F8 as u32),
        "F9" => Some(VK_F9 as u32),
        "F10" => Some(VK_F10 as u32),
        "F11" => Some(VK_F11 as u32),
        "F12" => Some(VK_F12 as u32),
        
        // Numbers
        "Num0" | "Digit0" => Some(0x30),
        "Num1" | "Digit1" => Some(0x31),
        "Num2" | "Digit2" => Some(0x32),
        "Num3" | "Digit3" => Some(0x33),
        "Num4" | "Digit4" => Some(0x34),
        "Num5" | "Digit5" => Some(0x35),
        "Num6" | "Digit6" => Some(0x36),
        "Num7" | "Digit7" => Some(0x37),
        "Num8" | "Digit8" => Some(0x38),
        "Num9" | "Digit9" => Some(0x39),
        
        _ => {
            warn!("Unknown key string: {}", key_str);
            None
        }
    }
}

/// Check if an IME is currently composing text
/// 
/// Returns false for now - full IME detection requires additional winapi features.
/// The debounce logic in clipboard.rs will still help with IME compatibility
/// through time+content stability checks.
pub fn is_ime_composing() -> bool {
    // TODO: Implement full IME composition detection using ImmGetContext/ImmGetCompositionStringW
    // This requires winapi imm feature which has compatibility issues in winapi 0.3
    // For now, rely on time+content debouncing in clipboard.rs
    false
}

/// Focus a window by its title (for single-instance child windows)
pub fn focus_window_by_title(title: &str) -> bool {
    let title_wide: Vec<u16> = OsStr::new(title)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title_wide.as_ptr());
        if !hwnd.is_null() {
            ShowWindow(hwnd, SW_RESTORE);
            SetForegroundWindow(hwnd);
            debug!("Focused existing window: {}", title);
            return true;
        }
    }

    false
}

/// Initialize platform-specific hotkey system using low-level keyboard hook
/// 
/// This replaces the old RegisterHotKey approach:
/// - Keys are NOT consumed (Ctrl+W works in browsers, etc.) when no recent copy
/// - Blocks hotkeys ONLY when copy occurred within the last 2 seconds
/// - Supports dynamic reconfiguration via reinstall()
/// - Works with any modifier combination
pub fn init_hotkey_system(
    tx: mpsc::Sender<AppEvent>,
    shared_hotkeys: Arc<RwLock<HotkeysConfig>>,
    last_copy_time: Arc<Mutex<Instant>>,
) -> Result<Arc<LowLevelKeyboardHook>, String> {
    let hook = Arc::new(LowLevelKeyboardHook::new(tx, shared_hotkeys, last_copy_time));
    hook.install()?;
    Ok(hook)
}
