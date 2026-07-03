use crate::core::actions::Action;
use crate::core::config_watcher::ConfigChange;

#[derive(Debug)]
pub enum AppEvent {
    TriggerAction(Action),
    UserQuery(String),
    /// Overwrite the system clipboard with the given text. Used by the floating
    /// toolbar to ensure the action handler picks up exactly the text that was
    /// selected when the toolbar appeared.
    SetClipboard(String),
    /// The user just released the mouse button after a potential text
    /// selection at screen coordinates (x, y). The receiver should grab the
    /// current selection and pop up the action toolbar near this point.
    SelectionAt { x: i32, y: i32 },
    ToggleProcessing(bool),
    ToggleLocalMode(bool),
    Cancel,
    ShowMemoryGraph,
    ShowHotkeyConfig,
    ConfigChanged(ConfigChange),
}
