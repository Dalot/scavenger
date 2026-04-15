//! Coverage metrics: ACS and FCTC proxy
//!
//! ACS (Architecture Coverage Score) measures how well the capsule covers
//! required architectural elements at symbol level.
//!
//! FCTC (First Correct Tool Call) proxy measures the position of the first
//! expected symbol in the capsule output order.

use serde::Serialize;
use std::collections::HashSet;

/// Calculate Architecture Coverage Score (ACS) at symbol level
///
/// ACS = |symbols_in_capsule ∩ required_symbols| / |required_symbols|
///
/// This is structurally identical to Recall@K when required_symbols = expected_symbols,
/// but conceptually different: ACS measures architectural coverage for a task,
/// while Recall@K measures retrieval success against a fixed K.
///
/// # Arguments
/// * `symbols_in_capsule` - Symbols present in the generated capsule
/// * `required_symbols` - Symbols required for the task
///
/// # Returns
/// ACS value in range [0.0, 1.0]. Returns 1.0 if required_symbols is empty.
pub fn calculate_acs(
    symbols_in_capsule: &HashSet<String>,
    required_symbols: &HashSet<String>,
) -> f64 {
    if required_symbols.is_empty() {
        return 1.0;
    }
    let intersection = symbols_in_capsule.intersection(required_symbols).count();
    intersection as f64 / required_symbols.len() as f64
}

/// FCTC Proxy: Position of first expected symbol in capsule output order
///
/// Lower is better:
/// - 1 = first item in capsule (best)
/// - N = Nth item in capsule
/// - 0 = not found in capsule
///
/// Note: Full FCTC requires agent-tier evaluation (counting actual tool calls).
/// This is a component-tier proxy using the position in the capsule output.
///
/// # Arguments
/// * `capsule_items` - Items in the capsule in output order
/// * `expected_symbols` - Set of expected symbol names
/// * `get_name` - Function to extract name from a capsule item
///
/// # Returns
/// 1-based position, or 0 if not found
pub fn first_correct_position<T>(
    capsule_items: &[T],
    expected_symbols: &HashSet<String>,
    get_name: impl Fn(&T) -> &str,
) -> usize {
    for (i, item) in capsule_items.iter().enumerate() {
        if expected_symbols.contains(get_name(item)) {
            return i + 1; // 1-based position
        }
    }
    0 // Not found
}

/// Token count metrics for injected context
///
/// Tracks the total context injected into the model, not just capsule size.
/// Per LongCodeBench findings, this is crucial for understanding context rot.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ContextMetrics {
    /// Tokens in the capsule content
    pub capsule_token_count: usize,
    /// Tokens in system prompt (approximate)
    pub system_prompt_tokens: usize,
    /// Tokens for annotations/metadata (approximate)
    pub annotation_tokens: usize,
    /// Total injected context
    pub total_context_tokens: usize,
}

impl ContextMetrics {
    /// Create metrics from capsule token count
    ///
    /// System prompt and annotation tokens are estimated.
    pub fn new(capsule_tokens: usize) -> Self {
        let system_prompt = 500; // Approximate system prompt size
        let annotations = 200; // Approximate annotation overhead

        Self {
            capsule_token_count: capsule_tokens,
            system_prompt_tokens: system_prompt,
            annotation_tokens: annotations,
            total_context_tokens: capsule_tokens + system_prompt + annotations,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_acs_perfect() {
        let capsule: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        let required: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        assert!((calculate_acs(&capsule, &required) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_acs_partial() {
        let capsule: HashSet<String> = ["a".to_string()].into_iter().collect();
        let required: HashSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
        assert!((calculate_acs(&capsule, &required) - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_calculate_acs_empty_required() {
        let capsule: HashSet<String> = ["a".to_string()].into_iter().collect();
        let required: HashSet<String> = HashSet::new();
        assert!((calculate_acs(&capsule, &required) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_calculate_acs_empty_capsule() {
        let capsule: HashSet<String> = HashSet::new();
        let required: HashSet<String> = ["a".to_string()].into_iter().collect();
        assert!((calculate_acs(&capsule, &required) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_first_correct_position_found() {
        let items = vec!["foo", "bar", "baz"];
        let expected: HashSet<String> = ["bar".to_string()].into_iter().collect();
        assert_eq!(first_correct_position(&items, &expected, |s| *s), 2);
    }

    #[test]
    fn test_first_correct_position_first() {
        let items = vec!["foo", "bar", "baz"];
        let expected: HashSet<String> = ["foo".to_string()].into_iter().collect();
        assert_eq!(first_correct_position(&items, &expected, |s| *s), 1);
    }

    #[test]
    fn test_first_correct_position_not_found() {
        let items = vec!["foo", "bar", "baz"];
        let expected: HashSet<String> = ["qux".to_string()].into_iter().collect();
        assert_eq!(first_correct_position(&items, &expected, |s| *s), 0);
    }

    #[test]
    fn test_context_metrics() {
        let metrics = ContextMetrics::new(1000);
        assert_eq!(metrics.capsule_token_count, 1000);
        assert_eq!(metrics.system_prompt_tokens, 500);
        assert_eq!(metrics.annotation_tokens, 200);
        assert_eq!(metrics.total_context_tokens, 1700);
    }
}
