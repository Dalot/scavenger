# Signal Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Migrate Scavenger's signal system from a broken real-time `AntiPatternDetector` to a post-hoc computation model with 7 signals (3 Improvement, 4 Insights), a triage function, and automatic computation in the maintenance loop.

**Architecture:** New `memory/signals/compute.rs` module reads from `session_log`, `capsule_log`, and graph state after sessions end. Signals are classified as Improvement (drive Scavenger's improvement loop) or Insights (actionable codebase knowledge). A triage module ranks sessions by informativeness. Computation runs every 15 minutes in the maintenance loop.

**Tech Stack:** Rust, rusqlite, petgraph, existing Scavenger codebase

---

## File Structure

| File | Action | Responsibility |
|------|--------|----------------|
| `src/db/schema.rs` | Modify | Schema v3→v4 migration, update CHECK constraint, update KNOWN_MAX_VERSION |
| `src/memory/signals.rs` | Modify | Extend SignalKind enum (7 variants), add SignalUtility enum, add utility() method |
| `src/memory/signals/compute.rs` | Create | Post-hoc signal computation module, SignalConfig, SignalRecord, all detect_* functions |
| `src/memory/signals/triage.rs` | Create | Triage module, SessionTriage, SignalSummary, triage_sessions() |
| `src/memory/mod.rs` | Modify | Integrate computation into maintenance(), remove detector from on_reindex() |
| `src/memory/antipattern.rs` | Delete | Remove entire file |
| `src/daemon/handlers.rs` | Modify | Remove record_search_miss() calls if any |

---

## Task 1: Schema Migration + SignalKind Refactor

### Task 1.1: Update SignalKind enum and add SignalUtility

**Files:**
- Modify: `src/memory/signals.rs`

- [ ] **Step 1: Replace the SignalKind enum and add SignalUtility**

Replace the existing `SignalKind` enum (lines 95-132) with the refined 7-variant version and add `SignalUtility`:

```rust
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
```

- [ ] **Step 2: Update existing tests to use new SignalKind variants**

Replace the test `test_query_by_session` which uses `SignalKind::DeadEnd` and `SignalKind::Untested` (both removed):

```rust
    #[test]
    fn test_query_by_session() {
        let conn = setup_db();
        insert_signal(&conn, SignalKind::Thrashing, Some("n1"), None, "sess1", None).unwrap();
        insert_signal(&conn, SignalKind::Churn, None, Some("/src/lib.rs"), "sess1", None).unwrap();
        let signals = signals_for_session(&conn, "sess1", 10).unwrap();
        assert_eq!(signals.len(), 2);
    }
```

- [ ] **Step 3: Add tests for SignalKind::utility()**

Add these tests to the `#[cfg(test)]` module in `signals.rs`:

```rust
    #[test]
    fn test_signal_utility_improvement() {
        assert_eq!(SignalKind::Thrashing.utility(), SignalUtility::Improvement);
        assert_eq!(SignalKind::EmptyCapsule.utility(), SignalUtility::Improvement);
        assert_eq!(SignalKind::FailedSearch.utility(), SignalUtility::Improvement);
    }

    #[test]
    fn test_signal_utility_insights() {
        assert_eq!(SignalKind::Churn.utility(), SignalUtility::Insights);
        assert_eq!(SignalKind::Hotspot.utility(), SignalUtility::Insights);
        assert_eq!(SignalKind::LargeBlastRadius.utility(), SignalUtility::Insights);
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
```

- [ ] **Step 4: Run tests to verify**

```bash
cargo test memory::signals --lib
```

Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/memory/signals.rs
git commit -m "refactor: update SignalKind enum with 7 variants and SignalUtility classification"
```

---

### Task 1.2: Schema v3→v4 Migration

**Files:**
- Modify: `src/db/schema.rs`

- [ ] **Step 1: Update KNOWN_MAX_VERSION**

Change line 4 from:
```rust
pub const KNOWN_MAX_VERSION: u32 = 3;
```
to:
```rust
pub const KNOWN_MAX_VERSION: u32 = 4;
```

- [ ] **Step 2: Add v3→v4 migration call in ensure_branch_schema**

After the `if current == 2` block (after line 33), add:

```rust
    if current == 3 {
        migrate_v3_to_v4(conn)?;
        conn.pragma_update(None, "user_version", 4)?;
    }
```

- [ ] **Step 3: Add migrate_v3_to_v4 function**

Add after `migrate_v2_to_v3` (after line 77):

```rust
fn migrate_v3_to_v4(conn: &Connection) -> DbResult<()> {
    // Extend behavioral_signals CHECK constraint to include new signal kinds.
    // SQLite doesn't support ALTER TABLE ADD CHECK, so we recreate the table.
    conn.execute_batch(
        "CREATE TABLE behavioral_signals_new (
            id         INTEGER PRIMARY KEY,
            kind       TEXT NOT NULL CHECK(kind IN (
                           'THRASHING', 'EMPTY_CAPSULE', 'FAILED_SEARCH',
                           'CHURN', 'HOTSPOT', 'LARGE_BLAST_RADIUS'
                       )),
            node_id    TEXT,
            file_path  TEXT,
            session_id TEXT NOT NULL,
            timestamp  INTEGER NOT NULL,
            detail     TEXT
        );
        INSERT INTO behavioral_signals_new SELECT * FROM behavioral_signals;
        DROP TABLE behavioral_signals;
        ALTER TABLE behavioral_signals_new RENAME TO behavioral_signals;
        CREATE INDEX IF NOT EXISTS idx_signals_node
            ON behavioral_signals(node_id, timestamp DESC);
        CREATE INDEX IF NOT EXISTS idx_signals_session
            ON behavioral_signals(session_id);",
    )?;
    Ok(())
}
```

- [ ] **Step 4: Update BRANCH_SCHEMA_V1 CHECK constraint**

In the `BRANCH_SCHEMA_V1` const (line 235-239), replace the CHECK constraint:

```sql
    kind       TEXT NOT NULL CHECK(kind IN (
                   'THRASHING', 'EMPTY_CAPSULE', 'FAILED_SEARCH',
                   'CHURN', 'HOTSPOT', 'LARGE_BLAST_RADIUS'
               )),
```

- [ ] **Step 5: Update schema version test**

Change the test `test_branch_schema_version_set` assertion from `assert_eq!(ver, 3)` to:

```rust
        assert_eq!(ver, 4);
```

- [ ] **Step 6: Add migration test**

Add to the `#[cfg(test)]` module in `schema.rs`:

```rust
    #[test]
    fn test_v3_to_v4_migration() {
        let conn = Connection::open_in_memory().unwrap();
        ensure_branch_schema(&conn).unwrap();

        // Insert a signal with the old schema's valid kinds
        conn.execute(
            "INSERT INTO behavioral_signals (kind, node_id, file_path, session_id, timestamp, detail)
             VALUES ('THRASHING', 'n1', '/src/lib.rs', 's1', 1000, 'test')",
            [],
        ).unwrap();

        // Verify the new CHECK constraint accepts all 7 kinds
        for kind in &["THRASHING", "EMPTY_CAPSULE", "FAILED_SEARCH", "CHURN", "HOTSPOT", "LARGE_BLAST_RADIUS"] {
            conn.execute(
                &format!("INSERT INTO behavioral_signals (kind, session_id, timestamp) VALUES ('{}', 's2', 1000)", kind),
                [],
            ).unwrap_or_else(|e| panic!("Failed to insert {}: {}", kind, e));
        }

        // Verify old kinds are rejected
        let result = conn.execute(
            "INSERT INTO behavioral_signals (kind, session_id, timestamp) VALUES ('DEAD_END', 's3', 1000)",
            [],
        );
        assert!(result.is_err(), "DEAD_END should be rejected after migration");
    }
```

