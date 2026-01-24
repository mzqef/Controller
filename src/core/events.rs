use crate::core::actions::Action;
use crate::core::config_watcher::ConfigChange;

#[derive(Debug)]
pub enum AppEvent {
    TriggerAction(Action),
    UserQuery(String),
    ToggleProcessing(bool),
    Cancel,
    ShowMemoryGraph,
    ShowHotkeyConfig,
    ConfigChanged(ConfigChange),
}
