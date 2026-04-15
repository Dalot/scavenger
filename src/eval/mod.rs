pub(crate) mod accuracy;
pub(crate) mod agent;
pub(crate) mod case;
pub(crate) mod corpus;
pub(crate) mod coverage;
pub(crate) mod relevance;
pub(crate) mod reporter;
pub(crate) mod runner;
pub(crate) mod thresholds;

pub use accuracy::run_accuracy_eval;
pub use case::{CaseAssert, CaseCategory, OwnedEvalCase, load_cases};
pub use corpus::{CorpusEntry, load_corpus};
pub use coverage::{ContextMetrics, calculate_acs, first_correct_position};
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

/// Correctness classification per DKB paper
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Correctness {
    /// All expected symbols present, no extraneous symbols
    Correct,
    /// Some expected symbols present
    Partial,
    /// No expected symbols found
    Incorrect,
}

impl Correctness {
    /// Derive correctness from recall and precision values
    pub fn from_recall_precision(recall: f64, precision: f64) -> Self {
        // Use epsilon for floating point comparison
        const EPSILON: f64 = 0.001;
        if recall >= 1.0 - EPSILON && precision >= 1.0 - EPSILON {
            Correctness::Correct
        } else if recall > EPSILON {
            Correctness::Partial
        } else {
            Correctness::Incorrect
        }
    }
}

#[derive(Error, Debug)]
pub enum EvalError {
    #[error("corpus path not found: {0}")]
    CorpusNotFound(std::path::PathBuf),
    #[error("cannot read directory {0}: {1}")]
    ReadError(std::path::PathBuf, std::io::Error),
    #[error("invalid case format in {0}: {1}")]
    ParseError(std::path::PathBuf, Box<dyn std::error::Error + Send + Sync>),
}

pub type EvalResult<T> = Result<T, EvalError>;

#[derive(Debug, Clone, Serialize)]
#[must_use = "eval case results should not be silently discarded"]
pub struct CaseResult {
    pub case_name: String,
    pub category: String,
    pub metrics: HashMap<String, f64>,
    pub correctness: Correctness,
    pub passed: bool,
    pub failure_reason: Option<String>,
    pub bm25_recall: f64,
    pub graph_recall: f64,
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
