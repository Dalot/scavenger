// T063: Unit tests for query engine — intent, search, traversal.

use scavenger::query::intent::{self, Intent};

#[test]
fn test_debug_intent_keywords() {
    for keyword in ["fix", "bug", "error", "crash", "broken"] {
        let result = intent::classify(&format!("please {keyword} this"));
        assert_eq!(result.primary, Intent::Debug, "'{keyword}' should trigger Debug");
    }
}

#[test]
fn test_refactor_intent_keywords() {
    for keyword in ["refactor", "rename", "extract", "restructure"] {
        let result = intent::classify(&format!("{keyword} the module"));
        assert_eq!(result.primary, Intent::Refactor, "'{keyword}' should trigger Refactor");
    }
}

#[test]
fn test_understand_intent_keywords() {
    for keyword in ["explain", "how does", "what does", "overview", "describe"] {
        let result = intent::classify(&format!("{keyword} this code"));
        assert_eq!(result.primary, Intent::Understand, "'{keyword}' should trigger Understand");
    }
}

#[test]
fn test_extend_intent_keywords() {
    for keyword in ["add", "implement", "create", "new feature"] {
        let result = intent::classify(&format!("{keyword} a handler"));
        assert_eq!(result.primary, Intent::Extend, "'{keyword}' should trigger Extend");
    }
}

#[test]
fn test_review_intent_keywords() {
    for keyword in ["review", "check", "audit", "inspect"] {
        let result = intent::classify(&format!("{keyword} the code"));
        assert_eq!(result.primary, Intent::Review, "'{keyword}' should trigger Review");
    }
}

#[test]
fn test_empty_query_defaults_to_understand() {
    let result = intent::classify("");
    assert_eq!(result.primary, Intent::Understand);
}

#[test]
fn test_intent_result_has_weights() {
    let result = intent::classify("fix this bug");
    assert!(result.primary_weight > 0.0);
    assert!(result.primary_weight <= 1.0);
}
