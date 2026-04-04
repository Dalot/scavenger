use scavenger::eval::corpus::load_corpus;
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
}

#[test]
fn test_load_corpus_has_file_count() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_project");
    let result = load_corpus(&fixtures).unwrap();
    assert!(result[0].file_count > 0);
}
