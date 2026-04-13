use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("failed to read config file: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to parse config: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("failed to extract config: {0}")]
    Figment(#[from] Box<figment::Error>),
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub budget: BudgetConfig,
    pub scopes: std::collections::HashMap<String, ScopeValue>,
    pub traversal: TraversalConfig,
    pub docs: DocsConfig,
    pub analytics: AnalyticsConfig,
    pub federation: FederationConfig,
    pub log_level: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ScopeValue {
    Single(String),
    Multiple(Vec<String>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct BudgetConfig {
    pub default: u32,
    pub warning_threshold: u32,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            default: 8000,
            warning_threshold: 20000,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct TraversalConfig {
    pub degree_cap: u32,
    pub node_budget: u32,
    pub builtins_blocklist: Vec<String>,
}

impl Default for TraversalConfig {
    fn default() -> Self {
        Self {
            degree_cap: 50,
            node_budget: 100,
            builtins_blocklist: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct DocsConfig {
    pub enabled: bool,
    pub patterns: Vec<String>,
    pub exclude: Vec<String>,
    pub priority: Vec<String>,
}

impl Default for DocsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            patterns: vec!["**/*.md".into(), "**/*.markdown".into()],
            exclude: vec!["**/node_modules/**".into(), "**/target/**".into()],
            priority: vec!["CLAUDE.md".into(), "README.md".into()],
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct AnalyticsConfig {
    pub enabled: bool,
    pub price_per_million_tokens: f64,
    pub session_retention_days: u32,
}

impl Default for AnalyticsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            price_per_million_tokens: 3.0,
            session_retention_days: 30,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct FederationConfig {
    pub repos: Vec<PathBuf>,
}

impl Config {
    /// Load config from `.scavenger.toml` in the given project root.
    /// Supports layered configuration: defaults < config file < environment variables.
    pub fn load(project_root: &Path) -> Result<Self, ConfigError> {
        use figment::{
            Figment,
            providers::{Env, Format, Serialized, Toml},
        };

        let config_path = project_root.join(".scavenger.toml");

        let figment = Figment::new()
            .merge(Serialized::defaults(Config::default()))
            .merge(Toml::file(config_path))
            .merge(Env::prefixed("SCAVENGER_").split("__"));

        let mut config: Config = figment
            .extract()
            .map_err(|e| ConfigError::Figment(Box::new(e)))?;
        config.clamp_and_warn();
        Ok(config)
    }

    fn clamp_and_warn(&mut self) {
        self.budget.default = clamp_warn("budget.default", self.budget.default, 1000, 100_000);
        self.traversal.degree_cap =
            clamp_warn("traversal.degree_cap", self.traversal.degree_cap, 5, 500);
        self.traversal.node_budget = clamp_warn(
            "traversal.node_budget",
            self.traversal.node_budget,
            10,
            10_000,
        );
        self.analytics.session_retention_days = clamp_warn(
            "analytics.session_retention_days",
            self.analytics.session_retention_days,
            1,
            365,
        );
    }
}

fn clamp_warn(field: &str, value: u32, min: u32, max: u32) -> u32 {
    if value < min {
        eprintln!("Config: {field}={value} is below minimum {min}, using {min}.");
        min
    } else if value > max {
        eprintln!("Config: {field}={value} is above maximum {max}, using {max}.");
        max
    } else {
        value
    }
}
