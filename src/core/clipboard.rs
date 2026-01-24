use arboard::Clipboard;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;

pub struct ClipboardManager {
    clipboard: Mutex<Clipboard>,
    last_programmatic_text: Mutex<Option<String>>,
    programmatic_until_ms: AtomicU64,
}

impl ClipboardManager {
    pub fn new() -> Result<Self> {
        let clipboard = Clipboard::new().map_err(|e| anyhow::anyhow!("Failed to init clipboard: {}", e))?;
        Ok(Self {
            clipboard: Mutex::new(clipboard),
            last_programmatic_text: Mutex::new(None),
            programmatic_until_ms: AtomicU64::new(0),
        })
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn get_text(&self) -> Result<String> {
        let mut cb = self.clipboard.lock().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
        cb.get_text().map_err(|e| anyhow::anyhow!("Failed to get text: {}", e))
    }

    pub fn set_text(&self, text: &str) -> Result<()> {
        let mut cb = self.clipboard.lock().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
        cb.set_text(text.to_string()).map_err(|e| anyhow::anyhow!("Failed to set text: {}", e))
    }

    /// Set clipboard text from the app itself.
    /// This marks the next matching clipboard-change event as programmatic so
    /// it won't be recorded into short-term memory.
    pub fn set_text_programmatic(&self, text: &str) -> Result<()> {
        {
            let mut guard = self
                .last_programmatic_text
                .lock()
                .map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
            *guard = Some(text.to_string());
        }
        self.programmatic_until_ms
            .store(Self::now_ms().saturating_add(1500), Ordering::Relaxed);

        if let Err(e) = self.set_text(text) {
            // Clear marker if the write failed.
            let mut guard = self
                .last_programmatic_text
                .lock()
                .map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
            *guard = None;
            self.programmatic_until_ms.store(0, Ordering::Relaxed);
            return Err(e);
        }

        Ok(())
    }

    /// Returns true if this clipboard text should be ignored (it matches a recent
    /// programmatic write), and clears the suppression marker.
    pub fn should_ignore_clipboard_text(&self, text: &str) -> bool {
        let until = self.programmatic_until_ms.load(Ordering::Relaxed);
        if until == 0 {
            return false;
        }

        let now = Self::now_ms();
        if now > until {
            self.programmatic_until_ms.store(0, Ordering::Relaxed);
            if let Ok(mut guard) = self.last_programmatic_text.lock() {
                *guard = None;
            }
            return false;
        }

        let mut matches = false;
        if let Ok(mut guard) = self.last_programmatic_text.lock() {
            if guard.as_deref() == Some(text) {
                matches = true;
                *guard = None;
            }
        }
        if matches {
            self.programmatic_until_ms.store(0, Ordering::Relaxed);
        }
        matches
    }
}