- [ ] **Step 7: Run tests**

```bash
cargo test db::schema --lib
```

Expected: All tests pass.

- [ ] **Step 8: Commit**

```bash
git add src/db/schema.rs
git commit -m "feat: add schema v4 migration with new signal kinds"
```

---

## Task 2: Improvement Signal Computation

### Task 2.1: Create compute.rs module structure

**Files:**
- Create: `src/memory/signals/compute.rs`
- Modify: `src/memory/signals.rs` (add mod declaration)

- [ ] **Step 1: Add mod declaration in signals.rs**

At the top of `src/memory/signals.rs`, after `use crate::db::queries;`, add:

```rust
pub mod compute;
```

- [ ] **Step 2: Create compute.rs with SignalConfig and SignalRecord**

Create `src/memory/signals/compute.rs`:

```rust
use rusqlite::Connection;

use super::{SignalKind, insert_signal};

/// Configuration for signal detection thresholds.
#[derive(Debug, Clone)]
pub struct SignalConfig {
    pub thrashing_edit_threshold: u32,
    pub thrashing_window_seconds: u64,
    pub failed_search_threshold: u32,
    pub compute_window_minutes: u64,
}

impl Default for SignalConfig {
    fn default() -> Self {
        Self {
            thrashing_edit_threshold: 5,
            thrashing_window_seconds: 300,
            failed_search_threshold: 3,
            compute_window_minutes: 15,
        }
    }
}

/// A detected signal ready for persistence.
#[derive(Debug, Clone)]
pub struct SignalRecord {
    pub kind: SignalKind,
    pub node_id: Option<String>,
    pub file_path: Option<String>,
    pub session_id: String,
    pub detail: String,
    /// Optional timestamp for cross-session signals (CHURN, HOTSPOT).
    /// Per-session signals use the current time when persisted.
    pub timestamp: Option<i64>,
}

/// Compute signals for a single session.
pub fn compute_signals_for_session(
    branch_conn: &Connection,
    meta_conn: &Connection,
    session_id: &str,
    config: &SignalConfig,
) -> Vec<SignalRecord> {
    let mut signals = Vec::new();

    signals.extend(detect_thrashing(branch_conn, session_id, config));
    signals.extend(detect_empty_capsule(meta_conn, session_id));
    signals.extend(detect_failed_search(branch_conn, session_id, config));

    signals
}

/// Compute signals for all sessions with recent activity.
///
/// Only processes sessions that have events in the last `compute_window_minutes`
/// and do not already have signals persisted.
pub fn compute_signals_for_recent_sessions(
    branch_conn: &Connection,
    meta_conn: &Connection,
    config: &SignalConfig,
) -> Vec<(String, Vec<SignalRecord>)> {
    let cutoff = now_secs() - (config.compute_window_minutes as i64 * 60);

    // Find sessions with recent activity
    let session_ids: Vec<String> = branch_conn
        .prepare(
            "SELECT DISTINCT session_id FROM session_log WHERE timestamp >= ?1",
        )
        .ok()
        .map(|mut stmt| {
            stmt.query_map(rusqlite::params![cutoff], |row| row.get(0))
                .ok()
                .map(|rows| rows.flatten().collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .unwrap_or_default();

    // Filter out sessions that already have signals
    let sessions_with_signals: Vec<String> = session_ids
        .iter()
        .filter(|sid| {
            branch_conn.query_row(
                "SELECT 1 FROM behavioral_signals WHERE session_id = ?1 LIMIT 1",
                rusqlite::params![sid],
                |_| Ok(()),
            ).is_ok()
        })
        .cloned()
        .collect();

    let sessions_to_compute: Vec<_> = session_ids
        .into_iter()
        .filter(|sid| !sessions_with_signals.contains(sid))
        .collect();

    sessions_to_compute
        .into_iter()
        .map(|sid| {
            let signals = compute_signals_for_session(branch_conn, meta_conn, &sid, config);
            (sid, signals)
        })
        .collect()
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
```

- [ ] **Step 3: Run cargo check**

```bash
cargo check --lib
```

Expected: Compiles with warnings about unused functions (detect_* not yet implemented).

- [ ] **Step 4: Commit**

```bash
git add src/memory/signals.rs src/memory/signals/compute.rs
git commit -m "feat: add compute module structure with SignalConfig and SignalRecord"
```

---

### Task 2.2: Implement detect_thrashing

**Files:**
- Modify: `src/memory/signals/compute.rs`

- [ ] **Step 1: Write the failing test**

Add to a new `#[cfg(test)]` module at the bottom of `compute.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn setup_branch_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();
        conn
    }

    fn setup_meta_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_daemon_meta_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_detect_thrashing_fires_at_threshold() {
        let conn = setup_branch_db();
        let now = now_secs();
        let config = SignalConfig::default();

        // Insert 5 edits to the same file within 5 minutes
        for i in 0..5 {
            conn.execute(
                "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
                 VALUES ('s1', 'edit', '/src/lib.rs', NULL, ?1)",
                rusqlite::params![now - (i * 30)],
            ).unwrap();
        }

        let signals = detect_thrashing(&conn, "s1", &config);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].kind, SignalKind::Thrashing);
        assert_eq!(signals[0].file_path, Some("/src/lib.rs".to_string()));
        assert!(signals[0].detail.contains("5 edits"));
    }

    #[test]
    fn test_detect_thrashing_does_not_fire_below_threshold() {
        let conn = setup_branch_db();
        let now = now_secs();
        let config = SignalConfig::default();

        // Insert 4 edits (below threshold of 5)
        for i in 0..4 {
            conn.execute(
                "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
                 VALUES ('s1', 'edit', '/src/lib.rs', NULL, ?1)",
                rusqlite::params![now - (i * 30)],
            ).unwrap();
        }

        let signals = detect_thrashing(&conn, "s1", &config);
        assert_eq!(signals.len(), 0);
    }

    #[test]
    fn test_detect_thrashing_respects_time_window() {
        let conn = setup_branch_db();
        let now = now_secs();
        let config = SignalConfig::default();

        // Insert 5 edits but spread over 10 minutes (outside 5min window)
        for i in 0..5 {
            conn.execute(
                "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
                 VALUES ('s1', 'edit', '/src/lib.rs', NULL, ?1)",
                rusqlite::params![now - (i * 120)],
            ).unwrap();
        }

        let signals = detect_thrashing(&conn, "s1", &config);
        assert_eq!(signals.len(), 0);
    }

    #[test]
    fn test_detect_thrashing_multiple_files() {
        let conn = setup_branch_db();
        let now = now_secs();
        let config = SignalConfig::default();

        // 5 edits to file A, 3 edits to file B (below threshold)
        for i in 0..5 {
            conn.execute(
                "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
                 VALUES ('s1', 'edit', '/src/a.rs', NULL, ?1)",
                rusqlite::params![now - (i * 30)],
            ).unwrap();
        }
        for i in 0..3 {
            conn.execute(
                "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
                 VALUES ('s1', 'edit', '/src/b.rs', NULL, ?1)",
                rusqlite::params![now - (i * 30)],
            ).unwrap();
        }

        let signals = detect_thrashing(&conn, "s1", &config);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].file_path, Some("/src/a.rs".to_string()));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test memory::signals::compute::tests::test_detect_thrashing --lib
```

