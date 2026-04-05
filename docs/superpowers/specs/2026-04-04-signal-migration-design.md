# Design: Signal Migration to Post-Hoc Computation

**Date:** 2026-04-04
**Status:** Draft
**Author:** Scavenger Team

## Summary

Migrate Scavenger's signal system from a broken real-time `AntiPatternDetector` to a post-hoc computation model aligned with the paper "Signals: Trajectory Sampling and Triage for Agentic Interactions" (arXiv:2604.00356). Signals are computed after session activity ends, then used to triage sessions for review and improvement.

## Architecture

### Signal Utility Classification

All signals are classified into exactly one utility category:

- **Improvement** — Signals that drive Scavenger's own improvement loop. Reviewing sessions with these signals leads to changes in capsule assembly, annotation quality, or retrieval logic.
- **Insights** — Signals that give the user actionable knowledge about their codebase. Future work will enable CLI commands to create annotations from these signals.

Classification is derived via `SignalKind::utility()` — a method on the enum, not stored state.

### Signal Taxonomy

#### Improvement Signals

| Signal | Data Source | Detection Logic | Threshold |
|--------|-------------|-----------------|-----------|
| **THRASHING** | `session_log` edit events | ≥5 edits to same `file_path` within 5min in a single session | Configurable, default 5 |
| **DEAD_END** | `nodes` + `edges` graph state | Nodes in edited files with zero non-test incoming edges after 10+ min session activity | Configurable, default 10 min |
| **EMPTY_CAPSULE** | `capsule_log` (daemon meta DB) | `tokens_served = 0` or `items_included = 0` | N/A |
| **FAILED_SEARCH** | `session_log` | Same normalized FTS query returning 0 results ≥3 times in a session | Configurable, default 3 |

#### Insights Signals

| Signal | Data Source | Detection Logic | Threshold |
|--------|-------------|-----------------|-----------|
| **CHURN** | `session_log` across sessions | File edited in N+ distinct sessions over rolling 3-week window | Configurable, default 3 sessions |
| **HOTSPOT** | CHURN × graph centrality | `churn_frequency × centrality` — files above median on both axes | Configurable |
| **CYCLE_INTRODUCED** | `nodes` + `edges` | New edge creates a back-path in the call graph | N/A |
| **LARGE_BLAST_RADIUS** | `nodes` + `edges` | >20 direct or >50 transitive dependents | Configurable |
| **UNTESTED** | `nodes` + `edges` | No test file references this symbol | N/A |
| **INDEX_BLIND_SPOT** | `nodes` + filesystem | File exists on disk but has zero indexed symbols | N/A |

### Computation Trigger

Signal computation runs in the daemon's periodic maintenance loop (`MemoryManager::maintenance()`). Default window: compute signals for all sessions with activity in the last **15 minutes**. This is configurable.

Additionally, a CLI command `scavenger signals compute` will allow on-demand computation for specific sessions or time windows.

### Dedup Strategy

`compute_signals_for_recent_sessions()` only computes signals for sessions that have at least one event in the `session_log` within the last 15 minutes AND do not already have any signals in `behavioral_signals` for that session_id. This prevents duplicate signal computation. If recomputation is needed, the existing signals for that session should be deleted first (handled by the on-demand CLI).

### Data Flow

```
session_log + capsule_log + nodes/edges
  → compute_signals_for_session()
    → behavioral_signals (persisted)
      → triage_score()
        → ranked sessions by informativeness
```

## Components

### 1. SignalKind Refactor (`memory/signals.rs`)

- Add 3 new variants: `EmptyCapsule`, `Churn`, `Hotspot`
- Add `SignalUtility` enum: `Improvement`, `Insights`
- Add `SignalKind::utility()` method returning the correct utility for each variant
- Update `as_str()` and `from_str()` with all new signal names
- Existing variants (`Thrashing`, `DeadEnd`, `CycleIntroduced`, `LargeBlastRadius`, `Untested`, `IndexBlindSpot`, `FailedSearch`) remain

### 2. Schema Migration (`db/schema.rs`)

- Increment `KNOWN_MAX_VERSION` from 3 to 4
- Add `migrate_v3_to_v4()` that extends the `behavioral_signals.kind` CHECK constraint to include `CHURN`, `HOTSPOT`, `EMPTY_CAPSULE`
- Since SQLite doesn't support `ALTER TABLE ... ADD CHECK`, the migration recreates the table with the updated constraint using the standard SQLite pattern: create new table → copy data → drop old → rename

### 3. Post-Hoc Computation Module (`memory/signals/compute.rs`)

New module with pure functions for each signal detection. No real-time state, no hooks, no `Instant` tracking.

**Public API:**

