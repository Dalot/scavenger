# Scavenger A/B Benchmark

Automated comparison of AI coding agent behavior **with** and **without** Scavenger's AST dependency graph, across multi-turn sessions.

## Quick Start

```bash
# 1. Set up the target project (tokio-rs/mini-redis)
./benchmark/setup.sh

# 2. Run the benchmark
python3 benchmark/benchmark.py \
  --session benchmark/sessions/mini-redis-explore.txt \
  --project /tmp/scavenger-bench-target

# 3. Stop the daemon when done
kill $(cat /tmp/scavenger-bench-target/.scavenger/daemon.pid 2>/dev/null) 2>/dev/null
```

## Prerequisites

| Dependency | Install |
|-----------|---------|
| Rust toolchain | [rustup.rs](https://rustup.rs) |
| Scavenger binary | `cargo install --path .` (from this repo) |
| Cursor Agent CLI | `curl https://cursor.com/install -fsS \| bash` |
| Agent auth | `agent login` |
| Python 3.10+ | System package manager |

After installing the agent CLI, enable the Scavenger MCP server:

```bash
agent mcp enable scavenger
```

## How It Works

The benchmark runs the same prompt sequence twice as continuous sessions (using `--resume`):

1. **WITHOUT Scavenger** — MCP disabled, agent uses only native tools (Grep, Read, Glob, Shell)
2. **WITH Scavenger** — MCP enabled, agent has access to `get_capsule`, `read_annotations`, `search_docs` plus native tools

After each session, the target project is reset via `git checkout -- .` to undo any edits.

Metrics are extracted from the `stream-json` output, including:
- Tool call counts by type (Read, Grep, Glob, Shell, MCP tools)
- Token usage from the API (input, output, cache read/write)
- Wall time per turn and total

## Session Prompts

Prompts are stored in `sessions/` as text files (one prompt per line, `#` comments ignored):

| File | Target | Pattern |
|------|--------|---------|
| `mini-redis-explore.txt` | tokio-rs/mini-redis | Explore SET command flow, analyze Db rename impact, execute rename |
| `scavenger-explore.txt` | Scavenger itself | Explore capsule pipeline, analyze config rename impact, execute rename |

### Writing Custom Sessions

Follow the **explore-analyze-execute** pattern for best results:

1. **T1 — Explore:** Ask the agent to trace a data flow or explain a subsystem. This forces it to build understanding.
2. **T2 — Analyze:** Ask about the impact of a specific change in the area explored in T1. This tests whether the agent retained structural knowledge.
3. **T3 — Execute:** Ask the agent to make the change from T2. This is where Scavenger's compounding effect shows — the WITH agent should need far fewer tool calls.

## Interpreting Results

The benchmark outputs two views:

### Turn-by-turn timeline
Shows per-turn metrics side by side. Look for:
- **T1:** WITH will typically use more tools/tokens (investment in capsule context)
- **T2:** Navigation calls should start decreasing for WITH
- **T3:** WITH should show dramatic reductions in tools, reads, and tokens

### Session totals
Aggregated metrics across all turns. Key indicators:
- **Total tool calls:** Lower = less work to accomplish the same task
- **File reads:** Lower = less re-navigation (the graph provided structure)
- **Input tokens:** Lower = less context window pressure (focused capsules vs full files)
- **Duration:** Lower = faster task completion

### What "good" looks like

A positive result for Scavenger shows:
- T1 tokens higher (investment), T3 tokens much lower (payoff)
- Session total tokens lower (net savings)
- Session total tool calls lower (less re-navigation)
- File reads replaced by capsule calls in early turns, then neither in later turns

## CLI Reference

```bash
# Single prompt (one-shot mode)
python3 benchmark/benchmark.py "What does the parse module do?"

# Multi-turn session
python3 benchmark/benchmark.py --session benchmark/sessions/mini-redis-explore.txt

# Custom project directory
python3 benchmark/benchmark.py --session sessions/my-session.txt --project /path/to/project

# Specific model
python3 benchmark/benchmark.py --session sessions/mini-redis-explore.txt --model sonnet-4
```

## File Structure

```
benchmark/
  README.md              ← This file
  report.md              ← Detailed analysis of benchmark results
  benchmark.py           ← The benchmark runner
  setup.sh               ← Clone + init target project
  sessions/
    mini-redis-explore.txt   ← Prompts for mini-redis (reproducible)
    scavenger-explore.txt    ← Prompts for scavenger (original run)
    benchmark-prompts.txt    ← Single-prompt examples
```
