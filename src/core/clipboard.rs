use arboard::{Clipboard, ImageData};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};
use anyhow::Result;
use base64::{Engine, engine::general_purpose::STANDARD};

/// Debounce configuration for clipboard events
const DEBOUNCE_STABLE_MS: u64 = 150;  // Wait for content to stabilize
const DEBOUNCE_IME_MS: u64 = 250;     // Longer wait during IME composition

/// Type of content currently in the clipboard
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardContentType {
    Text,
    Image,
    Empty,
}

pub struct ClipboardManager {
    clipboard: Mutex<Clipboard>,
    last_programmatic_text: Mutex<Option<String>>,
    programmatic_until_ms: AtomicU64,
    // Debounce state for IME compatibility
    last_change_ms: AtomicU64,
    last_content_hash: AtomicU64,
    pending_stable_since_ms: AtomicU64,
}

impl ClipboardManager {
    pub fn new() -> Result<Self> {
        let clipboard = Clipboard::new().map_err(|e| anyhow::anyhow!("Failed to init clipboard: {}", e))?;
        Ok(Self {
            clipboard: Mutex::new(clipboard),
            last_programmatic_text: Mutex::new(None),
            programmatic_until_ms: AtomicU64::new(0),
            last_change_ms: AtomicU64::new(0),
            last_content_hash: AtomicU64::new(0),
            pending_stable_since_ms: AtomicU64::new(0),
        })
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    fn hash_content(text: &str) -> u64 {
        let mut hasher = DefaultHasher::new();
        text.hash(&mut hasher);
        hasher.finish()
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
            .store(Self::now_ms().saturating_add(2000), Ordering::Relaxed);

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

    /// Check if clipboard content should be debounced (IME compatibility).
    /// 
    /// Returns `Some(wait_ms)` if we should wait longer before processing,
    /// or `None` if the content is stable and ready to process.
    /// 
    /// This implements both time-based and content-stability debouncing:
    /// - Time-based: Wait at least DEBOUNCE_STABLE_MS since last change
    /// - Content-based: Wait until the same content hash persists for the debounce period
    /// - IME-aware: Use longer debounce when IME is composing
    pub fn should_debounce(&self, text: &str, ime_composing: bool) -> Option<u64> {
        let now = Self::now_ms();
        let current_hash = Self::hash_content(text);
        let last_hash = self.last_content_hash.load(Ordering::Relaxed);
        
        let debounce_ms = if ime_composing { DEBOUNCE_IME_MS } else { DEBOUNCE_STABLE_MS };
        
        if current_hash != last_hash {
            // Content changed - reset stability timer
            self.last_content_hash.store(current_hash, Ordering::Relaxed);
            self.pending_stable_since_ms.store(now, Ordering::Relaxed);
            self.last_change_ms.store(now, Ordering::Relaxed);
            return Some(debounce_ms);
        }
        
        // Content is the same - check if it's been stable long enough
        let stable_since = self.pending_stable_since_ms.load(Ordering::Relaxed);
        let stable_duration = now.saturating_sub(stable_since);
        
        if stable_duration < debounce_ms {
            // Still waiting for stability
            return Some(debounce_ms.saturating_sub(stable_duration));
        }
        
        // Content has been stable long enough
        None
    }

    /// Mark that we've processed this content (reset debounce state)
    pub fn mark_processed(&self, text: &str) {
        let hash = Self::hash_content(text);
        self.last_content_hash.store(hash, Ordering::Relaxed);
        self.pending_stable_since_ms.store(0, Ordering::Relaxed);
    }

    /// Get image data from clipboard
    pub fn get_image(&self) -> Result<ImageData<'static>> {
        let mut cb = self.clipboard.lock().map_err(|_| anyhow::anyhow!("Lock poisoned"))?;
        cb.get_image().map_err(|e| anyhow::anyhow!("Failed to get image: {}", e))
    }

    /// Check what type of content is in the clipboard
    pub fn content_type(&self) -> ClipboardContentType {
        // Try image first (some apps put both text and image)
        if self.get_image().is_ok() {
            ClipboardContentType::Image
        } else if self.get_text().is_ok() {
            ClipboardContentType::Text
        } else {
            ClipboardContentType::Empty
        }
    }

    /// Convert clipboard image to base64-encoded PNG
    pub fn get_image_as_base64_png(&self) -> Result<String> {
        let img = self.get_image()?;
        image_data_to_base64_png(&img)
    }
}

/// Convert arboard ImageData (RGBA pixels) to base64-encoded PNG
pub fn image_data_to_base64_png(img: &ImageData) -> Result<String> {
    use image::{ImageBuffer, Rgba, codecs::png::PngEncoder, ImageEncoder, ColorType};
    
    let buffer: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_raw(
        img.width as u32,
        img.height as u32,
        img.bytes.to_vec(),
    ).ok_or_else(|| anyhow::anyhow!("Failed to create image buffer from clipboard data"))?;
    
    let mut png_bytes = Vec::new();
    let encoder = PngEncoder::new(&mut png_bytes);
    encoder.write_image(
        &buffer,
        img.width as u32,
        img.height as u32,
        ColorType::Rgba8,
    )?;
    
    Ok(STANDARD.encode(&png_bytes))
}
