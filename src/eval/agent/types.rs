use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct TokenUsage {
    pub tokens_used: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentResult {
    pub task_name: String,
    pub success: bool,
    pub success_details: Vec<String>,
    pub with_scavenger: TokenUsage,
    pub baseline: Option<TokenUsage>,
}
