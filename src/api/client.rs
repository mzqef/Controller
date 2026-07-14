//! LLM API client with streaming and remote/local fallback.
//!
//! This module provides the [`LlmClient`] which handles:
//! - Remote API calls with streaming responses
//! - Automatic fallback to local LLM when remote is unavailable
//! - Per-action configuration with extra parameters passthrough
//! - Real-time UI updates during streaming

use serde::{Deserialize, Serialize};
use reqwest::Client;
use std::time::Duration;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use anyhow::Result;
use log::{info, warn, error, debug};
use std::collections::HashMap;
use eventsource_stream::Eventsource;
use futures_util::StreamExt;

use crate::core::config::{ActionsConfig, ActionDefinition};

// Helper to detect network errors (timeout, connection, DNS, etc)
fn is_network_error(e: &anyhow::Error) -> bool {
    let s = e.to_string();
    s.contains("timed out") || s.contains("connection") || s.contains("dns") || s.contains("unreachable") || s.contains("network")
}

/// Content part for multimodal messages (text or image)
#[derive(Serialize, Clone)]
#[serde(tag = "type")]
pub enum ContentPart {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrlData },
}

/// Image URL data with optional pixel constraints
#[derive(Serialize, Clone)]
pub struct ImageUrlData {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_pixels: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_pixels: Option<u32>,
}

/// Message content: either simple text or multimodal array
#[derive(Serialize, Clone)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Multimodal(Vec<ContentPart>),
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    #[serde(skip_deserializing)]
    pub content: MessageContent,
}

// For deserialization (responses are always text)
impl Default for MessageContent {
    fn default() -> Self {
        MessageContent::Text(String::new())
    }
}

// Helper to create text-only ChatMessage
impl ChatMessage {
    pub fn user_text(content: String) -> Self {
        Self {
            role: "user".to_string(),
            content: MessageContent::Text(content),
        }
    }
    
    pub fn user_multimodal(parts: Vec<ContentPart>) -> Self {
        Self {
            role: "user".to_string(),
            content: MessageContent::Multimodal(parts),
        }
    }
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    translation_options: Option<TranslationOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
    /// Extra parameters passed through from config (flattened into the JSON)
    #[serde(flatten, skip_serializing_if = "HashMap::is_empty")]
    extra: HashMap<String, serde_json::Value>,
}

#[derive(Serialize)]
struct TranslationOptions {
    source_lang: String,
    target_lang: String,
}

