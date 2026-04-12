use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AgentTask {
    pub name: String,
    pub description: String,
    pub corpus: String,
    pub setup: String,
    pub task_prompt: String,
    pub success_criteria: Vec<String>,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentTaskResult {
    pub task_name: String,
    pub agent: String,
    pub with_scavenger: AgentRunMetrics,
    pub baseline: Option<AgentRunMetrics>,
    pub success: bool,
    pub success_details: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentRunMetrics {
    pub tokens_used: u64,
    pub tool_calls: u64,
    pub files_read: u64,
    pub wall_time_seconds: f64,
    pub navigation_efficiency: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TokenUsage {
    pub tokens_used: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentResult {
    pub task_name: String,
    pub success: bool,
    pub success_details: Vec<String>,
    pub with_scavenger: TokenUsage,
    pub baseline: Option<TokenUsage>,
}

#[derive(Debug, Clone, Copy)]
pub enum AgentType {
    Claude,
    Cursor,
    OpenCode,
}

impl AgentType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "claude" => Some(AgentType::Claude),
            "cursor" => Some(AgentType::Cursor),
            "opencode" => Some(AgentType::OpenCode),
            _ => None,
        }
    }

    pub fn cli_name(&self) -> &'static str {
        match self {
            AgentType::Claude => "claude",
            AgentType::Cursor => "cursor",
            AgentType::OpenCode => "opencode",
        }
    }
}

impl std::fmt::Display for AgentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AgentType::Claude => write!(f, "claude"),
            AgentType::Cursor => write!(f, "cursor"),
            AgentType::OpenCode => write!(f, "opencode"),
        }
    }
}
