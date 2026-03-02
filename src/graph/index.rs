use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use rayon::prelude::*;
use rusqlite::Connection;
use tree_sitter::{Language, Parser};

use crate::db::queries;
use crate::graph::GraphState;
use crate::graph::types::{Confidence, EdgeKind, EdgeWeight, NodeId, NodeKind, NodeWeight};

/// Extracted symbol from a single parse.
#[derive(Debug, Clone)]
pub struct ExtractedSymbol {
    pub id: NodeId,
    pub kind: NodeKind,
    pub name: String,
    pub file_path: String,
    pub line_start: u32,
    pub line_end: u32,
    pub signature: String,
    pub signature_hash: String,
    pub docstring: Option<String>,
    pub skeleton: String,
    pub checksum: Vec<u8>,
}

/// Extracted edge from a single parse.
#[derive(Debug, Clone)]
pub struct ExtractedEdge {
    pub from_id: NodeId,
    pub to_name: String,
    pub kind: EdgeKind,
    pub confidence: Confidence,
}

/// Result of parsing a single file.
pub struct FileParseResult {
    pub file_path: String,
    pub symbols: Vec<ExtractedSymbol>,
    pub edges: Vec<ExtractedEdge>,
    pub raw_token_estimate: u32,
}

/// Detect language from file extension.
pub fn detect_language(path: &Path) -> Option<(&'static str, Language)> {
    let ext = path.extension()?.to_str()?;
    match ext {
        "rs" => Some(("rust", tree_sitter_rust::LANGUAGE.into())),
        "py" | "pyi" => Some(("python", tree_sitter_python::LANGUAGE.into())),
        "ts" => Some((
            "typescript",
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        )),
        "tsx" => Some(("tsx", tree_sitter_typescript::LANGUAGE_TSX.into())),
        "js" | "mjs" | "cjs" => Some(("javascript", tree_sitter_javascript::LANGUAGE.into())),
        "jsx" => Some(("jsx", tree_sitter_javascript::LANGUAGE.into())),
        "go" => Some(("go", tree_sitter_go::LANGUAGE.into())),
        "java" => Some(("java", tree_sitter_java::LANGUAGE.into())),
        "cs" => Some(("csharp", tree_sitter_c_sharp::LANGUAGE.into())),
        "c" | "h" => Some(("c", tree_sitter_c::LANGUAGE.into())),
        "cpp" | "cc" | "cxx" | "hpp" | "hxx" => Some(("cpp", tree_sitter_cpp::LANGUAGE.into())),
        "rb" => Some(("ruby", tree_sitter_ruby::LANGUAGE.into())),
        "sh" | "bash" => Some(("bash", tree_sitter_bash::LANGUAGE.into())),
        // kotlin not yet supported (tree-sitter-kotlin crate on old API)
        "php" => Some(("php", tree_sitter_php::LANGUAGE_PHP.into())),
        "swift" => Some(("swift", tree_sitter_swift::LANGUAGE.into())),
        _ => None,
    }
}

/// Compute signature hash: first 8 hex chars of MD5 over whitespace-normalized signature.
pub fn signature_hash(sig: &str) -> String {
    let normalized: String = sig.split_whitespace().collect::<Vec<_>>().join(" ");
    let digest = md5::compute(normalized.as_bytes());
    format!("{digest:x}")[..8].to_string()
}

/// Split camelCase/snake_case for FTS5 indexing.
/// `getUserById` → `getUserById get User By Id`
#[allow(dead_code)]
pub fn fts_split(name: &str) -> String {
    use heck::ToSnakeCase;
    let snake = name.to_snake_case();
    let parts: Vec<&str> = snake.split('_').filter(|s| !s.is_empty()).collect();
    if parts.len() <= 1 {
        return name.to_string();
    }
    format!("{} {}", name, parts.join(" "))
}

