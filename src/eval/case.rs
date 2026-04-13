//! TOML case definitions for evaluation
//!
//! This module defines the schema for eval test cases stored in TOML files.
//! Cases are categorized into G1 (keyword-findable), G2 (structural), and G3 (hidden).

use serde::Deserialize;
use std::fs;
use std::path::Path;

use crate::eval::{EvalError, EvalResult};

/// A single evaluation test case
#[derive(Debug, Clone, Deserialize)]
pub struct EvalCase {
    /// Unique name for this test case
    pub name: String,
    /// Category: g1_keyword, g2_structural, or g3_hidden
    pub category: CaseCategory,
    /// Query string to execute
    pub query: String,
    /// Expected symbols that should be in the capsule
    pub expected_symbols: Vec<String>,
    /// Expected files that should be referenced (optional)
    #[serde(default)]
    pub expected_files: Vec<String>,
    /// Detail level: minimal, standard, or detailed
    #[serde(default = "default_detail_level")]
    pub detail_level: String,
    /// Optional token budget override
    #[serde(default)]
    pub budget_override: Option<u32>,
    /// Assertions for BM25 vs Graph comparison
    #[serde(default)]
    pub assert: Option<CaseAssert>,
}

fn default_detail_level() -> String {
    "standard".to_string()
}

/// Case categories following CodeCompass taxonomy
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseCategory {
    /// G1: Keyword-findable via BM25 (exact symbol name match)
    #[serde(rename = "g1_keyword")]
    G1Keyword,
    /// G2: Structural dependencies requiring 1-2 hop traversal
    #[serde(rename = "g2_structural")]
    G2Structural,
    /// G3: Hidden dependencies requiring 2+ hop traversal
    #[serde(rename = "g3_hidden")]
    G3Hidden,
}

impl CaseCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            CaseCategory::G1Keyword => "g1_keyword",
            CaseCategory::G2Structural => "g2_structural",
            CaseCategory::G3Hidden => "g3_hidden",
        }
    }
}

/// Assertions for validating retrieval method performance
#[derive(Debug, Clone, Deserialize)]
pub struct CaseAssert {
    /// Whether BM25 alone should find the expected symbols
    pub bm25_should_find: bool,
    /// Whether graph traversal should find the expected symbols
    pub graph_should_find: bool,
}

/// Container for multiple cases in a single TOML file
#[derive(Debug, Clone, Deserialize)]
pub struct EvalCases {
    #[serde(rename = "case")]
    pub cases: Vec<EvalCase>,
}

/// Load all TOML case files from a directory
pub fn load_cases(dir: &Path) -> EvalResult<Vec<EvalCase>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut all_cases = Vec::new();

    for entry in fs::read_dir(dir).map_err(|e| EvalError::ReadError(dir.to_path_buf(), e))? {
        let entry = entry.map_err(|e| EvalError::ReadError(dir.to_path_buf(), e))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("toml") {
            continue;
        }

        let content =
            fs::read_to_string(&path).map_err(|e| EvalError::ReadError(path.clone(), e))?;
        let cases: EvalCases =
            toml::from_str(&content).map_err(|e| EvalError::ParseError(path, e))?;

        all_cases.extend(cases.cases);
    }

    Ok(all_cases)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_load_cases_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let cases = load_cases(tmp.path()).unwrap();
        assert!(cases.is_empty());
    }

    #[test]
    fn test_load_cases_single_file() {
        let tmp = TempDir::new().unwrap();
        let case_file = tmp.path().join("test.toml");

        let content = r#"
[[case]]
name = "test-case"
category = "g1_keyword"
query = "test query"
expected_symbols = ["foo", "bar"]
"#;

        let mut file = fs::File::create(&case_file).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let cases = load_cases(tmp.path()).unwrap();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].name, "test-case");
        assert_eq!(cases[0].detail_level, "standard"); // default
    }

    #[test]
    fn test_case_category_serialization() {
        let content = r#"
[[case]]
name = "g1"
category = "g1_keyword"
query = "test"
expected_symbols = []

[[case]]
name = "g2"
category = "g2_structural"
query = "test"
expected_symbols = []

[[case]]
name = "g3"
category = "g3_hidden"
query = "test"
expected_symbols = []
"#;

        let cases: EvalCases = toml::from_str(content).unwrap();
        assert_eq!(cases.cases.len(), 3);
        assert!(matches!(cases.cases[0].category, CaseCategory::G1Keyword));
        assert!(matches!(
            cases.cases[1].category,
            CaseCategory::G2Structural
        ));
        assert!(matches!(cases.cases[2].category, CaseCategory::G3Hidden));
    }
}