Expected: FAIL with "function `detect_thrashing` not found"

- [ ] **Step 3: Implement detect_thrashing**

Add to `compute.rs` (before the `compute_signals_for_session` function):

```rust
/// Detect THRASHING: >=N edits to the same file within a sliding time window.
fn detect_thrashing(
    conn: &Connection,
    session_id: &str,
    config: &SignalConfig,
) -> Vec<SignalRecord> {
    let window_start = now_secs() - config.thrashing_window_seconds as i64;

    let mut stmt = match conn.prepare(
        "SELECT file_path, COUNT(*) as edit_count
         FROM session_log
         WHERE session_id = ?1 AND event_type = 'edit' AND timestamp >= ?2
         GROUP BY file_path
         HAVING edit_count >= ?3",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "detect_thrashing: failed to prepare query");
            return Vec::new();
        }
    };

    let rows = match stmt.query_map(
        rusqlite::params![session_id, window_start, config.thrashing_edit_threshold],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
            ))
        },
    ) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "detect_thrashing: failed to query");
            return Vec::new();
        }
    };

    rows.filter_map(|r| r.ok())
        .map(|(file_path, count)| SignalRecord {
            kind: SignalKind::Thrashing,
            node_id: None,
            file_path: Some(file_path),
            session_id: session_id.to_string(),
            detail: format!("{} edits in {}s window", count, config.thrashing_window_seconds),
            timestamp: None,
        })
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test memory::signals::compute::tests::test_detect_thrashing --lib
```

Expected: All 4 thrashing tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/memory/signals/compute.rs
git commit -m "feat: implement THRASHING detection with sliding window"
```

---

### Task 2.3: Implement detect_empty_capsule

**Files:**
- Modify: `src/memory/signals/compute.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)]` module in `compute.rs`:

```rust
    #[test]
    fn test_detect_empty_capsule_fires_on_zero_tokens() {
        let meta_conn = setup_meta_db();

        meta_conn.execute(
            "INSERT INTO capsule_log (capsule_id, timestamp, session_id, file, symbol, intent, tokens_served, items_included, total_us)
             VALUES ('c1', 1000, 's1', '/src/lib.rs', 'main', 'Read', 0, 5, 1000)",
            [],
        ).unwrap();

        let signals = detect_empty_capsule(&meta_conn, "s1");
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].kind, SignalKind::EmptyCapsule);
        assert_eq!(signals[0].file_path, Some("/src/lib.rs".to_string()));
    }

    #[test]
    fn test_detect_empty_capsule_fires_on_zero_items() {
        let meta_conn = setup_meta_db();

        meta_conn.execute(
            "INSERT INTO capsule_log (capsule_id, timestamp, session_id, file, symbol, intent, tokens_served, items_included, total_us)
             VALUES ('c1', 1000, 's1', '/src/lib.rs', 'main', 'Read', 100, 0, 1000)",
            [],
        ).unwrap();

        let signals = detect_empty_capsule(&meta_conn, "s1");
        assert_eq!(signals.len(), 1);
    }

    #[test]
    fn test_detect_empty_capsule_does_not_fire_on_valid_capsule() {
        let meta_conn = setup_meta_db();

        meta_conn.execute(
            "INSERT INTO capsule_log (capsule_id, timestamp, session_id, file, symbol, intent, tokens_served, items_included, total_us)
             VALUES ('c1', 1000, 's1', '/src/lib.rs', 'main', 'Read', 100, 5, 1000)",
            [],
        ).unwrap();

        let signals = detect_empty_capsule(&meta_conn, "s1");
        assert_eq!(signals.len(), 0);
    }

    #[test]
    fn test_detect_empty_capsule_multiple_empty() {
        let meta_conn = setup_meta_db();

        meta_conn.execute(
            "INSERT INTO capsule_log (capsule_id, timestamp, session_id, file, symbol, intent, tokens_served, items_included, total_us)
             VALUES ('c1', 1000, 's1', '/src/a.rs', 'main', 'Read', 0, 0, 1000)",
            [],
        ).unwrap();
        meta_conn.execute(
            "INSERT INTO capsule_log (capsule_id, timestamp, session_id, file, symbol, intent, tokens_served, items_included, total_us)
             VALUES ('c2', 1001, 's1', '/src/b.rs', 'helper', 'Read', 0, 0, 1000)",
            [],
        ).unwrap();

        let signals = detect_empty_capsule(&meta_conn, "s1");
        assert_eq!(signals.len(), 2);
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test memory::signals::compute::tests::test_detect_empty_capsule --lib
```

Expected: FAIL with "function `detect_empty_capsule` not found"

- [ ] **Step 3: Implement detect_empty_capsule**

Add to `compute.rs`:

```rust
/// Detect EMPTY_CAPSULE: capsules with zero tokens_served or zero items_included.
fn detect_empty_capsule(meta_conn: &Connection, session_id: &str) -> Vec<SignalRecord> {
    let mut stmt = match meta_conn.prepare(
        "SELECT capsule_id, file, tokens_served, items_included
         FROM capsule_log
         WHERE session_id = ?1 AND (tokens_served = 0 OR items_included = 0)",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "detect_empty_capsule: failed to prepare query");
            return Vec::new();
        }
    };

    let rows = match stmt.query_map(rusqlite::params![session_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<i64>>(2)?,
            row.get::<_, Option<i64>>(3)?,
        ))
    }) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "detect_empty_capsule: failed to query");
            return Vec::new();
        }
    };

    rows.filter_map(|r| r.ok())
        .map(|(capsule_id, file, tokens, items)| {
            let detail = match (tokens, items) {
                (Some(0), Some(0)) => "Zero tokens and zero items".to_string(),
                (Some(0), _) => "Zero tokens served".to_string(),
                (_, Some(0)) => "Zero items included".to_string(),
                _ => "Empty capsule".to_string(),
            };
            SignalRecord {
                kind: SignalKind::EmptyCapsule,
                node_id: None,
                file_path: Some(file),
                session_id: session_id.to_string(),
                detail,
                timestamp: None,
            }
        })
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test memory::signals::compute::tests::test_detect_empty_capsule --lib
```

Expected: All 4 empty capsule tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/memory/signals/compute.rs
git commit -m "feat: implement EMPTY_CAPSULE detection from capsule_log"
```

---

### Task 2.4: Implement detect_failed_search

**Files:**
- Modify: `src/memory/signals/compute.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)]` module in `compute.rs`:

```rust
    #[test]
    fn test_detect_failed_search_fires_at_threshold() {
        let conn = setup_branch_db();
        let now = now_secs();
        let config = SignalConfig::default();

        // Insert 3 failed searches (same normalized query, no results)
        // We simulate this with 'search' events that have no corresponding results
        // In practice, failed searches are tracked by the search endpoint returning 0 results
        // For post-hoc detection, we look for repeated grep/search events with no follow-up read
        // Since session_log doesn't have a 'failed_search' event type, we detect this pattern:
        // multiple grep events on the same query pattern with no subsequent read on matching files
        // For simplicity in this test, we use a heuristic: repeated grep events on the same pattern
        for i in 0..3 {
            conn.execute(
                "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
                 VALUES ('s1', 'grep', NULL, 'FooBar', ?1)",
                rusqlite::params![now - (i * 60)],
            ).unwrap();
        }

        let signals = detect_failed_search(&conn, "s1", &config);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].kind, SignalKind::FailedSearch);
    }

    #[test]
    fn test_detect_failed_search_does_not_fire_below_threshold() {
        let conn = setup_branch_db();
        let now = now_secs();
        let config = SignalConfig::default();

        for i in 0..2 {
            conn.execute(
                "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
                 VALUES ('s1', 'grep', NULL, 'FooBar', ?1)",
                rusqlite::params![now - (i * 60)],
            ).unwrap();
        }

        let signals = detect_failed_search(&conn, "s1", &config);
        assert_eq!(signals.len(), 0);
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test memory::signals::compute::tests::test_detect_failed_search --lib
```

