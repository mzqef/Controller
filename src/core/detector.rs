#![allow(dead_code)]
use regex::Regex;
use once_cell::sync::Lazy;
use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct DetectionConfig {
    #[serde(default = "default_split_threshold")]
    pub split_whitespace_threshold: usize,
}

fn default_split_threshold() -> usize {
    32
}

// Common ligatures often found in PDFs
static LIGATURES: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[\u{fb00}-\u{fb06}]").unwrap()
});

// Heuristic for math formulas:
// We distinguish between "printed" symbols (Unicode) which need fixing,
// and explicit LaTeX syntax which implies the text is already correct/source code.

static UNICODE_MATH: Lazy<Regex> = Lazy::new(|| {
    // Matches Unicode math symbols (Operators, Greek, etc.)
    Regex::new(r"([\u{2200}-\u{22FF}])|([\u{0370}-\u{03FF}])").unwrap()
});

static PATH_OR_URL: Lazy<Regex> = Lazy::new(|| {
    // Matches:
    // 1. URLs (http, https, www)
    // 2. Windows absolute paths (C:\, D:\)
    // 3. Windows UNC paths (\\)
    // 4. Unix absolute paths (starts with /, contains at least one more /)
    Regex::new(r"(?i)^(https?://|www\.|[a-z]:\\[^\n]+|\\\\[^\n]+|/[^\n\s]*/[^\n\s]*)").unwrap()
});

static CJK_CHARACTERS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"[\u{4e00}-\u{9fff}]").unwrap()
});

// Detects broken ligatures where 'fi' or 'fl' are replaced by 'W' or similar artifacts.
// e.g., "classiWcation", "deWne", "Wction" (fiction), "Wckle" (fickle)
static BROKEN_LIGATURES: Lazy<Regex> = Lazy::new(|| {
    // Matches:
    // 1. W inside a word: [a-z]W[a-z] (e.g. classiWcation)
    // 2. W at start of word followed by a consonant that isn't h, r, w, y: \bW[bcdfghjklmnpqstvxz]
    //    (e.g. Wction -> fiction, Wckle -> fickle, Wsh -> fish)
    Regex::new(r"[a-z]W[a-z]|\bW[bcdfghjklmnpqstvxz]").unwrap()
});

pub fn needs_processing(text: &str, config: &DetectionConfig) -> bool {
    if text.trim().is_empty() {
        return false;
    }

    // Ignore paths and URLs
    if PATH_OR_URL.is_match(text.trim()) {
        return false;
    }

    // Check for ligatures
    if LIGATURES.is_match(text) {
        return true;
    }

    // Check for broken ligatures (e.g. "classiWcation")
    if BROKEN_LIGATURES.is_match(text) {
        return true;
    }

    // Check for "printed" math symbols (Unicode)
    if UNICODE_MATH.is_match(text) {
        return true;
    }

    // Check for long words (missing spaces)
    // Only apply this check if the text does NOT contain CJK characters.
    // In CJK languages, long sequences without spaces are normal.
    if !CJK_CHARACTERS.is_match(text) {
        if text.split_whitespace().any(|word| {
            if word.len() > config.split_whitespace_threshold {
                // Ignore if it looks like code (contains brackets, slashes, etc.)
                // Code often contains no whitespace but has symbols like ()[]{}\/<>
                !word.chars().any(|c| "()[]{}\\/<>".contains(c))
            } else {
                false
            }
        }) {
            return true;
        }
    }
    
    false
}
