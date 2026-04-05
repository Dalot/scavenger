use scavenger::eval::{EvalError, load_corpus};
use std::fs;
use std::path::PathBuf;

#[test]
fn test_load_corpus_single_project() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_project");
    let result = load_corpus(&fixtures);
    assert!(result.is_ok());
    let entry = result.unwrap();
    assert_eq!(entry.name, "sample_project");
    assert_eq!(entry.path, fixtures);
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

    let entry = load_corpus(&proj).unwrap();
    assert!(entry.tracked_files.is_none());
}

#[test]
fn test_load_corpus_git_tracked_files() {
    let temp_dir = tempfile::tempdir().unwrap();
    let proj = temp_dir.path().join("git_project");
    fs::create_dir_all(&proj).unwrap();
    fs::write(proj.join("main.rs"), "fn main() {}").unwrap();
    fs::write(proj.join("lib.rs"), "pub fn lib() {}").unwrap();

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

    let entry = load_corpus(&proj).unwrap();
    assert_eq!(entry.tracked_files, Some(2));
}