```rust
pub struct SignalRecord {
    pub kind: SignalKind,
    pub node_id: Option<String>,
    pub file_path: Option<String>,
    pub session_id: String,
    pub detail: String,
}

pub fn compute_signals_for_session(
    branch_conn: &Connection,
    meta_conn: &Connection,
    session_id: &str,
    config: &SignalConfig,
) -> Vec<SignalRecord>

pub fn compute_signals_for_recent_sessions(
    branch_conn: &Connection,
    meta_conn: &Connection,
    config: &SignalConfig,
) -> Vec<(String, Vec<SignalRecord>)>
```

**Signal detection functions** (private, each returns `Vec<SignalRecord>`):

```rust
fn detect_thrashing(conn: &Connection, session_id: &str, config: &SignalConfig) -> Vec<SignalRecord>
fn detect_dead_end(branch_conn: &Connection, session_id: &str, config: &SignalConfig) -> Vec<SignalRecord>
fn detect_empty_capsule(meta_conn: &Connection, session_id: &str) -> Vec<SignalRecord>
fn detect_failed_search(conn: &Connection, session_id: &str, config: &SignalConfig) -> Vec<SignalRecord>
fn detect_churn(conn: &Connection, config: &SignalConfig) -> Vec<SignalRecord>
fn detect_hotspot(conn: &Connection, config: &SignalConfig) -> Vec<SignalRecord>
fn detect_cycle_introduced(branch_conn: &Connection, session_id: &str) -> Vec<SignalRecord>
fn detect_large_blast_radius(branch_conn: &Connection, session_id: &str, config: &SignalConfig) -> Vec<SignalRecord>
fn detect_untested(branch_conn: &Connection, session_id: &str) -> Vec<SignalRecord>
fn detect_index_blind_spot(branch_conn: &Connection, session_id: &str) -> Vec<SignalRecord>
```

**Configuration** via a `SignalConfig` struct with sensible defaults:

```rust
pub struct SignalConfig {
    pub thrashing_edit_threshold: u32,       // default 5
    pub thrashing_window_seconds: u64,       // default 300 (5 min)
    pub dead_end_min_session_minutes: u64,   // default 10
    pub failed_search_threshold: u32,        // default 3
    pub churn_session_threshold: u32,        // default 3
    pub churn_window_weeks: u64,             // default 3
    pub large_blast_radius_direct: u32,      // default 20
    pub large_blast_radius_transitive: u32,  // default 50
    pub compute_window_minutes: u64,         // default 15
}

impl Default for SignalConfig { ... }
```

### 4. Triage Module (`memory/signals/triage.rs`)

Composite scoring to rank sessions by informativeness.

```rust
pub struct SessionTriage {
    pub session_id: String,
    pub score: f64,
    pub signals: Vec<SignalSummary>,
}

pub struct SignalSummary {
    pub kind: SignalKind,
    pub count: u32,
    pub entity: String,  // node_id or file_path
}

pub fn triage_sessions(
    branch_conn: &Connection,
    meta_conn: &Connection,
) -> Vec<SessionTriage>
```

