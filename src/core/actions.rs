use std::sync::Arc;
use anyhow::{Result, anyhow};
use log::{info, error};
use crate::core::clipboard::{ClipboardManager, ClipboardContentType};
use crate::core::memory::MemoryEvent;
use crate::core::memory_store::MemoryStore;
use crate::core::memory::ActionType as MemActionType;
use crate::api::client::LlmClient;
use crate::ui::UiEvent;

/// Action now carries the action ID from config, or UserQuery with text
#[derive(Debug, Clone)]
pub enum Action {
    /// Execute a configured action by its ID
    Execute(String),
    /// User-provided query text (special case)
    UserQuery(String),
    /// Vision action with base64-encoded image data
    Vision { action_id: String, image_base64: String },
}

impl Action {
    /// Convert action name to Action enum (normalizes to lowercase)
    pub fn from_name(name: &str) -> Option<Self> {
        if name.is_empty() {
            None
        } else {
            // Normalize to lowercase for consistent matching with action IDs
            Some(Action::Execute(name.to_lowercase()))
        }
    }
    
    /// Get the action ID for memory storage
    pub fn to_mem_action_type(&self) -> MemActionType {
        match self {
            Action::Execute(id) | Action::Vision { action_id: id, .. } => match id.as_str() {
                "format" => MemActionType::Format,
                "translate_e2c" => MemActionType::TranslateE2C,
                "translate_c2e" => MemActionType::TranslateC2E,
                "explain" => MemActionType::Explain,
                _ => MemActionType::UserQuery, // Fallback for custom actions
            },
            Action::UserQuery(_) => MemActionType::UserQuery,
        }
    }
    
    /// Get the action ID string
    pub fn action_id(&self) -> &str {
        match self {
            Action::Execute(id) => id,
            Action::Vision { action_id, .. } => action_id,
            Action::UserQuery(_) => "user_query",
        }
    }
}

pub struct ActionHandler {
    clipboard: Arc<ClipboardManager>,
    llm_client: Arc<LlmClient>,
    ui_tx: Option<flume::Sender<UiEvent>>,
    graph_tx: Option<tokio::sync::mpsc::Sender<MemoryEvent>>,
    memory_store: Option<Arc<MemoryStore>>,
}

impl ActionHandler {
    pub fn new(
        clipboard: Arc<ClipboardManager>,
        llm_client: Arc<LlmClient>,
        ui_tx: Option<flume::Sender<UiEvent>>,
        graph_tx: Option<tokio::sync::mpsc::Sender<MemoryEvent>>,
        memory_store: Option<Arc<MemoryStore>>,
    ) -> Self {
        Self {
            clipboard,
            llm_client,
            ui_tx,
            graph_tx,
            memory_store,
        }
    }
    
    /// Get the display label for an action
    fn get_action_label(&self, action_id: &str) -> String {
        self.llm_client.get_action_label(action_id)
            .unwrap_or_else(|| action_id.to_string())
    }

    pub async fn handle(&self, action: Action) -> Result<()> {
        info!("Handling action: {:?}", action);

        // Handle vision action directly
        if let Action::Vision { action_id, image_base64 } = &action {
            let label = self.get_action_label(action_id);
            
            // Send UI update with action label
            if let Some(tx) = &self.ui_tx {
                let _ = tx.send(UiEvent::ProcessingStarted(label));
            }
            
            // Execute vision action
            let result = self.llm_client.execute_vision_action(action_id, image_base64).await;
            
            if let Some(tx) = &self.ui_tx {
                match &result {
                    Ok(extracted_text) => {
                        // Write extracted text to clipboard
                        if let Err(e) = self.clipboard.set_text_programmatic(extracted_text) {
                            error!("Failed to write OCR result to clipboard: {}", e);
                        }
                        let _ = tx.send(UiEvent::ShowResult("[Image]".to_string(), extracted_text.clone()));
                    }
                    Err(e) => {
                        let _ = tx.send(UiEvent::StreamError(e.to_string()));
                    }
                }
            }
            
            return result.map(|_| ());
        }

        // For UserQuery, use the query text directly instead of clipboard
        let (text, is_user_query, action_id) = match &action {
            Action::UserQuery(query) => (query.clone(), true, "user_query".to_string()),
            Action::Execute(id) => {
                // Check if this is a vision action and clipboard has image
                if self.llm_client.is_vision_action(id) {
                    match self.clipboard.content_type() {
                        ClipboardContentType::Image => {
                            // Convert to vision action
                            match self.clipboard.get_image_as_base64_png() {
                                Ok(base64) => {
                                    let vision_action = Action::Vision {
                                        action_id: id.clone(),
                                        image_base64: base64,
                                    };
                                    // Recursively handle as vision action
                                    return Box::pin(self.handle(vision_action)).await;
                                }
                                Err(e) => {
                                    error!("Failed to encode image: {}", e);
                                    return Err(anyhow!("Failed to encode clipboard image: {}", e));
                                }
                            }
                        }
                        ClipboardContentType::Text => {
                            // Vision action but text in clipboard - fall through to text handling
                            info!("Vision action {} but clipboard has text, processing as text", id);
                        }
                        ClipboardContentType::Empty => {
                            return Err(anyhow!("Clipboard is empty"));
                        }
                    }
                }
                
                // Text path from clipboard
                match self.clipboard.get_text() {
                    Ok(t) => (t, false, id.clone()),
                    Err(e) => {
                        error!("Failed to get text from clipboard: {}", e);
                        return Err(anyhow!("Clipboard empty or invalid"));
                    }
                }
            }
            Action::Vision { .. } => unreachable!(), // Handled above
        };

        if text.trim().is_empty() {
             return Ok(());
        }

        // Get action label for UI
        let label = self.get_action_label(&action_id);

        // Send UI update "Processing..." with action label
        if let Some(tx) = &self.ui_tx {
            let _ = tx.send(UiEvent::ProcessingStarted(label));
        }

        // Execute the action via LLM client
        let result = self.llm_client.execute_action(&action_id, &text).await;

        // Send UI update "Finished" or "Error"
        if let Some(tx) = &self.ui_tx {
            match &result {
                Ok(processed) => {
                    // Store to mid-term memory
                    if let Some(store) = &self.memory_store {
                        let input_id = store.find_input_for_clipboard(&text);
                        let action_type = action.to_mem_action_type();

                        if let Some(graph_tx) = &self.graph_tx {
                            let _ = graph_tx
                                .send(MemoryEvent::AddActionResult {
                                    input_text: text.clone(),
                                    input_id,
                                    output_text: processed.clone(),
                                    action_type,
                                })
                                .await;
                        } else {
                            store.add_action_result(&text, input_id, processed.clone(), action_type);
                        }
                    }
                    
                    // For user queries, don't modify clipboard - just show result
                    if !is_user_query {
                        // Update clipboard with result
                        if let Err(e) = self.clipboard.set_text_programmatic(processed) {
                            error!("Failed to write to clipboard: {}", e);
                        }
                    }
                    
                    // Send ShowResult to finalize the UI
                    let _ = tx.send(UiEvent::ShowResult(text.clone(), processed.clone()));
                },
                Err(e) => {
                    let _ = tx.send(UiEvent::StreamError(e.to_string()));
                }
            }
        }

        result.map(|_| ())
    }
}
