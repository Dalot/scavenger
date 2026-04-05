use scavenger::eval::{EvalError, corpus::load_corpus};
use std::fs;
use std::path::PathBuf;

#[test]
fn test_load_corpus_single_project() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_project");
    let result = load_corpus(&fixtures);
    assert!(result.is_ok());
    let entries = result.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "sample_project");
}

#[test]
fn test_load_corpus_missing() {
    let missing = PathBuf::from("/nonexistent/path");
    let result = load_corpus(&missing);
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(matches!(err, EvalError::CorpusNotFound(_)));
}

#[test]
fn test_load_corpus_non_git_has_no_tracked_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let proj = temp_dir.path().join("no_git_project");
    fs::create_dir_all(proj.join("src")).unwrap();
    fs::write(proj.join("src").join("main.rs"), "fn main() {}").unwrap();

    let result = load_corpus(&proj).unwrap();
    assert!(result[0].tracked_files.is_none());
}

#[test]
fn test_load_corpus_multi_project() {
    let temp_dir = tempfile::tempdir().unwrap();
    let proj_a = temp_dir.path().join("test_proj_a");
    let proj_b = temp_dir.path().join("test_proj_b");
    fs::create_dir_all(proj_a.join("src")).unwrap();
    fs::create_dir_all(proj_b.join("src")).unwrap();
    fs::write(proj_a.join("src").join("main.rs"), "fn main() {}").unwrap();
    fs::write(proj_b.join("src").join("lib.rs"), "pub fn lib() {}").unwrap();

    let result = load_corpus(temp_dir.path()).unwrap();
    let names: Vec<&str> = result.iter().map(|e| e.name.as_str()).collect();
    assert!(
        names.contains(&"test_proj_a"),
        "test_proj_a not found in {:?}",
        names
    );
    assert!(
        names.contains(&"test_proj_b"),
        "test_proj_b not found in {:?}",
        names
    );
    // temp_dir automatically cleaned up when dropped
}

#[test]
fn test_load_corpus_skips_hidden_dirs() {
    let temp_dir = tempfile::tempdir().unwrap();
    let visible = temp_dir.path().join("visible_project");
    let hidden = temp_dir.path().join(".hidden_project");
    fs::create_dir_all(visible.join("src")).unwrap();
    fs::create_dir_all(hidden.join("src")).unwrap();
    fs::write(visible.join("src").join("main.rs"), "fn main() {}").unwrap();
    fs::write(hidden.join("src").join("secret.rs"), "fn secret() {}").unwrap();

    let result = load_corpus(temp_dir.path()).unwrap();
    let names: Vec<&str> = result.iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"visible_project"));
    assert!(!names.contains(&".hidden_project"));
}

#[test]
fn test_load_corpus_git_tracked_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let proj = temp_dir.path().join("git_project");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("main.rs"), "fn main() {}").unwrap();
    fs::write(proj.join("lib.rs"), "pub fn lib() {}").unwrap();

    // Initialize git repo and commit files
    std::process::Command::new("git")
        .arg("init")
        .current_dir(&proj)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .arg("add")
        .arg(".")
        .current_dir(&proj)
        .output()
        .unwrap();
    std::process::Command::new("git")
        .args([
            "-c",
            "user.email=test@test.com",
            "-c",
            "user.name=test",
            "commit",
            "-m",
            "init",
        ])
        .current_dir(&proj)
        .output()
        .unwrap();

    let result = load_corpus(temp_dir.path()).unwrap();
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].tracked_files, Some(2));
}
