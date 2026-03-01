# Feature Specification: Scavenger v1 Core Engine

**Feature Branch**: `v1-core-engine`
**Workflow Mode**: spec
**Framework Options**: tdd=false, contracts=true, data_models=true, risk_tests=true
**Created**: 2026-02-28
**Status**: Clarified
**Input**: v1-core-engine — Full v1 implementation of Scavenger: AST dependency graph, capsule assembly, session memory, MCP server, daemon lifecycle, CLI, hooks integration

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Initialize Scavenger on a Project (Priority: P1)

A developer using Claude Code on a Rust/Python/TypeScript codebase runs `scavenger init` in their project root. The tool indexes all source files and documentation, creates the `.scavenger/` directory, registers hooks with Claude Code, and starts a background daemon. The developer sees a progress indicator during initial indexing and a confirmation when ready.

**Why this priority**: Without initialization, no other Scavenger functionality works. This is the entry point for every user.

**Independent Test**: Can be tested by running `scavenger init` in a sample multi-language project and verifying the `.scavenger/` directory structure, SQLite database contents, hook registration in `.claude/settings.local.json`, and a running daemon process.

**Acceptance Scenarios**:

1. **Given** a project directory with source code in supported languages, **When** the user runs `scavenger init`, **Then** a `.scavenger/` directory is created containing `daemon.lock`, `daemon.pid`, `daemon.sock`, `daemon_meta.db`, and `indexes/<branch>.db`, all source files are parsed and nodes/edges are stored in SQLite, `.scavenger/` is added to `.gitignore`, and the daemon is running in the background.
2. **Given** a project with existing `.claude/settings.local.json`, **When** `scavenger init` runs, **Then** Scavenger hooks and MCP bridge entries are deep-merged into the existing settings without overwriting other hooks.
3. **Given** a project already initialized, **When** the user runs `scavenger init` again, **Then** the system detects the existing `.scavenger/` directory and provides appropriate guidance rather than corrupting the existing index.

---

### User Story 2 - Automatic Capsule Injection on File Read (Priority: P1)

While using Claude Code, the developer asks Claude to read or understand a file. The PreToolUse hook intercepts the Read command, queries the daemon for a capsule, and injects focused context (symbol signatures, callers, callees, annotations, behavioral signals) into Claude's context window before the file is read. Claude receives a token-efficient view instead of just the raw file.

**Why this priority**: This is the core value proposition — reducing token usage automatically, without requiring any voluntary action from the user or Claude.

**Independent Test**: Can be tested by triggering a Read hook with a known file path and verifying the returned capsule contains the target symbol, its structural neighborhood, and any active annotations.

**Acceptance Scenarios**:

1. **Given** a running daemon with an indexed codebase, **When** Claude invokes the Read tool on a file containing a function `validateToken`, **Then** the PreToolUse hook returns a JSON response with `additionalContext` containing a capsule showing the target symbol, its callers, callees, and any annotations — all within the token budget.
2. **Given** the daemon is not running (crashed or stopped), **When** the hook fires, **Then** it exits with code 0 and returns an empty response, never blocking Claude Code.
3. **Given** capsule assembly takes longer than 100ms, **When** the hook fires, **Then** a partial capsule (pinned items only) is returned.

---

### User Story 3 - Incremental Re-indexing on Edits (Priority: P1)

As the developer edits code through Claude, the PostToolUse hook notifies the daemon about changed files. The daemon re-parses only the affected files, updates the graph, migrates annotations if symbols were renamed, and marks linked annotations stale if signatures changed — all within a 300ms debounce window.

**Why this priority**: Stale indexes produce wrong capsules. Incremental re-indexing keeps the index accurate as code evolves, which is essential for the capsule to provide value.

**Independent Test**: Can be tested by editing a file, waiting for the debounce window, and verifying that new/changed symbols appear in the graph and stale annotations are flagged.

**Acceptance Scenarios**:

1. **Given** an indexed file with function `getUserById`, **When** the function is renamed to `fetchUserById`, **Then** the daemon detects the NodeId change, runs the similarity heuristic, migrates annotations from the old symbol to the new one (score > 0.6), and creates a version history entry.
2. **Given** rapid edits to the same file within 300ms, **When** the debounce window closes, **Then** only one re-index operation runs for that file.
3. **Given** a file edit that changes a function's signature, **When** re-indexing completes, **Then** annotations anchored to that node are marked stale and version history records the change.

---

