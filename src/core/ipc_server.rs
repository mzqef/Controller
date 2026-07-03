use crate::core::memory_store::MemoryStore;
use crate::core::ipc_messages::{GraphRequest, GraphResponse};
use crate::core::memory::MemoryItem;
use crate::api::client::LlmClient;
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
    llm_client: Option<Arc<LlmClient>>,
}

impl GraphIpcServer {
    pub fn new(store: Arc<MemoryStore>, port: u16) -> Self {
        let (notify_tx, _) = broadcast::channel(16);
        Self { store, port, notify_tx, llm_client: None }
    }

    /// Attach the LLM client so the server can run AI-powered graph operations
    /// (e.g. Auto Connect). Optional: without it, Auto Connect falls back to a
    /// local heuristic.
    pub fn with_llm_client(mut self, llm_client: Arc<LlmClient>) -> Self {
        self.llm_client = Some(llm_client);
        self
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
                             let llm_client = self.llm_client.clone();
                             tokio::spawn(async move {
                                 handle_client(stream, store, notify_rx, llm_client).await;
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
    llm_client: Option<Arc<LlmClient>>,
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
                                // AutoConnectEdges needs async LLM access, so handle
                                // it here before the sync process_request path.
                                let resp = if matches!(req, GraphRequest::AutoConnectEdges) {
                                    auto_connect_edges(&store, llm_client.as_deref()).await
                                } else {
                                    process_request(&store, req)
                                };
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

/// Build a short, human-readable summary line for a memory item, used when
/// asking the LLM which items are related. Falls back to content snippet.
fn item_summary(item: &MemoryItem) -> String {
    let title = item.title.clone().unwrap_or_default();
    let body = if title.trim().is_empty() {
        // First ~60 chars of content
        let chars: Vec<char> = item.content.chars().take(60).collect();
        let mut s: String = chars.into_iter().collect();
        if item.content.chars().count() > 60 {
            s.push('…');
        }
        s
    } else {
        title
    };
    body.replace('\n', " ").trim().to_string()
}

/// Auto Connect: find pairs of memory items that are related but not yet
/// connected by an edge, then add UserLinked edges for them.
///
/// If an LLM client is available, the pairs are determined by asking the model
/// to analyse item summaries and return JSON `[[i,j], ...]` index pairs. If no
/// LLM client is configured (or the call fails), a local heuristic is used:
/// connect items that already share a derivation chain or have overlapping
/// tokens. The graph snapshot is always returned afterwards.
async fn auto_connect_edges(store: &MemoryStore, llm_client: Option<&LlmClient>) -> GraphResponse {
    info!("Auto Connect: starting");
    let items = store.get_all_items();
    if items.len() < 2 {
        return build_snapshot(store);
    }

    let pairs = if let Some(client) = llm_client {
        match auto_connect_via_llm(client, &items).await {
            Ok(p) => p,
            Err(e) => {
                log::warn!("Auto Connect LLM call failed ({}); falling back to heuristic", e);
                auto_connect_heuristic(store, &items)
            }
        }
    } else {
        log::info!("Auto Connect: no LLM client, using heuristic");
        auto_connect_heuristic(store, &items)
    };

    let added = store.add_user_edges_batch(pairs);
    log::info!("Auto Connect: added {} edges", added);
    build_snapshot(store)
}

/// Ask the LLM which item pairs are related. Builds a numbered list of item
/// summaries and instructs the model to return a JSON array of `[i, j]` index
/// pairs (1-based). Parses the response defensively.
async fn auto_connect_via_llm(
    client: &LlmClient,
    items: &[MemoryItem],
) -> Result<Vec<(uuid::Uuid, uuid::Uuid)>, anyhow::Error> {
    // Build a compact numbered catalogue.
    let mut catalogue = String::new();
    for (idx, item) in items.iter().enumerate() {
        catalogue.push_str(&format!("{}. [{:?}] {}\n", idx + 1, item.memory_type, item_summary(item)));
    }

    let prompt = format!(
        "You are analysing a knowledge graph of memory items.\n\
Below is a numbered list of items. Identify which pairs are RELATED (same topic, \
one explains/derives from the other, translations, reformattings, etc.).\n\n\
Return ONLY a JSON array of index pairs, e.g. [[1,3],[2,5]]. Use 1-based indices. \
Do not include any explanation, just the JSON array. If no pairs are related, \
return [].\n\nItems:\n{}",
        catalogue
    );

    let raw = client
        .execute_action("connect", &prompt)
        .await
        .map_err(|e| anyhow::anyhow!("LLM call failed: {}", e))?;

    // Extract the first JSON array substring from the response (the model may
    // wrap it in prose or code fences despite instructions).
    let pairs = parse_index_pairs(&raw, items);
    Ok(pairs)
}

/// Parse a JSON array of index pairs from an LLM response. Tolerates leading
/// prose and code fences; validates indices against the item list.
fn parse_index_pairs(raw: &str, items: &[MemoryItem]) -> Vec<(uuid::Uuid, uuid::Uuid)> {
    // Find the first '[' and matching ']' to extract the array body.
    let start = match raw.find('[') {
        Some(s) => s,
        None => return Vec::new(),
    };
    let end = match raw.rfind(']') {
        Some(e) if e > start => e + 1,
        _ => return Vec::new(),
    };
    let slice = &raw[start..end];
    let parsed: Result<Vec<Vec<usize>>, _> = serde_json::from_str(slice);
    let mut out = Vec::new();
    if let Ok(pairs) = parsed {
        for pair in pairs {
            if pair.len() != 2 {
                continue;
            }
            // Indices are 1-based per the prompt.
            let (a, b) = (pair[0], pair[1]);
            if a == 0 || b == 0 || a > items.len() || b > items.len() {
                continue;
            }
            let ia = &items[a - 1];
            let ib = &items[b - 1];
            if ia.id != ib.id {
                out.push((ia.id, ib.id));
            }
        }
    }
    out
}

/// Heuristic fallback when no LLM is available: connect items whose content
/// shares significant token overlap. Cheap but better than nothing.
fn auto_connect_heuristic(store: &MemoryStore, items: &[MemoryItem]) -> Vec<(uuid::Uuid, uuid::Uuid)> {
    // Gather existing edges so we don't duplicate them.
    let existing: std::collections::HashSet<(uuid::Uuid, uuid::Uuid)> = store
        .get_all_edges()
        .into_iter()
        .map(|e| (e.source_id, e.target_id))
        .collect();

    // Tokenise each item (lowercase, alphanumeric words length>=4).
    let tokenised: Vec<(uuid::Uuid, std::collections::HashSet<String>)> = items
        .iter()
        .map(|it| {
            let tokens: std::collections::HashSet<String> = it
                .content
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() >= 4)
                .map(|w| w.to_lowercase())
                .collect();
            (it.id, tokens)
        })
        .collect();

    let mut pairs = Vec::new();
    for i in 0..tokenised.len() {
        for j in (i + 1)..tokenised.len() {
            let (id_a, toks_a) = &tokenised[i];
            let (id_b, toks_b) = &tokenised[j];
            // Require at least 3 shared significant tokens to count as related.
            let shared = toks_a.intersection(toks_b).count();
            if shared >= 3 {
                let key = (*id_a, *id_b);
                let mirror = (*id_b, *id_a);
                if !existing.contains(&key) && !existing.contains(&mirror) {
                    pairs.push(key);
                }
            }
        }
    }
    pairs
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
        GraphRequest::ClearGraph => {
            match store.clear_graph() {
                Ok(_) => build_snapshot(store),
                Err(e) => GraphResponse::Error(format!("ClearGraph failed: {}", e)),
            }
        }
        // AutoConnectEdges is intercepted in handle_client (it needs async LLM
        // access). Reaching here means it was called on a connection without
        // the async wrapper; return an error rather than silently doing nothing.
        GraphRequest::AutoConnectEdges => {
            GraphResponse::Error("AutoConnectEdges requires the async handler".to_string())
        }
    }
}
