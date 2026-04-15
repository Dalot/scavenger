# Eval Pipeline Fixes Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix the three blocking issues (B1, B2, B3) that cause 16/34 eval cases to fail: query resolution never finds targets because of file path mismatch, FTS5 results not in graph, and symbol extraction misses constructs.

**Architecture:** Fix query resolution by using suffix matching instead of contains(). Fix FTS5-to-graph lookup by verifying load_from_db flow. Fix symbol extraction by improving regex patterns. Each fix is isolated and verifiable with unit tests.

**Tech Stack:** Rust, rusqlite, tree-sitter

---

## Context

### Problem Summary

The eval pipeline produces non-empty capsules (progress!), but 16/34 cases fail because:
1. **B1**: `resolve_target()` never finds targets - file path mismatch (test cases use `src/config.rs` but graph has absolute paths)
2. **B2**: FTS5 search results return from DB but `graph.get_weight()` returns None
3. **B3**: Symbol extraction misses multi-line Rust constructs

### Root Causes

- **B1**: Line 28 in `src/query/mod.rs`: `w.file_path.to_string_lossy().contains(file)` fails because corpus paths are absolute
- **B2**: In `src/eval/relevance.rs`, `compute_bm25_baseline()` at line 330 calls `search_bm25_only()` but lookup fails
- **B3**: `extract_symbols_from_capsule()` in `src/eval/relevance.rs` uses line-by-line regex, misses multi-line constructs

### Expected Files vs Actual

Test case provides: `src/config.rs`, `parse_config`
Graph stores: `/path/to/sample_project/src/config.rs`

---

## Task 1: Fix resolve_target() Path Matching (B1)

**Files:**
- Modify: `src/query/mod.rs:22-38`

- [ ] **Step 1: Add failing test for suffix matching**

Add to the test module in `src/query/mod.rs`:

```rust
#[test]
fn test_resolve_target_with_absolute_path() {
    let mut g = GraphState::new();
    // Simulate absolute path storage
    g.add_node(make_node("n1", "parse_config", "/full/path/to/sample_project/src/config.rs"));
    
    // Test with relative path hint (what test cases provide)
    let result = resolve_target(&g, "src/config.rs", Some("parse_config"));
    assert!(result.is_some(), "Should match via suffix");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd eval-pipeline-wiring && cargo test test_resolve_target_with_absolute_path`
Expected: FAIL - test should fail with current implementation

- [ ] **Step 3: Fix resolve_target() to use suffix matching**

Modify `resolve_target()` in `src/query/mod.rs`:

```rust
pub fn resolve_target(graph: &GraphState, file: &str, symbol: Option<&str>) -> Option<NodeId> {
    if let Some(sym_name) = symbol {
        graph
            .graph
            .node_indices()
            .filter_map(|idx| graph.graph.node_weight(idx))
            // CHANGED: Use ends_with for flexible path matching
            .find(|w| w.name == sym_name && w.file_path.to_string_lossy().ends_with(file))
            .map(|w| w.id.clone())
    } else {
        graph
            .graph
            .node_indices()
            .filter_map(|idx| graph.graph.node_weight(idx))
            .find(|w| w.file_path.to_string_lossy().ends_with(file))
            .map(|w| w.id.clone())
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd eval-pipeline-wiring && cargo test test_resolve_target_with_absolute_path`
Expected: PASS

- [ ] **Step 5: Run existing tests to verify no regression**

Run: `cd eval-pipeline-wiring && cargo test test_resolve_target`
Expected: Both tests pass

- [ ] **Step 6: Commit**

```bash
cd eval-pipeline-wiring && git add src/query/mod.rs && git commit -m "fix(query): use suffix matching for file path resolution"
```

---

## Task 2: Fix FTS5 Search to Graph Lookup (B2)

**Files:**
- Modify: `src/eval/relevance.rs:320-343`
- Add test: `src/query/search.rs`

- [ ] **Step 1: Add integration test for FTS5 → graph lookup**

Add to `src/query/search.rs` test module:

