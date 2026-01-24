use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryType {
    ShortTerm, // Raw clipboard captures
    MidTerm,   // Action results (translations, explanations, formats)
    LongTerm,  // User-promoted items with persistence
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionType {
    Format,
    TranslateE2C,
    TranslateC2E,
    Explain,
    UserQuery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemoryMetadata {
    Clipboard,
    ActionResult {
        action_type: ActionType,
        input_id: Option<Uuid>,
    },
    UserPromoted {
        original_id: Uuid,
        tags: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryItem {
    pub id: Uuid,
    pub title: Option<String>,
    pub content: String,
    pub preview: String,
    pub memory_type: MemoryType,
    pub created_at: DateTime<Utc>,
    pub promoted_at: Option<DateTime<Utc>>,
    pub metadata: MemoryMetadata,
    pub position: Option<(f32, f32)>,
}

impl MemoryItem {
    pub fn new_clipboard(content: String) -> Self {
        let preview = content.chars().take(50).collect::<String>();
        Self {
            id: Uuid::new_v4(),
            title: None,
            content,
            preview,
            memory_type: MemoryType::ShortTerm,
            created_at: Utc::now(),
            promoted_at: None,
            metadata: MemoryMetadata::Clipboard,
            position: None,
        }
    }

    pub fn new_action_result(
        content: String,
        action_type: ActionType,
        input_id: Option<Uuid>,
    ) -> Self {
        let preview = content.chars().take(50).collect::<String>();
        Self {
            id: Uuid::new_v4(),
            title: None,
            content,
            preview,
            memory_type: MemoryType::MidTerm,
            created_at: Utc::now(),
            promoted_at: None,
            metadata: MemoryMetadata::ActionResult {
                action_type,
                input_id,
            },
            position: None,
        }
    }

    pub fn promote_to(&mut self, new_type: MemoryType) {
        if new_type == MemoryType::LongTerm && self.memory_type != MemoryType::LongTerm {
            self.promoted_at = Some(Utc::now());
        }
        self.memory_type = new_type;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelationType {
    DerivedFrom,
    TranslatedTo,
    ExplainedBy,
    FormattedTo,
    PromotedFrom,
    UserLinked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEdge {
    pub id: Uuid,
    pub source_id: Uuid,
    pub target_id: Uuid,
    pub relation: RelationType,
    pub created_at: DateTime<Utc>,
}

impl MemoryEdge {
    pub fn new(source_id: Uuid, target_id: Uuid, relation: RelationType) -> Self {
        Self {
            id: Uuid::new_v4(),
            source_id,
            target_id,
            relation,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum MemoryEvent {
    AddClipboard(String),
    AddActionResult {
        input_text: String,
        input_id: Option<Uuid>,
        output_text: String,
        action_type: ActionType,
    },
    PromoteItem {
        item_id: Uuid,
        to_type: MemoryType,
    },
    AddUserEdge {
        source_id: Uuid,
        target_id: Uuid,
    },
    DeleteItem(Uuid),
    DeleteEdge(Uuid),
}
