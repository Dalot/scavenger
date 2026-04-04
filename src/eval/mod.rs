pub mod accuracy;
pub mod agent;
pub mod corpus;
pub mod relevance;
pub mod reporter;
pub mod runner;
pub mod thresholds;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct CaseResult {
    pub case_name: String,
    pub metrics: HashMap<String, f64>,
    pub passed: bool,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SuiteSummary {
    pub suite_name: String,
    pub corpus: String,
    pub total_cases: usize,
    pub passed: usize,
    pub failed: usize,
    pub averages: HashMap<String, f64>,
}

/// Evaluation tier — distinguishes fast component evals from slower agent-based evals.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EvalTier {
    Component,
    Agent,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvalRun {
    pub run_id: String,
    pub scavenger_version: String,
    pub tier: EvalTier,
    pub suite: String,
    pub corpus: String,
    pub results: Vec<CaseResult>,
    pub summary: SuiteSummary,
}
