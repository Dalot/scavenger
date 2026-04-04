use scavenger::eval::thresholds::{Thresholds, load_thresholds};
use std::path::PathBuf;

#[test]
fn test_load_default_thresholds() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("eval/thresholds.toml");
    let result = load_thresholds(&path);
    assert!(result.is_ok());
    let t = result.unwrap();
    assert!(t.relevance.min_recall > 0.0);
    assert!(t.relevance.min_precision > 0.0);
}

#[test]
fn test_threshold_check_pass() {
    let t = Thresholds::default();
    assert!(t.relevance.passes("recall", 0.90));
    assert!(t.relevance.passes("precision", 0.70));
}

#[test]
fn test_threshold_check_fail() {
    let t = Thresholds::default();
    assert!(!t.relevance.passes("recall", 0.50));
    assert!(!t.relevance.passes("precision", 0.30));
}

#[test]
fn test_threshold_missing_file() {
    let result = load_thresholds(&PathBuf::from("/nonexistent/thresholds.toml"));
    assert!(result.is_ok());
}