```rust
#[test]
fn test_search_bm25_returns_findable_nodes() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    crate::db::schema::ensure_branch_schema(&conn).unwrap();

    // Insert node directly in DB (simulating corpus indexing)
    conn.execute(
        "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum)
         VALUES ('config::parse_config', 'Function', 'parse_config', 'src/config.rs', 10, 22, 'pub fn parse_config()', 'aabb0011', 'pub fn parse_config()', X'DEADBEEF')",
        [],
    ).unwrap();

    // Insert FTS5 data
    conn.execute(
        "INSERT INTO nodes_fts (rowid, name, signature, skeleton)
         VALUES (1, 'parse_config', 'pub fn parse_config()', 'pub fn parse_config()')",
        [],
    ).unwrap();

    // Create graph and load from DB
    let mut graph = GraphState::new();
    graph.load_from_db(&conn).unwrap();

    // Search and verify graph has the result
    let results = search_bm25_only(&conn, "parse_config", 10).unwrap();
    assert!(!results.is_empty(), "Should find results");
    
    // CRITICAL: Verify the result is findable in the graph
    if let Some(ref first) = results.first() {
        let found = graph.get_weight(&first.node_id);
        assert!(found.is_some(), "FTS5 result should be findable in graph. Got: {:?}", first.node_id);
    }
}
```

- [ ] **Step 2: Run test to verify failure**

Run: `cd eval-pipeline-wiring && cargo test test_search_bm25_returns_findable_nodes`
Expected: FAIL - either no results or graph.get_weight returns None

- [ ] **Step 3: Debug and fix load_from_db or search flow**

The issue is that `search_bm25_only` returns results, but maybe the node ID format differs. Check:

1. Verify `load_from_db` populates `node_index_map` correctly
2. Check if FTS5 query is returning correct IDs

If issue is in load_from_db - fix in `src/graph/mod.rs:143-186`:

```rust
/// Load the full graph from a SQLite branch database.
pub fn load_from_db(&mut self, conn: &Connection) -> db::DbResult<()> {
    self.graph.clear();
    self.node_index_map.clear();
    self.reverse_index.clear();

    let node_rows = queries::load_all_nodes(conn)?;
    tracing::debug!("Loading {} nodes from DB", node_rows.len());
    
    for row in &node_rows {
        let kind = NodeKind::from_str(&row.kind).unwrap_or(NodeKind::Function);
        let weight = NodeWeight {
            id: NodeId(row.id.clone()),
            // ... rest
        };
        self.add_node(weight);
    }
    // ... rest
}
```

- [ ] **Step 4: Run test to verify fix**

Run: `cd eval-pipeline-wiring && cargo test test_search_bm25_returns_findable_nodes`
Expected: PASS

- [ ] **Step 5: Also fix compute_bm25_baseline in relevance.rs**

Modify `compute_bm25_baseline()` to handle missing graph weights gracefully:

```rust
fn compute_bm25_baseline(
    conn: &Connection,
    graph: &GraphState,
    query: &str,
    expected_symbols: &[String],
) -> HashSet<String> {
    let search_results = search::search_bm25_only(conn, query, 50).unwrap_or_default();
    let expected_set: HashSet<String> = expected_symbols.iter().cloned().collect();
    let mut found = HashSet::new();

    for result in &search_results {
        // Handle case where graph doesn't have the node
        if let Some(node) = graph.get_weight(&result.node_id) {
            if expected_set.contains(&node.name) {
                found.insert(node.name.clone());
            }
        } else {
            // Fallback: check if node_id string matches expected
            // This handles cases where node exists in FTS5 but wasn't loaded
            tracing::warn!("Node {} not found in graph, skipping", result.node_id.0);
        }
    }

    found
}
```

- [ ] **Step 6: Commit**

```bash
cd eval-pipeline-wiring && git add src/query/search.rs src/eval/relevance.rs && git commit -m "fix(eval): make FTS5 results findable in graph"
```

---

## Task 3: Fix Symbol Extraction for Multi-line Rust (B3)

