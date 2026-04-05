use std::convert::Infallible;
use std::str::FromStr;

/// Controls how much context is included in a capsule response.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DetailLevel {
    Minimal,
    #[default]
    Standard,
    Detailed,
}

impl FromStr for DetailLevel {
    type Err = Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "minimal" | "min" => Ok(Self::Minimal),
            "detailed" | "detail" | "full" => Ok(Self::Detailed),
            _ => {
                tracing::warn!("unknown detail_level '{}', defaulting to standard", s);
                Ok(Self::Standard)
            }
        }
    }
}

impl DetailLevel {
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
            Self::Minimal | Self::Standard => 0,
            Self::Detailed => 3,
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
        max_callers: Option<u32>,
        max_callees: Option<u32>,
        max_annotations: Option<u32>,
        include_body: Option<bool>,
    ) -> Self {
        if let Some(v) = max_callers {
            self.max_callers = v;
        }
        if let Some(v) = max_callees {
            self.max_callees = v;
        }
        if let Some(v) = max_annotations {
            self.max_annotations = v;
        }
        if let Some(v) = include_body {
            self.include_body = v;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_detail_level() {
        assert_eq!(
            "minimal".parse::<DetailLevel>().unwrap(),
            DetailLevel::Minimal
        );
        assert_eq!("min".parse::<DetailLevel>().unwrap(), DetailLevel::Minimal);
        assert_eq!(
            "detailed".parse::<DetailLevel>().unwrap(),
            DetailLevel::Detailed
        );
        assert_eq!(
            "detail".parse::<DetailLevel>().unwrap(),
            DetailLevel::Detailed
        );
        assert_eq!(
            "full".parse::<DetailLevel>().unwrap(),
            DetailLevel::Detailed
        );
        assert_eq!(
            "MINIMAL".parse::<DetailLevel>().unwrap(),
            DetailLevel::Minimal
        );
        assert_eq!(
            "unknown".parse::<DetailLevel>().unwrap(),
            DetailLevel::Standard
        );
        assert_eq!("".parse::<DetailLevel>().unwrap(), DetailLevel::Standard);
    }

    #[test]
    fn test_capsule_constraints_from_detail() {
        let c = CapsuleConstraints::from_detail(DetailLevel::Standard);
        assert_eq!(c.max_callers, 10);
        assert_eq!(c.max_callees, 10);
        assert_eq!(c.max_annotations, 5);
        assert_eq!(c.max_file_annotations, 0);
        assert_eq!(c.max_project_annotations, 0);
        assert_eq!(c.max_doc_chunks, 0);
        assert_eq!(c.max_node_history, 0);
        assert_eq!(c.max_extended_neighbors, 0);
        assert!(!c.include_body);
    }

    #[test]
    fn test_capsule_constraints_detailed() {
        let c = CapsuleConstraints::from_detail(DetailLevel::Detailed);
        assert_eq!(c.max_callers, 20);
        assert_eq!(c.max_annotations, 10);
        assert_eq!(c.max_file_annotations, 3);
        assert_eq!(c.max_project_annotations, 3);
        assert_eq!(c.max_doc_chunks, 3);
        assert_eq!(c.max_node_history, 3);
        assert_eq!(c.max_extended_neighbors, 50);
        assert!(c.include_body);
    }

    #[test]
    fn test_capsule_constraints_minimal() {
        let c = CapsuleConstraints::from_detail(DetailLevel::Minimal);
        assert_eq!(c.max_callers, 5);
        assert_eq!(c.max_callees, 5);
        assert_eq!(c.max_annotations, 0);
        assert_eq!(c.max_extended_neighbors, 0);
        assert!(!c.include_body);
    }
}