**Scoring methodology** (following the paper's composite triage approach):

- Improvement signals: base weight 2.0 per signal instance
- Insights signals: base weight 1.0 per signal instance
- HOTSPOT compound: if a session has THRASHING on a file that is also flagged as HOTSPOT, multiply that signal's contribution by 1.5
- EMPTY_CAPSULE with effectiveness score 0.0: multiply by 1.3
- Final score = sum of all weighted signal contributions
- Sessions ranked descending by score

### 5. Deprecate AntiPatternDetector

- Remove `AntiPatternDetector` field from `MemoryManager`
- Remove `check_all()` calls from `on_reindex()` in `memory/mod.rs`
- Remove `DetectorContext` struct
- Delete `antipattern.rs` entirely
- Update quality decay logic: instead of checking signals in real-time during `on_reindex`, query computed Improvement signals from `behavioral_signals` table for the current session. If any Improvement signal exists for a node in the current session, decay annotation quality by 0.9 (same factor as before).
- Remove `record_search_miss()` from `handlers.rs` (FAILED_SEARCH moves to post-hoc)

### 6. Maintenance Integration

In `MemoryManager::maintenance()`:

```rust
pub fn maintenance(&self, branch_conn: &Connection, meta_conn: &Connection) {
    // Existing: prune expired signals and sessions
    let _ = signals::prune_expired(branch_conn);
    let _ = session::prune_expired(branch_conn);
    let _ = annotations::cleanup_orphans(branch_conn);

    // New: compute signals for recent sessions
    let config = SignalConfig::default();
    let session_signals = compute_signals_for_recent_sessions(
        branch_conn, meta_conn, &config,
    );
    for (session_id, signals) in session_signals {
        for signal in signals {
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
}
```

### 7. Insert Signal API Update

Update `signals::insert_signal()` to accept `SignalKind` directly instead of going through `as_str()`. The existing public API remains compatible.

## Error Handling

- All signal detection functions return `Vec<SignalRecord>` — empty on error (never panic)
- Individual detection failures are logged via `tracing::warn!` but don't block other signals
- Database errors during persistence are logged but don't crash the maintenance loop
- Triage gracefully handles sessions with no signals (score = 0.0)

## Testing Strategy

Each signal detection function has unit tests with synthetic data:

- **THRASHING**: Insert synthetic `session_log` edit events, verify detection at/above threshold, non-detection below
- **DEAD_END**: Insert synthetic `nodes` + `edges` with zero non-test callers, verify detection
- **EMPTY_CAPSULE**: Insert synthetic `capsule_log` entries with zero tokens, verify detection
- **FAILED_SEARCH**: Insert synthetic session events, verify threshold behavior
- **CHURN**: Insert synthetic `session_log` across multiple sessions, verify windowing
- **HOTSPOT**: Combine CHURN data with centrality values, verify above-median detection on both axes
- **CYCLE_INTRODUCED**: Create graph with back-path, verify detection
- **LARGE_BLAST_RADIUS**: Create node with >20 direct callers, verify detection
- **UNTESTED**: Create node with no test edges, verify detection
- **INDEX_BLIND_SPOT**: Create file on disk with no indexed nodes, verify detection
- **Triage**: Insert multiple sessions with varying signal densities, verify ranking order

## PR Decomposition

### PR 1: Schema Migration + SignalKind Refactor
- Extend `SignalKind` enum with `EmptyCapsule`, `Churn`, `Hotspot`
- Add `SignalUtility` enum and `SignalKind::utility()` method
- Update `as_str()` and `from_str()` 
- Schema v3 → v4 migration: extend CHECK constraint on `behavioral_signals.kind`
- Update `KNOWN_MAX_VERSION` to 4
- Files: `src/memory/signals.rs`, `src/db/schema.rs`

### PR 2: Improvement Signal Computation
- Create `memory/signals/compute.rs` module
- Implement `SignalConfig`, `SignalRecord`, `compute_signals_for_session()`, `compute_signals_for_recent_sessions()`
- Implement Improvement signal detectors: `detect_thrashing`, `detect_dead_end`, `detect_empty_capsule`, `detect_failed_search`
- Unit tests with synthetic data
- Files: `src/memory/signals/compute.rs` (new), `src/memory/signals/mod.rs` (extend), `src/memory/signals.rs` (extend insert_signal)

### PR 3: Insights Signal Computation
- Implement Insights signal detectors in `compute.rs`: `detect_churn`, `detect_hotspot`, `detect_cycle_introduced`, `detect_large_blast_radius`, `detect_untested`, `detect_index_blind_spot`
- Integrate with maintenance loop in `MemoryManager::maintenance()`
- Unit tests with synthetic data
- Files: `src/memory/signals/compute.rs` (extend), `src/memory/mod.rs` (update maintenance)

### PR 4: Triage Function
- Create `memory/signals/triage.rs` module
- Implement `SessionTriage`, `SignalSummary`, `triage_sessions()`
- Composite scoring with Improvement/Insights weighting
- Unit tests verifying ranking order
- Files: `src/memory/signals/triage.rs` (new), `src/memory/signals/mod.rs` (extend)

### PR 5: Deprecate Real-Time Detector
- Remove `AntiPatternDetector` from `MemoryManager`
- Remove `check_all()` calls from `on_reindex()`
- Delete `antipattern.rs`
- Update quality decay logic to query computed Improvement signals
- Remove `record_search_miss()` from handlers
- Files: `src/memory/antipattern.rs` (delete), `src/memory/mod.rs` (update), `src/daemon/handlers.rs` (update)

### PR 6: Meta-Relevance Assessment
- Write `.rdalot/research/signal-migration-relevance.md`
- Cover: paper methodology applied to Scavenger, informativeness rate definition, signal validation via capsule_log/effectiveness endpoints, gap analysis (discourse layer unavailable, structural layer unique)
- Files: `.rdalot/research/signal-migration-relevance.md` (new)

## Out of Scope

- Model-based signal detection
- Real-time capsule assembly changes based on signals
- Git-level churn analysis
- CLI annotation creation from Insights signals (future work)
- Changes to how signals are consumed in capsules

## Success Criteria

- THRASHING fires correctly on rapid edit patterns (unit test with synthetic session_log data)
- DEAD_END fires correctly on orphaned nodes (unit test with synthetic graph data)
- CHURN and HOTSPOT have well-specified computation formulas with configurable thresholds
- All signals have explicit Improvement vs Insights classification via `SignalKind::utility()`
- Triage function produces a ranked list of sessions by informativeness
- Real-time `AntiPatternDetector` is fully removed (no longer called from hooks)
- Schema migration adds new signal kinds to `behavioral_signals`
- Meta-relevance assessment document exists
- Signal computation runs automatically in maintenance loop with 15-minute window