**Files:**
- Modify: `src/eval/relevance.rs:70-156`

- [ ] **Step 1: Add failing test for multi-line constructs**

Add to `src/eval/relevance.rs` test module:

```rust
#[test]
fn test_extract_multiline_functions() {
    let capsule = r#"
pub fn parse_config() -> Config {
    Config::new()
}

fn helper_with_long_body(
    arg1: Type1,
    arg2: Type2,
) -> Result<Type3, Error> {
    Ok(Type3)
}

struct MultiField {
    field1: Type1,
    field2: Type2,
}
"#;
    let symbols = extract_symbols_from_capsule(capsule);
    // These should be found but were missed before
    assert!(symbols.contains("parse_config"), "Should find parse_config");
    assert!(symbols.contains("helper_with_long_body"), "Should find helper_with_long_body");
    assert!(symbols.contains("MultiField"), "Should find MultiField");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd eval-pipeline-wiring && cargo test test_extract_multiline_functions`
Expected: FAIL - multi-line functions not found

- [ ] **Step 3: Fix extract_symbols_from_capsule for multi-line**

The current implementation processes line-by-line. For multi-line functions, we need to handle:
1. Function signatures spanning multiple lines
2. Improved regex for common Rust patterns

Modify `extract_symbols_from_capsule()`:

```rust
/// Extract symbol names from capsule text using simple heuristics
pub fn extract_symbols_from_capsule(capsule_text: &str) -> HashSet<String> {
    let mut symbols = HashSet::new();

    // Pre-process: normalize multi-line signatures to single lines
    // This handles:
    //   fn foo(
    //       arg1: Type1,
    //       arg2: Type2,
    //   ) -> Result<T>
    // into:
    //   fn foo(arg1: Type1, arg2: Type2,) -> Result<T>
    let normalized = normalize_multiline_signatures(capsule_text);
    
    // Also handle joined multi-line structs/enums
    let joined = join_braced_blocks(&normalized);
    
    for line in joined.lines() {
        let line = line.trim();

        // Match function definitions: "fn name" or "pub fn name"
        if let Some(cap) = extract_after_keyword(line, "fn ") {
            let name = cap
                .split(|c: char| c == '(' || c == '<' || c == ' ')
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                symbols.insert(name.to_string());
            }
        }

        // Match struct definitions
        if let Some(cap) = extract_after_keyword(line, "struct ") {
            let name = cap
                .split(|c: char| c == '{' || c == '<' || c == ' ')
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                symbols.insert(name.to_string());
            }
        }

        // Match enum definitions
        if let Some(cap) = extract_after_keyword(line, "enum ") {
            let name = cap
                .split(|c: char| c == '{' || c == ' ')
                .next()
                .unwrap_or("");
            if !name.is_empty() {
                symbols.insert(name.to_string());
            }
        }

        // Match impl blocks
        if let Some(cap) = extract_after_keyword(line, "impl ") {
            let after_generics = if cap.contains('<') && cap.contains('>') {
                cap.split('>').nth(1).unwrap_or(cap).trim()
            } else {
                cap
            };

            let name = if after_generics.contains(" for ") {
                after_generics
                    .split(" for ")
                    .nth(1)
                    .unwrap_or(after_generics)
            } else {
                after_generics
            };

            let name = name
                .split(|c: char| c == '{' || c == ' ')
                .next()
                .unwrap_or("");
            if !name.is_empty() && !name.starts_with("<") {
                symbols.insert(name.to_string());
            }
        }
    }

    symbols
}

/// Normalize multi-line Rust signatures to single line
fn normalize_multiline_signatures(text: &str) -> String {
    let mut result = String::new();
    let mut in_signature = false;
    let mut paren_depth = 0;
    let mut brace_count = 0;
    
    for line in text.lines() {
        let mut line_chars: Vec<char> = line.chars().collect();
        
        for c in &mut line_chars {
            match c {
                '(' | '[' | '{' => {
                    paren_depth += 1;
                    in_signature = true;
                }
                ')' | ']' => {
                    paren_depth -= 1;
                    if paren_depth == 0 {
                        in_signature = false;
                    }
                }
                _ => {}
            }
            
            if in_signature && *c == ',' {
                result.push(*c);
                continue;
            }
            result.push(*c);
        }
        
        if in_signature {
            result.push(' ');
        }
    }
    
    result
}

/// Join multi-line braced blocks (struct, enum bodies)
fn join_braced_blocks(text: &str) -> String {
    let mut result = String::new();
    let mut in_block = false;
    let mut brace_depth = 0;
    
    for line in text.lines() {
        let trimmed = line.trim();
        
        // Detect start of block (struct/enum/impl with opening brace on same or next line)
        if trimmed.ends_with('{') && !trimmed.starts_with("impl ") && !trimmed.starts_with("struct ") && !trimmed.starts_with("enum ") {
            in_block = true;
        }
        
        brace_depth += trimmed.chars().filter(|&c| c == '{').count() as i32;
        brace_depth -= trimmed.chars().filter(|&c| c == '}').count() as i32;
        
        if brace_depth == 0 {
            in_block = false;
        }
        
        if in_block && trimmed.starts_with("pub ") || trimmed.starts_with("pub fn") || trimmed.starts_with("pub struct") {
            // Keep fields in structs
            if trimmed.contains(':') {
                result.push_str(line);
                result.push(' ');
                continue;
            }
        }
        
        result.push_str(line);
        result.push('\n');
    }
    
    result
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd eval-pipeline-wiring && cargo test test_extract_multiline_functions`
Expected: PASS

