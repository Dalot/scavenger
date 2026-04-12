use crate::eval::agent::types::AgentTask;
use std::fs;
use std::path::Path;

pub fn load_tasks(tasks_dir: &Path, pattern: Option<&str>) -> Result<Vec<AgentTask>, String> {
    if !tasks_dir.exists() {
        return Ok(Vec::new());
    }

    let mut tasks = Vec::new();

    for entry in fs::read_dir(tasks_dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();

        if path.extension().and_then(|e| e.to_str()) != Some("yaml")
            && path.extension().and_then(|e| e.to_str()) != Some("yml")
        {
            continue;
        }

        if let Some(pat) = pattern {
            if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                if !glob_match(pat, name) {
                    continue;
                }
            }
        }

        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let task: AgentTask = serde_yaml::from_str(&content)
            .map_err(|e| format!("Invalid YAML in {:?}: {}", path, e))?;
        tasks.push(task);
    }

    Ok(tasks)
}

fn glob_match(pattern: &str, text: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.contains('*') {
        let parts: Vec<&str> = pattern.split('*').collect();
        if parts.len() == 2 {
            return text.starts_with(parts[0]) && text.ends_with(parts[1]);
        }
    }
    pattern == text
}