/// Response message (always text content)
#[derive(Deserialize)]
struct ChatResponseMessage {
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatResponseMessage,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ChatChunkResponse {
    choices: Vec<ChatChunkChoice>,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ChatChunkChoice {
    delta: ChatChunkDelta,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ChatChunkDelta {
    content: Option<String>,
}

/// LLM client with remote/local fallback and streaming support.
///
/// # Features
/// - Lazy HTTP client initialization
/// - Automatic fallback to local LLM on network errors
/// - Streaming responses with real-time UI updates
/// - Per-action configuration with extra parameters
///
/// # Example
/// ```ignore
/// let client = LlmClient::new(config);
/// client.set_ui_tx(ui_sender);
/// let result = client.process_with_action("format", "text to process").await?;
/// ```
pub struct LlmClient {
    client: tokio::sync::OnceCell<Client>,
    config: Arc<RwLock<ActionsConfig>>,
    remote_available: Arc<AtomicBool>,
    force_local: Arc<AtomicBool>,
    ui_tx: Option<flume::Sender<crate::ui::UiEvent>>,
    cancel_flag: Option<Arc<AtomicBool>>,
    /// egui context for triggering UI repaints when streaming
    egui_ctx: Arc<std::sync::Mutex<Option<eframe::egui::Context>>>,
}

impl LlmClient {
    pub fn new(config: Arc<RwLock<ActionsConfig>>) -> Self {
        Self { 
            client: tokio::sync::OnceCell::new(), 
            config,
            remote_available: Arc::new(AtomicBool::new(true)),
            force_local: Arc::new(AtomicBool::new(false)),
            ui_tx: None,
            cancel_flag: None,
            egui_ctx: Arc::new(std::sync::Mutex::new(None)),
        }
    }

    fn config_snapshot(&self) -> ActionsConfig {
        self.config
            .read()
            .map(|config| config.clone())
            .unwrap_or_else(|_| ActionsConfig::default())
    }
    
    pub fn set_ui_tx(&mut self, tx: flume::Sender<crate::ui::UiEvent>) {
        self.ui_tx = Some(tx);
    }
    
    /// Set the shared egui context holder for triggering repaints during streaming
    pub fn set_egui_ctx(&mut self, ctx: Arc<std::sync::Mutex<Option<eframe::egui::Context>>>) {
        self.egui_ctx = ctx;
    }
    
    pub fn set_cancel_flag(&mut self, flag: Arc<AtomicBool>) {
        self.cancel_flag = Some(flag);
    }
    
    /// Set force local mode (user-controlled toggle)
    pub fn set_force_local(&self, force: bool) {
        self.force_local.store(force, Ordering::Relaxed);
    }
    
    /// Get current force local mode state
    pub fn is_force_local(&self) -> bool {
        self.force_local.load(Ordering::Relaxed)
    }
    
    /// Check if cancellation was requested
    fn is_cancelled(&self) -> bool {
        self.cancel_flag.as_ref().map_or(false, |f| f.load(Ordering::Relaxed))
    }
    
    async fn get_client(&self) -> &Client {
        self.client.get_or_init(|| async {
            let timeout = self.config_snapshot().defaults.timeout_ms.unwrap_or(60000) / 1000;
            Client::builder()
                .timeout(Duration::from_secs(timeout))
                .tcp_nodelay(true)
                .http1_only()
                .no_proxy() // Disable proxy
                .build()
                .unwrap_or_default()
        }).await
    }
    
    // Get action definition by ID
    fn get_action(&self, action_id: &str) -> Option<ActionDefinition> {
        self.config_snapshot().get_action(action_id).cloned()
    }
    
    /// Get the display label for an action
    pub fn get_action_label(&self, action_id: &str) -> Option<String> {
        self.get_action(action_id).map(|a| a.label().to_string())
    }
    
    /// Check if an action is a vision action
    pub fn is_vision_action(&self, action_id: &str) -> bool {
        self.get_action(action_id)
            .and_then(|a| a.remote)
            .map(|r| r.is_vision)
            .unwrap_or(false)
    }

    fn expand_env(val: &str) -> String {
        if val.starts_with("${") && val.ends_with("}") {
            let var = &val[2..val.len()-1];
            std::env::var(var).unwrap_or_else(|_| val.to_string())
        } else {
            val.to_string()
        }
    }

    /// Resolves endpoint, model, api_key, prompt, temperature, translation options, and extra params for an action.
    /// Returns: (url, model, api_key_opt, prompt_opt, temperature, translation_options, extra)
    fn resolve_action_config(&self, config: &ActionsConfig, action: &ActionDefinition, force_local: bool) -> Result<(String, String, Option<String>, Option<String>, f32, Option<HashMap<String, String>>, HashMap<String, serde_json::Value>)> {
        let defaults = &config.defaults;
        let use_local = force_local || config.force_local || self.force_local.load(Ordering::Relaxed) || !self.remote_available.load(Ordering::Relaxed);

        if use_local {
            // Local config: merge action.local with defaults.local
            let local_defaults = defaults.local.as_ref();
            let action_local = action.local.as_ref();
            
            let url = action_local.and_then(|l| l.api_url.clone())
                .or_else(|| local_defaults.and_then(|l| l.api_url.clone()))
                .ok_or_else(|| anyhow::anyhow!("No local API URL configured for action {}", action.id))?;
            let url = if url.ends_with("chat/completions") { url } else { format!("{}/chat/completions", url.trim_end_matches('/')) };
            
            let model = action_local.and_then(|l| l.model.clone())
                .or_else(|| local_defaults.and_then(|l| l.model.clone()))
                .unwrap_or_else(|| "local-model".to_string());
            
            // Resolve local API key: action → defaults → env var (LOCAL_API_KEY)
            let api_key = action_local.and_then(|l| l.api_key.clone())
                .or_else(|| local_defaults.and_then(|l| l.api_key.clone()))
                .or_else(|| std::env::var("LOCAL_API_KEY").ok())
                .map(|k| Self::expand_env(&k));
            
            let prompt = action_local.and_then(|l| l.prompt.clone())
                .or_else(|| action.remote.as_ref().and_then(|r| r.prompt.clone()));
            
            let temperature = action.remote.as_ref().and_then(|r| r.temperature)
                .or(defaults.temperature).unwrap_or(0.1);
            
            // Collect extra params from local config
            let extra = action_local.map(|l| l.extra.clone()).unwrap_or_default();
            
            Ok((url, model, api_key, prompt, temperature, None, extra))
        } else {
            // Remote config: merge action.remote with defaults
            let action_remote = action.remote.as_ref();
            
            let url = action_remote.and_then(|r| r.api_url.clone())
                .or_else(|| defaults.api_url.clone())
                .ok_or_else(|| anyhow::anyhow!("No API URL configured for action {}", action.id))?;
            let url = if url.ends_with("chat/completions") { url } else { format!("{}/chat/completions", url.trim_end_matches('/')) };
            
            let model = action_remote.and_then(|r| r.model.clone())
                .or_else(|| defaults.model.clone())
                .unwrap_or_else(|| "gpt-4".to_string());
            
            let api_key = action_remote.and_then(|r| r.api_key.clone())
                .or_else(|| defaults.api_key.clone())
                .map(|k| Self::expand_env(&k));
            
            let prompt = action_remote.and_then(|r| r.prompt.clone());
            
            let temperature = action_remote.and_then(|r| r.temperature)
                .or(defaults.temperature).unwrap_or(0.1);
            
            // Build translation options if this is a translation action
            let translation_options = if action_remote.is_some_and(|r| r.is_translation) {
                let mut opts: HashMap<String, String> = HashMap::new();
                if let Some(ref src) = action_remote.and_then(|r| r.source_lang.clone()) {
                    opts.insert("source_lang".to_string(), src.clone());
                }
                if let Some(ref tgt) = action_remote.and_then(|r| r.target_lang.clone()) {
                    opts.insert("target_lang".to_string(), tgt.clone());
                }
                if !opts.is_empty() { Some(opts) } else { None }
            } else {
                None
            };
            
            // Collect extra params from remote config
            let extra = action_remote.map(|r| r.extra.clone()).unwrap_or_default();
            
            Ok((url, model, api_key, prompt, temperature, translation_options, extra))
        }
    }

    // Spawn health check MUST be called inside a runtime
    pub fn spawn_health_check(self: &Arc<Self>) {
        let llm_client = self.clone();
        let remote_available = self.remote_available.clone();
        
        tokio::spawn(async move {
            let check_client = Client::builder()
                .timeout(Duration::from_secs(15))
                .tcp_nodelay(true)
                .build()
                .unwrap_or_default();

            loop {
                // Initial delay
                tokio::time::sleep(Duration::from_secs(60)).await;

                let config = llm_client.config_snapshot();
                let defaults = &config.defaults;
                let url = defaults.api_url.clone().unwrap_or_else(|| "https://api.openai.com/v1".to_string());
                let url = if url.ends_with("chat/completions") { url } else { format!("{}/chat/completions", url.trim_end_matches('/')) };
                let api_key = defaults.api_key.clone().map(|k| Self::expand_env(&k)).unwrap_or_default();

                // Use GET instead of HEAD for better compatibility. 
                // We accept any response that indicates the server received the request (even 4xx/5xx).
                // Connectivity is what we are checking, not service health per se.
                let result = check_client
                    .get(&url)
                    .header("Authorization", format!("Bearer {}", api_key))
                    .send()
                    .await;

                match result {
                     Ok(_) => {
                        // Any response means we reached the server
                        if !remote_available.load(Ordering::Relaxed) {
                            info!("Remote API connectivity detected, switching back to remote");
                            remote_available.store(true, Ordering::Relaxed);
                        }
                    }
                    Err(e) => {
                        // Only switch to local on network errors (timeout, dns, etc)
                         if remote_available.load(Ordering::Relaxed) {
                            warn!("Remote API unreachable ({}). Switching to local fallback.", e);
                            remote_available.store(false, Ordering::Relaxed);
                        }
                    }
                }
            }
        });
    }

    /// Execute an action by ID with remote/local fallback
    pub async fn execute_action(&self, action_id: &str, text: &str) -> Result<String> {
        let config = self.config_snapshot();
        let action = config.get_action(action_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Unknown action: {}", action_id))?;
        let local_mode = config.force_local
            || self.force_local.load(Ordering::Relaxed)
            || !self.remote_available.load(Ordering::Relaxed);
        
        if !local_mode {
            match self.resolve_action_config(&config, &action, false) {
                Ok((url, model, api_key, prompt, temperature, translation_options, extra)) => {
                    match self.execute_request(&url, &model, api_key, prompt.as_deref(), text, temperature, translation_options, extra).await {
                        Ok(res) => return Ok(res),
                        Err(e) => {
                            if is_network_error(&e) {
                                warn!("Remote API failed for {}, falling back to local: {}", action_id, e);
                                self.remote_available.store(false, Ordering::Relaxed);
                            } else {
                                return Err(e);
                            }
                        }
                    }
                },
                Err(e) => {
                    warn!("No remote config for {}: {}", action_id, e);
                }
            }
        }
        
        // Try local fallback
        let (url, model, api_key, prompt, temperature, translation_options, extra) = self.resolve_action_config(&config, &action, true)?;
        self.execute_request(&url, &model, api_key, prompt.as_deref(), text, temperature, translation_options, extra).await
    }

    /// Legacy chat_completion - maps to execute_action
    pub async fn chat_completion(&self, action_id: &str, text: &str) -> Result<String> {
        self.execute_action(action_id, text).await
    }

    /// User query with streaming support and remote/local fallback
    pub async fn user_query_streaming(&self, text: &str) -> Result<String> {
        self.execute_action("user_query", text).await
    }

    /// Translate text - uses the appropriate translation action based on source/target
    pub async fn translate(&self, text: &str, source: &str, target: &str) -> Result<String> {
        // Determine action ID based on languages
        let action_id = if (source.eq_ignore_ascii_case("English") || source.eq_ignore_ascii_case("en") || source.eq_ignore_ascii_case("auto")) && 
                           (target.eq_ignore_ascii_case("Chinese") || target.eq_ignore_ascii_case("zh")) {
            "translate_e2c"
        } else if (source.eq_ignore_ascii_case("Chinese") || source.eq_ignore_ascii_case("zh") || source.eq_ignore_ascii_case("auto")) && 
                  (target.eq_ignore_ascii_case("English") || target.eq_ignore_ascii_case("en")) {
            "translate_c2e"
        } else {
            return Err(anyhow::anyhow!("Unsupported translation pair: {} -> {}", source, target));
        };

        self.execute_action(action_id, text).await
    }

    /// Execute a vision action with base64-encoded image
    /// 
    /// # Arguments
    /// * `action_id` - The vision action ID (must have `is_vision: true`)
    /// * `image_base64` - Base64-encoded PNG image data
    /// 
    /// # Returns
    /// Extracted text from the image
    pub async fn execute_vision_action(&self, action_id: &str, image_base64: &str) -> Result<String> {
        let config = self.config_snapshot();
        let action = config.get_action(action_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Unknown action: {}", action_id))?;
        
        // Get config
        let (url, model, api_key, prompt, temperature, _, extra) = self.resolve_action_config(&config, &action, false)?;
        
        // Get vision-specific params
        let remote = action.remote.as_ref();
        let min_pixels = remote.and_then(|r| r.min_pixels);
        let max_pixels = remote.and_then(|r| r.max_pixels);
        
        // Build multimodal content with image and optional prompt
        let prompt_text = prompt.unwrap_or_else(|| "Extract all text from this image accurately.".to_string());
        
        let parts = vec![
            ContentPart::ImageUrl {
                image_url: ImageUrlData {
                    url: format!("data:image/png;base64,{}", image_base64),
                    min_pixels,
                    max_pixels,
                },
            },
            ContentPart::Text { text: prompt_text },
        ];
        
        let messages = vec![ChatMessage::user_multimodal(parts)];
        
        // Build request (no translation options for vision)
        let is_stream = self.ui_tx.is_some();
        let req = ChatRequest {
            model: model.to_string(),
            messages,
            temperature,
            translation_options: None,
            stream: Some(is_stream),
            extra,
        };
        
        info!("Vision API Request to: {}", url);
        info!("Model: {}, Stream: {}", model, is_stream);
        
        let mut builder = self.get_client().await.post(&url).header("Content-Type", "application/json");
        if let Some(key) = api_key {
            builder = builder.header("Authorization", format!("Bearer {}", key));
        }
        
        let res = builder.json(&req).send().await?;
        
        let status = res.status();
        info!("Vision API Response status: {}", status);
        
        if !status.is_success() {
            let body = res.text().await.unwrap_or_else(|_| "<failed to read body>".to_string());
            error!("Vision API error response: {}", body);
            return Err(anyhow::anyhow!("Vision API request failed: {} - {}", status, body));
        }
        
        // Handle streaming/non-streaming response
        if is_stream {
            self.stream_sse_response(res, "Vision").await
        } else {
            let chat_res: ChatResponse = res.json().await?;
            chat_res.choices.first()
                .map(|c| c.message.content.clone())
                .ok_or_else(|| anyhow::anyhow!("Empty content"))
        }
    }

    async fn execute_request(&self, url: &str, model: &str, api_key: Option<String>, prompt: Option<&str>, user_text: &str, temperature: f32, translation_options: Option<HashMap<String, String>>, extra: HashMap<String, serde_json::Value>) -> Result<String> {
        // Build user content: concatenate prompt with user text as a single user message
        // This avoids using a system message, per the requirement
        let user_content = if let Some(p) = prompt {
            // Concatenate prompt and user text with a clear separator
            format!("{}\n\n{}", p.trim(), user_text)
        } else if api_key.is_none() && !user_text.contains("/think") {
            // For local API, append /no_think to avoid reasoning output unless user explicitly wants it
            format!("{}/no_think", user_text)
        } else {
            user_text.to_string()
        };
        
        let messages = vec![ChatMessage::user_text(user_content)];

        // Convert HashMap options to TranslationOptions struct if present
        let trans_opts = translation_options.map(|map| {
            TranslationOptions {
                source_lang: map.get("source_lang").cloned().unwrap_or_default(),
                target_lang: map.get("target_lang").cloned().unwrap_or_default(),
            }
        });

        // Use streaming if we have a UI to update
        let is_stream = self.ui_tx.is_some();
        let req = ChatRequest {
            model: model.to_string(),
            messages,
            temperature,
            translation_options: trans_opts,
            stream: Some(is_stream),
            extra,
        };

        // Debug logging
        info!("API Request to: {}", url);
        info!("Model: {}, Stream: {}, Temperature: {}", model, is_stream, temperature);
        if let Ok(json_str) = serde_json::to_string_pretty(&req) {
            info!("Request body:\n{}", json_str);
        }

        let mut builder = self.get_client().await.post(url).header("Content-Type", "application/json");
        if let Some(key) = api_key {
            builder = builder.header("Authorization", format!("Bearer {}", key));
        }
        
        let res = builder.json(&req).send().await?;
        
        let status = res.status();
        info!("API Response status: {}", status);
        info!("Response headers: {:?}", res.headers());
        
        if !status.is_success() {
            // Try to get response body for debugging
            let body = res.text().await.unwrap_or_else(|_| "<failed to read body>".to_string());
            error!("API error response body: {}", body);
            return Err(anyhow::anyhow!("API request failed: {} - {}", status, body));
        }

        if is_stream {
            self.stream_sse_response(res, "Chat").await
        } else {
            let chat_res: ChatResponse = res.json().await?;
            chat_res.choices.first().map(|c| c.message.content.clone()).ok_or_else(|| anyhow::anyhow!("Empty content"))
        }
    }

    /// Stream SSE response with proper buffering using eventsource-stream.
    ///
    /// This method correctly handles SSE events that may be split across TCP chunks,
    /// ensuring no content is lost due to chunk boundary misalignment.
    async fn stream_sse_response(
        &self,
        res: reqwest::Response,
        context_label: &str,
    ) -> Result<String> {
        let mut stream = res.bytes_stream().eventsource();
        let mut full_response = String::new();
        let mut event_count = 0;

        while let Some(event_result) = stream.next().await {
            // Check for cancellation at start of each event
            if self.is_cancelled() {
                info!("{} request cancelled by user after {} events", context_label, event_count);
                return Err(anyhow::anyhow!("Cancelled"));
            }

            match event_result {
                Ok(event) => {
                    event_count += 1;
                    let data = &event.data;
                    
                    // Check for stream completion
                    if data == "[DONE]" {
                        info!("{} stream complete after {} events, total length: {}", 
                              context_label, event_count, full_response.len());
                        break;
                    }

                    // Parse the JSON chunk
                    match serde_json::from_str::<ChatChunkResponse>(data) {
                        Ok(json) => {
                            if let Some(content) = json.choices.first().and_then(|c| c.delta.content.clone()) {
                                debug!("{} SSE event {}: {} bytes", context_label, event_count, content.len());
                                full_response.push_str(&content);
                                if let Some(tx) = &self.ui_tx {
                                    if let Err(e) = tx.try_send(crate::ui::UiEvent::StreamUpdate(content)) {
                                        warn!("Failed to send StreamUpdate: {}", e);
                                    }
                                    // Clone context before releasing lock to avoid holding it during repaint
                                    let ctx_clone = self.egui_ctx.lock().ok().and_then(|g| g.clone());
                                    if let Some(ctx) = ctx_clone {
                                        ctx.request_repaint();
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            // Log parse errors but continue - some events may be heartbeats or metadata
                            debug!("{} SSE JSON parse error (event {}): {} - data: {}", 
                                   context_label, event_count, e, data);
                        }
                    }
                }
                Err(e) => {
                    // Log SSE-level errors and continue
                    warn!("{} SSE parse error: {}", context_label, e);
                }
            }
        }

        info!("{} streaming finished: {} events received, response length: {}", 
              context_label, event_count, full_response.len());
        
        // Send StreamEnd to signal completion to the UI
        if let Some(tx) = &self.ui_tx {
            let _ = tx.try_send(crate::ui::UiEvent::StreamEnd(true));
            // Wake up UI to process the end event
            let ctx_clone = self.egui_ctx.lock().ok().and_then(|g| g.clone());
            if let Some(ctx) = ctx_clone {
                ctx.request_repaint();
            }
        }
        
        Ok(full_response)
    }
}