/// Parse a single file and extract symbols and edges.
pub fn parse_file(path: &Path, content: &str) -> Option<FileParseResult> {
    let (lang_name, language) = detect_language(path)?;

    let mut parser = Parser::new();
    parser.set_language(&language).ok()?;
    let tree = parser.parse(content, None)?;
    let root = tree.root_node();

    let file_path = path.to_string_lossy().to_string();
    let raw_token_estimate = (content.len() / 4) as u32;

    let mut symbols = Vec::new();
    let mut edges = Vec::new();

    extract_symbols_recursive(
        root,
        content.as_bytes(),
        &file_path,
        lang_name,
        &mut symbols,
        &mut edges,
    );

    Some(FileParseResult {
        file_path,
        symbols,
        edges,
        raw_token_estimate,
    })
}

fn extract_symbols_recursive(
    node: tree_sitter::Node,
    source: &[u8],
    file_path: &str,
    lang: &str,
    symbols: &mut Vec<ExtractedSymbol>,
    edges: &mut Vec<ExtractedEdge>,
) {
    if let Some((kind, name)) = classify_node(node, source, lang) {
        let line_start = node.start_position().row as u32 + 1;
        let line_end = node.end_position().row as u32 + 1;

        let sig = extract_signature(node, source);
        let sig_hash = signature_hash(&sig);
        let docstring = extract_docstring(node, source, lang);
        let skeleton = match &docstring {
            Some(doc) => format!("{sig}\n{doc}"),
            None => sig.clone(),
        };

        let body_bytes = node.utf8_text(source).unwrap_or("");
        let checksum = md5::compute(body_bytes).0.to_vec();

        let id = NodeId::compute(file_path, &name, &sig);

        symbols.push(ExtractedSymbol {
            id: id.clone(),
            kind,
            name: name.clone(),
            file_path: file_path.to_string(),
            line_start,
            line_end,
            signature: sig,
            signature_hash: sig_hash,
            docstring,
            skeleton,
            checksum,
        });

        extract_call_edges(node, source, &id, &name, edges);
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        extract_symbols_recursive(child, source, file_path, lang, symbols, edges);
    }
}

