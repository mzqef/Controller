//! Platform-specific modules for OS-dependent functionality

#[cfg(windows)]
pub mod windows;

#[cfg(not(windows))]
pub mod unix;

// Re-export common interfaces
#[cfg(windows)]
pub use windows::*;

#[cfg(not(windows))]
pub use unix::*;
