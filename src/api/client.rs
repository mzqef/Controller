use serde::{Deserialize, Serialize};
use reqwest::Client;
use std::time::Duration;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use anyhow::Result;
use log::{info, warn, error};
use std::collections::HashMap;

use crate::core::config::{LlmConfig, FunctionalityConfig};

// Helper to detect network errors (timeout, connection, DNS, etc)
fn is_network_error(e: &anyhow::Error) -> bool {
    let s = e.to_string();
    s.contains("timed out") || s.contains("connection") || s.contains("dns") || s.contains("unreachable") || s.contains("network")
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Serialize)]
struct TranslationOptions {
    source_lang: String,
    target_lang: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    translation_options: Option<TranslationOptions>,
    // #[serde(skip_serializing_if = "Option::is_none")]
    stream: Option<bool>,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessage,
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

pub struct LlmClient {
    client: tokio::sync::OnceCell<Client>,
    config: LlmConfig,
    remote_available: Arc<AtomicBool>,
    ui_tx: Option<tokio::sync::mpsc::Sender<crate::ui::UiEvent>>,
}

impl LlmClient {
    pub fn new(config: LlmConfig) -> Self {
        Self { 
            client: tokio::sync::OnceCell::new(), 
            config,
            remote_available: Arc::new(AtomicBool::new(true)),
            ui_tx: None,
        }
    }
    
    pub fn set_ui_tx(&mut self, tx: tokio::sync::mpsc::Sender<crate::ui::UiEvent>) {
        self.ui_tx = Some(tx);
    }
    
    async fn get_client(&self) -> &Client {
        self.client.get_or_init(|| async {
            let timeout = self.config.timeout_ms.unwrap_or(60000) / 1000;
            Client::builder()
                .timeout(Duration::from_secs(timeout))
                .tcp_nodelay(true)
                .http1_only()
                .no_proxy() // Disable proxy
                .build()
                .unwrap_or_default()
        }).await
    }
    
    // Helper to get configuration for a specific functionality
    fn get_func_config(&self, functionality: &str) -> Option<&FunctionalityConfig> {
        match functionality {
            "copy_check" => Some(&self.config.copy_check),
            "translate_e2c" => Some(&self.config.translate_e2c),
            "translate_c2e" => Some(&self.config.translate_c2e),
            "explain" => Some(&self.config.explain),
            "user_query" => self.config.user_query.as_ref(),
            "visual" => self.config.visual.as_ref(),
            _ => None,
        }
    }

    fn expand_env(val: &str) -> String {
        if val.starts_with("${") && val.ends_with("}") {
            let var = &val[2..val.len()-1];
            std::env::var(var).unwrap_or_else(|_| val.to_string())
        } else {
            val.to_string()
        }
    }

    /// Determines the endpoint, model, and authentication needed for a request.
    /// 
    /// Returns: (url, model, needs_auth_key, api_key_if_needed, prompt)
    fn resolve_config(&self, functionality: &str, force_local: bool) -> Result<(String, String, bool, Option<String>, Option<String>)> {
        let func_cfg = self.get_func_config(functionality)
            .ok_or_else(|| anyhow::anyhow!("Unknown functionality: {}", functionality))?;

        // Check global force_local config OR runtime force_local parameter
        let use_local = force_local || self.config.force_local;
        let is_remote = !use_local && self.remote_available.load(Ordering::Relaxed);

        if is_remote {
            let url = func_cfg.api_url.clone();
            let base = if url.ends_with("chat/completions") { url } else { format!("{}/chat/completions", url.trim_end_matches('/')) };
            Ok((
                base,
                func_cfg.model.clone(),
                true,
                Some(Self::expand_env(&func_cfg.api_key)),
                func_cfg.prompt.clone(),
            ))
        } else {
             // Local Fallback
             if let Some(local) = &func_cfg.local {
                let url = local.api_url.clone();
                let base = if url.ends_with("chat/completions") { url } else { format!("{}/chat/completions", url.trim_end_matches('/')) };
                Ok((
                    base,
                    local.model.clone(),
                    false,
                    None,
                    local.prompt.clone().or(func_cfg.prompt.clone()), // Fallback to main prompt if local not specific
                ))
             } else {
                 Err(anyhow::anyhow!("No local configuration for {}", functionality))
             }
        }
    }