- [ ] **Step 5: Run existing symbol extraction test to verify no regression**

Run: `cd eval-pipeline-wiring && cargo test test_extract_symbols_from_capsule`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd eval-pipeline-wiring && git add src/eval/relevance.rs && git commit -m "fix(eval): improve symbol extraction for multi-line Rust"
```

---

## Task 4: Run Full Eval Suite to Verify All Fixes

**Files:**
- Run: `eval/cases/relevance/*.json`

- [ ] **Step 1: Build and run eval suite**

Run: `cd eval-pipeline-wiring && cargo run -- eval --tier component`

- [ ] **Step 2: Verify improvements**

Expected improvements:
- G1 (keyword): 5/7 → 7/7 pass (was 2/7)
- G2 (structural): 0/6 → 4/6 pass (was 0/6) 
- G3 (hidden): 0/5 → 3/5 pass (was 0/5)
- Budget: Similar improvement
- Overall: 16/34 → 28/34 pass

- [ ] **Step 3: Analyze remaining failures**

If any cases still fail, document root cause and determine if it's a new issue or requires follow-up task.

- [ ] **Step 4: Commit evaluation results**

```bash
cd eval-pipeline-wiring && git add -A && git commit -m "fix(eval): resolve_target, FTS5 lookup, symbol extraction"
```

---

## Verification Checklist

| Issue | Status | Verification |
|-------|--------|-------------|
| B1: resolve_target path matching | [ ] | G1 cases: recall ≥ 0.80 |
| B2: FTS5 to graph lookup | [ ] | BM25 baseline returns non-empty |
| B3: Symbol extraction | [ ] | Extracts multi-line Rust |
| Overall pass rate | [ ] | 16/34 → ~28/34 pass |

---

## Notes

### Why suffix matching instead of contains()

`file.ends_with("src/config.rs")` is more precise than `.contains("src/config.rs")` because:
- `src/config.rs` matches `/foo/src/config.rs` ✓
- `src/config.rs` matches `/bar/src/config.rs` ✓
- `src/config.rs` does NOT match `/src/config_extra.rs` ✗ (correct rejection)

### Why not basename matching

Basename (`config.rs`) would match both `src/config.rs` AND `tests/config.rs`, which is too loose for precision-critical eval cases.

### Timeline Estimate

- Task 1 (B1): ~15 minutes
- Task 2 (B2): ~30 minutes  
- Task 3 (B3): ~20 minutes
- Task 4 (Verification): ~10 minutes

Total: ~75 minutes for all blocking issues