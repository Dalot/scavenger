use rusqlite::Connection;

use crate::db::queries;

/// Insert a behavioral signal.
pub fn insert_signal(
    conn: &Connection,
    kind: SignalKind,
    node_id: Option<&str>,
    file_path: Option<&str>,
    session_id: &str,
    detail: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = now_secs();
    queries::insert_behavioral_signal(conn, kind.as_str(), node_id, file_path, session_id, now, detail)?;
    Ok(())
}

/// TTL pruning: delete signals older than 48h AND from sessions with count >= 2.
pub fn prune_expired(conn: &Connection) -> Result<u64, Box<dyn std::error::Error>> {
    let cutoff = now_secs() - 48 * 3600;
    Ok(queries::prune_old_signals(conn, cutoff)?)
}

/// Query signals by node_id for capsule inclusion.
pub fn signals_for_node(
    conn: &Connection,
    node_id: &str,
    limit: u32,
) -> Result<Vec<SignalView>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT kind, file_path, session_id, timestamp, detail
         FROM behavioral_signals WHERE node_id = ?1
         ORDER BY timestamp DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![node_id, limit], |row| {
            Ok(SignalView {
                kind: row.get(0)?,
                file_path: row.get(1)?,
                session_id: row.get(2)?,
                timestamp: row.get(3)?,
                detail: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Query signals by session_id.
pub fn signals_for_session(
    conn: &Connection,
    session_id: &str,
    limit: u32,
) -> Result<Vec<SignalView>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT kind, file_path, session_id, timestamp, detail
         FROM behavioral_signals WHERE session_id = ?1
         ORDER BY timestamp DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![session_id, limit], |row| {
            Ok(SignalView {
                kind: row.get(0)?,
                file_path: row.get(1)?,
                session_id: row.get(2)?,
                timestamp: row.get(3)?,
                detail: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get count of active signals (last 48h).
pub fn active_signal_count(conn: &Connection) -> Result<u64, Box<dyn std::error::Error>> {
    let cutoff = now_secs() - 48 * 3600;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM behavioral_signals WHERE timestamp >= ?1",
        rusqlite::params![cutoff],
        |row| row.get(0),
    )?;
    Ok(count as u64)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalKind {
    Thrashing,
    DeadEnd,
    CycleIntroduced,
    LargeBlastRadius,
    Untested,
    IndexBlindSpot,
    FailedSearch,
}

impl SignalKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Thrashing => "THRASHING",
            Self::DeadEnd => "DEAD_END",
            Self::CycleIntroduced => "CYCLE_INTRODUCED",
            Self::LargeBlastRadius => "LARGE_BLAST_RADIUS",
            Self::Untested => "UNTESTED",
            Self::IndexBlindSpot => "INDEX_BLIND_SPOT",
            Self::FailedSearch => "FAILED_SEARCH",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "THRASHING" => Some(Self::Thrashing),
            "DEAD_END" => Some(Self::DeadEnd),
            "CYCLE_INTRODUCED" => Some(Self::CycleIntroduced),
            "LARGE_BLAST_RADIUS" => Some(Self::LargeBlastRadius),
            "UNTESTED" => Some(Self::Untested),
            "INDEX_BLIND_SPOT" => Some(Self::IndexBlindSpot),
            "FAILED_SEARCH" => Some(Self::FailedSearch),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SignalView {
    pub kind: String,
    pub file_path: Option<String>,
    pub session_id: String,
    pub timestamp: i64,
    pub detail: Option<String>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_insert_and_query_signal() {
        let conn = setup_db();
        insert_signal(&conn, SignalKind::Thrashing, Some("n1"), None, "sess1", Some("3 edits in 5min")).unwrap();
        let signals = signals_for_node(&conn, "n1", 10).unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].kind, "THRASHING");
    }

    #[test]
    fn test_query_by_session() {
        let conn = setup_db();
        insert_signal(&conn, SignalKind::DeadEnd, Some("n1"), None, "sess1", None).unwrap();
        insert_signal(&conn, SignalKind::Untested, Some("n2"), None, "sess1", None).unwrap();
        let signals = signals_for_session(&conn, "sess1", 10).unwrap();
        assert_eq!(signals.len(), 2);
    }

    #[test]
    fn test_active_signal_count() {
        let conn = setup_db();
        insert_signal(&conn, SignalKind::Thrashing, None, Some("/test.rs"), "s1", None).unwrap();
        let count = active_signal_count(&conn).unwrap();
        assert_eq!(count, 1);
    }
}