### User Story 4 - Session Memory Persistence (Priority: P2)

The developer uses Claude Code's MCP tools to write annotations about architectural decisions, discovered bugs, or learned patterns. These annotations are anchored to specific symbols, files, or project scopes. In the next session, the annotations are automatically surfaced through capsules when relevant code is queried, and a session summary is available on demand.

**Why this priority**: Memory persistence is what distinguishes Scavenger from a one-shot code indexer. Without it, the same discovery work repeats every session.

**Independent Test**: Can be tested by writing an annotation via the MCP tool, restarting the daemon (simulating a new session), and verifying the annotation appears in capsule output for the anchored symbol.

**Acceptance Scenarios**:

1. **Given** a running MCP bridge session, **When** Claude calls `write_annotation` with `symbol: "validateToken"` and `text: "Performance-critical — called on every request"`, **Then** the annotation is stored in the per-branch SQLite database, anchored to the resolved NodeId, and returned via `read_annotations`.
2. **Given** annotations exist from a previous session, **When** Claude calls `read_annotations` with `session_summary: true`, **Then** the response includes last session activity, stale annotations, and active behavioral signals.
3. **Given** an annotation anchored to a node whose NodeId changes due to a rename, **When** the similarity heuristic matches it (score > 0.6), **Then** the annotation is migrated to the new NodeId.

---

### User Story 5 - Branch-Aware Index Management (Priority: P2)

The developer switches git branches while working. The daemon detects the branch change, swaps to the corresponding per-branch index (or creates one on cold start by copying the parent and re-indexing changed files), and serves capsules with branch-specific context. Annotations, behavioral signals, and session logs are per-branch.

**Why this priority**: Developers frequently switch branches. Without branch awareness, the index would serve stale or wrong context from the wrong branch.

**Independent Test**: Can be tested by creating a new branch, making changes, switching back, and verifying that each branch has independent index state and capsule results.

**Acceptance Scenarios**:

1. **Given** the daemon is running on branch `main`, **When** the developer checks out `feature-x` which has an existing index, **Then** the daemon performs a warm switch: closes the `main.db`, opens `feature-x.db`, reloads the graph, runs a freshness check, and sets `reindex_state = 'ready'`.
2. **Given** the developer checks out a new branch with no existing index, **When** cold start triggers, **Then** the daemon copies the parent branch's DB, clears ephemeral data (versions, signals, session log), preserves annotations, re-indexes only the changed files (via `git diff`), and recomputes PageRank.
3. **Given** a merge commit is detected after branch switch, **When** two parent hashes are found, **Then** annotations from the source branch are union-merged (dedup same anchor + same text, keep both if different text).

---

### User Story 6 - Token Savings Analytics (Priority: P3)

The developer wants to understand Scavenger's value. Running `scavenger stats` shows per-session and all-time token savings — how many tokens were actually used versus how many would have been consumed without Scavenger — along with estimated cost savings.

**Why this priority**: Analytics demonstrate ROI and build confidence in the tool, but the core functionality works without them.

**Independent Test**: Can be tested by running several MCP tool calls, then running `scavenger stats` and verifying the output shows accurate per-tool token counts and savings calculations.

**Acceptance Scenarios**:

1. **Given** a session where 8 `get_capsule` calls returned 820 tokens total, **When** `scavenger stats` is run, **Then** the output shows the actual tokens used, estimated tokens without Scavenger, and percentage savings, formatted as a human-readable report.
2. **Given** multiple sessions recorded in `token_log`, **When** `scavenger stats --session all` is run, **Then** all-time savings are aggregated with cost estimates based on the configured price per million tokens.

---

### User Story 7 - Health Diagnostics (Priority: P3)

The developer suspects Scavenger is not working correctly. Running `scavenger doctor` performs comprehensive health checks across five categories (process, file integrity, config, dependencies, resources) and reports pass/warning/failure with actionable guidance.

**Why this priority**: Debugging tool issues is important for adoption but not core functionality.

**Independent Test**: Can be tested by running `scavenger doctor` in various states (healthy, daemon stopped, corrupted DB, missing hooks) and verifying accurate diagnostic output.

**Acceptance Scenarios**:

1. **Given** a healthy Scavenger installation, **When** `scavenger doctor` runs, **Then** all checks pass with green checkmarks.
2. **Given** the daemon is not running, **When** `scavenger doctor` runs, **Then** the process check fails with a clear error message and suggested fix.
3. **Given** `--format=json` flag, **When** `scavenger doctor` runs, **Then** output is structured JSON suitable for CI pipelines, with exit code 0 (all pass), 1 (warnings), or 2 (failures).

