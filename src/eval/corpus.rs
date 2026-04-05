use crate::eval::{EvalError, EvalResult};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize)]
pub struct CorpusEntry {
    pub name: String,
    pub path: PathBuf,
    pub tracked_files: Option<usize>,
}

pub fn load_corpus(root: &Path) -> EvalResult<Vec<CorpusEntry>> {
    if !root.exists() {
        return Err(EvalError::CorpusNotFound(root.to_path_buf()));
    }

    let subdirs = collect_subdirs(root);

    if subdirs.is_empty() || !subdirs.iter().any(|d| has_subdirs(d)) {
        return Ok(vec![CorpusEntry {
            name: root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            path: root.to_path_buf(),
            tracked_files: git_tracked_file_count(root),
        }]);
    }

    let mut entries = Vec::new();
    for path in subdirs {
        if is_dir_non_empty(&path) {
            let tracked_files = git_tracked_file_count(&path);
            entries.push(CorpusEntry {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                path,
                tracked_files,
            });
        }
    }

    Ok(entries)
}

fn collect_subdirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name() {
                    let name = name.to_string_lossy();
                    if name.starts_with('.') {
                        continue;
                    }
                }
                dirs.push(path);
            }
        }
    }
    dirs.sort();
    dirs
}

fn has_subdirs(path: &Path) -> bool {
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                return true;
            }
        }
    }
    false
}

fn is_dir_non_empty(path: &Path) -> bool {
    if let Ok(mut entries) = fs::read_dir(path) {
        entries.next().is_some()
    } else {
        false
    }
}

fn git_tracked_file_count(path: &Path) -> Option<usize> {
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
