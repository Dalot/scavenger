use crate::eval::corpus::CorpusEntry;
use crate::eval::thresholds::{AccuracyMetric, Thresholds};
use crate::eval::{CaseResult, Correctness};
use crate::query::intent::{Intent, classify};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
struct AccuracyCase {
    name: String,
    corpus: String,
    query: String,
    expected_intent: String,
}

#[derive(Debug, Clone, Deserialize)]
struct AccuracyCases {
    #[serde(rename = "case")]
    cases: Vec<AccuracyCase>,
}

pub fn run_accuracy_eval(
    corpus: &[CorpusEntry],
    thresholds: &Thresholds,
) -> Result<Vec<CaseResult>, String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let cases_dir = Path::new(&manifest_dir).join("eval/cases/accuracy");

    if !cases_dir.exists() {
        return Ok(Vec::new());
    }

    if corpus.is_empty() {
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
        let cases: AccuracyCases =
            toml::from_str(&content).map_err(|e| format!("Invalid TOML in {:?}: {}", path, e))?;

        for case in &cases.cases {
            let _ = corpus
                .iter()
                .find(|e| e.name == case.corpus)
                .ok_or_else(|| format!("Corpus '{}' not found", case.corpus))?;
            let result = run_single_accuracy_case(case, thresholds)?;
            all_results.push(result);
        }
    }

    Ok(all_results)
}

fn run_single_accuracy_case(
    case: &AccuracyCase,
    thresholds: &Thresholds,
) -> Result<CaseResult, String> {
    let intent_result = classify(&case.query);
    let primary_intent = intent_result.primary;

    let expected_intent = match case.expected_intent.as_str() {
        "Understand" => Intent::Understand,
        "Debug" => Intent::Debug,
        "Refactor" => Intent::Refactor,
        "Extend" => Intent::Extend,
        "Review" => Intent::Review,
        _ => Intent::Understand,
    };

    let correct = primary_intent == expected_intent;

    let mut metrics = HashMap::new();
    metrics.insert(
        "intent_accuracy".to_string(),
        if correct { 1.0 } else { 0.0 },
    );

    let passed = thresholds.accuracy.passes(
        AccuracyMetric::IntentAccuracy,
        if correct { 1.0 } else { 0.0 },
    );

    Ok(CaseResult {
        case_name: case.name.clone(),
        category: "accuracy".to_string(),
        metrics,
        correctness: if correct {
            Correctness::Correct
        } else {
            Correctness::Incorrect
        },
        passed,
        failure_reason: if correct {
            None
        } else {
            Some(format!(
                "expected intent {:?}, got {:?}",
                expected_intent, primary_intent
            ))
        },
        bm25_recall: 0.0,
        graph_recall: if correct { 1.0 } else { 0.0 },
    })
}
