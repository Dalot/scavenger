use crate::eval::agent::types::AgentResult;
use std::path::Path;

pub fn run_cursor_evals(
    _tasks_dir: &Path,
    _corpus_path: &Path,
    _tasks_pattern: Option<&str>,
    _baseline: bool,
) -> Result<Vec<AgentResult>, String> {
    Ok(Vec::new())
}
