//! Clipboard change listener.
//!
//! On Windows we use the dedicated `clipboard-master` crate which hooks the
//! Win32 clipboard event source. On Linux/macOS there is no universally
//! available notification mechanism (X11 has no clipboard change signal;
//! Wayland is even more restrictive), so we fall back to a polling loop that
//! hashes the clipboard contents every ~300 ms and emits an event when the
//! hash changes.

use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Windows — event-driven via clipboard-master
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use clipboard_master::{Master, CallbackResult, ClipboardHandler};
    use std::io;

    pub struct Listener {
        sender: mpsc::Sender<()>,
    }

    impl Listener {
        pub fn new(sender: mpsc::Sender<()>) -> Self {
            Self { sender }
        }
    }

    impl ClipboardHandler for Listener {
        fn on_clipboard_change(&mut self) -> CallbackResult {
            // Avoid blocking in the clipboard callback thread.
            let _ = self.sender.try_send(());
            if std::env::var_os("IntelliBoard_DIAG_CLIPBOARD").is_some() {
                log::debug!("[diag] clipboard_master reported clipboard change");
            }
            CallbackResult::Next
        }

        fn on_clipboard_error(&mut self, error: io::Error) -> CallbackResult {
            log::error!("Clipboard listener error: {}", error);
            CallbackResult::Next
        }
    }

    pub fn start_listener(sender: mpsc::Sender<()>) {
        std::thread::spawn(move || {
            let _ = Master::new(Listener::new(sender))
                .expect("Failed to create clipboard listener")
                .run();
        });
    }
}

// ---------------------------------------------------------------------------
// Linux / macOS — polling fallback
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
mod poll_impl {
    use super::*;
    use arboard::Clipboard;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::sync::Mutex;
    use std::time::Duration;

    /// Polling interval. X11 does not provide clipboard-change signals, so we
    /// hash the clipboard contents on a timer. 300 ms is fast enough to feel
    /// responsive while keeping CPU usage negligible.
    const POLL_INTERVAL_MS: u64 = 300;

    /// Compute a stable hash of the current clipboard contents. Image data is
    /// hashed by its dimensions + raw bytes so we do not clone the full buffer
    /// more than necessary.
    fn clipboard_hash(clipboard: &mut Clipboard) -> Option<u64> {
        // Prefer text (the common case). If text fails, try image.
        match clipboard.get_text() {
            Ok(text) => {
                let mut hasher = DefaultHasher::new();
                text.hash(&mut hasher);
                Some(hasher.finish())
            }
            Err(_) => match clipboard.get_image() {
                Ok(img) => {
                    let mut hasher = DefaultHasher::new();
                    img.width.hash(&mut hasher);
                    img.height.hash(&mut hasher);
                    img.bytes.as_ref().hash(&mut hasher);
                    Some(hasher.finish())
                }
                Err(_) => None,
            },
        }
    }

    pub fn start_listener(sender: mpsc::Sender<()>) {
        std::thread::spawn(move || {
            // Small retry loop so a transient arboard init failure (e.g. no X11
            // connection yet during early boot) does not permanently kill the
            // listener.
            let clipboard: Clipboard = loop {
                match Clipboard::new() {
                    Ok(cb) => break cb,
                    Err(e) => {
                        log::warn!(
                            "Clipboard init failed ({}); retrying in 1s",
                            e
                        );
                        std::thread::sleep(Duration::from_secs(1));
                    }
                }
            };

            // arboard::Clipboard is not Send across some backends; keep it
            // behind a Mutex on the polling thread.
            let clipboard = Mutex::new(clipboard);

            let mut last_hash = {
                let mut cb = clipboard.lock().unwrap();
                clipboard_hash(&mut cb).unwrap_or(0)
            };

            log::info!(
                "Clipboard polling listener started (interval={}ms)",
                POLL_INTERVAL_MS
            );

            loop {
                std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

                let current_hash = {
                    let mut cb = match clipboard.lock() {
                        Ok(g) => g,
                        Err(_) => continue,
                    };
                    clipboard_hash(&mut cb)
                };

                match current_hash {
                    Some(h) if h != last_hash => {
                        last_hash = h;
                        if std::env::var_os("IntelliBoard_DIAG_CLIPBOARD").is_some() {
                            log::debug!("[diag] clipboard poll detected change");
                        }
                        let _ = sender.try_send(());
                    }
                    Some(_) => { /* unchanged */ }
                    None => { /* clipboard empty / unreadable — reset hash */ }
                }
            }
        });
    }
}

// ---------------------------------------------------------------------------
// Public re-export
// ---------------------------------------------------------------------------

#[cfg(windows)]
pub use windows_impl::start_listener;

#[cfg(not(windows))]
pub use poll_impl::start_listener;