Expected: FAIL with "function `detect_failed_search` not found"

- [ ] **Step 3: Implement detect_failed_search**

Add to `compute.rs`:

```rust
/// Detect FAILED_SEARCH: repeated grep events on the same symbol with no follow-up read.
///
/// Post-hoc heuristic: if the same symbol is grepped N+ times in a session
/// with no intervening read event, the search likely failed repeatedly.
fn detect_failed_search(
    conn: &Connection,
    session_id: &str,
    config: &SignalConfig,
) -> Vec<SignalRecord> {
    // Find symbols that were grepped N+ times with no read events in the session
    let mut stmt = match conn.prepare(
        "SELECT symbol, COUNT(*) as grep_count
         FROM session_log
         WHERE session_id = ?1 AND event_type = 'grep' AND symbol IS NOT NULL
         GROUP BY symbol
         HAVING grep_count >= ?2
         AND symbol NOT IN (
             SELECT DISTINCT symbol FROM session_log
             WHERE session_id = ?1 AND event_type = 'read' AND symbol IS NOT NULL
         )",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "detect_failed_search: failed to prepare query");
            return Vec::new();
        }
    };

    let rows = match stmt.query_map(
        rusqlite::params![session_id, config.failed_search_threshold],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
            ))
        },
    ) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "detect_failed_search: failed to query");
            return Vec::new();
        }
    };

    rows.filter_map(|r| r.ok())
        .map(|(symbol, count)| SignalRecord {
            kind: SignalKind::FailedSearch,
            node_id: None,
            file_path: None,
            session_id: session_id.to_string(),
            detail: format!("Symbol \"{}\" grepped {} times with no read", symbol, count),
            timestamp: None,
        })
        .collect()
}
```

- [ ] **Step 4: Run tests to verify they pass**

```bash
cargo test memory::signals::compute::tests::test_detect_failed_search --lib
```

Expected: All 2 failed search tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/memory/signals/compute.rs
git commit -m "feat: implement FAILED_SEARCH detection from repeated grep patterns"
```

---

### Task 2.5: Test compute_signals_for_session integration

**Files:**
- Modify: `src/memory/signals/compute.rs`

- [ ] **Step 1: Write integration test**

Add to the `#[cfg(test)]` module in `compute.rs`:

```rust
    #[test]
    fn test_compute_signals_for_session_detects_multiple() {
        let branch_conn = setup_branch_db();
        let meta_conn = setup_meta_db();
        let now = now_secs();
        let config = SignalConfig::default();

        // Set up THRASHING: 5 edits to same file in 5min
        for i in 0..5 {
            branch_conn.execute(
                "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
                 VALUES ('s1', 'edit', '/src/lib.rs', NULL, ?1)",
                rusqlite::params![now - (i * 30)],
            ).unwrap();
        }

        // Set up EMPTY_CAPSULE: zero tokens
        meta_conn.execute(
            "INSERT INTO capsule_log (capsule_id, timestamp, session_id, file, symbol, intent, tokens_served, items_included, total_us)
             VALUES ('c1', 1000, 's1', '/src/lib.rs', 'main', 'Read', 0, 5, 1000)",
            [],
        ).unwrap();

        let signals = compute_signals_for_session(&branch_conn, &meta_conn, "s1", &config);
        assert_eq!(signals.len(), 2);

        let kinds: Vec<_> = signals.iter().map(|s| s.kind).collect();
        assert!(kinds.contains(&SignalKind::Thrashing));
        assert!(kinds.contains(&SignalKind::EmptyCapsule));
    }

    #[test]
    fn test_compute_signals_for_session_empty() {
        let branch_conn = setup_branch_db();
        let meta_conn = setup_meta_db();
        let config = SignalConfig::default();

        let signals = compute_signals_for_session(&branch_conn, &meta_conn, "empty_session", &config);
        assert_eq!(signals.len(), 0);
    }
```

- [ ] **Step 2: Run tests**

```bash
cargo test memory::signals::compute::tests::test_compute_signals_for_session --lib
```

Expected: Both tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/memory/signals/compute.rs
git commit -m "test: add integration test for compute_signals_for_session"
```

---

## Task 3: Insights Signal Computation

### Task 3.1: Implement detect_churn

**Files:**
- Modify: `src/memory/signals/compute.rs`

- [ ] **Step 1: Add churn fields to SignalConfig**

Update the `SignalConfig` struct:

```rust
pub struct SignalConfig {
    pub thrashing_edit_threshold: u32,
    pub thrashing_window_seconds: u64,
    pub failed_search_threshold: u32,
    pub churn_session_threshold: u32,
    pub churn_window_weeks: u64,
    pub compute_window_minutes: u64,
}
```

Update `Default`:

```rust
impl Default for SignalConfig {
    fn default() -> Self {
        Self {
            thrashing_edit_threshold: 5,
            thrashing_window_seconds: 300,
            failed_search_threshold: 3,
            churn_session_threshold: 3,
            churn_window_weeks: 3,
            compute_window_minutes: 15,
        }
    }
}
```

- [ ] **Step 2: Write the failing tests**

Add to the `#[cfg(test)]` module in `compute.rs`:

```rust
    #[test]
    fn test_detect_churn_fires_at_threshold() {
        let conn = setup_branch_db();
        let now = now_secs();
        let config = SignalConfig::default();

        // File edited in 3 distinct sessions over 3 weeks
        let week = 7 * 24 * 3600;
        conn.execute(
            "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
             VALUES ('s1', 'edit', '/src/lib.rs', NULL, ?1)",
            rusqlite::params![now],
        ).unwrap();
        conn.execute(
            "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
             VALUES ('s2', 'edit', '/src/lib.rs', NULL, ?1)",
            rusqlite::params![now - week as i64],
        ).unwrap();
        conn.execute(
            "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
             VALUES ('s3', 'edit', '/src/lib.rs', NULL, ?1)",
            rusqlite::params![now - (2 * week) as i64],
        ).unwrap();

        let signals = detect_churn(&conn, &config);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].kind, SignalKind::Churn);
        assert_eq!(signals[0].file_path, Some("/src/lib.rs".to_string()));
        assert!(signals[0].detail.contains("3 sessions"));
    }

    #[test]
    fn test_detect_churn_does_not_fire_below_threshold() {
        let conn = setup_branch_db();
        let now = now_secs();
        let config = SignalConfig::default();

        // Only 2 sessions (below threshold of 3)
        let week = 7 * 24 * 3600;
        conn.execute(
            "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
             VALUES ('s1', 'edit', '/src/lib.rs', NULL, ?1)",
            rusqlite::params![now],
        ).unwrap();
        conn.execute(
            "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
             VALUES ('s2', 'edit', '/src/lib.rs', NULL, ?1)",
            rusqlite::params![now - week as i64],
        ).unwrap();

        let signals = detect_churn(&conn, &config);
        assert_eq!(signals.len(), 0);
    }
```

- [ ] **Step 3: Run test to verify it fails**

```bash
cargo test memory::signals::compute::tests::test_detect_churn --lib
```

