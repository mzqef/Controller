use clipboard_master::{Master, CallbackResult, ClipboardHandler};
use std::io;
use tokio::sync::mpsc;

pub struct Listener {
    sender: mpsc::Sender<()>,
}

impl Listener {
    pub fn new(sender: mpsc::Sender<()>) -> Self {
        Self { sender }
    }
}

impl ClipboardHandler for Listener {
    fn on_clipboard_change(&mut self) -> CallbackResult {
        // Avoid blocking in the clipboard callback thread.
        let _ = self.sender.try_send(());
        if std::env::var_os("IntelliBoard_DIAG_CLIPBOARD").is_some() {
            log::debug!("[diag] clipboard_master reported clipboard change");
        }
        CallbackResult::Next
    }

    fn on_clipboard_error(&mut self, error: io::Error) -> CallbackResult {
        log::error!("Clipboard listener error: {}", error);
        CallbackResult::Next
    }
}

pub fn start_listener(sender: mpsc::Sender<()>) {
    std::thread::spawn(move || {
        let _ = Master::new(Listener::new(sender))
            .expect("Failed to create clipboard listener")
            .run();
    });
}
