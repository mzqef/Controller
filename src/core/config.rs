//! Configuration types and loaders for IntelliBoard.
//!
//! This module provides:
//! - [`ActionsConfig`] - AI action definitions with prompts and model settings
//! - [`HotkeysConfig`] - Keyboard shortcut bindings
//! - Hot-reload support via file watchers
//! - User override directories per platform

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use anyhow::Result;
use log::info;
use serde_json::Value;

pub fn load_commands_config() -> Result<Option<CommandsConfig>> {
    // Canonical commands file is optional
    let mut builder = config::Config::builder();

    // Add repo canonical if present
    let repo_path = PathBuf::from("config/commands.toml");
    if repo_path.exists() {
        builder = builder.add_source(config::File::with_name("config/commands.toml").required(false));
    }

    // Optional XDG override
    if let Some(mut dir) = dirs::config_dir() {
        dir.push("IntelliBoard");
        dir.push("commands.toml");
        if dir.exists() {
            builder = builder.add_source(config::File::from(dir).required(false));
        }
    }

    let settings = builder.build().ok();
    if let Some(s) = settings {
        let cfg: CommandsConfig = s.try_deserialize()?;
        Ok(Some(cfg))
    } else {
        Ok(None)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct CommandsConfig {
    pub commands: HashMap<String, String>,
}

/// A single hotkey binding that maps a key combination to an action.
///
/// # Example
/// ```toml
/// [[bindings]]
/// key = "KeyT"
/// action = "translate_e2c"
/// modifiers = "Ctrl"
/// ```
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HotkeyBinding {
    /// The key code (e.g., "KeyA", "KeyT", "F1", "Digit1")
    pub key: String,
    /// The action ID to trigger (must match an action's `id` field)
    pub action: String,
    /// Optional modifier keys: "Ctrl", "Ctrl+Shift", "Ctrl+Alt", "Alt"
    #[serde(default)]
    pub modifiers: Option<String>,
}

/// Configuration for all keyboard shortcuts.
///
/// Loaded from `config/hotkeys.toml` with optional user overrides.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HotkeysConfig {
    /// List of hotkey bindings
    pub bindings: Vec<HotkeyBinding>,
}

impl Default for HotkeysConfig {
    fn default() -> Self {
        Self {
            bindings: vec![
                HotkeyBinding {
                    key: "KeyR".to_string(),
                    action: "format".to_string(),
                    modifiers: Some("Ctrl".to_string()),
                },
                HotkeyBinding {
                    key: "KeyT".to_string(),
                    action: "translate_e2c".to_string(),
                    modifiers: Some("Ctrl".to_string()),
                },
                HotkeyBinding {
                    key: "KeyY".to_string(),
                    action: "translate_c2e".to_string(),
                    modifiers: Some("Ctrl".to_string()),
                },
                HotkeyBinding {
                    key: "KeyE".to_string(),
                    action: "explain".to_string(),
                    modifiers: Some("Ctrl".to_string()),
                },
                HotkeyBinding {
                    key: "KeyO".to_string(),
                    action: "vl_ocr".to_string(),
                    modifiers: Some("Ctrl".to_string()),
                },
            ],
        }
    }
}

pub fn load_hotkeys_config() -> Result<HotkeysConfig> {
    let mut builder = config::Config::builder();

    // Add repo canonical if present
    let repo_path = PathBuf::from("config/hotkeys.toml");
    if repo_path.exists() {
        builder = builder.add_source(config::File::with_name("config/hotkeys.toml").required(false));
    }

    // Optional XDG override
    if let Some(mut dir) = dirs::config_dir() {
        dir.push("IntelliBoard");
        dir.push("hotkeys.toml");
        if dir.exists() {
            builder = builder.add_source(config::File::from(dir).required(false));
        }
    }

    let settings = builder.build().ok();
    if let Some(s) = settings {
        let cfg: HotkeysConfig = s.try_deserialize()?;
        Ok(cfg)
    } else {
        Ok(HotkeysConfig::default())
    }
}

pub fn save_hotkeys_config(config: &HotkeysConfig) -> Result<()> {
    // Save to user config directory
    let mut dir = dirs::config_dir().ok_or_else(|| anyhow::anyhow!("No config directory"))?;
    dir.push("IntelliBoard");
    std::fs::create_dir_all(&dir)?;
    dir.push("hotkeys.toml");
    
    let toml = toml::to_string_pretty(config)?;
    std::fs::write(dir, toml)?;
    Ok(())
}

// ============================================================================
// Actions Configuration (Dynamic AI Functions)
// ============================================================================

/// Local LLM configuration - extensible with extra params
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ActionLocalConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// Extra parameters passed through to the API request
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

/// Remote API configuration - extensible with extra params
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ActionRemoteConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_translation: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_lang: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_lang: Option<String>,
    /// Enable vision/multimodal mode for this action (VL-OCR, image analysis)
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_vision: bool,
    /// Min pixels for vision model image processing (default: 32*32*3 = 3072)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_pixels: Option<u32>,
    /// Max pixels for vision model image processing (default: 32*32*8192 = 8388608)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_pixels: Option<u32>,
    /// Extra parameters passed through to the API request
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

fn is_false(b: &bool) -> bool { !*b }

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ActionDefinition {
    pub id: String,
    /// Display label; defaults to `id` if not provided
    #[serde(default, skip_serializing_if = "Option::is_none")]
    label: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub hidden: bool,
    
    /// Remote API configuration (new nested structure)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<ActionRemoteConfig>,
    /// Local LLM configuration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<ActionLocalConfig>,
    
    // Legacy flat fields - kept for backward compatibility during load, not serialized
    #[serde(default, skip_serializing)]
    prompt: Option<String>,
    #[serde(default, skip_serializing)]
    api_url: Option<String>,
    #[serde(default, skip_serializing)]
    api_key: Option<String>,
    #[serde(default, skip_serializing)]
    model: Option<String>,
    #[serde(default, skip_serializing)]
    temperature: Option<f32>,
    #[serde(default, skip_serializing)]
    is_translation: bool,
    #[serde(default, skip_serializing)]
    source_lang: Option<String>,
    #[serde(default, skip_serializing)]
    target_lang: Option<String>,
}

impl ActionDefinition {
    /// Returns label if set, otherwise falls back to id
    pub fn label(&self) -> &str {
        self.label.as_deref().unwrap_or(&self.id)
    }
    
    /// Sets the label value
    pub fn set_label(&mut self, label: String) {
        self.label = if label.is_empty() || label == self.id {
            None
        } else {
            Some(label)
        };
    }
    
    /// Gets the raw label Option for serialization
    pub fn label_raw(&self) -> Option<&String> {
        self.label.as_ref()
    }
    
    /// Create a new action with the given id and optional label
    pub fn new(id: impl Into<String>, label: Option<impl Into<String>>) -> Self {
        let id = id.into();
        Self {
            label: label.map(|l| l.into()),
            id,
            description: String::new(),
            hidden: false,
            remote: Some(ActionRemoteConfig::default()),
            local: Some(ActionLocalConfig::default()),
            // Legacy fields - not used for new actions
            prompt: None,
            api_url: None,
            api_key: None,
            model: None,
            temperature: None,
            is_translation: false,
            source_lang: None,
            target_lang: None,
        }
    }
    
    /// Migrate legacy flat fields to nested remote/local structure
    /// Called after deserialization to upgrade old configs
    pub fn migrate_to_nested(&mut self) {
        // Only migrate if remote is None and we have legacy fields
        let has_legacy = self.api_url.is_some() 
            || self.api_key.is_some() 
            || self.model.is_some() 
            || self.prompt.is_some()
            || self.temperature.is_some()
            || self.is_translation
            || self.source_lang.is_some()
            || self.target_lang.is_some();
        
        if self.remote.is_none() && has_legacy {
            self.remote = Some(ActionRemoteConfig {
                api_url: self.api_url.take(),
                api_key: self.api_key.take(),
                model: self.model.take(),
                prompt: self.prompt.take(),
                temperature: self.temperature.take(),
                is_translation: self.is_translation,
                source_lang: self.source_lang.take(),
                target_lang: self.target_lang.take(),
                is_vision: false,
                min_pixels: None,
                max_pixels: None,
                extra: HashMap::new(),
            });
            // Clear legacy fields
            self.is_translation = false;
        }
        
        // Ensure local exists (even if empty) for consistency
        if self.local.is_none() {
            self.local = Some(ActionLocalConfig::default());
        }
    }
}

/// Default settings for all AI actions.
///
/// These values are used when an action doesn't specify its own settings.
#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct ActionDefaults {
    /// Default API endpoint URL
    #[serde(default)]
    pub api_url: Option<String>,
    /// Default API key (supports `${ENV_VAR}` syntax)
    #[serde(default)]
    pub api_key: Option<String>,
    /// Default model name
    #[serde(default)]
    pub model: Option<String>,
    /// Default temperature (0.0 = deterministic, 1.0 = creative)
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Request timeout in milliseconds
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Default local LLM settings (fallback)
    #[serde(default)]
    pub local: Option<ActionLocalConfig>,
}

