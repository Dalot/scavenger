pub mod intent;
pub mod search;

use rusqlite::Connection;

use crate::config::Config;
use crate::graph::GraphState;
use crate::graph::types::NodeId;
use intent::{Intent, IntentResult};

/// Result of a query engine invocation.
#[derive(Debug)]
pub struct QueryResult {
    pub target: Option<NodeId>,
    pub intent: IntentResult,
    pub neighbor_ids: Vec<NodeId>,
    pub search_results: Vec<search::SearchResult>,
}

/// Resolve the target node from file + optional symbol name.
pub fn resolve_target(graph: &GraphState, file: &str, symbol: Option<&str>) -> Option<NodeId> {
    if let Some(sym_name) = symbol {
        graph
            .graph
            .node_indices()
            .filter_map(|idx| graph.graph.node_weight(idx))
            .find(|w| w.name == sym_name && w.file_path.to_string_lossy().contains(file))
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

#[allow(dead_code)]
/// Resolve a scope tag to path prefixes using the [scopes] config section.
/// Returns the set of NodeIds whose file_path matches any of the scope's path prefixes.
pub fn resolve_scope(graph: &GraphState, config: &Config, scope_tag: &str) -> Vec<NodeId> {
    let prefixes = match config.scopes.get(scope_tag) {
        Some(crate::config::ScopeValue::Single(p)) => vec![p.as_str()],
        Some(crate::config::ScopeValue::Multiple(ps)) => ps.iter().map(|s| s.as_str()).collect(),
        None => return Vec::new(),
    };

    graph
        .graph
        .node_indices()
        .filter_map(|idx| graph.graph.node_weight(idx))
        .filter(|w| {
            let path = w.file_path.to_string_lossy();
            prefixes
                .iter()
                .any(|prefix| path.starts_with(prefix) || path.contains(prefix))
        })
        .map(|w| w.id.clone())
        .collect()
}

/// Run the full query pipeline: intent detection → search → traversal.
pub fn run_query(
    conn: &Connection,
    graph: &GraphState,
    config: &Config,
    file: &str,
    symbol: Option<&str>,
    query: Option<&str>,
) -> QueryResult {
    let intent_result = match query {
        Some(q) if !q.is_empty() => intent::classify(q),
        _ => IntentResult::single(Intent::Understand),
    };

    let target = resolve_target(graph, file, symbol);

    let search_query = query.or(symbol).unwrap_or("");

    let search_results = search::search(conn, graph, search_query, 50).unwrap_or_default();

    let neighbor_ids = if let Some(ref target_id) = target {
        collect_neighbors(graph, target_id, &intent_result, config)
    } else {
        Vec::new()
    };

    QueryResult {
        target,
        intent: intent_result,
        neighbor_ids,
        search_results,
    }
}

/// Collect neighbor nodes based on intent-driven traversal.
fn collect_neighbors(
    graph: &GraphState,
    target: &NodeId,
    intent: &IntentResult,
    config: &Config,
) -> Vec<NodeId> {
    let node_budget = config.traversal.node_budget as usize;
    let degree_cap = config.traversal.degree_cap as usize;

    let mut primary = traversal_for_intent(graph, target, &intent.primary, node_budget, degree_cap);

    if let Some(ref secondary_intent) = intent.secondary {
        let secondary_budget = (node_budget as f64 * intent.secondary_weight) as usize;
        let secondary = traversal_for_intent(
            graph,
            target,
            secondary_intent,
            secondary_budget,
            degree_cap,
        );
        let primary_budget = (node_budget as f64 * intent.primary_weight) as usize;
        primary.truncate(primary_budget);
        for id in secondary {
            if !primary.contains(&id) && primary.len() < node_budget {
                primary.push(id);
            }
        }
    }

    primary
}

/// Traverse the graph based on a specific intent strategy.
fn traversal_for_intent(
    graph: &GraphState,
    target: &NodeId,
    intent: &Intent,
    budget: usize,
    degree_cap: usize,
) -> Vec<NodeId> {
    use petgraph::Direction;

    match intent {
        Intent::Debug => {
            // Reverse BFS (callers): 3 hops up, 2 down
            let mut result = bfs(graph, target, Direction::Incoming, 3, budget, degree_cap);
            let down = bfs(
                graph,
                target,
                Direction::Outgoing,
                2,
                budget.saturating_sub(result.len()),
                degree_cap,
            );
            for id in down {
                if !result.contains(&id) {
                    result.push(id);
                }
            }
            result
        }
        Intent::Refactor => {
            // Forward DFS (blast radius): transitive, cap at budget
            bfs(graph, target, Direction::Outgoing, 5, budget, degree_cap)
        }
        Intent::Understand => {
            // Bidirectional BFS: 2 hops each direction
            let incoming = bfs(
                graph,
                target,
                Direction::Incoming,
                2,
                budget / 2,
                degree_cap,
            );
            let mut result = incoming;
            let outgoing = bfs(
                graph,
                target,
                Direction::Outgoing,
                2,
                budget.saturating_sub(result.len()),
                degree_cap,
            );
            for id in outgoing {
                if !result.contains(&id) {
                    result.push(id);
                }
            }
            result
        }
        Intent::Extend => {
            // Sibling/implements BFS: 1-2 hops lateral
            bfs(graph, target, Direction::Outgoing, 2, budget, degree_cap)
        }
        Intent::Review => {
            // Bidirectional BFS: 2 hops all
            let incoming = bfs(
                graph,
                target,
                Direction::Incoming,
                2,
                budget / 2,
                degree_cap,
            );
            let mut result = incoming;
            let outgoing = bfs(
                graph,
                target,
                Direction::Outgoing,
                2,
                budget.saturating_sub(result.len()),
                degree_cap,
            );
            for id in outgoing {
                if !result.contains(&id) {
                    result.push(id);
                }
            }
            result
        }
    }
}

/// BFS traversal collecting NodeIds up to a hop limit and node budget.
fn bfs(
    graph: &GraphState,
    start: &NodeId,
    direction: petgraph::Direction,
    max_hops: usize,
    budget: usize,
    degree_cap: usize,
) -> Vec<NodeId> {
    use petgraph::visit::EdgeRef;
    use std::collections::{HashSet, VecDeque};

    let Some(start_idx) = graph.get_index(start) else {
        return Vec::new();
    };

    let mut visited = HashSet::new();
    visited.insert(start_idx);

    let mut queue = VecDeque::new();
    queue.push_back((start_idx, 0usize));

    let mut result = Vec::new();

    while let Some((idx, depth)) = queue.pop_front() {
        if depth > 0 {
            if let Some(w) = graph.graph.node_weight(idx) {
                result.push(w.id.clone());
                if result.len() >= budget {
                    break;
                }
            }
        }

        if depth >= max_hops {
            continue;
        }

        let neighbors: Vec<_> = graph.graph.edges_directed(idx, direction).collect();

        // Skip high-degree utility nodes
        if neighbors.len() > degree_cap {
            continue;
        }

        for edge in neighbors {
            let next = match direction {
                petgraph::Direction::Outgoing => edge.target(),
                petgraph::Direction::Incoming => edge.source(),
            };
            if visited.insert(next) {
                queue.push_back((next, depth + 1));
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::{Confidence, EdgeKind, EdgeWeight, NodeKind, NodeWeight};
    use std::path::PathBuf;

    fn make_node(id: &str, name: &str, file: &str) -> NodeWeight {
        NodeWeight {
            id: NodeId(id.to_string()),
            kind: NodeKind::Function,
            name: name.to_string(),
            file_path: PathBuf::from(file),
            line_start: 1,
            line_end: 10,
            signature: format!("fn {name}()"),
            signature_hash: "aabb0011".to_string(),
            docstring: None,
            skeleton: format!("fn {name}()"),
            centrality: 0.0,
            checksum: vec![0xDE, 0xAD],
        }
    }

    fn make_edge() -> EdgeWeight {
        EdgeWeight {
            kind: EdgeKind::Calls,
            weight: 1.0,
            confidence: Confidence::Precise,
        }
    }

    #[test]
    fn test_resolve_target() {
        let mut g = GraphState::new();
        g.add_node(make_node("n1", "hello", "src/lib.rs"));
        assert!(resolve_target(&g, "src/lib.rs", Some("hello")).is_some());
        assert!(resolve_target(&g, "src/lib.rs", Some("missing")).is_none());
    }

    #[test]
    fn test_bfs_collects_neighbors() {
        let mut g = GraphState::new();
        g.add_node(make_node("a", "alpha", "a.rs"));
        g.add_node(make_node("b", "beta", "b.rs"));
        g.add_node(make_node("c", "gamma", "c.rs"));
        g.add_edge(&NodeId("a".into()), &NodeId("b".into()), make_edge());
        g.add_edge(&NodeId("b".into()), &NodeId("c".into()), make_edge());

        let result = bfs(
            &g,
            &NodeId("a".into()),
            petgraph::Direction::Outgoing,
            2,
            100,
            50,
        );
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_bfs_respects_budget() {
        let mut g = GraphState::new();
        g.add_node(make_node("a", "alpha", "a.rs"));
        for i in 0..20 {
            let id = format!("n{i}");
            g.add_node(make_node(&id, &format!("fn{i}"), &format!("{id}.rs")));
            g.add_edge(&NodeId("a".into()), &NodeId(id), make_edge());
        }

        let result = bfs(
            &g,
            &NodeId("a".into()),
            petgraph::Direction::Outgoing,
            2,
            5,
            50,
        );
        assert!(result.len() <= 5);
    }
}
