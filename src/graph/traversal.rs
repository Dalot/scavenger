use std::collections::{HashSet, VecDeque};

use petgraph::Direction;
use petgraph::visit::EdgeRef;

use super::GraphState;
use super::types::NodeId;

/// Parameters controlling graph traversal explosion mitigation.
#[derive(Debug, Clone)]
pub struct TraversalParams {
    pub max_hops: usize,
    pub node_budget: usize,
    pub degree_cap: usize,
    pub builtins_blocklist: HashSet<String>,
}

impl Default for TraversalParams {
    fn default() -> Self {
        Self {
            max_hops: 2,
            node_budget: 100,
            degree_cap: 50,
            builtins_blocklist: HashSet::new(),
        }
    }
}

/// BFS traversal from a starting node, collecting neighbors up to configured limits.
pub fn bfs_collect(
    graph: &GraphState,
    start: &NodeId,
    direction: Direction,
    params: &TraversalParams,
) -> Vec<NodeId> {
    let Some(start_idx) = graph.get_index(start) else {
        return Vec::new();
    };

    let mut visited = HashSet::new();
    visited.insert(start_idx);

    let mut queue = VecDeque::new();
    queue.push_back((start_idx, 0usize));
    let mut result = Vec::new();

    while let Some((idx, depth)) = queue.pop_front() {
        if depth > 0
            && let Some(w) = graph.graph.node_weight(idx)
        {
            if params.builtins_blocklist.contains(&w.name) {
                continue;
            }
            result.push(w.id.clone());
            if result.len() >= params.node_budget {
                break;
            }
        }

        if depth >= params.max_hops {
            continue;
        }

        let neighbors: Vec<_> = graph.graph.edges_directed(idx, direction).collect();

        if neighbors.len() > params.degree_cap {
            continue;
        }

        for edge in neighbors {
            let next = match direction {
                Direction::Outgoing => edge.target(),
                Direction::Incoming => edge.source(),
            };
            if visited.insert(next) {
                queue.push_back((next, depth + 1));
            }
        }
    }

    result
}

/// Bidirectional BFS: collect from both incoming and outgoing edges.
pub fn bidirectional_bfs(
    graph: &GraphState,
    start: &NodeId,
    params: &TraversalParams,
) -> Vec<NodeId> {
    let half_budget = params.node_budget / 2;

    let in_params = TraversalParams {
        node_budget: half_budget,
        ..params.clone()
    };
    let incoming = bfs_collect(graph, start, Direction::Incoming, &in_params);

    let remaining = params.node_budget.saturating_sub(incoming.len());
    let out_params = TraversalParams {
        node_budget: remaining,
        ..params.clone()
    };
    let outgoing = bfs_collect(graph, start, Direction::Outgoing, &out_params);

    let mut result = incoming;
    for id in outgoing {
        if !result.contains(&id) && result.len() < params.node_budget {
            result.push(id);
        }
    }
    result
}

/// Collect 1-hop structural neighbors (for semi-pinned items in capsule).
pub fn one_hop_neighbors(graph: &GraphState, target: &NodeId) -> Vec<NodeId> {
    let params = TraversalParams {
        max_hops: 1,
        node_budget: 50,
        degree_cap: 100,
        builtins_blocklist: HashSet::new(),
    };
    bidirectional_bfs(graph, target, &params)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::types::*;
    use std::path::PathBuf;

    fn node(id: &str, name: &str) -> NodeWeight {
        NodeWeight {
            id: NodeId(id.to_string()),
            kind: NodeKind::Function,
            name: name.to_string(),
            file_path: PathBuf::from(format!("{id}.rs")),
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

    fn edge() -> EdgeWeight {
        EdgeWeight {
            kind: EdgeKind::Calls,
            weight: 1.0,
            confidence: Confidence::Precise,
        }
    }

    #[test]
    fn test_bfs_basic() {
        let mut g = GraphState::new();
        g.add_node(node("a", "alpha"));
        g.add_node(node("b", "beta"));
        g.add_node(node("c", "gamma"));
        g.add_edge(&NodeId("a".into()), &NodeId("b".into()), edge());
        g.add_edge(&NodeId("b".into()), &NodeId("c".into()), edge());

        let params = TraversalParams {
            max_hops: 2,
            ..Default::default()
        };
        let result = bfs_collect(&g, &NodeId("a".into()), Direction::Outgoing, &params);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_bfs_respects_budget() {
        let mut g = GraphState::new();
        g.add_node(node("a", "alpha"));
        for i in 0..20 {
            let id = format!("n{i}");
            g.add_node(node(&id, &format!("fn{i}")));
            g.add_edge(&NodeId("a".into()), &NodeId(id), edge());
        }

        let params = TraversalParams {
            max_hops: 2,
            node_budget: 5,
            ..Default::default()
        };
        let result = bfs_collect(&g, &NodeId("a".into()), Direction::Outgoing, &params);
        assert!(result.len() <= 5);
    }

    #[test]
    fn test_bfs_skips_high_degree() {
        let mut g = GraphState::new();
        g.add_node(node("hub", "hub_fn"));
        for i in 0..60 {
            let id = format!("s{i}");
            g.add_node(node(&id, &format!("spoke{i}")));
            g.add_edge(&NodeId("hub".into()), &NodeId(id), edge());
        }
        g.add_node(node("deep", "deep_fn"));
        g.add_edge(&NodeId("s0".into()), &NodeId("deep".into()), edge());

        let params = TraversalParams {
            max_hops: 3,
            degree_cap: 50,
            ..Default::default()
        };
        let result = bfs_collect(&g, &NodeId("hub".into()), Direction::Outgoing, &params);
        // Hub has >50 edges so its children should NOT be explored (degree cap skips it)
        assert!(result.is_empty());
    }

    #[test]
    fn test_bidirectional() {
        let mut g = GraphState::new();
        g.add_node(node("caller", "caller_fn"));
        g.add_node(node("target", "target_fn"));
        g.add_node(node("callee", "callee_fn"));
        g.add_edge(&NodeId("caller".into()), &NodeId("target".into()), edge());
        g.add_edge(&NodeId("target".into()), &NodeId("callee".into()), edge());

        let params = TraversalParams {
            max_hops: 1,
            ..Default::default()
        };
        let result = bidirectional_bfs(&g, &NodeId("target".into()), &params);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_blocklist() {
        let mut g = GraphState::new();
        g.add_node(node("a", "alpha"));
        g.add_node(node("b", "unwrap"));
        g.add_edge(&NodeId("a".into()), &NodeId("b".into()), edge());

        let params = TraversalParams {
            max_hops: 2,
            builtins_blocklist: ["unwrap".to_string()].into(),
            ..Default::default()
        };
        let result = bfs_collect(&g, &NodeId("a".into()), Direction::Outgoing, &params);
        assert!(result.is_empty());
    }
}
