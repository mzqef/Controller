//! # IntelliBoard
//!
//! A Rust desktop agent that enhances clipboard workflows using LLMs.
//!
//! IntelliBoard intercepts clipboard text, detects issues (ligatures, OCR errors, math formatting),
//! sends it to configured LLMs for fixes/translations/explanations, and updates the clipboard
//! with the result.
//!
//! ## Modules
//!
//! - [`core`] - Core business logic (actions, clipboard, config, memory)
//! - [`api`] - LLM API client with streaming and fallback
//! - [`ui`] - UI components and themes
//! - [`platform`] - Platform-specific keyboard hooks
//! - [`startup`] - Application initialization

pub mod core;
pub mod api;
pub mod ui;
pub mod startup;
pub mod platform;

// Re-export icon utilities for convenient access
pub use ui::theme::{load_tray_icon_active, load_tray_icon_inactive, load_tray_icon_local, load_egui_icon};