Expected: FAIL with "function `detect_churn` not found"

- [ ] **Step 4: Implement detect_churn**

Add to `compute.rs`:

```rust
/// Detect CHURN: files edited in N+ distinct sessions over a rolling window.
fn detect_churn(conn: &Connection, config: &SignalConfig) -> Vec<SignalRecord> {
    let window_start = now_secs() - (config.churn_window_weeks as i64 * 7 * 24 * 3600);

    let mut stmt = match conn.prepare(
        "SELECT file_path, COUNT(DISTINCT session_id) as session_count
         FROM session_log
         WHERE event_type = 'edit' AND file_path IS NOT NULL AND timestamp >= ?1
         GROUP BY file_path
         HAVING session_count >= ?2",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "detect_churn: failed to prepare query");
            return Vec::new();
        }
    };

    let rows = match stmt.query_map(
        rusqlite::params![window_start, config.churn_session_threshold],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, u32>(1)?,
            ))
        },
    ) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "detect_churn: failed to query");
            return Vec::new();
        }
    };

    let now = now_secs();
    rows.filter_map(|r| r.ok())
        .map(|(file_path, count)| SignalRecord {
            kind: SignalKind::Churn,
            node_id: None,
            file_path: Some(file_path),
            session_id: String::new(), // CHURN is cross-session, not per-session
            detail: format!("Edited in {} sessions over {} weeks", count, config.churn_window_weeks),
            timestamp: Some(now),
        })
        .collect()
}
```

- [ ] **Step 6: Run tests**

```bash
cargo test memory::signals::compute::tests::test_detect_churn --lib
```

Expected: Both churn tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/memory/signals/compute.rs
git commit -m "feat: implement CHURN detection across sessions"
```

---

### Task 3.2: Implement detect_hotspot

**Files:**
- Modify: `src/memory/signals/compute.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `#[cfg(test)]` module in `compute.rs`:

```rust
    #[test]
    fn test_detect_hotspot_fires_on_high_churn_and_centrality() {
        let conn = setup_branch_db();
        let config = SignalConfig::default();

        // Create two files: one with high centrality, one with low
        conn.execute(
            "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum, centrality)
             VALUES ('n1', 'Function', 'high_central', '/src/important.rs', 1, 10, 'fn high_central()', 'hash1', 'fn high_central()', X'01', 0.9)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum, centrality)
             VALUES ('n2', 'Function', 'low_central', '/src/minor.rs', 1, 10, 'fn low_central()', 'hash2', 'fn low_central()', X'02', 0.1)",
            [],
        ).unwrap();

        // Both files edited in 3 sessions (above churn threshold)
        let now = now_secs();
        let week = 7 * 24 * 3600;
        for session in &["s1", "s2", "s3"] {
            conn.execute(
                "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
                 VALUES (?1, 'edit', '/src/important.rs', NULL, ?2)",
                rusqlite::params![session, now],
            ).unwrap();
            conn.execute(
                "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
                 VALUES (?1, 'edit', '/src/minor.rs', NULL, ?2)",
                rusqlite::params![session, now],
            ).unwrap();
        }

        let signals = detect_hotspot(&conn, &config);
        // Only the high centrality file should be a hotspot
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].file_path, Some("/src/important.rs".to_string()));
        assert_eq!(signals[0].kind, SignalKind::Hotspot);
    }

    #[test]
    fn test_detect_hotspot_no_hotspot_when_below_median() {
        let conn = setup_branch_db();
        let config = SignalConfig::default();

        // Single file with low centrality
        conn.execute(
            "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum, centrality)
             VALUES ('n1', 'Function', 'low', '/src/low.rs', 1, 10, 'fn low()', 'hash1', 'fn low()', X'01', 0.1)",
            [],
        ).unwrap();

        // Edited in 3 sessions
        let now = now_secs();
        for session in &["s1", "s2", "s3"] {
            conn.execute(
                "INSERT INTO session_log (session_id, event_type, file_path, symbol, timestamp)
                 VALUES (?1, 'edit', '/src/low.rs', NULL, ?2)",
                rusqlite::params![session, now],
            ).unwrap();
        }

        let signals = detect_hotspot(&conn, &config);
        assert_eq!(signals.len(), 0);
    }
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cargo test memory::signals::compute::tests::test_detect_hotspot --lib
```

Expected: FAIL with "function `detect_hotspot` not found"

- [ ] **Step 3: Implement detect_hotspot**

Add to `compute.rs`:

```rust
/// Detect HOTSPOT: files with both high churn (above median) and high centrality (above median).
fn detect_hotspot(conn: &Connection, config: &SignalConfig) -> Vec<SignalRecord> {
    let window_start = now_secs() - (config.churn_window_weeks as i64 * 7 * 24 * 3600);

    // Get per-file churn counts
    let churn_counts: Vec<(String, u32)> = match conn
        .prepare(
            "SELECT file_path, COUNT(DISTINCT session_id) as session_count
             FROM session_log
             WHERE event_type = 'edit' AND file_path IS NOT NULL AND timestamp >= ?1
             GROUP BY file_path
             HAVING session_count >= ?2",
        )
        .ok()
        .and_then(|mut stmt| {
            stmt.query_map(rusqlite::params![window_start, config.churn_session_threshold], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
            })
            .ok()
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        }) {
        Some(c) => c,
        None => return Vec::new(),
    };

    if churn_counts.is_empty() {
        return Vec::new();
    }

    // Get per-file average centrality from nodes
    let centrality_map: std::collections::HashMap<String, f64> = match conn
        .prepare(
            "SELECT file_path, AVG(centrality) as avg_centrality
             FROM nodes
             GROUP BY file_path",
        )
        .ok()
        .and_then(|mut stmt| {
            stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
            })
            .ok()
            .map(|rows| rows.filter_map(|r| r.ok()).collect())
        }) {
        Some(c) => c,
        None => return Vec::new(),
    };

    if centrality_map.is_empty() {
        return Vec::new();
    }

    // Compute medians
    let mut churn_values: Vec<u32> = churn_counts.iter().map(|(_, c)| *c).collect();
    churn_values.sort();
    let churn_median = churn_values[churn_values.len() / 2];

    let mut centrality_values: Vec<f64> = centrality_map.values().copied().collect();
    centrality_values.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let centrality_median = centrality_values[centrality_values.len() / 2];

    // Files above both medians are hotspots
    let now = now_secs();
    churn_counts
        .into_iter()
        .filter(|(file, churn)| {
            let centrality = centrality_map.get(file).copied().unwrap_or(0.0);
            *churn >= churn_median && centrality >= centrality_median
        })
        .map(|(file_path, churn)| {
            let centrality = centrality_map.get(&file_path).copied().unwrap_or(0.0);
            SignalRecord {
                kind: SignalKind::Hotspot,
                node_id: None,
                file_path: Some(file_path),
                session_id: String::new(),
                detail: format!(
                    "Churn: {} sessions, Centrality: {:.3} (both above median)",
                    churn, centrality
                ),
                timestamp: Some(now),
            }
        })
        .collect()
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test memory::signals::compute::tests::test_detect_hotspot --lib
```

