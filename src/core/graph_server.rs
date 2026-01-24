use crate::core::memory::MemoryEvent;
use crate::core::memory_store::MemoryStore;
use crate::core::ipc_server::GraphNotification;
use log::{error, debug};
use std::sync::Arc;
use tokio::sync::{mpsc, broadcast};

pub async fn run(
    store: Arc<MemoryStore>,
    mut rx: mpsc::Receiver<MemoryEvent>,
    notify_tx: broadcast::Sender<GraphNotification>,
) {
    while let Some(event) = rx.recv().await {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            match event {
                MemoryEvent::AddClipboard(content) => {
                    store.add_clipboard(content);
                }
                MemoryEvent::AddActionResult {
                    input_text,
                    input_id,
                    output_text,
                    action_type,
                } => {
                    store.add_action_result(&input_text, input_id, output_text, action_type);
                }
                MemoryEvent::PromoteItem { item_id, to_type } => {
                    let _ = store.promote_item(item_id, to_type);
                }
                MemoryEvent::AddUserEdge { source_id, target_id } => {
                    let _ = store.add_user_edge(source_id, target_id);
                }
                MemoryEvent::DeleteItem(id) => {
                    let _ = store.delete_item(id);
                }
                MemoryEvent::DeleteEdge(edge_id) => {
                    store.delete_edge(edge_id);
                }
            }
        }));

        if result.is_err() {
            error!("GraphServer panicked while handling MemoryEvent; continuing");
        } else {
            // Notify connected graph UIs that data has changed
            if notify_tx.receiver_count() > 0 {
                debug!("Notifying {} graph UI client(s) of data change", notify_tx.receiver_count());
                let _ = notify_tx.send(GraphNotification);
            }
        }
    }
}
