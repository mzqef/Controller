use crate::core::memory_store::MemoryStore;
use crate::core::ipc_messages::{GraphRequest, GraphResponse};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncWriteExt, AsyncBufReadExt, BufReader};
use tokio::sync::broadcast;
use std::sync::Arc;
use log::{info, error, debug};

/// Notification sent to all connected graph UI clients
#[derive(Clone, Debug)]
pub struct GraphNotification;

fn build_snapshot(store: &MemoryStore) -> GraphResponse {
    // Retrieve all (client-side filtering happens in the UI)
    let short = store.list_items_by_type(crate::core::memory::MemoryType::ShortTerm, 100, 0);
    let mid = store.list_items_by_type(crate::core::memory::MemoryType::MidTerm, 100, 0);
    let long = store.list_items_by_type(crate::core::memory::MemoryType::LongTerm, 100, 0);
    let items: Vec<_> = short.into_iter().chain(mid).chain(long).collect();
    
    // Debug: log items with titles
    for item in &items {
        if item.title.is_some() {
            log::debug!("build_snapshot: item {} has title {:?}", item.id, item.title);
        }
    }
    
    let item_ids: std::collections::HashSet<_> = items.iter().map(|i| i.id).collect();
    let links = store.list_edges_for_items(&item_ids);
    GraphResponse::Snapshot { items, links }
}

pub struct GraphIpcServer {
    store: Arc<MemoryStore>,
    port: u16,
    notify_tx: broadcast::Sender<GraphNotification>,
}

impl GraphIpcServer {
    pub fn new(store: Arc<MemoryStore>, port: u16) -> Self {
        let (notify_tx, _) = broadcast::channel(16);
        Self { store, port, notify_tx }
    }
    
    /// Returns a sender that can be used to notify all connected clients of data changes
    pub fn get_notifier(&self) -> broadcast::Sender<GraphNotification> {
        self.notify_tx.clone()
    }

    pub async fn run(self) {
        let addr = format!("127.0.0.1:{}", self.port);
        let listener = TcpListener::bind(&addr).await;
        match listener {
             Ok(l) => {
                 info!("Graph IPC Server listening on {}", addr);
                 loop {
                     match l.accept().await {
                        Ok((stream, _)) => {
                             let store = self.store.clone();
                             let notify_rx = self.notify_tx.subscribe();
                             tokio::spawn(async move {
                                 handle_client(stream, store, notify_rx).await;
                             });
                        }
                        Err(e) => {
                             error!("Accept error: {}", e);
                        }
                     }
                 }
             }
             Err(e) => {
                 error!("Failed to bind Graph IPC server: {}", e);
             }
        }
    }
}

async fn handle_client(
    stream: TcpStream,
    store: Arc<MemoryStore>,
    mut notify_rx: broadcast::Receiver<GraphNotification>,
) {
    let (reader, mut writer) = stream.into_split();
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    loop {
        line.clear();
        tokio::select! {
            // Handle incoming requests from client
            result = buf_reader.read_line(&mut line) => {
                match result {
                    Ok(0) => break, // EOF
                    Ok(_) => {
                        let trimmed = line.trim();
                        match serde_json::from_str::<GraphRequest>(trimmed) {
                            Ok(req) => {
                                let resp = process_request(&store, req);
                                match serde_json::to_string(&resp) {
                                    Ok(json) => {
                                        if writer.write_all(json.as_bytes()).await.is_err() { break; }
                                        if writer.write_all(b"\n").await.is_err() { break; }
                                        if writer.flush().await.is_err() { break; }
                                    }
                                    Err(e) => error!("Failed to serialize response: {}", e),
                                }
                            }
                            Err(e) => {
                                error!("Invalid IPC request '{}': {}", trimmed, e);
                            }
                        }
                    }
                    Err(_) => break,
                }
            }
            // Handle server-side notifications (push to client)
            _ = notify_rx.recv() => {
                debug!("Pushing DataChanged notification to graph client");
                let resp = GraphResponse::DataChanged;
                match serde_json::to_string(&resp) {
                    Ok(json) => {
                        if writer.write_all(json.as_bytes()).await.is_err() { break; }
                        if writer.write_all(b"\n").await.is_err() { break; }
                        if writer.flush().await.is_err() { break; }
                    }
                    Err(e) => error!("Failed to serialize notification: {}", e),
                }
            }
        }
    }
}

fn process_request(store: &MemoryStore, req: GraphRequest) -> GraphResponse {
    match req {
        GraphRequest::GetSnapshot => {
             build_snapshot(store)
        }
        GraphRequest::UpdateNodePosition { id, x, y } => {
             match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = store.update_item_position(id, x, y);
            })) {
                Ok(_) => build_snapshot(store),
                Err(_) => GraphResponse::Error("Panic during UpdateNodePosition".to_string())
            }
        }
        GraphRequest::UpdateItemTitle { id, title } => {
            log::info!("IPC: UpdateItemTitle for {} with title '{}'", id, title);
            store.update_item_title(id, title);
            build_snapshot(store)
        }
        GraphRequest::PromoteItem { id, target_type } => {
            match store.clone_promote_item(id, target_type) {
                Ok(_) => build_snapshot(store),
                Err(e) => GraphResponse::Error(format!("PromoteItem failed: {}", e)),
            }
        }
        GraphRequest::AddUserEdge { source, target } => {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let _ = store.add_user_edge(source, target);
            })) {
                 Ok(_) => build_snapshot(store),
                 Err(_) => GraphResponse::Error("Panic during AddUserEdge".to_string())
            }
        }
        GraphRequest::DeleteItem { id } => {
            match store.delete_item(id) {
                Ok(_) => build_snapshot(store),
                Err(e) => GraphResponse::Error(format!("DeleteItem failed: {}", e)),
            }
        }
        GraphRequest::ClearAllPositions => {
            match store.clear_all_positions() {
                Ok(_) => build_snapshot(store),
                Err(e) => GraphResponse::Error(format!("ClearAllPositions failed: {}", e)),
            }
        }
    }
}