fn classify_node(node: tree_sitter::Node, source: &[u8], lang: &str) -> Option<(NodeKind, String)> {
    let kind_str = node.kind();

    let (node_kind, name_field) = match lang {
        "rust" => match kind_str {
            "function_item" => (NodeKind::Function, "name"),
            "struct_item" => (NodeKind::Class, "name"),
            "enum_item" => (NodeKind::Enum, "name"),
            "trait_item" => (NodeKind::Interface, "name"),
            "impl_item" => return classify_rust_impl(node, source),
            "type_item" => (NodeKind::Type, "name"),
            "mod_item" => (NodeKind::Module, "name"),
            _ => return None,
        },
        "python" => match kind_str {
            "function_definition" => (NodeKind::Function, "name"),
            "class_definition" => (NodeKind::Class, "name"),
            "decorated_definition" => {
                return classify_python_decorated(node, source);
            }
            _ => return None,
        },
        "typescript" | "tsx" | "javascript" | "jsx" => match kind_str {
            "function_declaration" => (NodeKind::Function, "name"),
            "method_definition" => (NodeKind::Method, "name"),
            "class_declaration" => (NodeKind::Class, "name"),
            "interface_declaration" => (NodeKind::Interface, "name"),
            "type_alias_declaration" => (NodeKind::Type, "name"),
            "enum_declaration" => (NodeKind::Enum, "name"),
            _ => return None,
        },
        "go" => match kind_str {
            "function_declaration" => (NodeKind::Function, "name"),
            "method_declaration" => (NodeKind::Method, "name"),
            "type_spec" => return classify_go_type_spec(node, source),
            _ => return None,
        },
        "java" => match kind_str {
            "method_declaration" => (NodeKind::Method, "name"),
            "class_declaration" => (NodeKind::Class, "name"),
            "interface_declaration" => (NodeKind::Interface, "name"),
            "enum_declaration" => (NodeKind::Enum, "name"),
            _ => return None,
        },
        "csharp" => match kind_str {
            "method_declaration" => (NodeKind::Method, "name"),
            "class_declaration" => (NodeKind::Class, "name"),
            "interface_declaration" => (NodeKind::Interface, "name"),
            "enum_declaration" => (NodeKind::Enum, "name"),
            "struct_declaration" => (NodeKind::Class, "name"),
            _ => return None,
        },
        "c" => match kind_str {
            "function_definition" => (NodeKind::Function, "declarator"),
            "struct_specifier" => (NodeKind::Class, "name"),
            "enum_specifier" => (NodeKind::Enum, "name"),
            "type_definition" => (NodeKind::Type, "declarator"),
            _ => return None,
        },
        "cpp" => match kind_str {
            "function_definition" => (NodeKind::Function, "declarator"),
            "class_specifier" => (NodeKind::Class, "name"),
            "struct_specifier" => (NodeKind::Class, "name"),
            "enum_specifier" => (NodeKind::Enum, "name"),
            "template_declaration" => {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if let Some(result) = classify_node(child, source, lang) {
                        return Some(result);
                    }
                }
                return None;
            }
            _ => return None,
        },
        "ruby" => match kind_str {
            "method" => (NodeKind::Method, "name"),
            "singleton_method" => (NodeKind::Method, "name"),
            "class" => (NodeKind::Class, "name"),
            "module" => (NodeKind::Module, "name"),
            _ => return None,
        },
        "bash" => match kind_str {
            "function_definition" => (NodeKind::Function, "name"),
            _ => return None,
        },
        // kotlin parser not yet compatible
        "php" => match kind_str {
            "function_definition" => (NodeKind::Function, "name"),
            "method_declaration" => (NodeKind::Method, "name"),
            "class_declaration" => (NodeKind::Class, "name"),
            "interface_declaration" => (NodeKind::Interface, "name"),
            "trait_declaration" => (NodeKind::Interface, "name"),
            _ => return None,
        },
        "swift" => match kind_str {
            "function_declaration" => (NodeKind::Function, "name"),
            "class_declaration" => (NodeKind::Class, "name"),
            "protocol_declaration" => (NodeKind::Interface, "name"),
            "struct_declaration" => (NodeKind::Class, "name"),
            "enum_declaration" => (NodeKind::Enum, "name"),
            _ => return None,
        },
        _ => return None,
    };

    let name = node
        .child_by_field_name(name_field)
        .and_then(|n| n.utf8_text(source).ok())
        .map(|s| s.to_string())?;

    Some((node_kind, name))
}

fn classify_rust_impl(node: tree_sitter::Node, source: &[u8]) -> Option<(NodeKind, String)> {
    let type_node = node.child_by_field_name("type")?;
    let name = type_node.utf8_text(source).ok()?.to_string();
    Some((NodeKind::Class, format!("impl {name}")))
}

fn classify_python_decorated(node: tree_sitter::Node, source: &[u8]) -> Option<(NodeKind, String)> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "function_definition" => {
                let name = child.child_by_field_name("name")?.utf8_text(source).ok()?;
                return Some((NodeKind::Function, name.to_string()));
            }
            "class_definition" => {
                let name = child.child_by_field_name("name")?.utf8_text(source).ok()?;
                return Some((NodeKind::Class, name.to_string()));
            }
            _ => {}
        }
    }
    None
}

fn classify_go_type_spec(node: tree_sitter::Node, source: &[u8]) -> Option<(NodeKind, String)> {
    let name = node
        .child_by_field_name("name")?
        .utf8_text(source)
        .ok()?
        .to_string();
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        match child.kind() {
            "struct_type" => return Some((NodeKind::Class, name)),
            "interface_type" => return Some((NodeKind::Interface, name)),
            _ => {}
        }
    }
    Some((NodeKind::Type, name))
}

fn extract_signature(node: tree_sitter::Node, source: &[u8]) -> String {
    if let Some(body) = node.child_by_field_name("body") {
        let sig_end = body.start_byte();
        let sig_start = node.start_byte();
        if sig_end > sig_start {
            let text = &source[sig_start..sig_end];
            return String::from_utf8_lossy(text).trim_end().to_string();
        }
    }
    // Fallback: first line
    let text = node.utf8_text(source).unwrap_or("");
    text.lines().next().unwrap_or("").to_string()
}

