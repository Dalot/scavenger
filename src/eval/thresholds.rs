use crate::eval::{EvalError, EvalResult};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelevanceThresholds {
    pub min_recall: f64,
    pub min_precision: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccuracyThresholds {
    pub min_intent_accuracy: f64,
    pub min_ndcg_at_5: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PerformanceThresholds {
    pub max_index_time_per_100_files_ms: u64,
    pub max_capsule_latency_p95_ms: u64,
    pub max_reindex_time_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentThresholds {
    pub min_token_reduction_pct: f64,
    pub min_success_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    #[serde(default)]
    pub relevance: RelevanceThresholds,
    #[serde(default)]
    pub accuracy: AccuracyThresholds,
    #[serde(default)]
    pub performance: PerformanceThresholds,
    #[serde(default)]
    pub agent: AgentThresholds,
}

/// Metrics for relevance eval threshold checks.
#[derive(Debug, Clone, Copy)]
pub enum RelevanceMetric {
    Recall,
    Precision,
}

/// Metrics for accuracy eval threshold checks.
#[derive(Debug, Clone, Copy)]
pub enum AccuracyMetric {
    IntentAccuracy,
    NdcgAt5,
}

/// Metrics for performance eval threshold checks.
#[derive(Debug, Clone, Copy)]
pub enum PerformanceMetric {
    IndexTimePer100FilesMs,
    CapsuleLatencyP95Ms,
    ReindexTimeMs,
    SetupTimeMs,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            relevance: RelevanceThresholds {
                min_recall: 0.80,
                min_precision: 0.60,
            },
            accuracy: AccuracyThresholds {
                min_intent_accuracy: 0.90,
                min_ndcg_at_5: 0.75,
            },
            performance: PerformanceThresholds {
                max_index_time_per_100_files_ms: 5000,
                max_capsule_latency_p95_ms: 200,
                max_reindex_time_ms: 500,
            },
            agent: AgentThresholds {
                min_token_reduction_pct: 20.0,
                min_success_rate: 0.80,
            },
        }
    }
}

impl RelevanceThresholds {
    pub fn passes(&self, metric: RelevanceMetric, value: f64) -> bool {
        match metric {
            RelevanceMetric::Recall => value >= self.min_recall,
            RelevanceMetric::Precision => value >= self.min_precision,
        }
    }
}

impl AccuracyThresholds {
    pub fn passes(&self, metric: AccuracyMetric, value: f64) -> bool {
        match metric {
            AccuracyMetric::IntentAccuracy => value >= self.min_intent_accuracy,
            AccuracyMetric::NdcgAt5 => value >= self.min_ndcg_at_5,
        }
    }
}

impl PerformanceThresholds {
    pub fn passes(&self, metric: PerformanceMetric, value: u64) -> bool {
        match metric {
            PerformanceMetric::IndexTimePer100FilesMs => {
                value <= self.max_index_time_per_100_files_ms
            }
            PerformanceMetric::CapsuleLatencyP95Ms => value <= self.max_capsule_latency_p95_ms,
            PerformanceMetric::ReindexTimeMs => value <= self.max_reindex_time_ms,
            PerformanceMetric::SetupTimeMs => true,
        }
    }
}

pub fn load_thresholds(path: &Path) -> EvalResult<Thresholds> {
    if !path.exists() {
        return Ok(Thresholds::default());
    }

    let content =
        fs::read_to_string(path).map_err(|e| EvalError::ReadError(path.to_path_buf(), e))?;
    let thresholds: Thresholds =
        toml::from_str(&content).map_err(|e| EvalError::ParseError(path.to_path_buf(), e))?;
    Ok(thresholds)
}
