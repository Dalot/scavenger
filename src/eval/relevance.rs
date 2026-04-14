//! Relevance evaluation runner
//!
//! Evaluates how well the capsule pipeline retrieves expected symbols
//! for different query types (G1, G2, G3 categories).

use crate::capsule::assemble;
use crate::capsule::budget::{CapsuleConstraints, DetailLevel};
use crate::config::Config;
use crate::db::schema;
use crate::eval::case::{CaseCategory, OwnedEvalCase, load_cases};
use crate::eval::corpus::CorpusEntry;
use crate::eval::coverage::{ContextMetrics, calculate_acs, first_correct_position};
use crate::eval::thresholds::{PerformanceMetric, RelevanceMetric, Thresholds};
use crate::eval::{CaseResult, Correctness};
use crate::graph::{self, GraphState};
use crate::query;
use crate::query::search;
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Run all relevance evaluation cases
pub fn run_relevance_eval(
    corpus: &[CorpusEntry],
    thresholds: &Thresholds,
) -> Result<Vec<CaseResult>, String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let cases_dir = Path::new(&manifest_dir).join("eval/cases/relevance");

    let cases = load_cases(&cases_dir).map_err(|e| e.to_string())?;

    if cases.is_empty() {
        return Ok(Vec::new());
    }

    let mut all_results = Vec::new();

    for case in cases {
        let result = run_single_relevance_case(&case, corpus, thresholds)?;
        all_results.push(result);
    }

    Ok(all_results)
}

/// Compute recall, precision, and correctness metrics
pub fn compute_metrics(
    capsule_symbols: &HashSet<String>,
    expected_symbols: &HashSet<String>,
) -> (f64, f64, Correctness) {
    if expected_symbols.is_empty() {
        return (1.0, 1.0, Correctness::Correct);
    }

    let hits = capsule_symbols.intersection(expected_symbols).count();

    let recall = hits as f64 / expected_symbols.len() as f64;

    let precision = if capsule_symbols.is_empty() {
        0.0
    } else {
        hits as f64 / capsule_symbols.len() as f64
    };

    let correctness = Correctness::from_recall_precision(recall, precision);

    (recall, precision, correctness)
}

/// Normalize multi-line Rust signatures to single line for extraction
fn normalize_text(text: &str) -> String {
    let mut result = String::new();
    let mut brace_depth = 0;
    let mut prev_was_newline = true;

    for ch in text.chars() {
        match ch {
            '{' => {
                brace_depth += 1;
                result.push(ch);
                prev_was_newline = false;
            }
            '}' => {
                brace_depth -= 1;
                result.push(ch);
                prev_was_newline = false;
            }
            '\n' | '\r' => {
                if brace_depth > 0 {
                    if !prev_was_newline {
                        result.push(' ');
                    }
                    prev_was_newline = true;
                } else {
                    result.push(ch);
                    prev_was_newline = true;
                }
            }
            _ => {
                result.push(ch);
                prev_was_newline = false;
            }
        }
    }

    result
}

/// Extract symbol names from capsule text using simple heuristics
pub fn extract_symbols_from_capsule(capsule_text: &str) -> HashSet<String> {
    let mut symbols = HashSet::new();

    let normalized = normalize_text(capsule_text);

    for line in normalized.lines() {
        let line = line.trim();

        // Match function definitions: "fn name" or "pub fn name"
        if let Some(cap) = extract_after_keyword(line, "fn ") {
            // Extract just the function name (before any parentheses or generics)
            let name = cap
                .split(|c: char| c == '(' || c == '<' || c == ' ')
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                symbols.insert(name.to_string());
            }
        }

        // Match struct definitions: "struct Name" or "pub struct Name"
        if let Some(cap) = extract_after_keyword(line, "struct ") {
            let name = cap
                .split(|c: char| c == '{' || c == '<' || c == ' ' || c == ';')
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                symbols.insert(name.to_string());
            }
        }

        // Match enum definitions: "enum Name" or "pub enum Name"
        if let Some(cap) = extract_after_keyword(line, "enum ") {
            let name = cap
                .split(|c: char| c == '{' || c == ' ' || c == ';')
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                symbols.insert(name.to_string());
            }
        }

        // Match impl blocks: "impl Name" or "impl Trait for Name"
        if let Some(cap) = extract_after_keyword(line, "impl ") {
            // Skip generic parameters and get the type name
            let after_generics = if cap.contains('<') && cap.contains('>') {
                cap.split('>').nth(1).unwrap_or(cap).trim()
            } else {
                cap
            };

            // For "impl Trait for Type", get "Type"
            let name = if after_generics.contains(" for ") {
                after_generics
                    .split(" for ")
                    .nth(1)
                    .unwrap_or(after_generics)
            } else {
                after_generics
            };

            let name = name
                .split(|c: char| c == '{' || c == ' ')
                .next()
                .unwrap_or("");
            if !name.is_empty() && !name.starts_with("<") {
                symbols.insert(name.to_string());
            }
        }
    }

    symbols
}

