use crate::capsule::assemble;
use crate::capsule::budget::{CapsuleConstraints, DetailLevel};
use crate::config::Config;
use crate::db::schema;
use crate::eval::CaseResult;
use crate::eval::corpus::CorpusEntry;
use crate::eval::thresholds::{PerformanceMetric, RelevanceMetric, Thresholds};
use crate::graph::{self, GraphState};
use crate::query::QueryResult;
use crate::query::intent::{IntentResult, classify};
use rusqlite::Connection;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
struct RelevanceCase {
    name: String,
    corpus: String,
    query: String,
    expected_symbols: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RelevanceCases {
    #[serde(rename = "case")]
    cases: Vec<RelevanceCase>,
}

pub fn run_relevance_eval(
    corpus: &[CorpusEntry],
    thresholds: &Thresholds,
) -> Result<Vec<CaseResult>, String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let cases_dir = Path::new(&manifest_dir).join("eval/cases/relevance");

    if !cases_dir.exists() {
        return Ok(Vec::new());
    }

    let mut all_results = Vec::new();

    for entry in fs::read_dir(&cases_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }

        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let cases: RelevanceCases =
            toml::from_str(&content).map_err(|e| format!("Invalid TOML in {:?}: {}", path, e))?;

        for case in cases.cases {
            let result = run_single_relevance_case(&case, corpus, thresholds)?;
            all_results.push(result);
        }
    }

    Ok(all_results)
}

fn run_single_relevance_case(
    case: &RelevanceCase,
    corpus: &[CorpusEntry],
    thresholds: &Thresholds,
) -> Result<CaseResult, String> {
    let corpus_entry = corpus
        .iter()
        .find(|e| e.name == case.corpus)
        .ok_or_else(|| format!("Corpus '{}' not found", case.corpus))?;

    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    schema::ensure_branch_schema(&conn).map_err(|e| e.to_string())?;

    let mut graph = GraphState::new();
    let source_files = graph::index::collect_source_files(&corpus_entry.path);
    graph::index::bulk_index(&conn, &mut graph, &source_files)
        .map_err(|e| format!("Failed to index corpus: {}", e))?;
    graph.load_from_db(&conn).map_err(|e| e.to_string())?;

    let config = Config::default();
    let intent = classify(&case.query);
    let qr = QueryResult {
        target: None,
        intent: IntentResult::single(intent.primary),
        neighbor_ids: Vec::new(),
        search_results: Vec::new(),
    };
    let constraints = CapsuleConstraints::from_detail(DetailLevel::Standard);

    let capsule = assemble(&conn, &graph, &config, &qr, None, &constraints);

    let mut metrics = HashMap::new();
    let mut passed = true;
    let mut failure_reason = None;

    let mut found_count = 0;
    for expected in &case.expected_symbols {
        if capsule.text.contains(expected) {
            found_count += 1;
        }
    }

    let recall = if case.expected_symbols.is_empty() {
        1.0
    } else {
        found_count as f64 / case.expected_symbols.len() as f64
    };

    metrics.insert("recall".to_string(), recall);

    if !thresholds.relevance.passes(RelevanceMetric::Recall, recall) {
        passed = false;
        failure_reason = Some(format!(
            "recall {:.2} below threshold {:.2}",
            recall, thresholds.relevance.min_recall
        ));
    }

    Ok(CaseResult {
        case_name: case.name.clone(),
        metrics,
        passed,
        failure_reason,
    })
}

pub fn run_performance_checks(
    corpus: &[CorpusEntry],
    thresholds: &Thresholds,
) -> Result<Vec<CaseResult>, String> {
    let mut results = Vec::new();

    for entry in corpus {
        let start = std::time::Instant::now();

        let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
        schema::ensure_branch_schema(&conn).map_err(|e| e.to_string())?;
        let _graph = GraphState::new();

        let setup_time_ms = start.elapsed().as_millis() as u64;

        let mut metrics = HashMap::new();
        metrics.insert("setup_time_ms".to_string(), setup_time_ms as f64);

        let passed = thresholds
            .performance
            .passes(PerformanceMetric::SetupTimeMs, setup_time_ms);

        results.push(CaseResult {
            case_name: format!("{}-setup", entry.name),
            metrics,
            passed,
            failure_reason: None,
        });
    }

    Ok(results)
}