Expected: Both hotspot tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/memory/signals/compute.rs
git commit -m "feat: implement HOTSPOT detection (churn × centrality above median)"
```

---

### Task 3.3: Implement detect_large_blast_radius

**Files:**
- Modify: `src/memory/signals/compute.rs`

- [ ] **Step 1: Add blast radius fields to SignalConfig**

Update `SignalConfig`:

```rust
pub struct SignalConfig {
    pub thrashing_edit_threshold: u32,
    pub thrashing_window_seconds: u64,
    pub failed_search_threshold: u32,
    pub churn_session_threshold: u32,
    pub churn_window_weeks: u64,
    pub large_blast_radius_direct: u32,
    pub large_blast_radius_transitive: u32,
    pub compute_window_minutes: u64,
}
```

Update `Default`:

```rust
            large_blast_radius_direct: 20,
            large_blast_radius_transitive: 50,
```

- [ ] **Step 2: Write the failing tests**

Add to the `#[cfg(test)]` module in `compute.rs`:

```rust
    #[test]
    fn test_detect_large_blast_radius_fires_on_high_direct_count() {
        let conn = setup_branch_db();
        let config = SignalConfig::default();

        // Create a central node
        conn.execute(
            "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum, centrality)
             VALUES ('central', 'Function', 'central_fn', '/src/central.rs', 1, 10, 'fn central()', 'hash0', 'fn central()', X'00', 0.5)",
            [],
        ).unwrap();

        // Create 21 callers (above threshold of 20)
        for i in 0..21 {
            let node_id = format!("caller_{}", i);
            conn.execute(
                "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum, centrality)
                 VALUES (?1, 'Function', ?2, ?3, 1, 10, 'fn caller()', ?4, 'fn caller()', X'01', 0.1)",
                rusqlite::params![node_id, format!("caller_{}", i), format!("/src/caller_{}.rs", i), format!("hash{}", i)],
            ).unwrap();
            conn.execute(
                "INSERT INTO edges (from_id, to_id, kind, weight, confidence)
                 VALUES (?1, 'central', 'calls', 1.0, 'precise')",
                rusqlite::params![node_id],
            ).unwrap();
        }

        let signals = detect_large_blast_radius(&conn, &config);
        assert_eq!(signals.len(), 1);
        assert_eq!(signals[0].kind, SignalKind::LargeBlastRadius);
        assert_eq!(signals[0].node_id, Some("central".to_string()));
    }

    #[test]
    fn test_detect_large_blast_radius_does_not_fire_below_threshold() {
        let conn = setup_branch_db();
        let config = SignalConfig::default();

        conn.execute(
            "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum, centrality)
             VALUES ('central', 'Function', 'central_fn', '/src/central.rs', 1, 10, 'fn central()', 'hash0', 'fn central()', X'00', 0.5)",
            [],
        ).unwrap();

        // Only 5 callers (below threshold of 20)
        for i in 0..5 {
            let node_id = format!("caller_{}", i);
            conn.execute(
                "INSERT INTO nodes (id, kind, name, file_path, line_start, line_end, signature, signature_hash, skeleton, checksum, centrality)
                 VALUES (?1, 'Function', ?2, ?3, 1, 10, 'fn caller()', ?4, 'fn caller()', X'01', 0.1)",
                rusqlite::params![node_id, format!("caller_{}", i), format!("/src/caller_{}.rs", i), format!("hash{}", i)],
            ).unwrap();
            conn.execute(
                "INSERT INTO edges (from_id, to_id, kind, weight, confidence)
                 VALUES (?1, 'central', 'calls', 1.0, 'precise')",
                rusqlite::params![node_id],
            ).unwrap();
        }

        let signals = detect_large_blast_radius(&conn, &config);
        assert_eq!(signals.len(), 0);
    }
```

- [ ] **Step 3: Run test to verify it fails**

```bash
cargo test memory::signals::compute::tests::test_detect_large_blast_radius --lib
```

Expected: FAIL with "function `detect_large_blast_radius` not found"

- [ ] **Step 4: Implement detect_large_blast_radius**

Add to `compute.rs`:

```rust
/// Detect LARGE_BLAST_RADIUS: nodes with >N direct or >M transitive dependents.
fn detect_large_blast_radius(conn: &Connection, config: &SignalConfig) -> Vec<SignalRecord> {
    let mut stmt = match conn.prepare(
        "SELECT to_id as node_id, COUNT(*) as direct_count
         FROM edges
         GROUP BY to_id
         HAVING direct_count > ?1",
    ) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "detect_large_blast_radius: failed to prepare query");
            return Vec::new();
        }
    };

    let rows = match stmt.query_map(rusqlite::params![config.large_blast_radius_direct], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, u32>(1)?,
        ))
    }) {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "detect_large_blast_radius: failed to query");
            return Vec::new();
        }
    };

    let now = now_secs();
    rows.filter_map(|r| r.ok())
        .map(|(node_id, count)| SignalRecord {
            kind: SignalKind::LargeBlastRadius,
            node_id: Some(node_id),
            file_path: None,
            session_id: String::new(),
            detail: format!("{} direct dependents", count),
            timestamp: Some(now),
        })
        .collect()
}
```

Note: Transitive detection via SQL is complex and expensive. The direct count covers the most common case. Transitive detection can be added later if needed.

- [ ] **Step 5: Run tests**

```bash
cargo test memory::signals::compute::tests::test_detect_large_blast_radius --lib
```

Expected: Both tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/memory/signals/compute.rs
git commit -m "feat: implement LARGE_BLAST_RADIUS detection from graph edges"
```

---

### Task 3.4: Integrate Insights signals into maintenance loop

**Files:**
- Modify: `src/memory/signals/compute.rs`
- Modify: `src/memory/mod.rs`

- [ ] **Step 1: Add Insights signals to compute_signals_for_session**

Update `compute_signals_for_session` in `compute.rs`:

```rust
pub fn compute_signals_for_session(
    branch_conn: &Connection,
    meta_conn: &Connection,
    session_id: &str,
    config: &SignalConfig,
) -> Vec<SignalRecord> {
    let mut signals = Vec::new();

    // Improvement signals (per-session)
    signals.extend(detect_thrashing(branch_conn, session_id, config));
    signals.extend(detect_empty_capsule(meta_conn, session_id));
    signals.extend(detect_failed_search(branch_conn, session_id, config));

    // Insights signals (per-session where applicable)
    signals.extend(detect_large_blast_radius(branch_conn, session_id, config));

    signals
}
```

- [ ] **Step 2: Add compute_insights_signals function**

Add to `compute.rs`:

```rust
/// Compute cross-session Insights signals (CHURN, HOTSPOT).
/// These are not per-session but computed across all recent data.
pub fn compute_insights_signals(
    branch_conn: &Connection,
    config: &SignalConfig,
) -> Vec<SignalRecord> {
    let mut signals = Vec::new();
    signals.extend(detect_churn(branch_conn, config));
    signals.extend(detect_hotspot(branch_conn, config));
    signals
}
```

- [ ] **Step 3: Update maintenance() in mod.rs**

Replace the existing `maintenance` method:

```rust
    /// Periodic maintenance: prune expired signals and sessions, clean orphan annotations,
    /// and compute post-hoc signals for recent sessions.
    pub fn maintenance(&self, branch_conn: &Connection, meta_conn: &Connection) {
        let _ = signals::prune_expired(branch_conn);
        let _ = session::prune_expired(branch_conn);
        let _ = annotations::cleanup_orphans(branch_conn);

        // Compute signals for recent sessions
        let config = compute::SignalConfig::default();
        let session_signals = compute::compute_signals_for_recent_sessions(
            branch_conn, meta_conn, &config,
        );
        for (session_id, sigs) in session_signals {
            for signal in sigs {
                let _ = signals::insert_signal(
                    branch_conn,
                    signal.kind,
                    signal.node_id.as_deref(),
                    signal.file_path.as_deref(),
                    &session_id,
                    &signal.detail,
                );
            }
        }

        // Compute cross-session insights signals
        let insights = compute::compute_insights_signals(branch_conn, &config);
        for signal in insights {
            let _ = signals::insert_signal(
                branch_conn,
                signal.kind,
                signal.node_id.as_deref(),
                signal.file_path.as_deref(),
                &signal.session_id,
                &signal.detail,
            );
        }
    }
