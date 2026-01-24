use crate::core::actions::Action;

#[derive(Debug)]
#[allow(dead_code)]
pub enum AppEvent {
    TriggerAction(Action),
    UiCopyPressed,
    UserQuery(String),
    ToggleProcessing(bool),
    Cancel,
    ShowMemoryGraph,
}
