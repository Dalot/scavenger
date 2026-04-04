# Eval System Design

**Date:** April 4, 2026
**Status:** Draft
**Author:** AI Assistant

## 1. Problem Statement

Scavenger needs a rigorous evaluation system to verify that its core components work as intended, measure quality over time, and provide confidence that changes don't introduce regressions. The current state has:

- Integration tests with timing assertions (not proper benchmarks)
- A Python-based A/B benchmark (`benchmark/`) that requires Cursor CLI and is fragile
- No coverage measurement, no eval harness for capsule relevance or query accuracy
- No systematic way to measure whether Scavenger actually improves AI agent outcomes

## 2. Goals

1. **Component-level evals** (Tier 1) — Fast, deterministic, CI-safe measurements of capsule relevance, query accuracy, and performance
2. **Agent-level evals** (Tier 2) — Real-world measurement of whether Scavenger improves AI coding agent outcomes
3. **Good local DX** — CLI with clear help text, JSON output for scripting, summary tables for humans
4. **Transparency about failure modes** — Following PindeX's model, measure and report where Scavenger helps vs hurts
5. **Replace `benchmark/`** — The Python A/B benchmark is replaced by the Tier 2 agent eval system

## 3. Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                       scavenger eval                          │
│                    (new CLI subcommand)                       │
├──────────────────────────────────────────────────────────────┤
│                                                               │
│  TIER 1: Component Evals (fast, deterministic, CI-safe)      │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐       │
│  │  Relevance   │  │   Accuracy   │  │  Performance │       │
│  │    Eval      │  │     Eval     │  │   Benchmark  │       │
│  └──────────────┘  └──────────────┘  └──────────────┘       │
│                                                               │
│  TIER 2: Agent Evals (slower, requires LLM, real outcomes)   │
│  ┌──────────────────────────────────────────────────────┐    │
│  │                  Agent Eval Runner                    │    │
│  │  - Spins up real coding agent (Claude Code, Cursor)  │    │
│  │  - Runs task suite WITH Scavenger                    │    │
│  │  - Runs task suite WITHOUT Scavenger (baseline)      │    │
│  │  - Compares: tokens, tool calls, time, success rate  │    │
│  │  - Reports delta and crossover points                │    │
│  └──────────────────────────────────────────────────────┘    │
│                                                               │
│  ┌──────────────────────────────────────────────────────┐    │
│  │              JSON Reporter (both tiers)               │    │
│  │  - Structured output, summary table, exit codes      │    │
│  └──────────────────────────────────────────────────────┘    │
└──────────────────────────────────────────────────────────────┘
```

## 4. Directory Structure

```
eval/
  corpus/                    # Code to evaluate against
    fixtures/                # Small sample projects (fast iteration)
      sample_project/        # Symlink or copy of tests/fixtures/sample_project/
    repos/                   # Real repos cloned for eval (cached)
      .gitignore             # Ignore all except .keep
      .keep
  cases/                     # Eval case definitions
    relevance/               # Capsule relevance cases (TOML)
    accuracy/                # Query accuracy cases (TOML)
  tasks/                     # Agent eval task definitions (YAML)
  thresholds.toml            # Pass/fail thresholds per suite
  results/                   # Eval run outputs (JSON)
    .gitignore               # Ignore all except .keep
    .keep
src/
  bench/                     # Criterion benchmark files
    relevance_bench.rs
    accuracy_bench.rs
    performance_bench.rs
    mod.rs
  eval/                      # Eval framework
    runner.rs                # Orchestrates eval execution
    reporter.rs              # JSON + human-readable output
    corpus.rs                # Corpus loading and management
    thresholds.rs            # Threshold loading and checking
    relevance.rs             # Tier 1: relevance eval logic
    accuracy.rs              # Tier 1: accuracy eval logic
    agent/                   # Tier 2: agent eval runners
      mod.rs
      claude_runner.rs       # Claude Code adapter
      cursor_runner.rs       # Cursor adapter
      types.rs               # Shared agent eval types
      task.rs                # Task definition parsing
    mod.rs