```

- [ ] **Step 4: Update maintenance() call sites**

Search for calls to `maintenance()` and update to pass both connections. Check:

```bash
rg "maintenance\(" src/
```

Update any call sites that only pass one connection. The daemon coordinator likely calls this.

- [ ] **Step 5: Run cargo check**

```bash
cargo check --lib
```

Expected: Compiles successfully.

- [ ] **Step 6: Run all tests**

```bash
cargo test --lib
```

Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/memory/signals/compute.rs src/memory/mod.rs
git commit -m "feat: integrate signal computation into maintenance loop"
```

---

## Task 4: Triage Function

### Task 4.1: Create triage.rs module

**Files:**
- Create: `src/memory/signals/triage.rs`
- Modify: `src/memory/signals.rs` (add mod declaration)

- [ ] **Step 1: Add mod declaration in signals.rs**

Add after `pub mod compute;`:

```rust
pub mod triage;
```

- [ ] **Step 2: Create triage.rs**

Create `src/memory/signals/triage.rs`:

```rust
use rusqlite::Connection;

use super::SignalKind;
use super::compute::SignalConfig;

/// Summary of signals for a session.
#[derive(Debug, Clone)]
pub struct SignalSummary {
    pub kind: SignalKind,
    pub count: u32,
    pub entity: String,
}

/// Triage result for a single session.
#[derive(Debug, Clone)]
pub struct SessionTriage {
    pub session_id: String,
    pub score: f64,
    pub signals: Vec<SignalSummary>,
}

/// Rank sessions by informativeness using composite scoring.
///
/// Scoring methodology (following the paper's composite triage approach):
/// - Improvement signals: base weight 2.0 per signal instance
/// - Insights signals: base weight 1.0 per signal instance
/// - Sessions ranked descending by score
pub fn triage_sessions(
    branch_conn: &Connection,
    meta_conn: &Connection,
) -> Vec<SessionTriage> {
    // Get all sessions with signals
    let sessions: Vec<String> = match branch_conn
        .prepare("SELECT DISTINCT session_id FROM behavioral_signals")
        .ok()
        .and_then(|mut stmt| {
            stmt.query_map([], |row| row.get(0))
                .ok()
                .map(|rows| rows.filter_map(|r| r.ok()).collect())
        }) {
        Some(s) => s,
        None => return Vec::new(),
    };

    if sessions.is_empty() {
        return Vec::new();
    }

    let config = SignalConfig::default();
    let mut results: Vec<SessionTriage> = sessions
        .into_iter()
        .map(|session_id| {
            let signals = compute_session_signals(branch_conn, &session_id);
            let score = compute_score(&signals);
            SessionTriage {
                session_id,
                score,
                signals,
            }
        })
        .collect();

    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results
}

fn compute_session_signals(conn: &Connection, session_id: &str) -> Vec<SignalSummary> {
    let mut stmt = match conn.prepare(
        "SELECT kind, COUNT(*) as count, COALESCE(node_id, file_path, 'session') as entity
         FROM behavioral_signals
         WHERE session_id = ?1
         GROUP BY kind, entity",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    match stmt.query_map(rusqlite::params![session_id], |row| {
        Ok(SignalSummary {
            kind: SignalKind::from_str(&row.get::<_, String>(0)?).unwrap_or(SignalKind::Thrashing),
            count: row.get(1)?,
            entity: row.get(2)?,
        })
    }) {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    }
}

fn compute_score(signals: &[SignalSummary]) -> f64 {
    signals
        .iter()
        .map(|s| {
            let base_weight = match s.kind.utility() {
                super::SignalUtility::Improvement => 2.0,
                super::SignalUtility::Insights => 1.0,
            };
            base_weight * s.count as f64
        })
        .sum()
}
```

- [ ] **Step 3: Write tests**

Add to `triage.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_branch_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_triage_ranks_by_score() {
        let conn = setup_db();

        // Session 1: 3 Improvement signals (score = 6.0)
        conn.execute(
            "INSERT INTO behavioral_signals (kind, session_id, timestamp) VALUES ('THRASHING', 's1', 1000)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO behavioral_signals (kind, session_id, timestamp) VALUES ('EMPTY_CAPSULE', 's1', 1001)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO behavioral_signals (kind, session_id, timestamp) VALUES ('FAILED_SEARCH', 's1', 1002)",
            [],
        ).unwrap();

        // Session 2: 2 Insights signals (score = 2.0)
        conn.execute(
            "INSERT INTO behavioral_signals (kind, session_id, timestamp) VALUES ('CHURN', 's2', 1000)",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO behavioral_signals (kind, session_id, timestamp) VALUES ('HOTSPOT', 's2', 1001)",
            [],
        ).unwrap();

        let results = triage_sessions(&conn, &setup_meta_db());
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].session_id, "s1");
        assert!((results[0].score - 6.0).abs() < 0.01);
        assert_eq!(results[1].session_id, "s2");
        assert!((results[1].score - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_triage_empty_when_no_signals() {
        let conn = setup_db();
        let results = triage_sessions(&conn, &setup_meta_db());
        assert_eq!(results.len(), 0);
    }

    fn setup_meta_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::db::schema::ensure_daemon_meta_schema(&conn).unwrap();
        conn
    }
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test memory::signals::triage --lib
```

Expected: All triage tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/memory/signals.rs src/memory/signals/triage.rs
git commit -m "feat: add triage module with composite scoring for session ranking"
```

---

## Task 5: Deprecate Real-Time Detector

### Task 5.1: Remove AntiPatternDetector from MemoryManager

**Files:**
- Modify: `src/memory/mod.rs`
- Delete: `src/memory/antipattern.rs`

- [ ] **Step 1: Remove detector from MemoryManager**

Update `mod.rs`:

```rust
pub mod annotations;
pub mod session;
pub mod signals;
pub mod versions;

use rusqlite::Connection;

use crate::graph::GraphState;
use crate::graph::index::ExtractedSymbol;
use signals::SignalKind;

/// Three-layer memory orchestrator.
///
/// Layer 1: Node version history (versions.rs)
/// Layer 2: Annotations with anchoring (annotations.rs)
/// Layer 3: Behavioral signals + session activity (signals.rs, session.rs)
pub struct MemoryManager;

impl MemoryManager {
    pub fn new() -> Self {
        Self
    }

    /// Called after a successful re-index of a file.
    /// Records new versions and checks annotation staleness.
    pub fn on_reindex(
        &self,
        conn: &Connection,
        graph: &GraphState,
        session_id: &str,
        symbols: &[ExtractedSymbol],
    ) {
        // Layer 1: Record versions
        let _ = versions::record_versions_batch(conn, symbols, Some(session_id));

        // Layer 2: Check staleness for each symbol
        for sym in symbols {
            let _ = annotations::detect_staleness_for_node(conn, &sym.id.0, &sym.checksum);
        }

        // Layer 3: Quality decay based on computed Improvement signals
        for sym in symbols {
            if let Ok(sigs) = signals::signals_for_node(conn, &sym.id.0, 5) {
                let has_improvement_signal = sigs.iter().any(|s| {
                    s.session_id == session_id
                        && SignalKind::from_str(&s.kind)
                            .map(|k| k.utility() == signals::SignalUtility::Improvement)
                            .unwrap_or(false)
                });
                if has_improvement_signal {
                    let _ = annotations::decay_quality_for_node(conn, &sym.id.0, 0.9);
                }
            }
        }
    }

