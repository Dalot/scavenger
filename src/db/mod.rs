pub mod queries;
pub mod schema;

use rusqlite::Connection;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum DbError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error(
        "schema version {found} is newer than supported {max} — refusing to open (downgrade guard)"
    )]
    VersionTooNew { found: u32, max: u32 },
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

pub type DbResult<T> = Result<T, DbError>;

fn apply_pragmas(conn: &Connection) -> DbResult<()> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA cache_size = -64000;
         PRAGMA mmap_size = 268435456;
         PRAGMA auto_vacuum = INCREMENTAL;",
    )?;
    Ok(())
}

/// Open (or create) a per-branch index database.
pub fn open_branch_db(scavenger_dir: &Path, branch: &str) -> DbResult<Connection> {
    let indexes_dir = scavenger_dir.join("indexes");
    std::fs::create_dir_all(&indexes_dir)?;
    let db_path = indexes_dir.join(format!("{}.db", sanitize_branch(branch)));
    let conn = Connection::open(&db_path)?;
    apply_pragmas(&conn)?;
    schema::ensure_branch_schema(&conn)?;
    Ok(conn)
}

/// Open (or create) the shared daemon_meta database.
pub fn open_daemon_meta_db(scavenger_dir: &Path) -> DbResult<Connection> {
    let db_path = scavenger_dir.join("daemon_meta.db");
    let conn = Connection::open(&db_path)?;
    apply_pragmas(&conn)?;
    schema::ensure_daemon_meta_schema(&conn)?;
    Ok(conn)
}

/// Return the `.scavenger` directory for a project root, without checking existence.
pub fn scavenger_dir(project_root: &Path) -> PathBuf {
    project_root.join(".scavenger")
}

fn sanitize_branch(branch: &str) -> String {
    branch.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_open_branch_db_creates_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let scav = tmp.path().join(".scavenger");
        std::fs::create_dir_all(&scav).unwrap();
        let conn = open_branch_db(&scav, "main").unwrap();
        let ver: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(ver, 4);
    }

    #[test]
    fn test_open_daemon_meta_db_creates_schema() {
        let tmp = tempfile::tempdir().unwrap();
        let scav = tmp.path().join(".scavenger");
        std::fs::create_dir_all(&scav).unwrap();
        let conn = open_daemon_meta_db(&scav).unwrap();
        let ver: u32 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(ver, 2);
    }

    #[test]
    fn test_sanitize_branch() {
        assert_eq!(sanitize_branch("feature/foo"), "feature_foo");
        assert_eq!(sanitize_branch("main"), "main");
    }
}