/// Extract content after a keyword, handling optional "pub " prefix
fn extract_after_keyword<'a>(line: &'a str, keyword: &str) -> Option<&'a str> {
    // Check for direct keyword match
    if let Some(pos) = line.find(keyword) {
        // Make sure it's not part of another word
        let before = &line[..pos];
        if before.is_empty() || before.ends_with("pub ") {
            let after = &line[pos + keyword.len()..];
            return Some(after.trim_start());
        }
    }
    None
}

/// Run a single relevance evaluation case through the full capsule pipeline.
///
/// This resolves the query via `crate::query::run_query` (target resolution,
/// BM25 search, neighbor collection) then assembles a capsule and extracts
/// symbols to compute recall/precision/correctness.
fn run_single_relevance_case(
    case: &OwnedEvalCase,
    corpus: &[CorpusEntry],
    thresholds: &Thresholds,
) -> Result<CaseResult, String> {
    let corpus_entry = corpus.first().ok_or("No corpus entries available")?;

    let conn = Connection::open_in_memory().map_err(|e| e.to_string())?;
    schema::ensure_branch_schema(&conn).map_err(|e| e.to_string())?;

    let mut graph = GraphState::new();
    let source_files = graph::index::collect_source_files(&corpus_entry.path);
    graph::index::bulk_index(&conn, &mut graph, &source_files)
        .map_err(|e| format!("Failed to index corpus: {}", e))?;
    graph.load_from_db(&conn).map_err(|e| e.to_string())?;

    let config = Config::default();

    // Resolve target using the first expected file + symbol as hints.
    // Fall back to pure BM25 search if no target resolves.
    let symbol_hint = case.expected_symbols.first().map(|s| s.as_str());
    let file_hint = case
        .expected_files
        .first()
        .map(|f| f.as_str())
        .unwrap_or("");

    let qr = query::run_query(
        &conn,
        &graph,
        &config,
        file_hint,
        symbol_hint,
        Some(&case.query),
        &CapsuleConstraints::from_detail(DetailLevel::Standard),
    );

    // Parse detail level from case
    let detail_level = match case.detail_level.as_str() {
        "minimal" => DetailLevel::Minimal,
        "detailed" => DetailLevel::Detailed,
        _ => DetailLevel::Standard,
    };

    let mut constraints = CapsuleConstraints::from_detail(detail_level);
    if let Some(budget) = case.budget_override {
        if budget < 1000 {
            constraints = constraints.with_overrides(Some(3), Some(3), None, None);
        } else if budget < 2000 {
            constraints = constraints.with_overrides(Some(5), Some(5), None, None);
        } else if budget < 4000 {
            constraints = constraints.with_overrides(Some(8), Some(8), None, None);
        }
    }

    let budget_override = case.budget_override;
    let capsule = assemble(&conn, &graph, &config, &qr, budget_override, &constraints);

    // Compute BM25-only baseline: run search without target resolution
    let bm25_symbols = compute_bm25_baseline(&conn, &graph, &case.query, &case.expected_symbols);

    let mut metrics = HashMap::new();
    let mut passed = true;
    let mut failure_reason = None;

    let capsule_symbols = extract_symbols_from_capsule(&capsule.text);

    let expected: HashSet<String> = case.expected_symbols.iter().cloned().collect();
    let expected_set_for_fctc: HashSet<String> = case.expected_symbols.iter().cloned().collect();

    let (recall, precision, correctness) = compute_metrics(&capsule_symbols, &expected);
    let acs = calculate_acs(&capsule_symbols, &expected);

    let bm25_recall = compute_metrics(&bm25_symbols, &expected).0;

    // FCTC proxy: position of first expected symbol in capsule output
    let capsule_items: Vec<String> = capsule_symbols.iter().cloned().collect();
    let fctc = first_correct_position(&capsule_items, &expected_set_for_fctc, |s| s);

    // Context metrics
    let ctx = ContextMetrics::new(capsule.token_count as usize);

    metrics.insert("recall".to_string(), recall);
    metrics.insert("precision".to_string(), precision);
    metrics.insert("acs".to_string(), acs);
    metrics.insert("capsule_tokens".to_string(), capsule.token_count as f64);
    metrics.insert("bm25_recall".to_string(), bm25_recall);
    metrics.insert("graph_recall".to_string(), recall);
    metrics.insert("fctc_proxy".to_string(), fctc as f64);
    metrics.insert("capsule_items".to_string(), capsule.items_included as f64);
    metrics.insert(
        "total_context_tokens".to_string(),
        ctx.total_context_tokens as f64,
    );

    if !thresholds.relevance.passes(RelevanceMetric::Recall, recall) {
        passed = false;
        failure_reason = Some(format!(
            "recall {:.2} below threshold {:.2}",
            recall, thresholds.relevance.min_recall
        ));
    }

    if let Some(assert) = &case.assert {
        let category = &case.category;

        match category {
            CaseCategory::G1Keyword => {
                if assert.graph && recall < 0.5 {
                    passed = false;
                    failure_reason = Some(format!(
                        "G1 case expected graph to find symbols but recall was {:.2}",
                        recall
                    ));
                }
                if assert.bm25 && bm25_recall < 0.5 {
                    passed = false;
                    failure_reason = Some(format!(
                        "G1 case expected BM25 to find symbols but recall was {:.2}",
                        bm25_recall
                    ));
                }
            }
            CaseCategory::G2Structural | CaseCategory::G3Hidden => {
                if assert.graph && recall < 0.5 {
                    passed = false;
                    failure_reason = Some(format!(
                        "G{}/G3 case expected graph to find symbols but recall was {:.2}",
                        if matches!(category, CaseCategory::G2Structural) {
                            2
                        } else {
                            3
                        },
                        recall
                    ));
                }
                if assert.bm25 && bm25_recall >= 0.5 {
                    passed = false;
                    failure_reason = Some(format!(
                        "G2/G3 case expected BM25 to fail but recall was {:.2}",
                        bm25_recall
                    ));
                }
            }
        }
    }

    Ok(CaseResult {
        case_name: case.name.clone(),
        category: case.category.as_str().to_string(),
        metrics,
        correctness,
        passed,
        failure_reason,
        bm25_recall,
        graph_recall: recall,
    })
}

