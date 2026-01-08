use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use anyhow::Result;

// Helper loaders: load canonical `config/` then overlay platform/user config (XDG)
// This provides a simple override mechanism: repo defaults -> $XDG_CONFIG_HOME/Controller -> environment
pub fn load_llm_config() -> Result<LlmConfig> {
    // Start with canonical repo config
    let mut builder = config::Config::builder()
        .add_source(config::File::with_name("config/llm.toml").required(true));

    // Optional XDG override: $XDG_CONFIG_HOME/Controller/llm.toml
    if let Some(mut dir) = dirs::config_dir() {
        dir.push("Controller");
        dir.push("llm.toml");
        if dir.exists() {
            builder = builder.add_source(config::File::from(dir).required(false));
        }
    }

    let settings = builder.build()?;
    let cfg: LlmConfig = settings.try_deserialize()?;
    Ok(cfg)
}

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
        dir.push("Controller");
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
pub struct LocalConfig {
    pub api_url: String,
    pub model: String,
    pub prompt: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct FunctionalityConfig {
    pub api_url: String,
    pub api_key: String,
    pub model: String,
    pub prompt: Option<String>,
    pub translation_options: Option<HashMap<String, String>>,
    pub local: Option<LocalConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    pub copy_check: FunctionalityConfig,
    pub translate_e2c: FunctionalityConfig,
    pub translate_c2e: FunctionalityConfig,
    pub explain: FunctionalityConfig,
    pub user_query: Option<FunctionalityConfig>,
    #[allow(dead_code)]
    pub visual: Option<FunctionalityConfig>,
    
    pub timeout_ms: Option<u64>,
    /// If true, always use local LLM instead of remote
    #[serde(default)]
    pub force_local: bool,
}

#[derive(Debug, Deserialize, Clone)]
pub struct CommandsConfig {
    pub commands: HashMap<String, String>,
}