---

### User Story 8 - Federation (Cross-Repo Search) (Priority: P3)

The developer works on a project that depends on another local repository. After running `scavenger federate add /path/to/other-repo`, capsule assembly and doc search fan out to federated repos when local results are empty, returning context with a `[FEDERATED]` marker.

**Why this priority**: Federation extends Scavenger's value to multi-repo setups but is not needed for single-repo use.

**Independent Test**: Can be tested by federating two repos, querying a symbol that only exists in the federated repo, and verifying the capsule contains the federated result with the correct marker.

**Acceptance Scenarios**:

1. **Given** a federated repo configured in `.scavenger.toml`, **When** `get_capsule` finds no local match for a symbol, **Then** it searches the federated repo's `nodes_fts` and returns a minimal capsule with `[FEDERATED: /path/to/repo]` marker.
2. **Given** a federated repo is unreachable, **When** federation lookup fails, **Then** the failure is logged and the capsule is returned without federated results (fail open).
3. **Given** `scavenger federate verify` is run, **Then** all federated repos are checked for accessibility, active branch DB existence, and index freshness.

---

### Edge Cases

- What happens when the daemon receives a request during initial indexing? It serves degraded responses with indexing progress.
- How does the system handle corrupted SQLite databases? `PRAGMA integrity_check` in `scavenger doctor`; the index is a rebuildable cache — delete and re-init.
- What happens during rapid branch switching? VCS deferral pauses event processing until `.git/index.lock` is released; only the final branch state is indexed.
- How does the system handle files with unsupported languages? They are skipped silently during indexing; the `files` table still records them for token estimation.
- What happens when disk space runs out? `SQLITE_FULL` triggers degraded read-only mode; `scavenger doctor` detects the condition.
- How does the system handle detached HEAD? Uses commit hash (first 12 chars) as the index filename.
- What happens when git worktrees are used? Each worktree gets its own independent `.scavenger/` directory.
- What happens when NodeId collisions occur (nested functions with identical signatures)? Known v1 limitation; deferred to v2 (parent-scope NodeId).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST parse source code in 15 languages (TypeScript, JavaScript, TSX/JSX, Python, Go, Rust, Java, C#, C, C++, Ruby, Bash, Kotlin, PHP, Swift) using tree-sitter grammars and extract symbols (functions, methods, classes, interfaces, types, enums, exported variables, modules) with their signatures, docstrings, and relationships.
- **FR-002**: System MUST build and maintain an in-memory directed graph of code symbols with seven edge types (Imports, Calls, TypeRef, Extends, Implements, Exports, Contains) and persist it in per-branch SQLite databases.
- **FR-003**: System MUST assemble focused context capsules via a 6-stage pipeline (GATHER, SCORE, PIN, TRIM, GROUP, RENDER) that fits within a configurable token budget (default 8000 tokens).
- **FR-004**: System MUST expose five MCP tools (`get_capsule`, `read_annotations`, `write_annotation`, `delete_annotation`, `search_docs`) via an MCP bridge that translates stdio JSON-RPC to Unix domain socket requests.
- **FR-005**: System MUST intercept Claude Code's Read operations via PreToolUse hooks and inject capsule context as `additionalContext`, and intercept Write/Edit/MultiEdit operations via PostToolUse hooks to trigger re-indexing.
- **FR-006**: System MUST run as a persistent background daemon with a single Unix domain socket listener, supporting multiple concurrent Claude Code sessions via per-session MCP bridge shims.
- **FR-007**: System MUST maintain three layers of session memory: node version history (last 5 versions per symbol), semantic annotations (flexible anchoring to node/file/scope/project), and behavioral signals with session log (TTL: 48h or 2 sessions).
- **FR-008**: System MUST detect seven anti-patterns with per-session deduplication (fire-once-at-N per `HashSet<(SignalType, key)>`) and surface them as pinned items in capsules:
  - **THRASHING**: ≥3 edits to same node in 5-min window with Levenshtein similarity >0.9 between consecutive edits (dedup key: `node_id`)
  - **DEAD_END**: Zero incoming edges from non-test code after ≥10 session actions or 15 min; excludes test files and public API endpoints (dedup key: `node_id`)
  - **CYCLE_INTRODUCED**: Any new cycle detected via `has_path_connecting(graph, v, u)` before adding edge `(u,v)` (dedup key: `from_node::to_node`)
  - **LARGE_BLAST_RADIUS**: >20 direct dependents OR >50 transitive reachable nodes via forward BFS (dedup key: `node_id`)
  - **UNTESTED**: Zero edges from test-file nodes (path patterns: `*_test.rs`, `tests/*.rs`, `test_*.py`, `*.test.ts`, `__tests__/`) to the target (dedup key: `node_id`)
  - **INDEX_BLIND_SPOT**: Seed file exists on disk but has zero indexed nodes locally AND in federated repos; fires only after federation fallback (dedup key: `file_path`)
  - **FAILED_SEARCH**: Same normalized FTS5 query returns 0 results ≥3 times in a session (dedup key: `normalized_query`)
- **FR-009**: System MUST manage per-branch SQLite indexes with warm switch (existing index), cold start (copy parent + re-index diff), and annotation fork/merge on branch operations.
- **FR-010**: System MUST incrementally re-index changed files with 300ms trailing-edge debounce, VCS-aware deferral during git operations, and identity migration via a similarity heuristic (threshold 0.6) to preserve annotations across renames.
- **FR-011**: System MUST provide intent-driven query traversals (Debug, Refactor, Understand, Extend, Review) with BM25+centrality scoring (`0.6 × bm25 + 0.4 × centrality`) and configurable explosion mitigation (degree cap, node budget, builtins blocklist).
- **FR-012**: System MUST index markdown documentation into searchable chunks with heading-boundary splitting and make them available via FTS5 search and the `search_docs` MCP tool.
- **FR-013**: System MUST provide a CLI with commands: `init`, `daemon`, `index`, `capsule`, `memory`, `graph stats`, `graph show`, `annotate`, `merge-annotations`, `doctor`, `stats`, `federate` (add/remove/list/verify).
- **FR-014**: System MUST support federation — read-only cross-repo search by connecting to other repos' `.scavenger/` indexes and fanning out queries to federated repos' FTS5 indexes.
- **FR-015**: System MUST log token usage per tool call and provide a savings report comparing actual tokens used vs estimated tokens without Scavenger.
- **FR-016**: System MUST load configuration from a project-local `.scavenger.toml` file with validated defaults for all configurable values (token budget, degree cap, node budget, builtins blocklist, doc patterns, analytics pricing, federation repos). Invalid values MUST be clamped to the nearest valid bound with a logged warning. Missing config file MUST use built-in defaults. Config changes MUST take effect on daemon restart without requiring re-initialization.
- **FR-017**: System MUST version all SQLite schemas via `PRAGMA user_version` and run sequential forward migrations on startup. If a database was created by a newer Scavenger version (user_version exceeds known max), the system MUST refuse to open it with a clear error message directing the user to upgrade or rebuild.
- **FR-018**: Capsule output MUST follow a stable section ordering: `[!]` behavioral signals → `[TARGET]` → `[CALLERS]` → `[CALLEES]` → `[CONTEXT]` (annotations, history, session activity) → `[DOCUMENTATION]` (doc chunks) → `[BODY]` (if leftover budget > 200 tokens). Empty sections MUST be omitted. Stale annotations MUST display a `[STALE ⚠]` suffix. Scores MUST NOT appear in output.

### Key Entities

- **Node**: A code symbol (function, method, class, etc.) identified by `NodeId = hash(file_path, symbol_name, signature)`. Contains kind, name, file_path, line range, signature, docstring, skeleton, centrality score, and body checksum.
- **Edge**: A directed relationship between two nodes with type (Imports, Calls, TypeRef, Extends, Implements, Exports, Contains), weight, and confidence level (precise, heuristic, speculative).
- **Annotation**: An LLM-managed memory entry with flexible anchoring (node, file, scope, or project-level), text content, tags, staleness tracking, and timestamps.
- **Behavioral Signal**: An ephemeral diagnostic event (one of seven types) anchored to a node or file, with session context, TTL pruning, and fire-once-at-N deduplication.
- **Session Log Entry**: A lightweight activity record (read, query, edit) with session ID, file/symbol reference, and timestamp.
- **Token Log Entry**: A per-tool-call record in `daemon_meta.db` tracking actual tokens, estimated without-index tokens, tool name, query, intent, and branch.
- **Doc Chunk**: A markdown documentation segment split at heading boundaries, with content hash for incremental updates.
- **Capsule**: The assembled output of the 6-stage pipeline — a ranked, budget-constrained view of context items grouped by relationship type.

### Non-Functional Requirements

- **NFR-001**: PreToolUse hook latency MUST be under 50ms total; partial capsule fallback MUST trigger at 100ms.
- **NFR-002**: System MUST ship as a single statically-linked binary with zero runtime dependencies — no cloud calls, no external APIs, no network egress.
- **NFR-003**: Initial indexing of a 5000-file project MUST complete within 5 seconds using parallel parsing via rayon.
- **NFR-004**: Incremental re-indexing of a single file (tree-sitter re-parse + graph update) MUST complete within 50ms.
- **NFR-005**: Daemon MUST support multiple concurrent Claude Code sessions connecting via separate MCP bridge instances to a single Unix domain socket.
- **NFR-006**: System MUST fail open on all error paths — hooks exit 0, MCP bridge degrades gracefully, Claude Code is never blocked or degraded by Scavenger failures.
- **NFR-007**: System MUST handle `SIGTERM`/`SIGINT` gracefully with request draining, write flushing, clean shutdown flag, and PID file cleanup within 5 seconds.
- **NFR-008**: SQLite databases MUST use WAL mode with configured PRAGMAs (`synchronous=NORMAL`, `busy_timeout=5000`, `cache_size=-64000`, `mmap_size=268435456`) for concurrent read/write safety.
- **NFR-009**: CLI output MUST support both JSON (`--format=json`) and human-readable formats, respecting `NO_COLOR` environment variable for terminal coloring.
- **NFR-010**: System MUST be language-agnostic at launch, supporting 15 programming languages via tree-sitter grammars with no language-specific logic in the core pipeline.
- **NFR-011**: Daemon MUST write structured operational events (startup, shutdown, branch switch, re-index, errors) to `.scavenger/daemon.log`, rotated by size (default 10 MB, max 2 rotated files). `scavenger doctor` MUST parse this log for diagnostic insights.

### Quality Attributes

- **Security**: All data is local-only. No network calls. `.scavenger/` directory created with mode `0700`. Settings file writes use exclusive file locking (`fs2`).
- **Performance**: Sub-50ms hook latency. Sub-1ms file re-parse. 300ms debounce. Split-phase graph locking minimizes write lock duration to ~5-15ms.
- **Reliability**: SQLite WAL guarantees crash recovery. Dirty-flag triggers freshness scan on restart. The index is a rebuildable cache — delete `.scavenger/` and re-init to recover from any corruption.
- **Usability**: Single `scavenger init` command to get started. Automatic hook-based operation requires no user intervention. `scavenger doctor` provides actionable diagnostics.
- **Maintainability**: Modular architecture (daemon, bridge, graph, capsule, query, memory, hooks, db). Schema migration via `PRAGMA user_version`. Trait-based diagnostic check registry for extensibility.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Capsules reduce token consumption by 70%+ compared to raw file reads for typical symbol queries, as measured by the token estimator and reported via `scavenger stats`.
- **SC-002**: PreToolUse hook responds within 50ms for 95% of requests on a 5000-file codebase with warm caches.
- **SC-003**: Initial indexing of a 5000-file, 15-language codebase completes in under 5 seconds.
- **SC-004**: Annotations written in session N are automatically surfaced in session N+1 capsules when the anchored code is queried, with zero manual intervention.
- **SC-005**: Branch switching (warm) completes in under 500ms, including graph reload and freshness check.
- **SC-006**: The system runs continuously without memory leaks or unbounded resource growth, verified by 24-hour soak testing with simulated Claude Code sessions.
- **SC-007**: All seven anti-pattern detectors correctly identify their respective conditions with zero false negatives on synthetic test scenarios.
- **SC-008**: The complete system ships as a single binary under 50 MB, installable with zero external dependencies.

## Assumptions

- Claude Code v2.1.9+ is available, providing `additionalContext` in PreToolUse hook responses and `session_id` in hook payloads.
- The `rmcp` Rust MCP SDK stabilizes sufficiently for v1; if not, a hand-rolled JSON-RPC stdio implementation is the fallback.
- Users have git installed and projects are git repositories (required for branch detection and diff-based cold start).
- Rust stable toolchain (edition 2024, minimum 1.85+). The `rmcp` crate (v0.17.0+) works on stable Rust.
- FTS5 BM25 with default parameters (k1=1.2, b=0.75) provides acceptable ranking quality for code search; empirical validation during implementation may trigger a pivot to `tantivy`.

## Contracts

### MCP Tool Contracts

**`get_capsule`**: `(file: String, symbol?: String, query?: String) → CapsuleResult`
- Seed: resolves file + symbol to target node. Symbol omission → file's primary export.
- Pipeline: GATHER → SCORE → PIN → TRIM → GROUP → RENDER within token budget.
- Federation fallback: if local GATHER empty and federation configured → search federated repos.
- Error: `CallToolResult { isError: true }` if file not found.

**`read_annotations`**: `(anchor_type?: String, anchor_value?: String, tags?: String, query?: String, session_summary?: bool, limit?: u32) → Vec<AnnotationResult>`
- Filters: anchor, tags, FTS5 query — combinable.
- Session summary mode: returns last session activity + stale annotations + active signals.

**`write_annotation`**: `(id?: String, text: String, tags?: String, symbol?: String, file?: String, scope?: String) → AnnotationResult`
- Upsert: `id` provided → update; omitted → create.
- Anchor resolution cascade: symbol (FTS5 top-1) → file → scope → project-level.
- Disambiguation: multiple FTS5 matches within 20% of top score → return alternatives in `note` field.

**`delete_annotation`**: `(id: String) → DeleteResult`

**`search_docs`**: `(query: String, limit?: u32) → Vec<DocChunkResult>`
- FTS5 search over `doc_chunks_fts`. Fans out to federated repos.

### Hook Contracts

**PreToolUse (Read)**: stdin JSON `{ session_id, tool_name, tool_input }` → UDS to daemon → capsule → stdout `{ "additionalContext": "..." }`. Exit 0 always.

**PostToolUse (Write/Edit/MultiEdit)**: stdin JSON → UDS to daemon → enqueue re-index → stdout `{}`. Exit 0 always.

### Daemon UDS Protocol

Request/response over Unix domain socket. Length-prefixed JSON messages. Request types: `capsule`, `hook_pre`, `hook_post`, `annotation_read`, `annotation_write`, `annotation_delete`, `search_docs`, `status`.

## Data Models

### Per-Branch Index DB (`.scavenger/indexes/<branch>.db`)

Tables: `nodes`, `edges`, `files`, `node_versions`, `annotations`, `behavioral_signals`, `session_log`, `doc_chunks`
FTS5 virtual tables: `nodes_fts`, `annotations_fts`, `doc_chunks_fts` (all with content sync triggers)

### Shared DB (`daemon_meta.db`)

Tables: `daemon_meta` (key-value), `federated_repos`, `token_log`

### In-Memory Structures

- `GraphState`: `petgraph::StableGraph` + reverse index `HashMap<NodeId, Vec<PathBuf>>` wrapped in `Arc<parking_lot::RwLock<>>`
- Anti-pattern dedup: `HashSet<(SignalType, String)>` per session
- Pending orphans buffer: `HashSet<OrphanData>`
- Session state: current session_id, thrashing ring buffers, failed search counters

## Clarifications

### Session 2026-02-28

- **Rust toolchain (auto-fix)**: Corrected spec assumption from "nightly required by rmcp" to "stable toolchain (edition 2024, minimum 1.85+)". The `rmcp` crate v0.17.0 works on stable Rust; constitution was already correct. Design doc §9.1, §14, §15 contain outdated nightly references.
- **Configuration management (Q1 → FR-016)**: Added requirement for `.scavenger.toml` with validated defaults, range clamping, logged warnings for invalid values, and restart-based config reload.
- **Daemon operational logging (Q2 → NFR-011)**: Added requirement for structured daemon log at `.scavenger/daemon.log` with size rotation (10 MB, 2 rotated files), parseable by `scavenger doctor`.
- **Schema versioning (Q3 → FR-017)**: Added requirement for `PRAGMA user_version` schema migration on startup with downgrade guard that refuses incompatible databases.
- **Capsule output format (Q4 → FR-018)**: Added stable output contract specifying section ordering (`[!]` → `[TARGET]` → `[CALLERS]` → `[CALLEES]` → `[CONTEXT]` → `[DOCUMENTATION]` → `[BODY]`), empty section omission, `[STALE ⚠]` markers, and score suppression.
- **Anti-pattern thresholds (Q5 → FR-008 expanded)**: Added per-detector triggering conditions, thresholds, and dedup keys for all seven anti-pattern detectors.
