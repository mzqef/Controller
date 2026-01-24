use std::sync::Arc;
use std::sync::mpsc;
use anyhow::{Result, anyhow};
use log::{info, error};
use crate::core::clipboard::ClipboardManager;
use crate::core::memory::MemoryEvent;
use crate::core::memory_store::MemoryStore;
use crate::core::memory::ActionType as MemActionType;
use crate::api::client::LlmClient;
use crate::ui::UiEvent;

#[derive(Debug, Clone)]
pub enum Action {
    Format,
    TranslateE2C,
    TranslateC2E,
    Explain,
    UserQuery(String), // User-provided query text
}

pub struct ActionHandler {
    clipboard: Arc<ClipboardManager>,
    llm_client: Arc<LlmClient>,
    ui_tx: Option<mpsc::Sender<UiEvent>>,
    graph_tx: Option<tokio::sync::mpsc::Sender<MemoryEvent>>,
    memory_store: Option<Arc<MemoryStore>>,
}

impl ActionHandler {
    pub fn new(
        clipboard: Arc<ClipboardManager>,
        llm_client: Arc<LlmClient>,
        ui_tx: Option<mpsc::Sender<UiEvent>>,
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

    pub async fn handle(&self, action: Action) -> Result<()> {
        info!("Handling action: {:?}", action);

        // Check for Image first (VLM/OCR path)
        // Note: arboard image handling might be tricky. 
        // For this refactor we prioritize Text, but structure is here for Image.
        /* 
        if let Ok(_img) = self.clipboard.get_image() {
             info!("Image detected in clipboard. VLM/OCR not fully implemented yet.");
             // TODO: specific VLM logic
             // return Ok(());
        }
        */

        // For UserQuery, use the query text directly instead of clipboard
        let (text, is_user_query) = match &action {
            Action::UserQuery(query) => (query.clone(), true),
            _ => {
                // Text path from clipboard
                match self.clipboard.get_text() {
                    Ok(t) => (t, false),
                    Err(e) => {
                        error!("Failed to get text from clipboard: {}", e);
                        return Err(anyhow!("Clipboard empty or invalid"));
                    }
                }
            }
        };

        if text.trim().is_empty() {
             return Ok(());
        }

        // Send UI update "Processing..."
        if let Some(tx) = &self.ui_tx {
            let _ = tx.send(UiEvent::ProcessingStarted);
        }

        let result = match &action {
            Action::Format => self.process_format(&text).await,
            Action::TranslateE2C => self.process_translate(&text, "English", "Chinese").await,
            Action::TranslateC2E => self.process_translate(&text, "Chinese", "English").await,
            Action::Explain => self.process_explain(&text).await,
            Action::UserQuery(_) => self.process_user_query(&text).await,
        };

        // Send UI update "Finished" or "Error"
        if let Some(tx) = &self.ui_tx {
            match &result {
                Ok(processed) => {
                    // Store to mid-term memory
                    if let Some(store) = &self.memory_store {
                        let input_id = store.find_input_for_clipboard(&text);
                        let action_type = match &action {
                            Action::Format => MemActionType::Format,
                            Action::TranslateE2C => MemActionType::TranslateE2C,
                            Action::TranslateC2E => MemActionType::TranslateC2E,
                            Action::Explain => MemActionType::Explain,
                            Action::UserQuery(_) => MemActionType::UserQuery,
                        };

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
                        // Replace clipboard with processed output only
                        if let Err(e) = self.clipboard.set_text_programmatic(processed) {
                            error!("Failed to write to clipboard: {}", e);
                        }
                    }
                    let _ = tx.send(UiEvent::ShowResult(text.clone(), processed.clone()));
                },
                Err(e) => {
                    let _ = tx.send(UiEvent::StreamError(e.to_string()));
                }
            }
        }

        result.map(|_| ())
    }

    async fn process_format(&self, text: &str) -> Result<String> {
        self.llm_client.chat_completion( "copy_check", text).await
    }

    async fn process_translate(&self, text: &str, source: &str, target: &str) -> Result<String> {
        // We use a specialized method for translation if needed, or chat_completion with specific prompt
        self.llm_client.translate(text, source, target).await
    }

    async fn process_explain(&self, text: &str) -> Result<String> {
        self.llm_client.chat_completion("explain", text).await
    }

    async fn process_user_query(&self, text: &str) -> Result<String> {
        // Use the streaming-enabled user query method with remote/local fallback
        self.llm_client.user_query_streaming(text).await
    }
}