fn extract_docstring(node: tree_sitter::Node, source: &[u8], lang: &str) -> Option<String> {
    match lang {
        "rust" => extract_rust_docstring(node, source),
        "python" => extract_python_docstring(node, source),
        "java" | "csharp" | "php" | "kotlin" | "swift" => {
            extract_block_comment_docstring(node, source)
        }
        "typescript" | "tsx" | "javascript" | "jsx" | "go" | "c" | "cpp" => {
            extract_preceding_comment(node, source)
        }
        "ruby" => extract_ruby_docstring(node, source),
        _ => None,
    }
}

fn extract_rust_docstring(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut doc_lines = Vec::new();
    let mut prev = node.prev_sibling();
    while let Some(sibling) = prev {
        if sibling.kind() == "line_comment" {
            let text = sibling.utf8_text(source).ok()?;
            if text.starts_with("///") || text.starts_with("//!") {
                doc_lines.push(text.trim_start_matches('/').trim_start_matches('!').trim());
                prev = sibling.prev_sibling();
                continue;
            }
        }
        break;
    }
    if doc_lines.is_empty() {
        return None;
    }
    doc_lines.reverse();
    Some(doc_lines.join("\n"))
}

fn extract_python_docstring(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let body = node.child_by_field_name("body")?;
    let first = body.child(0)?;
    if first.kind() == "expression_statement" {
        let string_node = first.child(0)?;
        if string_node.kind() == "string" || string_node.kind() == "concatenated_string" {
            let text = string_node.utf8_text(source).ok()?;
            let trimmed = text
                .trim_start_matches("\"\"\"")
                .trim_start_matches("'''")
                .trim_end_matches("\"\"\"")
                .trim_end_matches("'''")
                .trim();
            return Some(trimmed.to_string());
        }
    }
    None
}