/// Main configuration for AI actions.
///
/// Loaded from `config/actions.toml` with optional user overrides.
/// Supports hot-reload when files change.
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ActionsConfig {
    /// Default settings applied to all actions
    #[serde(default)]
    pub defaults: ActionDefaults,
    /// Force local LLM mode (ignore remote API)
    #[serde(default)]
    pub force_local: bool,
    /// Export path for memory graph exports (defaults to Downloads folder)
    #[serde(default)]
    pub export_path: Option<String>,
    #[serde(default)]
    pub actions: Vec<ActionDefinition>,
}

impl Default for ActionsConfig {
    fn default() -> Self {
        Self {
            defaults: ActionDefaults {
                api_url: Some("https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions".to_string()),
                api_key: Some("${API_KEY}".to_string()),
                model: Some("qwen-max".to_string()),
                temperature: Some(0.1),
                timeout_ms: Some(60000),
                local: Some(ActionLocalConfig {
                    api_url: Some("http://127.0.0.1:8000/api/v1/chat/completions".to_string()),
                    model: Some("Qwen3-4B-Hybrid".to_string()),
                    prompt: None,
                    extra: HashMap::new(),
                }),
            },
            force_local: false,
            export_path: None,
            actions: vec![
                {
                    let mut a = ActionDefinition::new("format", Some("Format"));
                    a.description = "Fix PDF text and format math".to_string();
                    a.remote = Some(ActionRemoteConfig {
                        prompt: Some("You are a helpful assistant that fixes text copied from PDFs. Fix ligatures, typos, format math with LaTeX, remove mid-sentence line breaks. Return ONLY the corrected text.".to_string()),
                        ..Default::default()
                    });
                    a.local = Some(ActionLocalConfig {
                        prompt: Some("Fix this PDF text. Return ONLY the fixed text.".to_string()),
                        ..Default::default()
                    });
                    a
                },
                {
                    let mut a = ActionDefinition::new("translate_e2c", Some("Translate E→C"));
                    a.description = "Translate English to Chinese".to_string();
                    a.remote = Some(ActionRemoteConfig {
                        model: Some("qwen-mt-flash".to_string()),
                        is_translation: true,
                        source_lang: Some("auto".to_string()),
                        target_lang: Some("Chinese".to_string()),
                        ..Default::default()
                    });
                    a.local = Some(ActionLocalConfig {
                        prompt: Some("Translate to Chinese. Return ONLY the translation.".to_string()),
                        ..Default::default()
                    });
                    a
                },
                {
                    let mut a = ActionDefinition::new("translate_c2e", Some("Translate C→E"));
                    a.description = "Translate Chinese to English".to_string();
                    a.remote = Some(ActionRemoteConfig {
                        model: Some("qwen-mt-flash".to_string()),
                        is_translation: true,
                        source_lang: Some("auto".to_string()),
                        target_lang: Some("English".to_string()),
                        ..Default::default()
                    });
                    a.local = Some(ActionLocalConfig {
                        prompt: Some("Translate to English. Return ONLY the translation.".to_string()),
                        ..Default::default()
                    });
                    a
                },
                {
                    let mut a = ActionDefinition::new("explain", Some("Explain"));
                    a.description = "Explain selected text".to_string();
                    a.remote = Some(ActionRemoteConfig {
                        prompt: Some("Explain the following text clearly and concisely.".to_string()),
                        ..Default::default()
                    });
                    a.local = Some(ActionLocalConfig {
                        prompt: Some("Explain concisely.".to_string()),
                        ..Default::default()
                    });
                    a
                },
            ],
        }
    }
}

