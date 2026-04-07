use crate::eval::corpus::load_corpus;
use crate::eval::reporter::{print_json, print_summary, run_suite};
use crate::eval::{CaseResult, EvalRun, EvalTier};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub enum EvalSuite {
    Relevance,
    Accuracy,
    Performance,
    Agent,
}

#[derive(Debug, Clone)]
pub struct EvalOptions {
    pub suites: Vec<EvalSuite>,
    pub tier: EvalTier,
    pub corpus_path: Option<String>,
    pub json: bool,
    pub thresholds_path: Option<String>,
    pub agent: Option<String>,
    pub tasks_pattern: Option<String>,
    pub baseline: bool,
    pub compare_run_id: Option<String>,
    pub report: bool,
}

impl Default for EvalOptions {
    fn default() -> Self {
        Self {
            suites: vec![
                EvalSuite::Relevance,
                EvalSuite::Accuracy,
                EvalSuite::Performance,
            ],
            tier: EvalTier::Component,
            corpus_path: None,
            json: false,
            thresholds_path: None,
            agent: None,
            tasks_pattern: None,
            baseline: false,
            compare_run_id: None,
            report: false,
        }
    }
}

pub fn run_evals(opts: &EvalOptions) -> Result<Vec<EvalRun>, String> {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
    let corpus_path: PathBuf = opts
        .corpus_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(&manifest_dir).join("eval/corpus"));

    let thresholds_path: PathBuf = opts
        .thresholds_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(&manifest_dir).join("eval/thresholds.toml"));

    let thresholds = crate::eval::thresholds::load_thresholds(&thresholds_path)
        .map_err(|e| format!("Failed to load thresholds: {}", e))?;

    let corpus_entry =
        load_corpus(&corpus_path).map_err(|e| format!("Failed to load corpus: {}", e))?;
    let corpus_entries = vec![corpus_entry];

    if corpus_entries.is_empty() {
        return Err(
            "No corpus entries found. Add projects to eval/corpus/ or use --corpus".to_string(),
        );
    }

    let mut all_runs = Vec::new();

    for suite in &opts.suites {
        match suite {
            EvalSuite::Relevance => {
                let results =
                    crate::eval::relevance::run_relevance_eval(&corpus_entries, &thresholds)?;
                let run = run_suite(
                    EvalTier::Component,
                    "relevance",
                    &corpus_entries
                        .first()
                        .map(|e| e.name.clone())
                        .unwrap_or_default(),
                    results,
                );
                all_runs.push(run);
            }
            EvalSuite::Accuracy => {
                let results =
                    crate::eval::accuracy::run_accuracy_eval(&corpus_entries, &thresholds)?;
                let run = run_suite(
                    EvalTier::Component,
                    "accuracy",
                    &corpus_entries
                        .first()
                        .map(|e| e.name.clone())
                        .unwrap_or_default(),
                    results,
                );
                all_runs.push(run);
            }
            EvalSuite::Performance => {
                let results =
                    crate::eval::relevance::run_performance_checks(&corpus_entries, &thresholds)?;
                let run = run_suite(
                    EvalTier::Component,
                    "performance",
                    &corpus_entries
                        .first()
                        .map(|e| e.name.clone())
                        .unwrap_or_default(),
                    results,
                );
                all_runs.push(run);
            }
            EvalSuite::Agent => {
                if matches!(opts.tier, EvalTier::Component) {
                    continue;
                }
                let manifest_dir =
                    std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string());
                let tasks_dir = std::path::Path::new(&manifest_dir).join("eval/tasks");

                let agent_type = opts.agent.as_deref().unwrap_or("claude");
                let agent_results = match agent_type {
                    "claude" => crate::eval::agent::claude_runner::run_claude_evals(
                        &tasks_dir,
                        &corpus_path,
                        opts.tasks_pattern.as_deref(),
                        opts.baseline,
                    )?,
                    "cursor" => crate::eval::agent::cursor_runner::run_cursor_evals(
                        &tasks_dir,
                        &corpus_path,
                        opts.tasks_pattern.as_deref(),
                        opts.baseline,
                    )?,
                    other => return Err(format!("Unknown agent: {}", other)),
                };

                let case_results: Vec<CaseResult> = agent_results
                    .iter()
                    .map(|r| {
                        let mut metrics = std::collections::HashMap::new();
                        metrics.insert(
                            "token_delta".to_string(),
                            if let Some(b) = &r.baseline {
                                if b.tokens_used > 0 {
                                    ((r.with_scavenger.tokens_used as f64 - b.tokens_used as f64)
                                        / b.tokens_used as f64)
                                        * 100.0
                                } else {
                                    0.0
                                }
                            } else {
                                0.0
                            },
                        );
                        CaseResult {
                            case_name: r.task_name.clone(),
                            metrics,
                            passed: r.success,
                            failure_reason: if r.success {
                                None
                            } else {
                                Some(r.success_details.join("; "))
                            },
                        }
                    })
                    .collect();

                let run = run_suite(
                    EvalTier::Agent,
                    "agent",
                    &corpus_entries
                        .first()
                        .map(|e| e.name.clone())
                        .unwrap_or_default(),
                    case_results,
                );
                all_runs.push(run);
            }
        }
    }

    for run in &all_runs {
        print_summary(run);
        if opts.json {
            print_json(run).map_err(|e| format!("Failed to print JSON: {}", e))?;
        }
    }

    Ok(all_runs)
}
