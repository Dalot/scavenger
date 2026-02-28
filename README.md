# Scavenger

Terminal-first AST dependency graph and session memory engine for Claude Code CLI. Reduces token usage by serving focused "capsules" instead of full files, and persists session memory anchored to code symbols across sessions.

## Installation

Requires Rust 1.85+ (edition 2024).

```bash
cargo install --path .
```

Or build from source:

```bash
cargo build --release
# Binary at target/release/scavenger
```

## Quick Start

```bash
# Initialize in your project directory
cd your-project
scavenger init

# Start the daemon (required for hooks)
scavenger daemon
```

After initialization, Scavenger automatically:
- Indexes all source files (14 languages) and markdown docs
- Registers Claude Code hooks (`PreToolUse` / `PostToolUse`)
- Creates `.scavenger/` directory (auto-added to `.gitignore`)

## How It Works

1. **PreToolUse hook** — When Claude Code reads a file, Scavenger injects a capsule (focused context: signatures, docstrings, relationships) as `additionalContext`
2. **PostToolUse hook** — When Claude Code writes/edits a file, Scavenger incrementally re-indexes it within the debounce window
3. **Session memory** — Annotations, behavioral signals, and version history persist across sessions

## CLI Commands

| Command | Description |
|---------|-------------|
| `scavenger init` | Initialize on a project (index + register hooks) |
| `scavenger daemon` | Start the daemon in foreground |
| `scavenger index [path]` | Manually re-index files |
| `scavenger capsule <file> [symbol]` | Print a capsule to stdout |
| `scavenger graph stats` | Show node/edge counts and top centrality |
| `scavenger graph show <symbol>` | ASCII neighborhood tree |
| `scavenger annotate <symbol> "<text>"` | Add an annotation |
| `scavenger memory --query "<text>"` | Search annotations via FTS5 |
| `scavenger merge-annotations <branch>` | Merge annotations from another branch |
| `scavenger stats [--session] [--branch]` | Token savings report |
| `scavenger doctor [--format=json]` | Health diagnostics |
| `scavenger federate add/remove/list/verify` | Manage federated repos |

## Configuration

Create `.scavenger.toml` in your project root (optional — sensible defaults apply):

```toml
[capsule]
token_budget = 8000        # Max tokens per capsule (default: 8000)

[traversal]
max_hops = 3               # BFS hop limit (default: 3)
node_budget = 100          # Max nodes to traverse (default: 100)
degree_cap = 30            # Skip high-degree utility nodes (default: 30)

[docs]
patterns = ["**/*.md"]     # Markdown patterns to index
exclude = ["node_modules", "target", ".git"]
```

## Supported Languages

Rust, Python, TypeScript, TSX, JavaScript, JSX, Go, Java, C#, C, C++, Ruby, Bash, PHP, Swift

## Architecture

```
Claude Code ←→ Hooks (CLI) ←→ UDS Socket ←→ Daemon
                                               ├── Graph (petgraph + tree-sitter)
                                               ├── SQLite (per-branch index, WAL)
                                               ├── Capsule Pipeline (6-stage)
                                               ├── Memory (3-layer model)
                                               ├── File Watcher (notify)
                                               └── Federation (read-only)
```

## Troubleshooting

Run `scavenger doctor` to check:
- Daemon process alive
- Socket accessible
- DB integrity
- Hook registration
- Config validity

Set `NO_COLOR=1` for plain output. Use `--format=json` for machine-readable diagnostics.

## License

MIT
