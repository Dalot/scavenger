pub(crate) mod accuracy;
pub(crate) mod agent;
pub(crate) mod corpus;
pub(crate) mod relevance;
pub(crate) mod reporter;
pub(crate) mod runner;
pub(crate) mod thresholds;

pub use accuracy::run_accuracy_eval;
pub use corpus::{CorpusEntry, load_corpus};
pub use relevance::{run_performance_checks, run_relevance_eval};
pub use reporter::{print_json, print_summary, run_suite};
pub use runner::{EvalOptions, EvalSuite, run_evals};
pub use thresholds::{
    AccuracyMetric, AccuracyThresholds, AgentThresholds, PerformanceMetric, PerformanceThresholds,
    RelevanceMetric, RelevanceThresholds, Thresholds, load_thresholds,
};

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum EvalError {
    #[error("corpus path not found: {0}")]
    CorpusNotFound(std::path::PathBuf),
    #[error("cannot read directory {0}: {1}")]
    ReadError(std::path::PathBuf, std::io::Error),
    #[error("invalid TOML in {0}: {1}")]
    ParseError(std::path::PathBuf, toml::de::Error),
}

pub type EvalResult<T> = Result<T, EvalError>;

#[derive(Debug, Clone, Serialize)]
#[must_use = "eval case results should not be silently discarded"]
pub struct CaseResult {
    pub case_name: String,
    pub metrics: HashMap<String, f64>,
    pub passed: bool,
    pub failure_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[must_use = "eval summaries should not be silently discarded"]
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
    All,
}

#[derive(Debug, Clone, Serialize)]
#[must_use = "eval run results should not be silently discarded"]
pub struct EvalRun {
    pub run_id: String,
    pub scavenger_version: String,
    pub tier: EvalTier,
    pub suite: String,
    pub corpus: String,
    pub results: Vec<CaseResult>,
    pub summary: SuiteSummary,
}
