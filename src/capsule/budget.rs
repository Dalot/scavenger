use serde::Deserialize;

/// Controls how much context is included in a capsule response.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub enum DetailLevel {
    Minimal,
    #[default]
    Standard,
    Detailed,
}

impl DetailLevel {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "minimal" | "min" => Self::Minimal,
            "detailed" | "detail" | "full" => Self::Detailed,
            _ => Self::Standard,
        }
    }

    pub fn max_callers(&self) -> u32 {
        match self {
            Self::Minimal => 5,
            Self::Standard => 10,
            Self::Detailed => 20,
        }
    }

    pub fn max_callees(&self) -> u32 {
        match self {
            Self::Minimal => 5,
            Self::Standard => 10,
            Self::Detailed => 20,
        }
    }

    pub fn max_annotations(&self) -> u32 {
        match self {
            Self::Minimal => 0,
            Self::Standard => 5,
            Self::Detailed => 10,
        }
    }

    pub fn max_file_annotations(&self) -> u32 {
        match self {
            Self::Minimal => 0,
            Self::Standard => 3,
            Self::Detailed => 5,
        }
    }

    pub fn max_project_annotations(&self) -> u32 {
        match self {
            Self::Minimal => 0,
            Self::Standard => 0,
            Self::Detailed => 3,
        }
    }

    pub fn max_doc_chunks(&self) -> u32 {
        match self {
            Self::Minimal => 0,
            Self::Standard => 0,
            Self::Detailed => 3,
        }
    }

    pub fn max_node_history(&self) -> u32 {
        match self {
            Self::Minimal => 0,
            Self::Standard => 0,
            Self::Detailed => 3,
        }
    }

    pub fn max_extended_neighbors(&self) -> u32 {
        match self {
            Self::Minimal => 0,
            Self::Standard => 0,
            Self::Detailed => 50,
        }
    }

    pub fn include_body(&self) -> bool {
        match self {
            Self::Minimal | Self::Standard => false,
            Self::Detailed => true,
        }
    }
}

/// Per-source caps derived from a DetailLevel, with optional overrides applied.
#[derive(Debug, Clone)]
pub struct CapsuleConstraints {
    pub detail_level: DetailLevel,
    pub max_callers: u32,
    pub max_callees: u32,
    pub max_annotations: u32,
    pub max_file_annotations: u32,
    pub max_project_annotations: u32,
    pub max_doc_chunks: u32,
    pub max_node_history: u32,
    pub max_extended_neighbors: u32,
    pub include_body: bool,
}

impl CapsuleConstraints {
    pub fn from_detail(level: DetailLevel) -> Self {
        Self {
            detail_level: level,
            max_callers: level.max_callers(),
            max_callees: level.max_callees(),
            max_annotations: level.max_annotations(),
            max_file_annotations: level.max_file_annotations(),
            max_project_annotations: level.max_project_annotations(),
            max_doc_chunks: level.max_doc_chunks(),
            max_node_history: level.max_node_history(),
            max_extended_neighbors: level.max_extended_neighbors(),
            include_body: level.include_body(),
        }
    }