    // Spawn health check MUST be called inside a runtime
    pub fn spawn_health_check(self: &Arc<Self>) {
        // We use copy_check as the canary for health checking
        let func_cfg = &self.config.copy_check;
        let url = func_cfg.api_url.clone();
        let url = if url.ends_with("chat/completions") { url } else { format!("{}/chat/completions", url.trim_end_matches('/')) };
        let api_key = Self::expand_env(&func_cfg.api_key);
        
        let remote_available = self.remote_available.clone();

        // Get the shared client (initializing if needed) before spawning loop?
        // Actually, we want a separate client for health checks with short timeout.
        
        tokio::spawn(async move {
            // Because we are inside spawn, we are in runtime. 
            // We create a FRESH client dedicated for health checks to avoid polluting the main pool and with simpler timeout.
            let check_client = Client::builder()
                .timeout(Duration::from_secs(15))
                .tcp_nodelay(true)
                .build()
                .unwrap_or_default();

            loop {
                // Initial delay
                tokio::time::sleep(Duration::from_secs(60)).await;

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

    pub async fn chat_completion(&self, functionality: &str, text: &str) -> Result<String> {
        // Try remote first
        let remote_result = match self.resolve_config(functionality, false) {
            Ok((url, model, _, api_key, prompt)) => {
                match self.execute_request(&url, &model, api_key, prompt.as_deref(), text, None).await {
                    Ok(res) => return Ok(res),
                    Err(e) => {
                        // If it's a network error, mark remote unavailable and try local
                        if is_network_error(&e) {
                            self.remote_available.store(false, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            return Err(e);
                        }
                    }
                }
            },
            Err(_) => {} // If config missing, fall through to local
        };
        // Try local fallback
        let (url, model, _, api_key, prompt) = self.resolve_config(functionality, true)?;
        self.execute_request(&url, &model, api_key, prompt.as_deref(), text, None).await
    }

    /// User query with streaming support and remote/local fallback
    pub async fn user_query_streaming(&self, text: &str) -> Result<String> {
        const FUNCTIONALITY: &str = "user_query";
        
        // Try remote first
        match self.resolve_config(FUNCTIONALITY, false) {
            Ok((url, model, _, api_key, prompt)) => {
                match self.execute_request(&url, &model, api_key, prompt.as_deref(), text, None).await {
                    Ok(res) => return Ok(res),
                    Err(e) => {
                        // If it's a network error, mark remote unavailable and try local
                        if is_network_error(&e) {
                            warn!("Remote API failed for user_query, falling back to local: {}", e);
                            self.remote_available.store(false, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            // Non-network error, still try local as fallback for robustness
                            warn!("Remote API error for user_query ({}), attempting local fallback", e);
                        }
                    }
                }
            },
            Err(e) => {
                warn!("No remote config for user_query: {}", e);
            }
        };
        
        // Try local fallback
        match self.resolve_config(FUNCTIONALITY, true) {
            Ok((url, model, _, api_key, prompt)) => {
                self.execute_request(&url, &model, api_key, prompt.as_deref(), text, None).await
            },
            Err(e) => {
                Err(anyhow::anyhow!("No configuration available for user_query: {}", e))
            }
        }
    }

    pub async fn translate(&self, text: &str, source: &str, target: &str) -> Result<String> {
        // Determine functionality key based on languages
        let functionality = if (source.eq_ignore_ascii_case("English") || source.eq_ignore_ascii_case("en")) && 
                               (target.eq_ignore_ascii_case("Chinese") || target.eq_ignore_ascii_case("zh")) {
            "translate_e2c"
        } else if (source.eq_ignore_ascii_case("Chinese") || source.eq_ignore_ascii_case("zh")) && 
                  (target.eq_ignore_ascii_case("English") || target.eq_ignore_ascii_case("en")) {
            "translate_c2e"
        } else {
             // Fallback or generic - technically not supported by current detailed config structure rigidly, 
             // but we can default to one or fail. Let's try e2c structure as base or just fail.
             // For now, let's map to E2C config but override prompt? No, prompts are fixed.
             // We return error for unsupported pairs in this strict mode.
             return Err(anyhow::anyhow!("Unsupported translation pair for strict config: {} -> {}", source, target));
        };

        // Try remote first
        let mut options = None;
        let remote_result = match self.resolve_config(functionality, false) {
            Ok((url, model, _, api_key, prompt)) => {
                options = self.get_func_config(functionality).and_then(|c| c.translation_options.clone());
                match self.execute_request(&url, &model, api_key, prompt.as_deref(), text, options.clone()).await {
                    Ok(res) => return Ok(res),
                    Err(e) => {
                        if is_network_error(&e) {
                            self.remote_available.store(false, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            return Err(e);
                        }
                    }
                }
            },
            Err(_) => {}
        };
        // Try local fallback
        let (url, model, _, api_key, prompt) = self.resolve_config(functionality, true)?;
        options = self.get_func_config(functionality).and_then(|c| c.translation_options.clone());
        self.execute_request(&url, &model, api_key, prompt.as_deref(), text, options).await
    }

// Helper to detect network errors (timeout, connection, DNS, etc)
fn is_network_error(e: &anyhow::Error) -> bool {
    let s = e.to_string();
    s.contains("timed out") || s.contains("connection") || s.contains("dns") || s.contains("unreachable") || s.contains("network")
}

    async fn execute_request(&self, url: &str, model: &str, api_key: Option<String>, system_prompt: Option<&str>, user_text: &str, translation_options: Option<HashMap<String, String>>) -> Result<String> {
        let mut messages = Vec::new();
        if let Some(p) = system_prompt {
             messages.push(ChatMessage { role: "system".to_string(), content: p.to_string() });
        }
        
        // For local API, append /no_think to avoid reasoning output unless user explicitly wants it
        let user_content = if api_key.is_none() && !user_text.contains("/think") {
            format!("{}/no_think", user_text)
        } else {
            user_text.to_string()
        };
        
        messages.push(ChatMessage { role: "user".to_string(), content: user_content });

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
            temperature: 0.1,
            translation_options: trans_opts,
            stream: Some(is_stream),
        };

        // Debug logging
        info!("API Request to: {}", url);
        info!("Model: {}, Stream: {}", model, is_stream);
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
            use futures_util::StreamExt;
            let mut stream = res.bytes_stream();
            let mut full_response = String::new();
            
            while let Some(item) = stream.next().await {
                let chunk = item?;
                let s = String::from_utf8_lossy(&chunk);
                // Simple SSE parsing (this is naive but works for standard OpenAI format)
                for line in s.lines() {
                    if line.starts_with("data: ") {
                        let data = &line[6..];
                        if data == "[DONE]" { break; }
                        if let Ok(json) = serde_json::from_str::<ChatChunkResponse>(data) {
                            if let Some(content) = json.choices.first().and_then(|c| c.delta.content.clone()) {
                                full_response.push_str(&content);
                                if let Some(tx) = &self.ui_tx {
                                     let _ = tx.try_send(crate::ui::UiEvent::StreamUpdate(content));
                                }
                            }
                        }
                    }
                }
            }
            Ok(full_response)
        } else {
            let chat_res: ChatResponse = res.json().await?;
            chat_res.choices.first().map(|c| c.message.content.clone()).ok_or_else(|| anyhow::anyhow!("Empty content"))
        }
    }
}
