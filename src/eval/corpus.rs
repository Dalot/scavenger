use crate::eval::{EvalError, EvalResult};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct CorpusEntry {
    pub name: String,
    pub path: PathBuf,
    pub tracked_files: Option<usize>,
}

pub fn load_corpus(root: &Path) -> EvalResult<CorpusEntry> {
    if !root.exists() {
        return Err(EvalError::CorpusNotFound(root.to_path_buf()));
    }

    Ok(CorpusEntry {
        name: root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string(),
        path: root.to_path_buf(),
        tracked_files: git_tracked_file_count(root),
    })
}

fn git_tracked_file_count(path: &Path) -> Option<usize> {
    if !path.join(".git").exists() {
        return None;
    }

    let output = Command::new("git")
        .arg("ls-files")
        .current_dir(path)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let count = stdout.lines().filter(|l| !l.is_empty()).count();
    Some(count)
}