fn extract_block_comment_docstring(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let prev = node.prev_sibling()?;
    if prev.kind() == "comment" || prev.kind() == "block_comment" {
        let text = prev.utf8_text(source).ok()?;
        if text.starts_with("/**") {
            let cleaned = text
                .trim_start_matches("/**")
                .trim_end_matches("*/")
                .lines()
                .map(|l| l.trim().trim_start_matches('*').trim())
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

fn extract_preceding_comment(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut doc_lines = Vec::new();
    let mut prev = node.prev_sibling();
    while let Some(sibling) = prev {
        if sibling.kind() == "comment" {
            let text = sibling.utf8_text(source).ok()?;
            if text.starts_with("/**") {
                let cleaned = text
                    .trim_start_matches("/**")
                    .trim_end_matches("*/")
                    .lines()
                    .map(|l| l.trim().trim_start_matches('*').trim())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();
                if !cleaned.is_empty() {
                    doc_lines.push(cleaned);
                }
            } else {
                let cleaned = text.trim_start_matches("//").trim();
                doc_lines.push(cleaned.to_string());
            }
            prev = sibling.prev_sibling();
            continue;
        }
        break;
    }
    if doc_lines.is_empty() {
        return None;
    }
    doc_lines.reverse();
    Some(doc_lines.join("\n"))
}

fn extract_ruby_docstring(node: tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut doc_lines = Vec::new();
    let mut prev = node.prev_sibling();
    while let Some(sibling) = prev {
        if sibling.kind() == "comment" {
            let text = sibling.utf8_text(source).ok()?;
            doc_lines.push(text.trim_start_matches('#').trim().to_string());
            prev = sibling.prev_sibling();
            continue;
        }
        break;
    }
    if doc_lines.is_empty() {
        return None;
    }
    doc_lines.reverse();
    Some(doc_lines.join("\n"))
}

fn extract_call_edges(
    node: tree_sitter::Node,
    source: &[u8],
    from_id: &NodeId,
    _from_name: &str,
    edges: &mut Vec<ExtractedEdge>,
) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "call_expression" || child.kind() == "call" {
            if let Some(func) = child.child_by_field_name("function") {
                if let Ok(callee_name) = func.utf8_text(source) {
                    let name = callee_name.split('.').next_back().unwrap_or(callee_name);
                    edges.push(ExtractedEdge {
                        from_id: from_id.clone(),
                        to_name: name.to_string(),
                        kind: EdgeKind::Calls,
                        confidence: Confidence::Heuristic,
                    });
                }
            }
        }
        extract_call_edges(child, source, from_id, _from_name, edges);
    }
}

/// Bulk index all source files from the given paths.
/// Uses rayon for parallelism (tree-sitter Parser is not Send, so we create
/// thread-local instances).
pub fn bulk_index(
    conn: &Connection,
    graph: &mut GraphState,
    source_paths: &[PathBuf],
) -> Result<IndexStats, Box<dyn std::error::Error>> {
    let results: Vec<FileParseResult> = source_paths
        .par_iter()
        .filter_map(|path| {
            let content = std::fs::read_to_string(path).ok()?;
            let rel_path = path.clone();
            parse_file(&rel_path, &content)
        })
        .collect();

    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let mut total_symbols = 0u64;
    let mut total_edges = 0u64;
    let mut total_files = 0u64;
    let mut name_to_id: HashMap<String, NodeId> = HashMap::new();

    let tx = conn.unchecked_transaction()?;

    for result in &results {
        queries::upsert_file(
            &tx,
            &result.file_path,
            "code",
            result.raw_token_estimate,
            now,
        )?;
        total_files += 1;

        for sym in &result.symbols {
            queries::upsert_node(
                &tx,
                &sym.id.0,
                sym.kind,
                &sym.name,
                &sym.file_path,
                sym.line_start,
                sym.line_end,
                &sym.signature,
                &sym.signature_hash,
                sym.docstring.as_deref(),
                &sym.skeleton,
                &sym.checksum,
            )?;

            let weight = NodeWeight {
                id: sym.id.clone(),
                kind: sym.kind,
                name: sym.name.clone(),
                file_path: PathBuf::from(&sym.file_path),
                line_start: sym.line_start,
                line_end: sym.line_end,
                signature: sym.signature.clone(),
                signature_hash: sym.signature_hash.clone(),
                docstring: sym.docstring.clone(),
                skeleton: sym.skeleton.clone(),
                centrality: 0.0,
                checksum: sym.checksum.clone(),
            };
            graph.add_node(weight);
            name_to_id.insert(sym.name.clone(), sym.id.clone());
            total_symbols += 1;
        }
    }

    for result in &results {
        for edge in &result.edges {
            if let Some(to_id) = name_to_id.get(&edge.to_name) {
                queries::upsert_edge(
                    &tx,
                    &edge.from_id.0,
                    &to_id.0,
                    edge.kind,
                    1.0,
                    edge.confidence,
                )?;
                graph.add_edge(
                    &edge.from_id,
                    to_id,
                    EdgeWeight {
                        kind: edge.kind,
                        weight: 1.0,
                        confidence: edge.confidence,
                    },
                );
                total_edges += 1;
            }
        }
    }

    tx.commit()?;

    graph.rebuild_reverse_index();
    graph.compute_pagerank(0.85, 30);
    graph.save_centrality(conn)?;

    Ok(IndexStats {
        files_indexed: total_files,
        symbols_extracted: total_symbols,
        edges_created: total_edges,
    })
}

#[derive(Debug)]
pub struct IndexStats {
    pub files_indexed: u64,
    pub symbols_extracted: u64,
    pub edges_created: u64,
}

/// Result of the prep phase (Phase 1) — computed without holding the graph lock.
pub struct ReindexPrep {
    pub file_path: String,
    pub parse_result: Option<FileParseResult>,
    pub old_node_ids: Vec<String>,
    #[allow(dead_code)]
    pub name_to_new_id: HashMap<String, NodeId>,
    pub similarity_matches: Vec<super::similarity::SimilarityMatch>,
}

/// Incremental reindex: 13-step flow with split-phase concurrency.
///
/// Phase 1 (Prep — no lock): re-parse, build new structures, diff, similarity.
/// Phase 2 (Swap — write lock): commit SQLite + graph mutations.
/// Phase 3 (Deferred): PageRank recomputed once per batch, not per file.
pub fn incremental_reindex_prep(
    conn: &Connection,
    graph: &super::GraphState,
    file_path: &str,
) -> Result<ReindexPrep, Box<dyn std::error::Error + Send + Sync>> {
    // Step 1: Collect old NodeIds for this file
    let old_node_ids = queries::get_node_ids_for_file(conn, file_path)?;

    // Step 2: Re-parse with tree-sitter
    let content = std::fs::read_to_string(file_path).ok();
    let parse_result = content
        .as_deref()
        .and_then(|c| parse_file(std::path::Path::new(file_path), c));

    // Step 3: Build name→NodeId map from new parse
    let mut name_to_new_id = HashMap::new();
    if let Some(ref pr) = parse_result {
        for sym in &pr.symbols {
            name_to_new_id.insert(sym.name.clone(), sym.id.clone());
        }
    }

    // Step 4: Diff old vs new NodeId sets
    let new_ids: std::collections::HashSet<String> = if let Some(ref pr) = parse_result {
        pr.symbols.iter().map(|s| s.id.0.clone()).collect()
    } else {
        std::collections::HashSet::new()
    };
    let old_set: std::collections::HashSet<String> = old_node_ids.iter().cloned().collect();

    let orphaned_ids: Vec<String> = old_set.difference(&new_ids).cloned().collect();

    // Step 5: Run similarity on orphans
    let similarity_matches = if !orphaned_ids.is_empty() {
        if let Some(ref pr) = parse_result {
            let orphan_weights: Vec<&super::types::NodeWeight> = orphaned_ids
                .iter()
                .filter_map(|id| graph.get_weight(&super::types::NodeId(id.clone())))
                .collect();

            let new_candidates_for_match: Vec<&ExtractedSymbol> = pr
                .symbols
                .iter()
                .filter(|s| !old_set.contains(&s.id.0))
                .collect();

            if !orphan_weights.is_empty() && !new_candidates_for_match.is_empty() {
                use std::collections::HashSet;
                let candidates_owned: Vec<ExtractedSymbol> =
                    new_candidates_for_match.into_iter().cloned().collect();
                super::similarity::find_matches(
                    &orphan_weights,
                    &candidates_owned,
                    &|id| {
                        let mut set = HashSet::new();
                        for callee in graph.callees_of(id) {
                            set.insert(callee.id.clone());
                        }
                        set
                    },
                    &|_id| HashSet::new(),
                    true,
                )
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        }
    } else {
        Vec::new()
    };

    Ok(ReindexPrep {
        file_path: file_path.to_string(),
        parse_result,
        old_node_ids,
        name_to_new_id,
        similarity_matches,
    })
}

/// Phase 2 (Swap): commit changes under write lock. Target: ~5-15ms.
/// SQLite-before-graph ordering invariant: DB writes first, then graph mutations.
pub fn incremental_reindex_swap(
    conn: &Connection,
    graph: &mut super::GraphState,
    prep: ReindexPrep,
) -> Result<IncrementalStats, Box<dyn std::error::Error + Send + Sync>> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;

    let tx = conn.unchecked_transaction()?;

    // Step 6: Migrate annotations for similarity matches
    for m in &prep.similarity_matches {
        let _ = tx.execute(
            "UPDATE annotations SET anchor_value = ?1, stale = FALSE, updated_at = ?3
             WHERE anchor_type = 'node' AND anchor_value = ?2",
            rusqlite::params![m.new_id.0, m.old_id.0, now],
        );
    }

    // Step 6b: Mark annotations stale for orphaned nodes (not matched by similarity)
    let migrated_old_ids: std::collections::HashSet<&str> = prep
        .similarity_matches
        .iter()
        .map(|m| m.old_id.0.as_str())
        .collect();

    for old_id in &prep.old_node_ids {
        if !migrated_old_ids.contains(old_id.as_str()) {
            let _ = queries::mark_annotations_stale_for_node(&tx, old_id);
        }
    }

    // Step 7: Delete old nodes and their edges from DB
    if !prep.old_node_ids.is_empty() {
        queries::delete_edges_for_nodes(&tx, &prep.old_node_ids)?;
        for old_id in &prep.old_node_ids {
            tx.execute("DELETE FROM nodes WHERE id = ?1", rusqlite::params![old_id])?;
        }
    }

    // Step 8: Remove old nodes from graph
    for old_id in &prep.old_node_ids {
        graph.remove_edges_from(&super::types::NodeId(old_id.clone()));
        graph.remove_node(&super::types::NodeId(old_id.clone()));
    }

    let mut symbols_added = 0u64;
    let mut edges_added = 0u64;
    let mut name_to_id: HashMap<String, NodeId> = HashMap::new();

    // Step 9-11: Insert new nodes and edges
    if let Some(ref pr) = prep.parse_result {
        queries::upsert_file(&tx, &prep.file_path, "code", pr.raw_token_estimate, now)?;

        for sym in &pr.symbols {
            queries::upsert_node(
                &tx,
                &sym.id.0,
                sym.kind,
                &sym.name,
                &sym.file_path,
                sym.line_start,
                sym.line_end,
                &sym.signature,
                &sym.signature_hash,
                sym.docstring.as_deref(),
                &sym.skeleton,
                &sym.checksum,
            )?;

            let weight = super::types::NodeWeight {
                id: sym.id.clone(),
                kind: sym.kind,
                name: sym.name.clone(),
                file_path: PathBuf::from(&sym.file_path),
                line_start: sym.line_start,
                line_end: sym.line_end,
                signature: sym.signature.clone(),
                signature_hash: sym.signature_hash.clone(),
                docstring: sym.docstring.clone(),
                skeleton: sym.skeleton.clone(),
                centrality: 0.0,
                checksum: sym.checksum.clone(),
            };
            graph.add_node(weight);
            name_to_id.insert(sym.name.clone(), sym.id.clone());
            symbols_added += 1;
        }

        // Also include existing name→id mappings from the graph for cross-file edge resolution
        for idx in graph.graph.node_indices() {
            if let Some(w) = graph.graph.node_weight(idx) {
                name_to_id
                    .entry(w.name.clone())
                    .or_insert_with(|| w.id.clone());
            }
        }

        for edge in &pr.edges {
            let to_id = if let Some(id) = name_to_id.get(&edge.to_name) {
                id.clone()
            } else if edge.confidence == Confidence::Heuristic
                || edge.confidence == Confidence::Speculative
            {
                // Create phantom node for unresolved callees (design §4.6)
                let phantom_id = NodeId::compute("<unresolved>", &edge.to_name, &edge.to_name);
                if name_to_id.contains_key(&edge.to_name) {
                    name_to_id[&edge.to_name].clone()
                } else {
                    let phantom_weight = super::types::NodeWeight {
                        id: phantom_id.clone(),
                        kind: super::types::NodeKind::Function,
                        name: edge.to_name.clone(),
                        file_path: PathBuf::from("<unresolved>"),
                        line_start: 0,
                        line_end: 0,
                        signature: format!("[UNRESOLVED] {}", edge.to_name),
                        signature_hash: String::new(),
                        docstring: None,
                        skeleton: format!("[UNRESOLVED] {}", edge.to_name),
                        centrality: 0.0,
                        checksum: Vec::new(),
                    };
                    graph.add_node(phantom_weight);
                    name_to_id.insert(edge.to_name.clone(), phantom_id.clone());
                    phantom_id
                }
            } else {
                continue;
            };

            queries::upsert_edge(
                &tx,
                &edge.from_id.0,
                &to_id.0,
                edge.kind,
                1.0,
                edge.confidence,
            )?;
            graph.add_edge(
                &edge.from_id,
                &to_id,
                super::types::EdgeWeight {
                    kind: edge.kind,
                    weight: 1.0,
                    confidence: edge.confidence,
                },
            );
            edges_added += 1;
        }
    }

    tx.commit()?;

    // Step 12: Update reverse index
    graph.rebuild_reverse_index();

    Ok(IncrementalStats {
        nodes_removed: prep.old_node_ids.len() as u64,
        nodes_added: symbols_added,
        edges_added,
        annotations_migrated: prep.similarity_matches.len() as u64,
    })
}

/// Collect cross-file source files affected by changes in the given file.
/// Uses the reverse index to find files whose nodes reference nodes in the changed file.
pub fn cross_file_affected(graph: &super::GraphState, changed_file: &str) -> Vec<PathBuf> {
    let changed_path = PathBuf::from(changed_file);
    let mut affected = std::collections::HashSet::new();

    for idx in graph.graph.node_indices() {
        if let Some(w) = graph.graph.node_weight(idx) {
            if w.file_path == changed_path {
                if let Some(paths) = graph.reverse_index.get(&w.id) {
                    for p in paths {
                        if p != &changed_path {
                            affected.insert(p.clone());
                        }
                    }
                }
            }
        }
    }

    affected.into_iter().collect()
}

/// WAL checkpoint for idle periods.
pub fn wal_checkpoint(conn: &Connection) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
    Ok(())
}

