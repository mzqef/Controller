use crate::core::memory::*;
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use log::{info, debug, warn};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;
use uuid::Uuid;

pub struct MemoryStore {
    items: RwLock<HashMap<Uuid, MemoryItem>>,
    edges: RwLock<HashMap<Uuid, MemoryEdge>>,
    db_path: PathBuf,
    max_short_term: usize,
    max_mid_term: usize,
    revision: AtomicU64,
}

impl MemoryStore {
    pub fn new() -> Result<Self> {
        let db_path = Self::get_db_path()?;
        info!("Memory store DB path: {:?}", db_path);

        let store = Self {
            items: RwLock::new(HashMap::new()),
            edges: RwLock::new(HashMap::new()),
            db_path,
            max_short_term: 100,
            max_mid_term: 50,
            revision: AtomicU64::new(0),
        };

        store.init_db()?;
        store.load_from_db()?;

        Ok(store)
    }

    /// Get the current revision number for change detection
    pub fn get_revision(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    /// Increment revision to signal a change
    fn bump_revision(&self) {
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    fn get_db_path() -> Result<PathBuf> {
        let data_dir = dirs::data_local_dir()
            .ok_or_else(|| anyhow!("Could not find local data directory"))?
            .join("IntelliBoard");

        std::fs::create_dir_all(&data_dir)?;
        Ok(data_dir.join("memory.db"))
    }

    fn init_db(&self) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_items (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                preview TEXT NOT NULL,
                memory_type TEXT NOT NULL,
                created_at TEXT NOT NULL,
                promoted_at TEXT,
                metadata TEXT NOT NULL,
                pos_x REAL,
                pos_y REAL
            )",
            [],
        )?;

        // Add title column for backward compatibility (ignore error if it already exists)
        let _ = conn.execute("ALTER TABLE memory_items ADD COLUMN title TEXT", []);

        conn.execute(
            "CREATE TABLE IF NOT EXISTS memory_edges (
                id TEXT PRIMARY KEY,
                source_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                relation TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (source_id) REFERENCES memory_items(id),
                FOREIGN KEY (target_id) REFERENCES memory_items(id)
            )",
            [],
        )?;

