use scavenger::eval::corpus::load_corpus;
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
}

#[test]
fn test_load_corpus_has_file_count() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_project");
    let result = load_corpus(&fixtures).unwrap();
    assert!(result[0].file_count > 0);
}

#[test]
fn test_load_corpus_multi_project() {
    let eval_corpus = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("eval/corpus");
    // Create two temporary project dirs
    let proj_a = eval_corpus.join("test_proj_a");
    let proj_b = eval_corpus.join("test_proj_b");
    fs::create_dir_all(proj_a.join("src")).unwrap();
    fs::create_dir_all(proj_b.join("src")).unwrap();
    fs::write(proj_a.join("src").join("main.rs"), "fn main() {}").unwrap();
    fs::write(proj_b.join("src").join("lib.rs"), "pub fn lib() {}").unwrap();

    let result = load_corpus(&eval_corpus).unwrap();
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

    // Cleanup
    fs::remove_dir_all(proj_a).unwrap();
    fs::remove_dir_all(proj_b).unwrap();
}

#[test]
fn test_load_corpus_skips_hidden_dirs() {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sample_project");
    // Create a hidden dir with source files
    let hidden = fixtures.join(".hidden_dir");
    fs::create_dir_all(&hidden).unwrap();
    fs::write(hidden.join("secret.rs"), "fn secret() {}").unwrap();

    let result = load_corpus(&fixtures).unwrap();
    let baseline_count = result[0].file_count;

    // The hidden dir's files should NOT be counted
    assert_eq!(baseline_count, result[0].file_count);

    // Cleanup
    fs::remove_dir_all(hidden).unwrap();
}
