// T071: BM25 / FTS5 validation — search ranking quality.

use rusqlite::Connection;
use scavenger::db::{queries, schema};
use scavenger::graph::types::*;

fn setup_with_nodes() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    schema::ensure_branch_schema(&conn).unwrap();

    let nodes = [
        (
            "n1",
            "user_authentication",
            "auth.rs",
            "fn user_authentication(token: &str) -> bool",
        ),
        (
            "n2",
            "user_profile",
            "profile.rs",
            "fn user_profile(id: UserId) -> Profile",
        ),
        (
            "n3",
            "render_template",
            "template.rs",
            "fn render_template(ctx: &Context) -> Html",
        ),
        (
            "n4",
            "database_connection",
            "db.rs",
            "fn database_connection(url: &str) -> Connection",
        ),
        (
            "n5",
            "parse_json_request",
            "parser.rs",
            "fn parse_json_request(body: &[u8]) -> Request",
        ),
        (
            "n6",
            "authenticate_user",
            "auth.rs",
            "fn authenticate_user(cred: &Credentials) -> Session",
        ),
        (
            "n7",
            "validate_token",
            "auth.rs",
            "fn validate_token(t: &str) -> bool",
        ),
    ];

    for (id, name, file, sig) in &nodes {
        queries::upsert_node(
            &conn,
            id,
            NodeKind::Function,
            name,
            file,
            1,
            10,
            sig,
            &format!("hash_{id}"),
            None,
            sig,
            &[0xDE],
        )
        .unwrap();
    }
    conn
}

fn node_name(conn: &Connection, node_id: &str) -> String {
    conn.query_row("SELECT name FROM nodes WHERE id = ?1", [node_id], |r| {
        r.get(0)
    })
    .unwrap()
}

fn node_file(conn: &Connection, node_id: &str) -> String {
    conn.query_row(
        "SELECT file_path FROM nodes WHERE id = ?1",
        [node_id],
        |r| r.get(0),
    )
    .unwrap()
}

#[test]
fn test_fts_exact_match_ranks_first() {
    let conn = setup_with_nodes();
    let results = queries::search_nodes_fts(&conn, "user_authentication", 5).unwrap();
    assert!(!results.is_empty(), "should find results");
    let name = node_name(&conn, &results[0].id);
    assert_eq!(name, "user_authentication");
}

#[test]
fn test_fts_partial_match() {
    let conn = setup_with_nodes();
    let results = queries::search_nodes_fts(&conn, "auth*", 10).unwrap();
    assert!(
        results.len() >= 2,
        "should find auth-related nodes, got {}",
        results.len()
    );
    let names: Vec<String> = results.iter().map(|r| node_name(&conn, &r.id)).collect();
    assert!(
        names.iter().any(|n| n.contains("auth")),
        "auth search should find auth functions, got: {names:?}"
    );
}

#[test]
fn test_fts_file_specific_search() {
    let conn = setup_with_nodes();
    let results = queries::search_nodes_fts(&conn, "validate_token", 10).unwrap();
    assert!(!results.is_empty());
    let file = node_file(&conn, &results[0].id);
    assert_eq!(file, "auth.rs");
}

#[test]
fn test_fts_no_results_for_gibberish() {
    let conn = setup_with_nodes();
    let results = queries::search_nodes_fts(&conn, "xyzzy_nonexistent_symbol_qwerty", 5).unwrap();
    assert!(results.is_empty(), "gibberish should return 0 results");
}

#[test]
fn test_fts_ranking_relevance() {
    let conn = setup_with_nodes();
    let results = queries::search_nodes_fts(&conn, "user*", 10).unwrap();
    assert!(
        results.len() >= 2,
        "should find multiple user-related nodes"
    );
    let names: Vec<String> = results.iter().map(|r| node_name(&conn, &r.id)).collect();
    let user_count = names.iter().filter(|n| n.contains("user")).count();
    assert!(
        user_count >= 2,
        "should find at least 2 user-related, got: {names:?}"
    );
}

#[test]
fn test_fts_doc_search() {
    let conn = Connection::open_in_memory().unwrap();
    schema::ensure_branch_schema(&conn).unwrap();

    let now = 1000i64;
    queries::upsert_doc_chunk(
        &conn,
        "README.md",
        0,
        Some("Getting Started"),
        1,
        10,
        "Install and run the project",
        50,
        now,
        "h1",
    )
    .unwrap();
    queries::upsert_doc_chunk(
        &conn,
        "API.md",
        0,
        Some("Authentication"),
        1,
        10,
        "Use JWT tokens for auth",
        50,
        now,
        "h2",
    )
    .unwrap();
    queries::upsert_doc_chunk(
        &conn,
        "GUIDE.md",
        0,
        Some("Deployment"),
        1,
        10,
        "Deploy to production servers",
        50,
        now,
        "h3",
    )
    .unwrap();

    let results = queries::search_doc_chunks_fts(&conn, "authentication", 5).unwrap();
    assert!(!results.is_empty(), "should find auth doc chunk");
    assert!(
        results[0]
            .heading
            .as_deref()
            .unwrap_or("")
            .contains("Authentication")
            || results[0].content.contains("auth")
    );
}

#[test]
fn test_fts_ranking_order_stable() {
    let conn = setup_with_nodes();
    let r1 = queries::search_nodes_fts(&conn, "user*", 10).unwrap();
    let r2 = queries::search_nodes_fts(&conn, "user*", 10).unwrap();
    assert_eq!(r1.len(), r2.len());
    for (a, b) in r1.iter().zip(r2.iter()) {
        assert_eq!(a.id, b.id, "ranking order should be stable");
    }
}
