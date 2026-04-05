#![allow(dead_code)]
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
    queries::insert_behavioral_signal(
        conn,
        kind.as_str(),
        node_id,
        file_path,
        session_id,
        now,
        detail,
    )?;
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

/// Utility classification for signals.
///
/// - Improvement: signals that drive Scavenger's own improvement loop
///   (capsule assembly, annotation quality, retrieval).
/// - Insights: signals that give the user actionable knowledge about their codebase.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalUtility {
    Improvement,
    Insights,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignalKind {
    // Improvement signals
    Thrashing,
    EmptyCapsule,
    FailedSearch,
    // Insights signals
    Churn,
    Hotspot,
    LargeBlastRadius,
}

impl SignalKind {
    pub fn utility(&self) -> SignalUtility {
        match self {
            Self::Thrashing | Self::EmptyCapsule | Self::FailedSearch => SignalUtility::Improvement,
            Self::Churn | Self::Hotspot | Self::LargeBlastRadius => SignalUtility::Insights,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Thrashing => "THRASHING",
            Self::EmptyCapsule => "EMPTY_CAPSULE",
            Self::FailedSearch => "FAILED_SEARCH",
            Self::Churn => "CHURN",
            Self::Hotspot => "HOTSPOT",
            Self::LargeBlastRadius => "LARGE_BLAST_RADIUS",
        }
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "THRASHING" => Some(Self::Thrashing),
            "EMPTY_CAPSULE" => Some(Self::EmptyCapsule),
            "FAILED_SEARCH" => Some(Self::FailedSearch),
            "CHURN" => Some(Self::Churn),
            "HOTSPOT" => Some(Self::Hotspot),
            "LARGE_BLAST_RADIUS" => Some(Self::LargeBlastRadius),
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
        insert_signal(
            &conn,
            SignalKind::Thrashing,
            Some("n1"),
            None,
            "sess1",
            Some("3 edits in 5min"),
        )
        .unwrap();
        let signals = signals_for_node(&conn, "n1", 10).unwrap();
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].kind, "THRASHING");
    }

    #[test]
    fn test_query_by_session() {
        let conn = setup_db();
        insert_signal(
            &conn,
            SignalKind::Thrashing,
            Some("n1"),
            None,
            "sess1",
            None,
        )
        .unwrap();
        insert_signal(
            &conn,
            SignalKind::Churn,
            None,
            Some("/src/lib.rs"),
            "sess1",
            None,
        )
        .unwrap();
        let signals = signals_for_session(&conn, "sess1", 10).unwrap();
        assert_eq!(signals.len(), 2);
    }

    #[test]
    fn test_active_signal_count() {
        let conn = setup_db();
        insert_signal(
            &conn,
            SignalKind::Thrashing,
            None,
            Some("/test.rs"),
            "s1",
            None,
        )
        .unwrap();
        let count = active_signal_count(&conn).unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_signal_utility_improvement() {
        assert_eq!(SignalKind::Thrashing.utility(), SignalUtility::Improvement);
        assert_eq!(
            SignalKind::EmptyCapsule.utility(),
            SignalUtility::Improvement
        );
        assert_eq!(
            SignalKind::FailedSearch.utility(),
            SignalUtility::Improvement
        );
    }

    #[test]
    fn test_signal_utility_insights() {
        assert_eq!(SignalKind::Churn.utility(), SignalUtility::Insights);
        assert_eq!(SignalKind::Hotspot.utility(), SignalUtility::Insights);
        assert_eq!(
            SignalKind::LargeBlastRadius.utility(),
            SignalUtility::Insights
        );
    }

    #[test]
    fn test_signal_from_str_roundtrip() {
        for kind in [
            SignalKind::Thrashing,
            SignalKind::EmptyCapsule,
            SignalKind::FailedSearch,
            SignalKind::Churn,
            SignalKind::Hotspot,
            SignalKind::LargeBlastRadius,
        ] {
            assert_eq!(SignalKind::from_str(kind.as_str()), Some(kind));
        }
        assert_eq!(SignalKind::from_str("UNKNOWN"), None);
    }
}