```

## 5. Tier 1: Component Evals

### 5a. Relevance Eval

**Purpose:** Does the capsule contain the right symbols?

**Method:** Use known commit diffs from real repos as ground truth. For each eval case:
1. Index the repo at commit A
2. Query for a symbol that was modified in commit B
3. Check whether the capsule includes the symbols that were actually changed in that commit

**Metrics:**
- **Recall@K:** What fraction of actually-changed symbols appear in the top-K capsule results?
- **Precision@K:** What fraction of capsule contents were actually relevant?
- **F1:** Harmonic mean of recall and precision
- **Noise ratio:** Proportion of symbols included that have no relationship to the query

**Eval case format** (`eval/cases/relevance/*.toml`):
```toml
[[case]]
name = "mini-redis-process-message"
repo = "mini-redis"
query = "process_message"
expected_symbols = ["process_message", "Frame::parse", "Connection::read_frame"]
```

The `repo` field references a directory under `eval/corpus/repos/`. Initial eval cases will use the existing `tests/fixtures/sample_project/` as the corpus.

### 5b. Accuracy Eval

**Purpose:** Does query/search return the right results with good ranking?

**Method:** Golden dataset of queries with expected results. Each query has an intent type and expected top results.

**Metrics:**
- **Intent classification accuracy:** Does the query parser correctly classify the intent?
- **NDCG@K:** Normalized Discounted Cumulative Gain — measures ranking quality
- **MRR:** Mean Reciprocal Rank — how high is the first relevant result?
- **BM25 quality:** Does FTS5 ranking correlate with human judgment?

**Eval case format** (`eval/cases/accuracy/*.toml`):
```toml
[[case]]
name = "find-callers"
corpus = "sample_project"
query = "who calls process_message"
expected_intent = "find_callers"
expected_top_symbols = ["process_message", "handle_request"]
```

### 5c. Performance Benchmark

**Purpose:** How fast and efficient is Scavenger?

**Method:** Using `criterion` for statistical rigor. Each benchmark runs multiple iterations and reports confidence intervals.

**Metrics:**
- **Index time:** Time to index a corpus of N files
- **Capsule latency:** p50, p95, p99 for capsule generation
- **Memory usage:** RSS at various corpus sizes
- **Reindex time:** Incremental reindex after a single file change
- **Crossover point:** Project size where Scavenger's overhead < cost of reading files directly (following PindeX's transparency model)

**Implementation:** Criterion benchmarks in `src/bench/`. Added as `[[bench]]` entries in `Cargo.toml`.

## 6. Tier 2: Agent Evals

**Purpose:** Does Scavenger actually make AI coding agents better?

**Method:** For each task, run the agent twice — WITH Scavenger and WITHOUT (baseline). Compare outcomes.

**Task definition format** (`eval/tasks/*.yaml`):
```yaml
name: "explore-callers"
description: "Find all callers of the process_message function"
corpus: "mini-redis"
setup: "scavenger init && scavenger index"
task_prompt: "Find all callers of process_message and explain the call flow"
success_criteria:
  - "Identifies Frame::parse as a caller"
  - "Identifies Connection::read_frame as a caller"
  - "Does not read unrelated files"
timeout_seconds: 300
```

**Agent runner adapters:**
- `ClaudeCodeRunner` — Uses `claude` CLI with MCP config
- `CursorRunner` — Uses Cursor CLI (replaces benchmark.py's approach)
- Each adapter captures: tokens used, tool calls, files read, wall time, final answer

**Metrics:**
- **Token delta:** % change in tokens consumed
- **Tool call delta:** % change in tool calls made
- **Time delta:** % change in wall time
- **Success rate:** Did the agent complete the task correctly?
- **Navigation efficiency:** Ratio of relevant files read vs total files read

## 7. CLI Interface

```
scavenger eval [OPTIONS]

Run evaluation suites to measure Scavenger's quality and performance

Options:
  --suite <SUITE>        Which eval suite to run:
                           relevance   — does the capsule contain the right symbols?
                           accuracy    — do queries return the right results?
                           performance — how fast and efficient is Scavenger?
                           agent       — does Scavenger make AI agents better?
  --all                  Run all suites (default)
  --tier <TIER>          Which tier to run:
                           component   — fast, deterministic, no API keys (default)
                           agent       — requires a configured AI coding agent
                           all         — both tiers
  --corpus <PATH>        Code to evaluate against — the project(s) that
                         Scavenger will index and run evals on. Can point to
                         a single project directory (e.g., eval/corpus/fixtures/sample_project)
                         or a directory of projects (e.g., eval/corpus/) in which
                         case all subdirectories are used. Defaults to eval/corpus/
  --tasks <PATTERN>      Run agent tasks matching this glob pattern
  --agent <AGENT>        Which AI agent to use for tier-2 evals: claude, cursor
  --json                 Output results as structured JSON
  --thresholds <FILE>    Use a custom thresholds file instead of eval/thresholds.toml
  --baseline             Run agent eval without Scavenger (baseline only)
  --compare <RUN_ID>     Compare results against a previous eval run
  --report               Generate an HTML report from the last eval run
```

## 8. Thresholds

Stored in `eval/thresholds.toml`. Defines pass/fail boundaries per suite.

```toml
[relevance]
min_recall = 0.80
min_precision = 0.60

[accuracy]
min_intent_accuracy = 0.90
min_ndcg_at_5 = 0.75

[performance]
max_index_time_per_100_files_ms = 5000
max_capsule_latency_p95_ms = 200
max_reindex_time_ms = 500

[agent]
min_token_reduction_pct = 20
min_success_rate = 0.80
```

The eval runner checks thresholds and exits with code 1 if any are violated. Thresholds can be overridden per-run via `--thresholds`.

## 9. Output Format

**JSON output** (stdout, when `--json` is passed):
```json
{
  "run_id": "2026-04-04T12:00:00Z",
  "scavenger_version": "0.2.2",
  "tier": "component",
  "suite": "relevance",
  "corpus": "sample_project",
  "results": [
    {
      "case": "mini-redis-process-message",
      "recall_at_10": 0.85,
      "precision_at_10": 0.72,
      "f1": 0.78,
      "noise_ratio": 0.28
    }
  ],
  "summary": {
    "total_cases": 12,
    "passed": 10,
    "failed": 2,
    "avg_recall": 0.82,
    "avg_precision": 0.71
  }
}
```

**Human-readable summary** (stderr, always):
```
Eval: relevance (corpus: sample_project)
─────────────────────────────────────────
Cases:     12 total, 10 passed, 2 failed
Recall:    0.82 avg (threshold: 0.80) ✓
Precision: 0.71 avg (threshold: 0.60) ✓

FAILURES:
  mini-redis-explore-api — recall 0.65 (threshold: 0.80)
  mini-redis-find-handler — precision 0.45 (threshold: 0.60)
```

## 10. Dependencies

New dev-dependencies and regular dependencies:
- `criterion` — Statistical benchmarking
- `serde_yaml` — Task definition parsing
- `tempfile` — Already present, used for isolated eval runs

## 11. Deletions

- `benchmark/` directory — Replaced by Tier 2 agent eval system

## 12. Implementation Phases

This design will be broken into an implementation plan with small, reviewable PRs. Each PR should be independently testable and mergeable. The phases will cover:

1. Eval framework scaffolding (runner, reporter, corpus loading, thresholds)
2. Relevance eval suite
3. Accuracy eval suite
4. Performance benchmarks (criterion)
5. Agent eval framework (task definitions, runner adapters)
6. CLI integration (`scavenger eval` subcommand)
7. CI integration (run Tier 1 on every PR)
8. Delete `benchmark/`