    /// Periodic maintenance: prune expired signals and sessions, clean orphan annotations,
    /// and compute post-hoc signals for recent sessions.
    pub fn maintenance(&self, branch_conn: &Connection, meta_conn: &Connection) {
        let _ = signals::prune_expired(branch_conn);
        let _ = session::prune_expired(branch_conn);
        let _ = annotations::cleanup_orphans(branch_conn);

        // Compute signals for recent sessions
        let config = signals::compute::SignalConfig::default();
        let session_signals = signals::compute::compute_signals_for_recent_sessions(
            branch_conn, meta_conn, &config,
        );
        for (session_id, sigs) in session_signals {
            for signal in sigs {
                let _ = signals::insert_signal(
                    branch_conn,
                    signal.kind,
                    signal.node_id.as_deref(),
                    signal.file_path.as_deref(),
                    &session_id,
                    &signal.detail,
                );
            }
        }

        // Compute cross-session insights signals
        let insights = signals::compute::compute_insights_signals(branch_conn, &config);
        for signal in insights {
            let _ = signals::insert_signal(
                branch_conn,
                signal.kind,
                signal.node_id.as_deref(),
                signal.file_path.as_deref(),
                &signal.session_id,
                &signal.detail,
            );
        }
    }
```

Keep `fork_annotations` and `merge_annotations` as static methods (no `self` changes needed).

- [ ] **Step 2: Delete antipattern.rs**

```bash
rm src/memory/antipattern.rs
```

- [ ] **Step 3: Update mod.rs tests**

Remove the `use super::*;` import of antipattern if any. The existing tests in mod.rs don't reference the detector, so they should be fine.

- [ ] **Step 4: Run cargo check**

```bash
cargo check --lib
```

Fix any compilation errors from removed references to `antipattern`.

- [ ] **Step 5: Run all tests**

```bash
cargo test --lib
```

Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/memory/mod.rs
git rm src/memory/antipattern.rs
git commit -m "refactor: remove real-time AntiPatternDetector, use post-hoc signals"
```

---

### Task 5.2: Remove detector calls from handlers

**Files:**
- Modify: `src/daemon/handlers.rs`

- [ ] **Step 1: Search for any detector references in handlers**

```bash
rg "detector|record_search_miss|AntiPattern" src/daemon/handlers.rs
```

If any references exist, remove them. The `FAILED_SEARCH` signal is now computed post-hoc, so `record_search_miss` calls should be removed.

- [ ] **Step 2: Run cargo check**

```bash
cargo check --lib
```

- [ ] **Step 3: Run all tests**

```bash
cargo test --lib
```

- [ ] **Step 4: Commit**

```bash
git add src/daemon/handlers.rs
git commit -m "refactor: remove detector calls from handlers"
```

---

## Task 6: Meta-Relevance Assessment

### Task 6.1: Write the document

**Files:**
- Create: `.rdalot/research/signal-migration-relevance.md`

- [ ] **Step 1: Create the document**

Create `.rdalot/research/signal-migration-relevance.md`:

```markdown
# Meta-Relevance Assessment: Signal-Based Triage for Scavenger

## Paper Methodology Applied to Scavenger

The paper "Signals: Trajectory Sampling and Triage for Agentic Interactions" proposes using lightweight, deterministic signals to prioritize which agent trajectories deserve human review. Scavenger adapts this from agent trajectories to **coding sessions** — the unit of work where an AI agent interacts with a codebase through Scavenger's context-serving interface.

The paper's improvement loop is: signals → triage → review → improvement. For Scavenger:
1. **Signals** are computed post-hoc from session data (session_log, capsule_log, graph state)
2. **Triage** ranks sessions by composite informativeness score
3. **Review** means examining high-scoring sessions to understand why signals fired
4. **Improvement** means adjusting capsule assembly, annotation quality, or retrieval logic based on findings

## What "Informativeness Rate" Means for Scavenger

The paper measures informativeness as: "what percentage of triaged sessions contain actionable insights for improving the system?"

For Scavenger:
- An **informative session** is one where reviewing the signals leads to a concrete improvement in capsule assembly, annotation quality, or retrieval logic
- The **informativeness rate** = (informative sessions / reviewed sessions) × 100
- Target: >50% informativeness rate (the paper achieved 82% with signal-based sampling vs. 54% random)

Measurement approach:
1. Review top-N sessions by triage score
2. For each, determine if the signal led to an actionable finding
3. Track the ratio over time

## Signal Validation via capsule_log and Effectiveness

Scavenger already tracks capsule effectiveness (`effectiveness.rs`):
- `tokens_served`: how much context was provided
- `follow_up_reads/greps`: whether the agent needed additional context
- `edit_within_5min`: whether the agent acted on the provided context

Validation methodology:
1. **EMPTY_CAPSULE validation**: Sessions with EMPTY_CAPSULE signals should correlate with low effectiveness scores. If they don't, the signal definition needs adjustment.
2. **THRASHING validation**: Sessions with THRASHING should show high follow_up_reads (agent couldn't find what it needed). Correlate THRASHING with effectiveness score drops.
3. **FAILED_SEARCH validation**: Sessions with FAILED_SEARCH should show zero or low capsule hits for the searched symbols.

The effectiveness endpoint (`/effectiveness`) provides the data needed for this validation without additional instrumentation.

## Gap Analysis

### What Scavenger Has That the Paper Doesn't

**Structural layer**: Scavenger has unique access to the AST dependency graph, enabling signals the paper can't compute:
- CHURN: cross-session edit patterns (paper only has per-trajectory signals)
- HOTSPOT: combining edit frequency with graph centrality
- LARGE_BLAST_RADIUS: dependency impact analysis

These are Insights signals — they inform the user about their codebase, not Scavenger's own improvement.

### What the Paper Has That Scavenger Doesn't

**Discourse layer**: The paper's most valuable signals (Misalignment, Stagnation, Disengagement, Satisfaction) require visibility into user-agent natural language conversation. Scavenger sits below this layer — it serves context, it doesn't observe dialogue.

This is a fundamental limitation. Scavenger can only observe **execution-layer** signals (what the agent did with the context) and **structural signals** (what the codebase looks like). It cannot observe **discourse-layer** signals (what the agent and user said to each other).

### Implications

Scavenger's signal system is necessarily narrower than the paper's. However, the execution-layer signals are still valuable:
- They're cheaper to compute (no NLP, no conversation parsing)
- They're more directly actionable (edit patterns, capsule effectiveness, search results)
- They integrate with Scavenger's existing data model

The structural signals (CHURN, HOTSPOT, LARGE_BLAST_RADIUS) are Scavenger's unique contribution — they provide codebase-level insights that the paper's framework doesn't cover.
```

- [ ] **Step 2: Commit**

```bash
git add .rdalot/research/signal-migration-relevance.md
git commit -m "docs: add meta-relevance assessment for signal migration"
```

---

## Final Verification

- [ ] **Run full test suite**

```bash
cargo test --lib
```

Expected: All tests pass.

- [ ] **Run cargo clippy**

```bash
cargo clippy --lib -- -D warnings
```

Expected: No warnings.

- [ ] **Run cargo fmt**

```bash
cargo fmt --check
```

Expected: All code formatted.
