//! Config file watcher for hot-reload support
//! 
//! Monitors config directory for changes and notifies the app to reload configuration.

use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher, Event, EventKind};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use log::{info, error, debug};

/// Message sent when config files change
#[derive(Debug, Clone)]
pub enum ConfigChange {
    ActionsConfig,
    HotkeysConfig,
    Unknown(String),
}

/// Watches config directory for file changes with debouncing
pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<ConfigChange>,
}

impl ConfigWatcher {
    /// Create a new config watcher monitoring the given directory
    pub fn new<P: AsRef<Path>>(config_dir: P) -> Result<Self, notify::Error> {
        let (tx, rx) = channel();
        
        // Create debounced sender
        let debounced_tx = DebouncedSender::new(tx, Duration::from_millis(100));
        
        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                match res {
                    Ok(event) => {
                        if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                            for path in event.paths {
                                let change = path_to_config_change(&path);
                                debounced_tx.send(change);
                            }
                        }
                    }
                    Err(e) => error!("Config watch error: {:?}", e),
                }
            },
            Config::default(),
        )?;
        
        watcher.watch(config_dir.as_ref(), RecursiveMode::NonRecursive)?;
        info!("Config watcher started for {:?}", config_dir.as_ref());
        
        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }
    
    /// Try to receive a config change notification (non-blocking)
    pub fn try_recv(&self) -> Option<ConfigChange> {
        self.rx.try_recv().ok()
    }
    
    /// Get the receiver for integration with event loops
    pub fn receiver(&self) -> &Receiver<ConfigChange> {
        &self.rx
    }
}

/// Debounced sender to avoid rapid-fire notifications
struct DebouncedSender {
    tx: Sender<ConfigChange>,
    last_send: std::sync::Mutex<Option<Instant>>,
    debounce_duration: Duration,
}

impl DebouncedSender {
    fn new(tx: Sender<ConfigChange>, debounce_duration: Duration) -> Self {
        Self {
            tx,
            last_send: std::sync::Mutex::new(None),
            debounce_duration,
        }
    }
    
    fn send(&self, change: ConfigChange) {
        let now = Instant::now();
        let mut last = self.last_send.lock().unwrap();
        
        let should_send = match *last {
            Some(t) if now.duration_since(t) < self.debounce_duration => false,
            _ => true,
        };
        
        if should_send {
            *last = Some(now);
            if self.tx.send(change.clone()).is_ok() {
                debug!("Config change notification: {:?}", change);
            }
        }
    }
}

/// Map file path to config change type
fn path_to_config_change(path: &Path) -> ConfigChange {
    let filename = path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("");
    
    match filename {
        "actions.toml" => ConfigChange::ActionsConfig,
        "hotkeys.toml" => ConfigChange::HotkeysConfig,
        _ => ConfigChange::Unknown(filename.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_path_to_config_change() {
        assert!(matches!(
            path_to_config_change(Path::new("config/actions.toml")),
            ConfigChange::ActionsConfig
        ));
        assert!(matches!(
            path_to_config_change(Path::new("config/hotkeys.toml")),
            ConfigChange::HotkeysConfig
        ));
    }
}