    pub fn with_overrides(
        mut self,
        detail_level: Option<&str>,
        max_callers: Option<u32>,
        max_callees: Option<u32>,
        max_annotations: Option<u32>,
        include_body: Option<bool>,
    ) -> Self {
        let level = detail_level
            .map(DetailLevel::from_str)
            .unwrap_or(self.detail_level);

        self.detail_level = level;
        self.max_callers = max_callers.unwrap_or(level.max_callers());
        self.max_callees = max_callees.unwrap_or(level.max_callees());
        self.max_annotations = max_annotations.unwrap_or(level.max_annotations());
        self.include_body = include_body.unwrap_or(level.include_body());

        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detail_level_from_str_minimal() {
        assert_eq!(DetailLevel::from_str("minimal"), DetailLevel::Minimal);
        assert_eq!(DetailLevel::from_str("min"), DetailLevel::Minimal);
        assert_eq!(DetailLevel::from_str("MINIMAL"), DetailLevel::Minimal);
    }

    #[test]
    fn test_detail_level_from_str_detailed() {
        assert_eq!(DetailLevel::from_str("detailed"), DetailLevel::Detailed);
        assert_eq!(DetailLevel::from_str("detail"), DetailLevel::Detailed);
        assert_eq!(DetailLevel::from_str("full"), DetailLevel::Detailed);
        assert_eq!(DetailLevel::from_str("DETAILED"), DetailLevel::Detailed);
    }

    #[test]
    fn test_detail_level_from_str_defaults_to_standard() {
        assert_eq!(DetailLevel::from_str("standard"), DetailLevel::Standard);
        assert_eq!(DetailLevel::from_str("unknown"), DetailLevel::Standard);
        assert_eq!(DetailLevel::from_str(""), DetailLevel::Standard);
    }

    #[test]
    fn test_detail_level_minimal_caps() {
        let level = DetailLevel::Minimal;
        assert_eq!(level.max_callers(), 5);
        assert_eq!(level.max_callees(), 5);
        assert_eq!(level.max_annotations(), 0);
        assert_eq!(level.max_file_annotations(), 0);
        assert_eq!(level.max_project_annotations(), 0);
        assert_eq!(level.max_doc_chunks(), 0);
        assert_eq!(level.max_node_history(), 0);
        assert_eq!(level.max_extended_neighbors(), 0);
        assert!(!level.include_body());
    }

    #[test]
    fn test_detail_level_standard_caps() {
        let level = DetailLevel::Standard;
        assert_eq!(level.max_callers(), 10);
        assert_eq!(level.max_callees(), 10);
        assert_eq!(level.max_annotations(), 5);
        assert_eq!(level.max_file_annotations(), 3);
        assert_eq!(level.max_project_annotations(), 0);
        assert_eq!(level.max_doc_chunks(), 0);
        assert_eq!(level.max_node_history(), 0);
        assert_eq!(level.max_extended_neighbors(), 0);
        assert!(!level.include_body());
    }

    #[test]
    fn test_detail_level_detailed_caps() {
        let level = DetailLevel::Detailed;
        assert_eq!(level.max_callers(), 20);
        assert_eq!(level.max_callees(), 20);
        assert_eq!(level.max_annotations(), 10);
        assert_eq!(level.max_file_annotations(), 5);
        assert_eq!(level.max_project_annotations(), 3);
        assert_eq!(level.max_doc_chunks(), 3);
        assert_eq!(level.max_node_history(), 3);
        assert_eq!(level.max_extended_neighbors(), 50);
        assert!(level.include_body());
    }

    #[test]
    fn test_capsule_constraints_from_detail() {
        let constraints = CapsuleConstraints::from_detail(DetailLevel::Minimal);
        assert_eq!(constraints.detail_level, DetailLevel::Minimal);
        assert_eq!(constraints.max_callers, 5);
        assert_eq!(constraints.max_annotations, 0);
        assert!(!constraints.include_body);
    }

    #[test]
    fn test_capsule_constraints_with_overrides() {
        let base = CapsuleConstraints::from_detail(DetailLevel::Standard);
        let constraints = base.with_overrides(Some("minimal"), Some(2), None, Some(10), Some(true));

        assert_eq!(constraints.detail_level, DetailLevel::Minimal);
        assert_eq!(constraints.max_callers, 2);
        assert_eq!(constraints.max_callees, 5);
        assert_eq!(constraints.max_annotations, 10);
        assert!(constraints.include_body);
    }

    #[test]
    fn test_capsule_constraints_no_overrides_uses_level_defaults() {
        let base = CapsuleConstraints::from_detail(DetailLevel::Detailed);
        let constraints = base.with_overrides(None, None, None, None, None);

        assert_eq!(constraints.detail_level, DetailLevel::Detailed);
        assert_eq!(constraints.max_callers, 20);
        assert_eq!(constraints.max_callees, 20);
        assert_eq!(constraints.max_annotations, 10);
        assert!(constraints.include_body);
    }
}
