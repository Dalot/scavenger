//! Configuration management

use crate::utils::{parse_input, validate_email};

/// Configuration for the application
pub struct Config {
    pub port: u16,
    pub host: String,
    pub debug: bool,
}

/// Parse configuration from environment and files
pub fn parse_config() -> Config {
    Config {
        port: std::env::var("PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(8080),
        host: std::env::var("HOST").unwrap_or_else(|_| "localhost".to_string()),
        debug: std::env::var("DEBUG").is_ok(),
    }
}

/// Settings builder for configuration
pub struct Settings {
    pub values: std::collections::HashMap<String, String>,
}

impl Settings {
    /// Create a new Settings instance
    pub fn new() -> Self {
        Self {
            values: std::collections::HashMap::new(),
        }
    }

    /// Load settings from environment variables
    pub fn load_from_env(&mut self) {
        for (key, value) in std::env::vars() {
            self.values.insert(key, value);
        }
    }
}

/// Validate configuration settings
pub fn validate_config(config: &Config) -> Result<(), String> {
    if config.port == 0 {
        return Err("Port cannot be 0".to_string());
    }
    if config.host.is_empty() {
        return Err("Host cannot be empty".to_string());
    }
    Ok(())
}
