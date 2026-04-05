use crate::eval::{CaseResult, EvalRun, EvalTier, SuiteSummary};
use chrono::Utc;
use owo_colors::OwoColorize;
use std::io::{self, Write};

pub fn run_suite(
    tier: EvalTier,
    suite: &str,
    corpus_name: &str,
    results: Vec<CaseResult>,
) -> EvalRun {
    let passed = results.iter().filter(|r| r.passed).count();
    let failed = results.iter().filter(|r| !r.passed).count();
    let total = results.len();

    let mut averages = std::collections::HashMap::new();
    if !results.is_empty() {
        let mut metric_sums: std::collections::HashMap<String, f64> =
            std::collections::HashMap::new();
        let mut metric_counts: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        for r in &results {
            for (k, v) in &r.metrics {
                *metric_sums.entry(k.clone()).or_insert(0.0) += *v;
                *metric_counts.entry(k.clone()).or_insert(0) += 1;
            }
        }

        for (k, sum) in metric_sums {
            let count = metric_counts.get(&k).copied().unwrap_or(1);
            averages.insert(k, sum / count as f64);
        }
    }

    EvalRun {
        run_id: Utc::now().to_rfc3339(),
        scavenger_version: env!("CARGO_PKG_VERSION").to_string(),
        tier,
        suite: suite.to_string(),
        corpus: corpus_name.to_string(),
        results,
        summary: SuiteSummary {
            suite_name: suite.to_string(),
            corpus: corpus_name.to_string(),
            total_cases: total,
            passed,
            failed,
            averages,
        },
    }
}

pub fn print_summary(run: &EvalRun) {
    let s = &run.summary;
    let stderr = io::stderr();
    let mut out = stderr.lock();

    writeln!(out).unwrap();
    writeln!(out, "Eval: {} (corpus: {})", s.suite_name.bold(), s.corpus).unwrap();
    writeln!(out, "{}", "─".repeat(50)).unwrap();
    writeln!(
        out,
        "Cases:     {} total, {} passed, {} failed",
        s.total_cases, s.passed, s.failed
    )
    .unwrap();

    for (metric, value) in &s.averages {
        let display = format!("{:.2}", value);
        writeln!(out, "{}: {}", metric.replace('_', " ").bold(), display).unwrap();
    }

    let failures: Vec<&CaseResult> = run.results.iter().filter(|r| !r.passed).collect();
    if !failures.is_empty() {
        writeln!(out).unwrap();
        writeln!(out, "{}", "FAILURES:".red().bold()).unwrap();
        for f in &failures {
            if let Some(reason) = &f.failure_reason {
                writeln!(out, "  {} — {}", f.case_name.red(), reason).unwrap();
            } else {
                writeln!(out, "  {}", f.case_name.red()).unwrap();
            }
        }
    }

    writeln!(out).unwrap();
}

pub fn print_json(run: &EvalRun) -> Result<(), String> {
    let json = serde_json::to_string_pretty(run).map_err(|e| e.to_string())?;
    println!("{}", json);
    Ok(())
}
