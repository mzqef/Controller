use serde::{Deserialize, Serialize};
use uuid::Uuid;
use crate::core::memory::{MemoryItem, MemoryEdge};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum GraphRequest {
    GetSnapshot,
    UpdateNodePosition { id: Uuid, x: f32, y: f32 },
    UpdateItemTitle { id: Uuid, title: String },
    PromoteItem { id: Uuid, target_type: crate::core::memory::MemoryType },
    AddUserEdge { source: Uuid, target: Uuid },
    DeleteItem { id: Uuid },
    /// Clear all stored positions (used by Auto Align)
    ClearAllPositions,
    /// Clear the entire graph: all items and all edges (used by Trash button)
    ClearGraph,
    /// Auto-connect unconnected nodes via AI. The server side gathers all
    /// item summaries, asks the LLM which pairs are related, and adds the
    /// resulting edges as UserLinked. The client waits for the refreshed
    /// snapshot returned in the response.
    AutoConnectEdges,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum GraphResponse {
    Snapshot {
        items: Vec<MemoryItem>,
        links: Vec<MemoryEdge>,
    },
    /// Notifies the client that data has changed and they should refresh
    DataChanged,
    Ack,
    Error(String),
}
