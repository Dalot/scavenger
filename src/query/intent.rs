use strsim::jaro_winkler;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Intent {
    Debug,
    Refactor,
    Understand,
    Extend,
    Review,
}

impl Intent {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Refactor => "refactor",
            Self::Understand => "understand",
            Self::Extend => "extend",
            Self::Review => "review",
        }
    }
}

impl std::fmt::Display for Intent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

const DEBUG_KEYWORDS: &[&str] = &[
    "error", "bug", "fix", "crash", "failing", "broken", "traceback", "panic", "why is",
];
const REFACTOR_KEYWORDS: &[&str] = &[
    "refactor", "clean up", "simplify", "extract", "rename", "restructure", "move", "split",
];
const UNDERSTAND_KEYWORDS: &[&str] = &[
    "explain", "what does", "how does", "walk me through", "overview", "describe", "where is",
];
const EXTEND_KEYWORDS: &[&str] = &[
    "add", "implement", "create", "new feature", "integrate", "build",
];
const REVIEW_KEYWORDS: &[&str] = &[
    "review", "check", "audit", "inspect", "validate",
];

struct IntentKeywords {
    intent: Intent,
    keywords: &'static [&'static str],
}

const ALL_INTENTS: &[IntentKeywords] = &[
    IntentKeywords { intent: Intent::Debug, keywords: DEBUG_KEYWORDS },
    IntentKeywords { intent: Intent::Refactor, keywords: REFACTOR_KEYWORDS },
    IntentKeywords { intent: Intent::Understand, keywords: UNDERSTAND_KEYWORDS },
    IntentKeywords { intent: Intent::Extend, keywords: EXTEND_KEYWORDS },
    IntentKeywords { intent: Intent::Review, keywords: REVIEW_KEYWORDS },
];

#[derive(Debug, Clone)]
pub struct IntentResult {
    pub primary: Intent,
    pub secondary: Option<Intent>,
    pub primary_weight: f64,
    pub secondary_weight: f64,
}

impl IntentResult {
    pub fn single(intent: Intent) -> Self {
        Self {
            primary: intent,
            secondary: None,
            primary_weight: 1.0,
            secondary_weight: 0.0,
        }
    }

    pub fn multi(primary: Intent, secondary: Intent) -> Self {
        Self {
            primary,
            secondary: Some(secondary),
            primary_weight: 0.6,
            secondary_weight: 0.4,
        }
    }
}

/// Classify the intent from a query string.
///
/// Pipeline: keyword priority → fuzzy match → default Understand.
/// Multi-intent: if top-2 are within 0.1 score, 60/40 weighted union.
pub fn classify(query: &str) -> IntentResult {
    if query.is_empty() {
        return IntentResult::single(Intent::Understand);
    }

    let lower = query.to_lowercase();

    // Phase 1: Exact keyword matching (highest priority)
    let mut keyword_scores: Vec<(Intent, f64)> = Vec::new();
    for ik in ALL_INTENTS {
        let score = keyword_match_score(&lower, ik.keywords);
        if score > 0.0 {
            keyword_scores.push((ik.intent, score));
        }
    }

    if !keyword_scores.is_empty() {
        keyword_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        return build_result(&keyword_scores);
    }

    // Phase 2: Fuzzy matching via Jaro-Winkler
    let mut fuzzy_scores: Vec<(Intent, f64)> = Vec::new();
    for ik in ALL_INTENTS {
        let score = fuzzy_match_score(&lower, ik.keywords);
        if score > 0.7 {
            fuzzy_scores.push((ik.intent, score));
        }
    }

    if !fuzzy_scores.is_empty() {
        fuzzy_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        return build_result(&fuzzy_scores);
    }

    // Phase 3: Default to Understand
    IntentResult::single(Intent::Understand)
}

fn keyword_match_score(query: &str, keywords: &[&str]) -> f64 {
    let mut score = 0.0;
    for kw in keywords {
        if query.contains(kw) {
            score += 1.0;
            if query.starts_with(kw) {
                score += 0.5;
            }
        }
    }
    score
}

fn fuzzy_match_score(query: &str, keywords: &[&str]) -> f64 {
    let words: Vec<&str> = query.split_whitespace().collect();
    let mut best = 0.0f64;
    for kw in keywords {
        for word in &words {
            let sim = jaro_winkler(word, kw);
            best = best.max(sim);
        }
        let full_sim = jaro_winkler(query, kw);
        best = best.max(full_sim);
    }
    best
}

fn build_result(scores: &[(Intent, f64)]) -> IntentResult {
    let top = scores[0];
    if scores.len() > 1 && (top.1 - scores[1].1).abs() < 0.1 && scores[0].0 != scores[1].0 {
        IntentResult::multi(top.0, scores[1].0)
    } else {
        IntentResult::single(top.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_query_defaults_to_understand() {
        let r = classify("");
        assert_eq!(r.primary, Intent::Understand);
    }

    #[test]
    fn test_debug_keywords() {
        assert_eq!(classify("why is this function crashing").primary, Intent::Debug);
        assert_eq!(classify("fix the bug in auth").primary, Intent::Debug);
        assert_eq!(classify("error handling in parser").primary, Intent::Debug);
    }

    #[test]
    fn test_refactor_keywords() {
        assert_eq!(classify("refactor the auth module").primary, Intent::Refactor);
        assert_eq!(classify("simplify this function").primary, Intent::Refactor);
    }

    #[test]
    fn test_understand_keywords() {
        assert_eq!(classify("explain how auth works").primary, Intent::Understand);
        assert_eq!(classify("what does validateToken do").primary, Intent::Understand);
        assert_eq!(classify("where is the config loaded").primary, Intent::Understand);
    }

    #[test]
    fn test_extend_keywords() {
        assert_eq!(classify("add caching to the query engine").primary, Intent::Extend);
        assert_eq!(classify("implement rate limiting").primary, Intent::Extend);
    }

    #[test]
    fn test_review_keywords() {
        assert_eq!(classify("review the database module").primary, Intent::Review);
        assert_eq!(classify("audit security of auth").primary, Intent::Review);
    }

    #[test]
    fn test_no_keyword_defaults_to_understand() {
        let r = classify("getUser function");
        assert_eq!(r.primary, Intent::Understand);
    }

    #[test]
    fn test_multi_intent() {
        // Both keywords in the middle → equal scores → multi-intent
        let r = classify("the code has a bug so rename it");
        assert!(
            r.secondary.is_some(),
            "Expected multi-intent, got single: {:?} (weight: {})",
            r.primary, r.primary_weight
        );
    }
}
