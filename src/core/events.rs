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
    /// Reserved hook for a future mouse-driven toolbar trigger.
    ///
    /// IMPORTANT — current toolbar semantics: the floating action toolbar is
    /// triggered EXCLUSIVELY by a clipboard copy event (see the `cb_rx.recv()`
    /// branch in `main.rs`), NOT by mouse-selection release. There is no
    /// "select text → toolbar appears" path today; only "copy text → toolbar
    /// appears". This variant is kept for a possible future mouse-hook path
    /// but currently has no sender and is only handled with a debug log.
    SelectionAt { x: i32, y: i32 },
    ToggleProcessing(bool),
    ToggleLocalMode(bool),
    Cancel,
    ShowMemoryGraph,
    ShowHotkeyConfig,
    ConfigChanged(ConfigChange),
}
