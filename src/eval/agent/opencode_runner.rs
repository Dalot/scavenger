use crate::eval::agent::task::load_tasks;
use crate::eval::agent::types::{AgentRunMetrics, AgentTask, AgentTaskResult};
use std::path::Path;
use std::process::Command;

pub fn run_opencode_evals(
    tasks_dir: &Path,
    _corpus_path: &Path,
    tasks_pattern: Option<&str>,
    baseline: bool,
) -> Result<Vec<AgentTaskResult>, String> {
    let tasks = load_tasks(tasks_dir, tasks_pattern)?;
    let mut results = Vec::new();

    let opencode_available = Command::new("opencode").arg("--version").output().is_ok();

    if !opencode_available {
        return Err("opencode CLI not found. Install OpenCode to run agent evals.".to_string());
    }

    for task in &tasks {
        let result = run_single_opencode_task(task, baseline)?;
        results.push(result);
    }

    Ok(results)
}

fn run_single_opencode_task(task: &AgentTask, baseline: bool) -> Result<AgentTaskResult, String> {
    Ok(AgentTaskResult {
        task_name: task.name.clone(),
        agent: "opencode".to_string(),
        with_scavenger: AgentRunMetrics {
            tokens_used: 0,
            tool_calls: 0,
            files_read: 0,
            wall_time_seconds: 0.0,
            navigation_efficiency: 0.0,
        },
        baseline: if baseline {
            None
        } else {
            Some(AgentRunMetrics {
                tokens_used: 0,
                tool_calls: 0,
                files_read: 0,
                wall_time_seconds: 0.0,
                navigation_efficiency: 0.0,
            })
        },
        success: false,
        success_details: vec![
            "Not yet implemented — requires opencode CLI integration".to_string(),
        ],
    })
}
