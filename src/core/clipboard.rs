use arboard::Clipboard;
use std::sync::Mutex;
use anyhow::Result;

pub struct ClipboardManager {
    clipboard: Mutex<Clipboard>,
}

impl ClipboardManager {
    pub fn new() -> Result<Self> {
        let clipboard = Clipboard::new().map_err(|e| anyhow::anyhow!("Failed to init clipboard: {}", e))?;
        Ok(Self {
            clipboard: Mutex::new(clipboard),
        })
    }

    pub fn get_text(&self) -> Result<String> {
        let mut cb = self.clipboard.lock().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
        cb.get_text().map_err(|e| anyhow::anyhow!("Failed to get text: {}", e))
    }

    pub fn set_text(&self, text: &str) -> Result<()> {
        let mut cb = self.clipboard.lock().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
        cb.set_text(text.to_string()).map_err(|e| anyhow::anyhow!("Failed to set text: {}", e))
    }
}
