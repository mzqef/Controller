//! Application startup helpers for cleaner main.rs organization

use crate::core::config::HotkeysConfig;
use crate::core::events::AppEvent;
use std::sync::{RwLock, Mutex};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::mpsc;

#[cfg(windows)]
use crate::platform::LowLevelKeyboardHook;

#[cfg(windows)]
use crate::platform::init_hotkey_system as platform_init_hotkey;

#[cfg(not(windows))]
use crate::platform::init_hotkey_system as platform_init_hotkey;

/// Platform-agnostic hotkey system handle that supports reinstallation
pub trait HotkeySystem: Send + Sync {
    /// Reinstall the hotkey system with updated configuration
    fn reinstall(&self) -> Result<(), String>;
}

#[cfg(windows)]
impl HotkeySystem for LowLevelKeyboardHook {
    fn reinstall(&self) -> Result<(), String> {
        LowLevelKeyboardHook::reinstall(self)
    }
}

#[cfg(not(windows))]
impl HotkeySystem for () {
    fn reinstall(&self) -> Result<(), String> {
        // Unix rdev::grab reads from shared_hotkeys on each event,
        // so no reinstall is needed - config changes are picked up automatically
        Ok(())
    }
}

/// Initialize platform-specific global key input listener.
/// 
/// On Windows: Uses SetWindowsHookEx with WH_KEYBOARD_LL (non-consuming).
/// On other platforms: Uses rdev::grab to intercept matched hotkeys.
/// 
/// The hotkeys_config is wrapped in Arc<RwLock> to support hot-reload.
/// The last_copy_time is shared with the main loop to gate hotkey blocking.
/// Hotkeys are only blocked (consumed) when a copy occurred within 2 seconds.
/// Returns a handle that can be used to reinstall the hook after config changes.
pub fn init_hotkey_system(
    tx: mpsc::Sender<AppEvent>,
    shared_hotkeys: Arc<RwLock<HotkeysConfig>>,
    last_copy_time: Arc<Mutex<Instant>>,
) -> Result<Arc<dyn HotkeySystem>, String> {
    #[cfg(windows)]
    {
        let handle = platform_init_hotkey(tx, shared_hotkeys, last_copy_time)?;
        Ok(handle as Arc<dyn HotkeySystem>)
    }
    #[cfg(not(windows))]
    {
        // Unix platforms don't use last_copy_time gating (yet)
        let _ = last_copy_time;
        let handle = platform_init_hotkey(tx, shared_hotkeys)?;
        Ok(handle as Arc<dyn HotkeySystem>)
    }
}
