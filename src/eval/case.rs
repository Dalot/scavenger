//! JSON Lines case definitions for evaluation
//!
//! This module defines the schema for eval test cases stored in JSON Lines files.
//! Cases are categorized into G1 (keyword-findable), G2 (structural), and G3 (hidden).
//! Format: one JSON object per line, e.g.:
//! {"name": "g1-find-parse_config", "category": "g1_keyword", "query": "parse_config function", "expected": ["parse_config"], "files": ["src/config.rs"], "detail": "standard", "assert": {"bm25": true, "graph": true}}

use serde::Deserialize;
use std::fs;
use std::path::Path;

use crate::eval::{EvalError, EvalResult};

/// A single evaluation test case (JSON Lines format)
#[derive(Debug, Clone, Deserialize)]
pub struct EvalCase {
    /// Unique name for this test case
    pub name: String,
    /// Category: g1_keyword, g2_structural, or g3_hidden
    pub category: CaseCategory,
    /// Query string to execute
    pub query: String,
    /// Expected symbols that should be in the capsule
    pub expected: Vec<String>,
    /// Expected files that should be referenced (optional)
    #[serde(default)]
    pub files: Vec<String>,
    /// Detail level: minimal, standard, or detailed
    #[serde(default)]
    pub detail: Option<String>,
    /// Optional token budget override
    #[serde(default)]
    pub budget: Option<u32>,
    /// Assertions for BM25 vs Graph comparison
    #[serde(default)]
    pub assert: Option<CaseAssert>,
}

impl EvalCase {
    /// Convert to internal representation for eval runner
    pub fn to_owned(&self) -> OwnedEvalCase {
        OwnedEvalCase {
            name: self.name.clone(),
            category: self.category.clone(),
            query: self.query.clone(),
            expected_symbols: self.expected.clone(),
            expected_files: self.files.clone(),
            detail_level: self
                .detail
                .clone()
                .unwrap_or_else(|| "standard".to_string()),
            budget_override: self.budget,
            assert: self.assert.clone(),
        }
    }
}

/// Owned version for use in eval runner
#[derive(Debug, Clone)]
pub struct OwnedEvalCase {
    pub name: String,
    pub category: CaseCategory,
    pub query: String,
    pub expected_symbols: Vec<String>,
    pub expected_files: Vec<String>,
    pub detail_level: String,
    pub budget_override: Option<u32>,
    pub assert: Option<CaseAssert>,
}

/// Case categories following CodeCompass taxonomy
#[derive(Debug, Clone, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct CaseAssert {
    /// Whether BM25 alone should find the expected symbols
    #[serde(alias = "bm25_should_find")]
    pub bm25: bool,
    /// Whether graph traversal should find the expected symbols
    #[serde(alias = "graph_should_find")]
    pub graph: bool,
}

/// Load all JSON Lines case files from a directory
pub fn load_cases(dir: &Path) -> EvalResult<Vec<OwnedEvalCase>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut all_cases = Vec::new();

    for entry in fs::read_dir(dir).map_err(|e| EvalError::ReadError(dir.to_path_buf(), e))? {
        let entry = entry.map_err(|e| EvalError::ReadError(dir.to_path_buf(), e))?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }

        let content =
            fs::read_to_string(&path).map_err(|e| EvalError::ReadError(path.clone(), e))?;

        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let case: EvalCase = serde_json::from_str(trimmed)
                .map_err(|e| EvalError::ParseError(path.clone(), Box::new(e)))?;
            all_cases.push(case.to_owned());
        }
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
    fn test_load_cases_single_jsonl() {
        let tmp = TempDir::new().unwrap();
        let case_file = tmp.path().join("test.jsonl");

        let content = r#"{"name": "test-case", "category": "g1_keyword", "query": "test query", "expected": ["foo", "bar"]}
"#;

        let mut file = fs::File::create(&case_file).unwrap();
        file.write_all(content.as_bytes()).unwrap();

        let cases = load_cases(tmp.path()).unwrap();
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].name, "test-case");
        assert_eq!(cases[0].detail_level, "standard");
    }

    #[test]
    fn test_case_json_deserialization() {
        let json = r#"{"name": "g1", "category": "g1_keyword", "query": "test", "expected": []}"#;
        let case: EvalCase = serde_json::from_str(json).unwrap();
        assert_eq!(case.name, "g1");
        assert!(matches!(case.category, CaseCategory::G1Keyword));
    }

    #[test]
    fn test_case_with_assert() {
        let json = r#"{"name": "test", "category": "g1_keyword", "query": "test", "expected": [], "assert": {"bm25": true, "graph": true}}"#;
        let case: EvalCase = serde_json::from_str(json).unwrap();
        assert!(case.assert.is_some());
        assert!(case.assert.as_ref().unwrap().bm25);
        assert!(case.assert.as_ref().unwrap().graph);
    }
}
