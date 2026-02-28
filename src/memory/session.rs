use rusqlite::Connection;

use crate::db::queries;

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

/// Record a session activity event.
pub fn record_event(
    conn: &Connection,
    session_id: &str,
    event_type: &str,
    file_path: Option<&str>,
    symbol: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = now_secs();
    queries::insert_session_event(conn, session_id, event_type, file_path, symbol, now)?;
    Ok(())
}

/// TTL pruning: delete session log entries older than 48h.
pub fn prune_expired(conn: &Connection) -> Result<u64, Box<dyn std::error::Error>> {
    let cutoff = now_secs() - 48 * 3600;
    Ok(queries::prune_old_sessions(conn, cutoff)?)
}

/// Get recent session activity for a given session.
pub fn recent_activity(
    conn: &Connection,
    session_id: &str,
    limit: u32,
) -> Result<Vec<SessionEvent>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT event_type, file_path, symbol, timestamp
         FROM session_log WHERE session_id = ?1
         ORDER BY timestamp DESC LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![session_id, limit], |row| {
            Ok(SessionEvent {
                event_type: row.get(0)?,
                file_path: row.get(1)?,
                symbol: row.get(2)?,
                timestamp: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get files touched in this session (for Jaccard scoring in capsule).
pub fn session_files(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT file_path FROM session_log
         WHERE session_id = ?1 AND file_path IS NOT NULL",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![session_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Get symbols touched in this session.
pub fn session_symbols(
    conn: &Connection,
    session_id: &str,
) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT symbol FROM session_log
         WHERE session_id = ?1 AND symbol IS NOT NULL",
    )?;
    let rows = stmt
        .query_map(rusqlite::params![session_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Generate a session summary: recent activity + file/symbol counts.
pub fn session_summary(
    conn: &Connection,
    session_id: &str,
) -> Result<SessionSummary, Box<dyn std::error::Error>> {
    let events = recent_activity(conn, session_id, 20)?;
    let files = session_files(conn, session_id)?;
    let symbols = session_symbols(conn, session_id)?;

    let total_events: i64 = conn.query_row(
        "SELECT COUNT(*) FROM session_log WHERE session_id = ?1",
        rusqlite::params![session_id],
        |row| row.get(0),
    )?;

    Ok(SessionSummary {
        session_id: session_id.to_string(),
        total_events: total_events as u64,
        unique_files: files.len() as u64,
        unique_symbols: symbols.len() as u64,
        recent_events: events,
        files_touched: files,
    })
}

#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub event_type: String,
    pub file_path: Option<String>,
    pub symbol: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_id: String,
    pub total_events: u64,
    pub unique_files: u64,
    pub unique_symbols: u64,
    pub recent_events: Vec<SessionEvent>,
    pub files_touched: Vec<String>,
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
    fn test_record_and_query_events() {
        let conn = setup_db();
        record_event(&conn, "s1", "read", Some("/a.rs"), Some("foo")).unwrap();
        record_event(&conn, "s1", "edit", Some("/b.rs"), None).unwrap();

        let events = recent_activity(&conn, "s1", 10).unwrap();
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_session_files_and_symbols() {
        let conn = setup_db();
        record_event(&conn, "s1", "read", Some("/a.rs"), Some("foo")).unwrap();
        record_event(&conn, "s1", "read", Some("/a.rs"), Some("bar")).unwrap();
        record_event(&conn, "s1", "edit", Some("/b.rs"), None).unwrap();

        let files = session_files(&conn, "s1").unwrap();
        assert_eq!(files.len(), 2);

        let symbols = session_symbols(&conn, "s1").unwrap();
        assert_eq!(symbols.len(), 2);
    }

    #[test]
    fn test_session_summary() {
        let conn = setup_db();
        record_event(&conn, "s1", "read", Some("/a.rs"), Some("foo")).unwrap();
        let summary = session_summary(&conn, "s1").unwrap();
        assert_eq!(summary.total_events, 1);
        assert_eq!(summary.unique_files, 1);
    }
}
