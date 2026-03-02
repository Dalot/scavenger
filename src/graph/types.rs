use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// Unique identifier for a graph node: `hash(file_path, symbol_name, signature)`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl NodeId {
    pub fn compute(file_path: &str, name: &str, signature: &str) -> Self {
        let digest = md5::compute(format!("{file_path}\0{name}\0{signature}"));
        Self(format!("{digest:x}"))
    }
}

/// The 9 kinds of symbols scavenger tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    Function,
    Method,
    Class,
    Interface,
    Type,
    Enum,
    ExportedVar,
    Module,
    File,
}

impl NodeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Function => "Function",
            Self::Method => "Method",
            Self::Class => "Class",
            Self::Interface => "Interface",
            Self::Type => "Type",
            Self::Enum => "Enum",
            Self::ExportedVar => "ExportedVar",
            Self::Module => "Module",
            Self::File => "File",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Function" => Some(Self::Function),
            "Method" => Some(Self::Method),
            "Class" => Some(Self::Class),
            "Interface" => Some(Self::Interface),
            "Type" => Some(Self::Type),
            "Enum" => Some(Self::Enum),
            "ExportedVar" => Some(Self::ExportedVar),
            "Module" => Some(Self::Module),
            "File" => Some(Self::File),
            _ => None,
        }
    }
}

impl fmt::Display for NodeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The 7 kinds of edges in the dependency graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    Imports,
    Calls,
    TypeRef,
    Extends,
    Implements,
    Exports,
    Contains,
}

impl EdgeKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Imports => "Imports",
            Self::Calls => "Calls",
            Self::TypeRef => "TypeRef",
            Self::Extends => "Extends",
            Self::Implements => "Implements",
            Self::Exports => "Exports",
            Self::Contains => "Contains",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "Imports" => Some(Self::Imports),
            "Calls" => Some(Self::Calls),
            "TypeRef" => Some(Self::TypeRef),
            "Extends" => Some(Self::Extends),
            "Implements" => Some(Self::Implements),
            "Exports" => Some(Self::Exports),
            "Contains" => Some(Self::Contains),
            _ => None,
        }
    }
}

impl fmt::Display for EdgeKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Edge confidence level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Confidence {
    Precise,
    Heuristic,
    Speculative,
}

impl Confidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Precise => "precise",
            Self::Heuristic => "heuristic",
            Self::Speculative => "speculative",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "precise" => Some(Self::Precise),
            "heuristic" => Some(Self::Heuristic),
            "speculative" => Some(Self::Speculative),
            _ => None,
        }
    }
}

impl fmt::Display for Confidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// In-memory representation of a graph node.
#[derive(Debug, Clone)]
pub struct NodeWeight {
    pub id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    pub file_path: PathBuf,
    pub line_start: u32,
    pub line_end: u32,
    pub signature: String,
    #[allow(dead_code)]
    pub signature_hash: String,
    #[allow(dead_code)]
    pub docstring: Option<String>,
    pub skeleton: String,
    pub centrality: f32,
    pub checksum: Vec<u8>,
}

/// In-memory representation of a graph edge.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct EdgeWeight {
    pub kind: EdgeKind,
    pub weight: f32,
    pub confidence: Confidence,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id_compute_deterministic() {
        let a = NodeId::compute("src/main.rs", "main", "fn main()");
        let b = NodeId::compute("src/main.rs", "main", "fn main()");
        assert_eq!(a, b);
    }

    #[test]
    fn test_node_id_differs_on_signature() {
        let a = NodeId::compute("src/lib.rs", "foo", "fn foo(x: i32)");
        let b = NodeId::compute("src/lib.rs", "foo", "fn foo(x: i32, y: i32)");
        assert_ne!(a, b);
    }

    #[test]
    fn test_node_kind_roundtrip() {
        for kind in [
            NodeKind::Function,
            NodeKind::Method,
            NodeKind::Class,
            NodeKind::Interface,
            NodeKind::Type,
            NodeKind::Enum,
            NodeKind::ExportedVar,
            NodeKind::Module,
            NodeKind::File,
        ] {
            assert_eq!(NodeKind::from_str(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn test_edge_kind_roundtrip() {
        for kind in [
            EdgeKind::Imports,
            EdgeKind::Calls,
            EdgeKind::TypeRef,
            EdgeKind::Extends,
            EdgeKind::Implements,
            EdgeKind::Exports,
            EdgeKind::Contains,
        ] {
            assert_eq!(EdgeKind::from_str(kind.as_str()), Some(kind));
        }
    }

    #[test]
    fn test_confidence_roundtrip() {
        for c in [
            Confidence::Precise,
            Confidence::Heuristic,
            Confidence::Speculative,
        ] {
            assert_eq!(Confidence::from_str(c.as_str()), Some(c));
        }
    }
}
