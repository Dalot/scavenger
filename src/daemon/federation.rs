use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use serde_json::{json, Value};

use crate::db;
use crate::db::queries;

/// Manages connections to federated repositories.
pub struct FederationManager {
    connections: HashMap<PathBuf, FederatedRepo>,
    validation_cache: HashMap<PathBuf, (Instant, bool)>,
}

struct FederatedRepo {
    path: PathBuf,
    conn: Connection,
    branch: String,
}

impl FederationManager {
    pub fn new() -> Self {
        Self {
            connections: HashMap::new(),
            validation_cache: HashMap::new(),
        }
    }

    /// Connect to a federated repo. Validates schema and caches connection.
    pub fn connect(&mut self, repo_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let scav_dir = repo_path.join(".scavenger");
        if !scav_dir.exists() {
            return Err(format!("No .scavenger directory in {}", repo_path.display()).into());
        }

        // Read current branch from daemon_meta
        let meta_path = scav_dir.join("daemon_meta.db");
        if !meta_path.exists() {
            return Err("No daemon_meta.db found".into());
        }

        let meta_conn = Connection::open(&meta_path)?;
        let branch: String = meta_conn
            .query_row(
                "SELECT value FROM daemon_meta WHERE key = 'current_branch'",
                [],
                |row| row.get(0),
            )
            .unwrap_or_else(|_| "main".to_string());

        // Open branch DB read-only
        let sanitized = branch.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let db_path = scav_dir.join("indexes").join(format!("{sanitized}.db"));
        if !db_path.exists() {
            return Err(format!("Branch DB not found: {}", db_path.display()).into());
        }

        let conn = Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;

        // Validate schema
        if !self.is_validated(repo_path) {
            self.validate_schema(&conn)?;
            self.validation_cache
                .insert(repo_path.to_path_buf(), (Instant::now(), true));
        }

        self.connections.insert(
            repo_path.to_path_buf(),
            FederatedRepo {
                path: repo_path.to_path_buf(),
                conn,
                branch,
            },
        );

        Ok(())
    }

    /// Search nodes across all federated repos via FTS5.
    pub fn search_nodes(
        &self,
        query: &str,
        limit: u32,
        timeout: Duration,
    ) -> Vec<FederatedResult> {
        let mut results = Vec::new();
        let start = Instant::now();

        for (path, repo) in &self.connections {
            if start.elapsed() > timeout {
                break;
            }

            match queries::search_nodes_fts(&repo.conn, query, limit) {
                Ok(matches) => {
                    for m in matches {
                        results.push(FederatedResult {
                            repo_path: path.clone(),
                            id: m.id,
                            rank: m.rank,
                            source: "nodes_fts".to_string(),
                        });
                    }
                }
                Err(e) => {
                    eprintln!("Federation search error for {}: {e}", path.display());
                }
            }
        }

        results
    }

    /// Search doc chunks across all federated repos via FTS5.
    pub fn search_docs(
        &self,
        query: &str,
        limit: u32,
        timeout: Duration,
    ) -> Vec<FederatedDocResult> {
        let mut results = Vec::new();
        let start = Instant::now();

        for (path, repo) in &self.connections {
            if start.elapsed() > timeout {
                break;
            }

            match queries::search_doc_chunks_fts(&repo.conn, query, limit) {
                Ok(matches) => {
                    for m in matches {
                        results.push(FederatedDocResult {
                            repo_path: path.clone(),
                            file_path: m.file_path,
                            heading: m.heading,
                            content: m.content,
                            rank: m.rank,
                        });
                    }
                }
                Err(e) => {
                    eprintln!("Federation doc search error for {}: {e}", path.display());
                }
            }
        }

        results
    }

    /// Disconnect a federated repo.
    pub fn disconnect(&mut self, repo_path: &Path) {
        self.connections.remove(repo_path);
        self.validation_cache.remove(repo_path);
    }

    /// List all connected federated repos.
    pub fn list(&self) -> Vec<FederatedRepoInfo> {
        self.connections
            .values()
            .map(|r| FederatedRepoInfo {
                path: r.path.clone(),
                branch: r.branch.clone(),
            })
            .collect()
    }

    fn is_validated(&self, path: &Path) -> bool {
        self.validation_cache
            .get(path)
            .is_some_and(|(when, valid)| *valid && when.elapsed() < Duration::from_secs(3600))
    }

    fn validate_schema(&self, conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
        let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < 1 {
            return Err("Incompatible schema version".into());
        }

        // Quick check
        let result: String = conn.query_row("PRAGMA quick_check", [], |row| row.get(0))?;
        if result != "ok" {
            return Err(format!("DB integrity check failed: {result}").into());
        }

        // Verify required tables exist
        let tables: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT name FROM sqlite_master WHERE type='table' AND name IN ('nodes', 'edges', 'files')",
            )?;
            stmt.query_map([], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        if tables.len() < 3 {
            return Err("Missing required tables in federated DB".into());
        }

        Ok(())
    }
}

#[derive(Debug)]
pub struct FederatedResult {
    pub repo_path: PathBuf,
    pub id: String,
    pub rank: f64,
    pub source: String,
}

#[derive(Debug)]
pub struct FederatedDocResult {
    pub repo_path: PathBuf,
    pub file_path: String,
    pub heading: Option<String>,
    pub content: String,
    pub rank: f64,
}

#[derive(Debug)]
pub struct FederatedRepoInfo {
    pub path: PathBuf,
    pub branch: String,
}

/// Verify all federated repos are accessible and healthy.
pub fn verify_all(repos: &FederationManager) -> Vec<Value> {
    repos
        .list()
        .iter()
        .map(|r| {
            let accessible = r.path.join(".scavenger").exists();
            json!({
                "path": r.path.display().to_string(),
                "branch": r.branch,
                "accessible": accessible,
            })
        })
        .collect()
}
