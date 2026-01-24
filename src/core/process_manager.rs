//! Process manager for child UI windows

use log::{debug, error, info};
use std::collections::HashMap;
use std::process::{Child, Command};
use std::sync::Arc;
use parking_lot::Mutex;

#[cfg(windows)]
use crate::platform::focus_window_by_title;

/// Manages spawned child processes for UI windows
pub struct ProcessManager {
    /// Map of process name to Child handle
    children: Arc<Mutex<HashMap<String, Child>>>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            children: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn or focus a child UI process
    /// 
    /// If the process is already running, attempts to focus it.
    /// Otherwise, spawns a new process.
    pub fn spawn_or_focus(
        &self,
        name: &str,
        window_title: &str,
        command: Command,
    ) -> Result<(), String> {
        // Check if process already exists and is running
        {
            let mut children = self.children.lock();
            if let Some(child) = children.get_mut(name) {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        // Process has exited, remove it
                        debug!("Process {} exited with status: {:?}", name, status);
                        children.remove(name);
                    }
                    Ok(None) => {
                        // Process is still running, try to focus it
                        debug!("Process {} is already running, attempting to focus", name);
                        
                        #[cfg(windows)]
                        {
                            if focus_window_by_title(window_title) {
                                info!("Focused existing window: {}", window_title);
                                return Ok(());
                            } else {
                                error!("Failed to focus window: {}", window_title);
                            }
                        }
                        
                        #[cfg(not(windows))]
                        {
                            info!("Window focus not supported on this platform: {}", window_title);
                        }
                        
                        return Ok(());
                    }
                    Err(e) => {
                        error!("Failed to check process status for {}: {}", name, e);
                        // Remove the child and try to spawn a new one
                        children.remove(name);
                    }
                }
            }
        }

        // Spawn new process
        self.spawn(name, command)
    }

    /// Spawn a new child process
    fn spawn(&self, name: &str, mut command: Command) -> Result<(), String> {
        match command.spawn() {
            Ok(child) => {
                info!("Spawned process: {}", name);
                let mut children = self.children.lock();
                children.insert(name.to_string(), child);
                Ok(())
            }
            Err(e) => {
                let err = format!("Failed to spawn process {}: {}", name, e);
                error!("{}", err);
                Err(err)
            }
        }
    }

    /// Check if a process is currently running
    pub fn is_running(&self, name: &str) -> bool {
        let mut children = self.children.lock();
        if let Some(child) = children.get_mut(name) {
            match child.try_wait() {
                Ok(Some(_)) => {
                    // Process has exited
                    children.remove(name);
                    false
                }
                Ok(None) => {
                    // Still running
                    true
                }
                Err(_) => {
                    // Error checking status, assume not running
                    children.remove(name);
                    false
                }
            }
        } else {
            false
        }
    }

    /// Kill all managed child processes
    pub fn kill_all(&self) {
        let mut children = self.children.lock();
        for (name, mut child) in children.drain() {
            match child.kill() {
                Ok(_) => {
                    info!("Killed child process: {}", name);
                    let _ = child.wait(); // Reap the process
                }
                Err(e) => {
                    error!("Failed to kill child process {}: {}", name, e);
                }
            }
        }
    }

    /// Get the number of running child processes
    pub fn count(&self) -> usize {
        let children = self.children.lock();
        children.len()
    }
}

impl Default for ProcessManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for ProcessManager {
    fn drop(&mut self) {
        self.kill_all();
    }
}

/// Helper function to build memory graph UI command
pub fn build_memory_graph_command(port: u16) -> Result<Command, String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed to get current exe: {}", e))?;
    let bin_dir = current_exe.parent()
        .ok_or("Failed to get bin directory")?;
    
    let ui_exe = bin_dir.join("memory_graph_ui");
    let ui_path = if cfg!(windows) {
        ui_exe.with_extension("exe")
    } else {
        ui_exe
    };

    if !ui_path.exists() {
        return Err(format!("Memory graph UI executable not found: {:?}", ui_path));
    }

    let mut cmd = Command::new(ui_path);
    cmd.arg(port.to_string());
    Ok(cmd)
}

/// Helper function to build functions config UI command
pub fn build_functions_config_command() -> Result<Command, String> {
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed to get current exe: {}", e))?;
    let bin_dir = current_exe.parent()
        .ok_or("Failed to get bin directory")?;

    // Search in current bin dir first, then sibling release/debug dirs
    let mut candidates = Vec::new();

    let push_candidate = |list: &mut Vec<std::path::PathBuf>, dir: &std::path::Path, stem: &str| {
        let base = dir.join(stem);
        if cfg!(windows) {
            list.push(base.with_extension("exe"));
        } else {
            list.push(base);
        }
    };

    // Primary targets in the current bin dir
    push_candidate(&mut candidates, bin_dir, "functions_config_ui");
    push_candidate(&mut candidates, bin_dir, "hotkey_config_ui");

    // Fallback to sibling release/debug folders
    if let Some(target_dir) = bin_dir.parent() {
        let release_dir = target_dir.join("release");
        let debug_dir = target_dir.join("debug");
        push_candidate(&mut candidates, &release_dir, "functions_config_ui");
        push_candidate(&mut candidates, &release_dir, "hotkey_config_ui");
        push_candidate(&mut candidates, &debug_dir, "functions_config_ui");
        push_candidate(&mut candidates, &debug_dir, "hotkey_config_ui");
    }

    let final_path = candidates.into_iter().find(|p| p.exists())
        .ok_or("Functions config UI executable not found")?;

    Ok(Command::new(final_path))
}
