use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelevanceThresholds {
    pub min_recall: f64,
    pub min_precision: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccuracyThresholds {
    pub min_intent_accuracy: f64,
    pub min_ndcg_at_5: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceThresholds {
    pub max_index_time_per_100_files_ms: u64,
    pub max_capsule_latency_p95_ms: u64,
    pub max_reindex_time_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentThresholds {
    pub min_token_reduction_pct: f64,
    pub min_success_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thresholds {
    pub relevance: RelevanceThresholds,
    pub accuracy: AccuracyThresholds,
    pub performance: PerformanceThresholds,
    pub agent: AgentThresholds,
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
    pub fn passes(&self, metric: &str, value: f64) -> bool {
        match metric {
            "recall" => value >= self.min_recall,
            "precision" => value >= self.min_precision,
            _ => true,
        }
    }
}

impl AccuracyThresholds {
    pub fn passes(&self, metric: &str, value: f64) -> bool {
        match metric {
            "intent_accuracy" => value >= self.min_intent_accuracy,
            "ndcg_at_5" => value >= self.min_ndcg_at_5,
            _ => true,
        }
    }
}

impl PerformanceThresholds {
    pub fn passes(&self, metric: &str, value: u64) -> bool {
        match metric {
            "index_time_per_100_files_ms" => value <= self.max_index_time_per_100_files_ms,
            "capsule_latency_p95_ms" => value <= self.max_capsule_latency_p95_ms,
            "reindex_time_ms" => value <= self.max_reindex_time_ms,
            _ => true,
        }
    }
}

pub fn load_thresholds(path: &Path) -> Result<Thresholds, String> {
    if !path.exists() {
        return Ok(Thresholds::default());
    }

    let content =
        fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {}", path.display(), e))?;
    let thresholds: Thresholds = toml::from_str(&content)
        .map_err(|e| format!("Invalid TOML in {}: {}", path.display(), e))?;
    Ok(thresholds)
}