impl ActionsConfig {
    pub fn get_action(&self, id: &str) -> Option<&ActionDefinition> {
        self.actions.iter().find(|a| a.id == id)
    }
    
    pub fn visible_actions(&self) -> Vec<&ActionDefinition> {
        self.actions.iter().filter(|a| !a.hidden).collect()
    }
}

/// Load actions from a single TOML file
fn load_actions_from_file(path: &std::path::Path) -> Option<ActionsConfig> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

pub fn load_actions_config() -> Result<ActionsConfig> {
    // Load default config from repo
    let default_path = PathBuf::from("config/actions.toml");
    let mut config = load_actions_from_file(&default_path).unwrap_or_default();
    
    // Load user overrides
    if let Some(mut user_dir) = dirs::config_dir() {
        user_dir.push("IntelliBoard");
        user_dir.push("actions.toml");
        
        if let Some(user_config) = load_actions_from_file(&user_dir) {
            // Merge user config into default:
            // - User defaults override repo defaults
            if user_config.defaults.api_url.is_some() {
                config.defaults = user_config.defaults;
            }
            config.force_local = user_config.force_local;
            if user_config.export_path.is_some() {
                config.export_path = user_config.export_path;
            }
            
            // Merge actions: user actions override defaults by ID, but keep defaults not in user config
            let mut merged_actions = Vec::new();
            let user_ids: std::collections::HashSet<String> = 
                user_config.actions.iter().map(|a| a.id.clone()).collect();
            
            // First, add all user-defined actions (they take priority)
            for mut action in user_config.actions {
                action.migrate_to_nested();
                merged_actions.push(action);
            }
            
            // Then add default actions that aren't in user config
            for mut action in config.actions {
                if !user_ids.contains(&action.id) {
                    action.migrate_to_nested();
                    merged_actions.push(action);
                }
            }
            
            config.actions = merged_actions;
        }
    }
    
    // Migrate any remaining legacy actions
    for action in &mut config.actions {
        action.migrate_to_nested();
    }
    
    info!("Loaded {} actions from config", config.actions.len());
    Ok(config)
}

pub fn save_actions_config(config: &ActionsConfig) -> Result<()> {
    // Save to user config directory
    let mut dir = dirs::config_dir().ok_or_else(|| anyhow::anyhow!("No config directory"))?;
    dir.push("IntelliBoard");
    std::fs::create_dir_all(&dir)?;
    dir.push("actions.toml");
    
    let toml = toml::to_string_pretty(config)?;
    std::fs::write(dir, toml)?;
    Ok(())
}