#[derive(Debug)]
pub struct IncrementalStats {
    pub nodes_removed: u64,
    pub nodes_added: u64,
    pub edges_added: u64,
    pub annotations_migrated: u64,
}

/// Collect all indexable source files under a root directory using the `ignore` crate
/// (respects .gitignore).
pub fn collect_source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(true)
        .build();

    for entry in walker.flatten() {
        if entry.file_type().is_some_and(|ft| ft.is_file())
            && detect_language(entry.path()).is_some()
        {
            files.push(entry.into_path());
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_language() {
        assert!(detect_language(Path::new("foo.rs")).is_some());
        assert!(detect_language(Path::new("bar.py")).is_some());
        assert!(detect_language(Path::new("baz.ts")).is_some());
        assert!(detect_language(Path::new("qux.txt")).is_none());
    }

    #[test]
    fn test_signature_hash_deterministic() {
        let a = signature_hash("fn hello(x: i32) -> bool");
        let b = signature_hash("fn hello(x: i32) -> bool");
        assert_eq!(a, b);
        assert_eq!(a.len(), 8);
    }

    #[test]
    fn test_signature_hash_whitespace_normalized() {
        let a = signature_hash("fn  hello( x:  i32 )");
        let b = signature_hash("fn hello( x: i32 )");
        assert_eq!(a, b);
    }

    #[test]
    fn test_fts_split() {
        let result = fts_split("getUserById");
        assert!(result.contains("getUserById"));
        assert!(result.contains("get"));
    }

    #[test]
    fn test_parse_rust_file() {
        let src = r#"
/// Add two numbers
fn add(a: i32, b: i32) -> i32 {
    a + b
}

struct Point {
    x: f64,
    y: f64,
}
"#;
        let result = parse_file(Path::new("test.rs"), src).unwrap();
        assert!(!result.symbols.is_empty());
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"add"), "Expected 'add', found: {names:?}");
        assert!(
            names.contains(&"Point"),
            "Expected 'Point', found: {names:?}"
        );
    }

    #[test]
    fn test_parse_python_file() {
        let src = r#"
def hello(name):
    """Say hello to someone."""
    print(f"Hello, {name}!")

class Greeter:
    def greet(self, name):
        pass
"#;
        let result = parse_file(Path::new("test.py"), src).unwrap();
        let names: Vec<&str> = result.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"Greeter"));
    }

    #[test]
    fn test_bulk_index() {
        let tmp = tempfile::tempdir().unwrap();
        let test_file = tmp.path().join("test.rs");
        std::fs::write(&test_file, "fn foo() {}\nfn bar() { foo(); }").unwrap();

        let conn = rusqlite::Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();

        let mut graph = GraphState::new();
        let stats = bulk_index(&conn, &mut graph, &[test_file]).unwrap();
        assert!(stats.files_indexed >= 1);
        assert!(stats.symbols_extracted >= 2);
    }
}