        Ok(())
    }

    fn load_from_db(&self) -> Result<()> {
        let conn = Connection::open(&self.db_path)?;

        // Load items (ShortTerm/MidTerm/LongTerm are persisted; ShortTerm is capped to max_short_term)
        let mut stmt = conn.prepare(
            "SELECT id, content, preview, memory_type, created_at, promoted_at, metadata, pos_x, pos_y, title 
             FROM memory_items WHERE memory_type IN ('ShortTerm', 'MidTerm', 'LongTerm')",
        )?;

        let items_iter = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let content: String = row.get(1)?;
            let preview: String = row.get(2)?;
            let memory_type_str: String = row.get(3)?;
            let created_at_str: String = row.get(4)?;
            let promoted_at_str: Option<String> = row.get(5)?;
            let metadata_str: String = row.get(6)?;
            let pos_x: Option<f32> = row.get(7)?;
            let pos_y: Option<f32> = row.get(8)?;
            // title may be NULL or missing; use ok() to avoid panic on missing column
            let title: Option<String> = row.get(9).ok();

            Ok((
                id_str,
                content,
                preview,
                memory_type_str,
                created_at_str,
                promoted_at_str,
                metadata_str,
                pos_x,
                pos_y,
                title,
            ))
        })?;

        let mut items = self.items.write().unwrap();
        for row in items_iter {
            let (
                id_str,
                content,
                preview,
                memory_type_str,
                created_at_str,
                promoted_at_str,
                metadata_str,
                pos_x,
                pos_y,
                title,
            ) = row?;

            let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
            let memory_type = match memory_type_str.as_str() {
                "ShortTerm" => MemoryType::ShortTerm,
                "MidTerm" => MemoryType::MidTerm,
                "LongTerm" => MemoryType::LongTerm,
                _ => MemoryType::ShortTerm,
            };
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let promoted_at = promoted_at_str.and_then(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .ok()
            });
            let metadata: MemoryMetadata =
                serde_json::from_str(&metadata_str).unwrap_or(MemoryMetadata::Clipboard);
            let position = match (pos_x, pos_y) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            };

            items.insert(
                id,
                MemoryItem {
                    id,
                    title,
                    content,
                    preview,
                    memory_type,
                    created_at,
                    promoted_at,
                    metadata,
                    position,
                },
            );
        }

        // Enforce strict ShortTerm cap after loading from DB.
        // We also delete trimmed IDs from SQLite so the DB doesn't grow unbounded.
        let (removed_short_items, removed_short_edges) = self.trim_memory_type(&mut items, MemoryType::ShortTerm, self.max_short_term);
        // Also enforce MidTerm cap on load
        let (removed_mid_items, removed_mid_edges) = self.trim_memory_type(&mut items, MemoryType::MidTerm, self.max_mid_term);
        drop(items);

        // Delete trimmed items and edges from DB
        if !removed_short_items.is_empty() {
            info!("Trimmed {} ShortTerm items on load", removed_short_items.len());
            let _ = self.delete_items_from_db(&removed_short_items);
        }
        if !removed_short_edges.is_empty() {
            debug!("Removed {} orphan edges from ShortTerm trim", removed_short_edges.len());
            let _ = self.delete_edges_from_db(&removed_short_edges);
        }
        if !removed_mid_items.is_empty() {
            info!("Trimmed {} MidTerm items on load", removed_mid_items.len());
            let _ = self.delete_items_from_db(&removed_mid_items);
        }
        if !removed_mid_edges.is_empty() {
            debug!("Removed {} orphan edges from MidTerm trim", removed_mid_edges.len());
            let _ = self.delete_edges_from_db(&removed_mid_edges);
        }

        // Load edges
        let mut stmt = conn.prepare(
            "SELECT id, source_id, target_id, relation, created_at FROM memory_edges",
        )?;

        let edges_iter = stmt.query_map([], |row| {
            let id_str: String = row.get(0)?;
            let source_str: String = row.get(1)?;
            let target_str: String = row.get(2)?;
            let relation_str: String = row.get(3)?;
            let created_at_str: String = row.get(4)?;
            Ok((id_str, source_str, target_str, relation_str, created_at_str))
        })?;

        let mut edges = self.edges.write().unwrap();
        for row in edges_iter {
            let (id_str, source_str, target_str, relation_str, created_at_str) = row?;

            let id = Uuid::parse_str(&id_str).unwrap_or_else(|_| Uuid::new_v4());
            let source_id = Uuid::parse_str(&source_str).unwrap_or_else(|_| Uuid::new_v4());
            let target_id = Uuid::parse_str(&target_str).unwrap_or_else(|_| Uuid::new_v4());
            let relation = match relation_str.as_str() {
                "DerivedFrom" => RelationType::DerivedFrom,
                "TranslatedTo" => RelationType::TranslatedTo,
                "ExplainedBy" => RelationType::ExplainedBy,
                "FormattedTo" => RelationType::FormattedTo,
                "PromotedFrom" => RelationType::PromotedFrom,
                "UserLinked" => RelationType::UserLinked,
                _ => RelationType::UserLinked,
            };
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            edges.insert(
                id,
                MemoryEdge {
                    id,
                    source_id,
                    target_id,
                    relation,
                    created_at,
                },
            );
        }

        // Cleanup orphan edges (edges pointing to non-existent items)
        let item_ids: std::collections::HashSet<Uuid> = self.items.read().unwrap().keys().copied().collect();
        let orphan_edge_ids: Vec<Uuid> = edges
            .iter()
            .filter(|(_, e)| !item_ids.contains(&e.source_id) || !item_ids.contains(&e.target_id))
            .map(|(id, _)| *id)
            .collect();
        for eid in &orphan_edge_ids {
            edges.remove(eid);
        }
        drop(edges);
        if !orphan_edge_ids.is_empty() {
            info!("Cleaned up {} orphan edges on startup", orphan_edge_ids.len());
            let _ = self.delete_edges_from_db(&orphan_edge_ids);
        }

        info!(
            "Loaded {} items and {} edges from database",
            self.items.read().map(|m| m.len()).unwrap_or(0),
            self.edges.read().map(|m| m.len()).unwrap_or(0)
        );
        Ok(())
    }

    pub fn add_clipboard(&self, content: String) -> Uuid {
        if content.trim().is_empty() {
            return Uuid::nil();
        }

        // Dedupe: don't add if same as last short-term item
        {
            let items = self.items.read().unwrap();
            let last_short = items
                .values()
                .filter(|i| i.memory_type == MemoryType::ShortTerm)
                .max_by_key(|i| i.created_at);
            if let Some(last) = last_short {
                if last.content == content {
                    return last.id;
                }
            }
        }

        let item = MemoryItem::new_clipboard(content);
        let id = item.id;
        debug!("Adding clipboard item {}", id);

        let (removed_item_ids, removed_edge_ids) = {
            let mut items = self.items.write().unwrap();
            items.insert(id, item);
            // Trim old short-term items (strict latest-N stack)
            self.trim_memory_type(&mut items, MemoryType::ShortTerm, self.max_short_term)
        };

        // Persist ShortTerm item to SQLite
        let _ = self.persist_item(id);

        // Delete trimmed ShortTerm items and edges from DB so history remains capped across restarts.
        if !removed_item_ids.is_empty() {
            debug!("Trimmed {} ShortTerm items", removed_item_ids.len());
            let _ = self.delete_items_from_db(&removed_item_ids);
        }
        if !removed_edge_ids.is_empty() {
            debug!("Removed {} edges from ShortTerm trim", removed_edge_ids.len());
            let _ = self.delete_edges_from_db(&removed_edge_ids);
        }

        self.bump_revision();
        id
    }

    pub fn add_action_result(
        &self,
        _input_text: &str,
        input_id: Option<Uuid>,
        output_text: String,
        action_type: ActionType,
    ) -> (Uuid, Option<Uuid>) {
        let item = MemoryItem::new_action_result(output_text.clone(), action_type, input_id);
        let output_id = item.id;
        debug!("Adding action result {} (type={:?}, input={:?})", output_id, action_type, input_id);

        let (removed_item_ids, removed_edge_ids) = {
            let mut items = self.items.write().unwrap();
            items.insert(output_id, item);
            // Trim old mid-term items
            self.trim_memory_type(&mut items, MemoryType::MidTerm, self.max_mid_term)
        };

        // Delete trimmed MidTerm items and edges from DB
        if !removed_item_ids.is_empty() {
            debug!("Trimmed {} MidTerm items", removed_item_ids.len());
            let _ = self.delete_items_from_db(&removed_item_ids);
        }
        if !removed_edge_ids.is_empty() {
            debug!("Removed {} edges from MidTerm trim", removed_edge_ids.len());
            let _ = self.delete_edges_from_db(&removed_edge_ids);
        }

        // Persist mid-term item to SQLite
        let _ = self.persist_item(output_id);

        // Create edge from input to output
        let edge_id = if let Some(in_id) = input_id {
            let relation = match action_type {
                ActionType::TranslateE2C | ActionType::TranslateC2E => RelationType::TranslatedTo,
                ActionType::Explain => RelationType::ExplainedBy,
                ActionType::Format => RelationType::FormattedTo,
                ActionType::UserQuery => RelationType::DerivedFrom,
            };
            let edge = MemoryEdge::new(in_id, output_id, relation);
            let eid = edge.id;
            self.edges.write().unwrap().insert(eid, edge);
            // Persist edge so action-result relations survive restarts
            let _ = self.persist_edge(eid);
            Some(eid)
        } else {
            None
        };

        self.bump_revision();
        (output_id, edge_id)
    }

    /// Trim items of a given type to max_count, removing oldest first.
    /// Returns (removed_item_ids, removed_edge_ids) so callers can delete from DB.
    fn trim_memory_type(
        &self,
        items: &mut HashMap<Uuid, MemoryItem>,
        mem_type: MemoryType,
        max_count: usize,
    ) -> (Vec<Uuid>, Vec<Uuid>) {
        let mut typed_items: Vec<_> = items
            .values()
            .filter(|i| i.memory_type == mem_type)
            .map(|i| (i.id, i.created_at))
            .collect();

        let mut removed_item_ids = Vec::new();
        let mut removed_edge_ids = Vec::new();

        if typed_items.len() > max_count {
            typed_items.sort_by_key(|(_, created)| *created);
            let to_remove = typed_items.len() - max_count;
            debug!("Trimming {} {:?} items (have {}, max {})", to_remove, mem_type, typed_items.len(), max_count);
            for (id, _) in typed_items.into_iter().take(to_remove) {
                items.remove(&id);
                removed_item_ids.push(id);
                // Also remove related edges and track their IDs for DB deletion
                let mut edges = self.edges.write().unwrap();
                let edge_ids_to_remove: Vec<Uuid> = edges
                    .iter()
                    .filter(|(_, e)| e.source_id == id || e.target_id == id)
                    .map(|(eid, _)| *eid)
                    .collect();
                for eid in &edge_ids_to_remove {
                    edges.remove(eid);
                }
                removed_edge_ids.extend(edge_ids_to_remove);
            }
        }

        (removed_item_ids, removed_edge_ids)
    }

    fn delete_items_from_db(&self, item_ids: &[Uuid]) -> Result<()> {
        if item_ids.is_empty() {
            return Ok(());
        }
        debug!("Deleting {} items from DB", item_ids.len());
        let mut conn = Connection::open(&self.db_path)?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("DELETE FROM memory_items WHERE id = ?1")?;
            for id in item_ids {
                let _ = stmt.execute(params![id.to_string()]);
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn delete_edges_from_db(&self, edge_ids: &[Uuid]) -> Result<()> {
        if edge_ids.is_empty() {
            return Ok(());
        }
        debug!("Deleting {} edges from DB", edge_ids.len());
        let mut conn = Connection::open(&self.db_path)?;
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("DELETE FROM memory_edges WHERE id = ?1")?;
            for id in edge_ids {
                let _ = stmt.execute(params![id.to_string()]);
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Clone-promote: creates a NEW node in the destination tier with a PromotedFrom edge
    /// Returns the new node's ID
    pub fn clone_promote_item(&self, item_id: Uuid, to_type: MemoryType) -> Result<Uuid> {
        // Get the original item
        let original = {
            let items = self.items.read().unwrap();
            items.get(&item_id).cloned()
                .ok_or_else(|| anyhow!("Item not found: {}", item_id))?
        };

        // Create a new item in the destination tier
        let new_item = MemoryItem {
            id: Uuid::new_v4(),
            title: original.title.clone(),
            content: original.content.clone(),
            preview: original.preview.clone(),
            memory_type: to_type,
            created_at: Utc::now(),
            promoted_at: Some(Utc::now()),
            metadata: MemoryMetadata::UserPromoted {
                original_id: item_id,
                tags: vec![],
            },
            position: None,
        };
        
        let new_id = new_item.id;

        // Insert new item
        {
            let mut items = self.items.write().unwrap();
            items.insert(new_id, new_item);
        }

        // Persist the new item (mid-term and long-term are persisted)
        if to_type == MemoryType::MidTerm || to_type == MemoryType::LongTerm {
            self.persist_item(new_id)?;
        }

        // Create PromotedFrom edge
        let edge = MemoryEdge::new(item_id, new_id, RelationType::PromotedFrom);
        let edge_id = edge.id;
        self.edges.write().unwrap().insert(edge_id, edge);

        // Always persist PromotedFrom edges
        let _ = self.persist_edge(edge_id);
        debug!("Created PromotedFrom edge {} ({} -> {})", edge_id, item_id, new_id);

        self.bump_revision();
        info!("Clone-promoted item {} -> {} ({:?})", item_id, new_id, to_type);
        Ok(new_id)
    }

    /// Legacy promote (mutates in place) - kept for compatibility but prefer clone_promote_item
    pub fn promote_item(&self, item_id: Uuid, to_type: MemoryType) -> Result<()> {
        {
            let mut items = self.items.write().unwrap();
            if let Some(item) = items.get_mut(&item_id) {
                item.promote_to(to_type);
            } else {
                return Err(anyhow!("Item not found: {}", item_id));
            }
        }

        // If promoting to mid-term or long-term, persist
        if to_type == MemoryType::MidTerm || to_type == MemoryType::LongTerm {
            self.persist_item(item_id)?;
        }

        self.bump_revision();
        Ok(())
    }

    fn persist_item(&self, item_id: Uuid) -> Result<()> {
        let items_guard = match self.items.read() {
            Ok(g) => g,
            Err(poison) => {
                log::error!("MemoryStore.items read lock poisoned; recovering");
                poison.into_inner()
            }
        };
        let item = items_guard
            .get(&item_id)
            .ok_or_else(|| anyhow!("Item not found"))?;

        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "INSERT OR REPLACE INTO memory_items 
             (id, content, preview, memory_type, created_at, promoted_at, metadata, pos_x, pos_y, title)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                item.id.to_string(),
                item.content,
                item.preview,
                format!("{:?}", item.memory_type),
                item.created_at.to_rfc3339(),
                item.promoted_at.map(|dt| dt.to_rfc3339()),
                serde_json::to_string(&item.metadata)?,
                item.position.map(|(x, _)| x),
                item.position.map(|(_, y)| y),
                item.title,
            ],
        )?;

        info!("Persisted item {} to long-term memory", item_id);
        Ok(())
    }

    pub fn persist_edge(&self, edge_id: Uuid) -> Result<()> {
        let edges = self.edges.read().unwrap();
        let edge = edges
            .get(&edge_id)
            .ok_or_else(|| anyhow!("Edge not found"))?;

        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "INSERT OR REPLACE INTO memory_edges 
             (id, source_id, target_id, relation, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                edge.id.to_string(),
                edge.source_id.to_string(),
                edge.target_id.to_string(),
                format!("{:?}", edge.relation),
                edge.created_at.to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    pub fn add_user_edge(&self, source_id: Uuid, target_id: Uuid) -> Option<Uuid> {
        let items = self.items.read().unwrap();
        if !items.contains_key(&source_id) || !items.contains_key(&target_id) {
            return None;
        }
        drop(items);

        let edge = MemoryEdge::new(source_id, target_id, RelationType::UserLinked);
        let id = edge.id;
        debug!("Adding user edge {} ({} -> {})", id, source_id, target_id);
        self.edges.write().unwrap().insert(id, edge);
        // Persist user-created edges immediately
        let _ = self.persist_edge(id);
        self.bump_revision();
        Some(id)
    }

    pub fn delete_item(&self, item_id: Uuid) -> Result<()> {
        debug!("Deleting item {}", item_id);
        let mut items = self.items.write().unwrap();
        if let Some(item) = items.remove(&item_id) {
            // Remove from DB for all tiers (ShortTerm is now persisted too)
            debug!("Removed {:?} item {} from memory", item.memory_type, item_id);
            if let Ok(conn) = Connection::open(&self.db_path) {
                let _ = conn.execute(
                    "DELETE FROM memory_items WHERE id = ?1",
                    params![item_id.to_string()],
                );
            }
        } else {
            warn!("Attempted to delete non-existent item {}", item_id);
        }
        drop(items);

        // Remove related edges from memory and DB
        let edge_ids: Vec<Uuid> = self.edges
            .read()
            .unwrap()
            .iter()
            .filter(|(_, e)| e.source_id == item_id || e.target_id == item_id)
            .map(|(id, _)| *id)
            .collect();

        if !edge_ids.is_empty() {
            debug!("Removing {} related edges for item {}", edge_ids.len(), item_id);
            let mut edges = self.edges.write().unwrap();
            for eid in &edge_ids {
                edges.remove(eid);
            }
            drop(edges);
            // Also delete edges from DB
            let _ = self.delete_edges_from_db(&edge_ids);
        }

        self.bump_revision();
        Ok(())
    }

    pub fn delete_edge(&self, edge_id: Uuid) {
        debug!("Deleting edge {}", edge_id);
        self.edges.write().unwrap().remove(&edge_id);
        
        if let Ok(conn) = Connection::open(&self.db_path) {
            let _ = conn.execute(
                "DELETE FROM memory_edges WHERE id = ?1",
                params![edge_id.to_string()],
            );
        }
        self.bump_revision();
    }

    pub fn search_long_term(&self, query: &str) -> Vec<MemoryItem> {
        let items = self.items.read().unwrap();
        let query_lower = query.to_lowercase();
        
        items
            .values()
            .filter(|i| i.memory_type == MemoryType::LongTerm)
            .filter(|i| i.content.to_lowercase().contains(&query_lower))
            .cloned()
            .collect()
    }

    pub fn get_items_by_type(&self, mem_type: MemoryType) -> Vec<MemoryItem> {
        self.items
            .read()
            .unwrap()
            .values()
            .filter(|i| i.memory_type == mem_type)
            .cloned()
            .collect()
    }

    pub fn get_all_items(&self) -> Vec<MemoryItem> {
        self.items.read().unwrap().values().cloned().collect()
    }

    pub fn get_all_edges(&self) -> Vec<MemoryEdge> {
        self.edges.read().unwrap().values().cloned().collect()
    }

    pub fn get_item(&self, id: Uuid) -> Option<MemoryItem> {
        match self.items.read() {
            Ok(g) => g.get(&id).cloned(),
            Err(poison) => {
                log::error!("MemoryStore.items read lock poisoned while get_item; recovering");
                let g = poison.into_inner();
                g.get(&id).cloned()
            }
        }
    }

    pub fn update_item_position(&self, id: Uuid, x: f32, y: f32) {
        let mut should_persist = false;
        {
            // Acquire write lock, recovering from poison if necessary
            let mut items_guard = match self.items.write() {
                Ok(g) => g,
                Err(poison) => {
                    log::error!("MemoryStore.items write lock poisoned; recovering");
                    poison.into_inner()
                }
            };
            if let Some(item) = items_guard.get_mut(&id) {
                item.position = Some((x, y));
                should_persist = item.memory_type == MemoryType::MidTerm || item.memory_type == MemoryType::LongTerm;
            }
        }
        if should_persist {
            let _ = self.persist_item(id);
        }
        self.bump_revision();
    }

    /// Clear all stored positions (for Auto Align feature)
    pub fn clear_all_positions(&self) -> Result<()> {
        // Clear in-memory positions
        {
            let mut items = self.items.write().unwrap();
            for item in items.values_mut() {
                item.position = None;
            }
        }
        // Clear positions in database
        let conn = Connection::open(&self.db_path)?;
        conn.execute("UPDATE memory_items SET pos_x = NULL, pos_y = NULL", [])?;
        self.bump_revision();
        info!("Cleared all item positions");
        Ok(())
    }

    /// Update an item's title. Persists to SQLite for all item types.
    pub fn update_item_title(&self, id: Uuid, title: String) {
        let item_exists = {
            let mut items = self.items.write().unwrap();
            if let Some(item) = items.get_mut(&id) {
                let new_title = if title.trim().is_empty() { None } else { Some(title.clone()) };
                info!("Updating title for item {}: {:?} -> {:?}", id, item.title, new_title);
                item.title = new_title;
                true
            } else {
                warn!("update_item_title: item {} not found", id);
                false
            }
        };
        if item_exists {
            if let Err(e) = self.persist_item(id) {
                log::error!("Failed to persist item {} after title update: {}", id, e);
            } else {
                debug!("Successfully persisted title update for item {}", id);
            }
        }
        self.bump_revision();
    }

    /// Get items by type with limit, sorted by recency (newest first)
    /// For LongTerm, sorts by promoted_at; otherwise by created_at
    pub fn list_items_by_type(&self, mem_type: MemoryType, limit: usize, offset: usize) -> Vec<MemoryItem> {
        let items = self.items.read().unwrap();
        let mut typed_items: Vec<_> = items
            .values()
            .filter(|i| i.memory_type == mem_type)
            .cloned()
            .collect();
        
        // Sort by recency (newest first)
        typed_items.sort_by(|a, b| {
            let a_time = if mem_type == MemoryType::LongTerm {
                a.promoted_at.unwrap_or(a.created_at)
            } else {
                a.created_at
            };
            let b_time = if mem_type == MemoryType::LongTerm {
                b.promoted_at.unwrap_or(b.created_at)
            } else {
                b.created_at
            };
            b_time.cmp(&a_time) // Descending (newest first)
        });
        
        typed_items.into_iter().skip(offset).take(limit).collect()
    }

    /// Get edges where BOTH endpoints are in the given set of item IDs
    pub fn list_edges_for_items(&self, item_ids: &std::collections::HashSet<Uuid>) -> Vec<MemoryEdge> {
        self.edges
            .read()
            .unwrap()
            .values()
            .filter(|e| item_ids.contains(&e.source_id) && item_ids.contains(&e.target_id))
            .cloned()
            .collect()
    }

    /// Check if an item exists
    pub fn item_exists(&self, id: Uuid) -> bool {
        self.items.read().unwrap().contains_key(&id)
    }

    pub fn find_input_for_clipboard(&self, content: &str) -> Option<Uuid> {
        self.items
            .read()
            .unwrap()
            .values()
            .filter(|i| i.memory_type == MemoryType::ShortTerm && i.content == content)
            .max_by_key(|i| i.created_at)
            .map(|i| i.id)
    }
}