/// Compute BM25-only baseline: search without graph traversal.
///
/// Returns the set of symbol names found by pure FTS5/BM25 search for the
/// given query, intersected with expected_symbols to compute baseline recall.
fn compute_bm25_baseline(
    conn: &Connection,
    graph: &GraphState,
    query: &str,
    expected_symbols: &[String],
) -> HashSet<String> {
    let search_results = search::search_bm25_only(conn, query, 50).unwrap_or_default();
    let expected_set: HashSet<String> = expected_symbols.iter().cloned().collect();
    let mut found = HashSet::new();

    for result in &search_results {
        if let Some(node) = graph.get_weight(&result.node_id)
            && expected_set.contains(&node.name)
        {
            found.insert(node.name.clone());
        } else {
            tracing::warn!("Node {} not found in graph, skipping", result.node_id.0);
        }
    }

    found
}

/// Run performance checks on the corpus
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
            category: "performance".to_string(),
            metrics,
            correctness: if passed {
                Correctness::Correct
            } else {
                Correctness::Incorrect
            },
            passed,
            failure_reason: None,
            bm25_recall: 0.0,
            graph_recall: 0.0,
        });
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_metrics_perfect() {
        let capsule: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        let expected: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        let (recall, precision, correctness) = compute_metrics(&capsule, &expected);
        assert!((recall - 1.0).abs() < 0.001);
        assert!((precision - 1.0).abs() < 0.001);
        assert!(matches!(correctness, Correctness::Correct));
    }

    #[test]
    fn test_compute_metrics_partial() {
        let capsule: HashSet<String> = ["a".to_string(), "c".to_string()].into_iter().collect();
        let expected: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        let (recall, precision, correctness) = compute_metrics(&capsule, &expected);
        assert!((recall - 0.5).abs() < 0.001);
        assert!((precision - 0.5).abs() < 0.001);
        assert!(matches!(correctness, Correctness::Partial));
    }

    #[test]
    fn test_extract_symbols_from_capsule() {
        let capsule = r#"
pub fn parse_config() -> Config {
    Config::new()
}

struct HttpServer {
    port: u16,
}

impl HttpServer {
    fn new() -> Self {
        Self { port: 8080 }
    }
}
"#;
        let symbols = extract_symbols_from_capsule(capsule);
        assert!(symbols.contains("parse_config"));
        assert!(symbols.contains("HttpServer"));
    }

    #[test]
    fn test_extract_unit_struct() {
        let capsule = r#"
pub struct DataProcessor;
"#;
        let symbols = extract_symbols_from_capsule(capsule);
        assert!(
            symbols.contains("DataProcessor"),
            "Should extract DataProcessor without semicolon"
        );
    }

    #[test]
    fn test_extract_unit_enum() {
        let capsule = r#"
pub enum Status;
"#;
        let symbols = extract_symbols_from_capsule(capsule);
        assert!(
            symbols.contains("Status"),
            "Should extract Status without semicolon"
        );
    }

    #[test]
    fn test_extract_multiline_functions() {
        let capsule = r#"
pub fn parse_config() -> Config {
    Config::new()
}

fn helper_with_long_body(
    arg1: Type1,
    arg2: Type2,
) -> Result<Type3, Error> {
    Ok(Type3)
}

struct MultiField {
    field1: Type1,
    field2: Type2,
}
"#;
        let symbols = extract_symbols_from_capsule(capsule);
        // These should be found but were missed before
        assert!(symbols.contains("parse_config"), "Should find parse_config");
        assert!(
            symbols.contains("helper_with_long_body"),
            "Should find helper_with_long_body"
        );
        assert!(symbols.contains("MultiField"), "Should find MultiField");
    }
}
