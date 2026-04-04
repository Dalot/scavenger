use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize)]
pub struct CorpusEntry {
    pub name: String,
    pub path: PathBuf,
    pub file_count: usize,
}

pub fn load_corpus(root: &Path) -> Result<Vec<CorpusEntry>, String> {
    if !root.exists() {
        return Err(format!("Corpus path not found: {}", root.display()));
    }

    if is_project_dir(root) {
        let file_count = count_source_files(root);
        Ok(vec![CorpusEntry {
            name: root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string(),
            path: root.to_path_buf(),
            file_count,
        }])
    } else {
        let mut entries = Vec::new();
        let dir =
            fs::read_dir(root).map_err(|e| format!("Cannot read {}: {}", root.display(), e))?;

        for entry in dir {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() && is_project_dir(&path) {
                let file_count = count_source_files(&path);
                entries.push(CorpusEntry {
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string(),
                    path,
                    file_count,
                });
            }
        }

        Ok(entries)
    }
}

fn is_project_dir(path: &Path) -> bool {
    // Check for build-tool markers at the top level
    if path.join("Cargo.toml").exists()
        || path.join("package.json").exists()
        || path.join("go.mod").exists()
        || path.join("pyproject.toml").exists()
    {
        return true;
    }
    // Check for source files only in immediate children (not recursively),
    // so a directory of projects isn't mistaken for a single project.
    has_immediate_source_files(path)
}

fn has_immediate_source_files(path: &Path) -> bool {
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && is_source_file(&path) {
                return true;
            }
            // Also check src/ or lib/ directory
            if path.is_dir()
                && let Some(name) = path.file_name()
                && (name == "src" || name == "lib")
                && let Ok(src_entries) = fs::read_dir(&path)
            {
                for src_entry in src_entries.flatten() {
                    let src_path = src_entry.path();
                    if src_path.is_file() && is_source_file(&src_path) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

fn count_source_files(path: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(name) = path.file_name() {
                    let name = name.to_string_lossy();
                    if name.starts_with('.')
                        || name == "target"
                        || name == "node_modules"
                        || name == ".git"
                        || name == "build"
                    {
                        continue;
                    }
                }
                count += count_source_files(&path);
            } else if is_source_file(&path) {
                count += 1;
            }
        }
    }
    count
}

fn is_source_file(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(
        ext,
        "rs" | "py"
            | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "go"
            | "java"
            | "cs"
            | "c"
            | "cpp"
            | "rb"
            | "sh"
            | "php"
            | "swift"
    )
}
