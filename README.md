# Scavenger

[![CI](https://github.com/Dalot/scavenger/actions/workflows/ci.yml/badge.svg)](https://github.com/Dalot/scavenger/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/thescavenger.svg)](https://crates.io/crates/thescavenger)
[![downloads](https://img.shields.io/crates/d/thescavenger.svg)](https://crates.io/crates/thescavenger)
[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![MSRV: 1.85](https://img.shields.io/badge/rustc-1.85+-orange.svg)](https://blog.rust-lang.org/2025/02/20/Rust-1.85.0/)

Terminal-first AST dependency graph and session memory engine for AI coding agents. Reduces token usage by serving focused "capsules" instead of full files, and persists session memory anchored to code symbols across sessions.

## Why Scavenger?

Got tired of watching people sell the same idea wrapped in marketing copy. Built this, made it free, open source it. No pitch, no waitlist, no pricing page.

- **Fewer tokens, better context** — Instead of dumping entire files into the context window, Scavenger serves focused capsules: signatures, docstrings, call graphs, and dependency neighborhoods — only what the agent needs.
- **Cross-session memory** — Annotations (facts, strategies, pitfalls) are anchored to symbols and persist across sessions. Your agent picks up where it left off. This still has a way to go, I am still testing it out.
- **Simple setup** — Install with a single command, run `scavenger init`, and you're done.
- **Real dependency graph** — Built with tree-sitter over 15 languages. Answers "what calls X?", "what does X call?", and "what breaks if X changes?" in milliseconds, without grep.
- **Branch-aware index** — Each git branch gets its own SQLite database, so context always matches your current branch.
- **Federated repos** *(work in progress)* — Query symbols from linked repositories as if they were local.

## Supported Agents

| Agent | Integration | Status |
|-------|-------------|--------|
| **Claude Code** | Hooks + MCP bridge | Tested |
| **Cursor** | Hooks + MCP bridge | Tested |
| **Other MCP tools** | MCP bridge only | Untested — see below |

Scavenger has two integration layers:

1. **Hooks** — Automatically inject capsules on file reads, trigger re-indexing on writes, and manage daemon lifecycle. This is the full experience but requires the agent to support hooks. Currently only Claude Code and Cursor have hook support.
2. **MCP bridge** — Exposes tools (`get_capsule`, `read_annotations`, etc.) that the agent calls explicitly. Any MCP-compatible agent can use this, but the agent won't automatically receive capsules or trigger re-indexing — it has to call the tools itself.

Agents that only support MCP (Windsurf, Continue, Amp, etc.) get the MCP bridge but not hooks. The tools work, but the experience is less seamless. If you try Scavenger with another tool, please [open a discussion](https://github.com/Dalot/scavenger/discussions) and let us know how it went.

## Platform Support

| Platform | Status |
|----------|--------|
| Linux x86_64 | Supported (pre-built binary) |
| macOS x86_64 | Supported (pre-built binary) |
| macOS Apple Silicon | Supported (pre-built binary) |
| Windows | Not natively supported — Scavenger relies on Unix domain sockets for daemon communication. Use WSL2 instead. |

## Installation

### Pre-built binary (no Rust required)

Download the binary for your platform from the [latest release](https://github.com/Dalot/scavenger/releases/latest):

| Platform | Asset |
|----------|-------|
| Linux x86_64 | `scavenger-x86_64-linux.tar.gz` |
| macOS x86_64 | `scavenger-x86_64-macos.tar.gz` |
| macOS Apple Silicon | `scavenger-aarch64-macos.tar.gz` |

```bash
# Example: Linux
curl -L https://github.com/Dalot/scavenger/releases/latest/download/scavenger-x86_64-linux.tar.gz | tar xz
sudo mv scavenger /usr/local/bin/
```

### From crates.io (requires Rust 1.85+)

```bash
cargo install thescavenger
```

### From source

```bash
cargo install --path .
```

Verify:

```bash
scavenger --version
```

## Quick Start

```bash
# 1. Go to your project directory
cd your-project

# 2. Initialize Scavenger (indexes files, registers hooks and MCP config)
scavenger init
```

That's it. `scavenger init` automatically:
- Indexes all source files (15 languages) and markdown docs into a per-branch SQLite graph
- Creates the Claude Code plugin at `.scavenger/claude-plugin/`
- Registers the MCP bridge via `claude mcp add` (if `claude` CLI is on PATH)
- Writes `.mcp.json` for any other MCP-compatible tool
- Writes `.cursor/mcp.json` and `.cursor/hooks.json` for Cursor
- Adds `.scavenger/` to `.gitignore`

The daemon starts and stops automatically with each agent session — no manual `scavenger daemon` needed.

## Agent Setup

### Claude Code

After `scavenger init`, launch Claude Code with the plugin flag:

```bash
claude --plugin-dir .scavenger/claude-plugin/
```

If the `claude` CLI wasn't found during `init`, register the MCP bridge manually:

```bash
claude mcp add scavenger -- scavenger mcp-bridge
```

### Cursor

No extra steps after `scavenger init`. Cursor picks up `.cursor/mcp.json` and `.cursor/hooks.json` automatically.

### Other MCP-compatible tools

`scavenger init` writes a `.mcp.json` at the project root. Any tool that reads this file will discover the MCP bridge. If your tool doesn't read `.mcp.json` automatically, point it at `scavenger mcp-bridge` as the command for a stdio-based MCP server.

## How It Works

### Hooks (Claude Code & Cursor)

Hooks give agents the full Scavenger experience automatically:

- **On file reads** — Injects a capsule (signatures, docstrings, call graph neighborhood) so the agent sees focused context instead of raw file content.
- **On file writes** — Incrementally re-indexes the changed file within a debounce window.
- **On session start/end** — Starts and stops the daemon automatically.

### MCP bridge tools

Available in any MCP-connected agent as callable tools. Agents without hook support use these directly.

| Tool | Description |
|------|-------------|
| `get_capsule` | Pass a file path for focused context, or a symbol name to get callers, callees, and breakage impact. |
| `read_annotations` | Retrieve persisted memory: facts, strategies, pitfalls anchored to symbols, files, or scopes. |
| `write_annotation` | Persist a note anchored to a symbol, file, or scope. Survives across sessions and branches. |
| `delete_annotation` | Remove an annotation by ID. |
| `search_docs` | Search over indexed markdown docs and code. |

### Session memory

1. **Annotations** — Agent-written notes (facts, strategies, pitfalls), anchored to symbols.
2. **Behavioral signals** — Auto-captured metrics: which files were touched, token savings per session.
3. **Version history** — Annotation edits are versioned; annotations from other branches can be merged.

## CLI Reference

| Command | Description |
|---------|-------------|
| `scavenger init` | Initialize on a project (index + register hooks for all agents) |
| `scavenger daemon` | Start the daemon manually in foreground (normally auto-managed) |
| `scavenger index [path]` | Manually re-index files |
| `scavenger capsule <file> [symbol]` | Print a capsule to stdout |
| `scavenger graph stats` | Show node/edge counts and top centrality |
| `scavenger graph show <symbol>` | ASCII neighborhood tree |
| `scavenger annotate <symbol> "<text>"` | Add an annotation |
| `scavenger memory --query "<text>"` | Search annotations via FTS5 |
| `scavenger merge-annotations <branch>` | Merge annotations from another branch |
| `scavenger stats [--session] [--branch]` | Token savings report |
| `scavenger observe [--interval N]` | Live observability dashboard (TUI) |
| `scavenger doctor [--format=json]` | Health diagnostics |
| `scavenger db summary` | Node/edge/file/annotation counts, DB sizes |
| `scavenger db nodes [--limit N]` | List indexed symbols |
| `scavenger db files [--limit N]` | List indexed files |
| `scavenger db annotations [--limit N]` | List annotations |
| `scavenger db tokens [--limit N]` | Show token_log entries |
| `scavenger db query "SQL" [--meta]` | Run read-only SQL against the DB |
| `scavenger federate add/remove/list/verify` | Manage federated repos *(work in progress)* |
| `scavenger clean [--purge]` | Remove plugin and legacy config (--purge removes all data) |

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

> **Note**: Kotlin is not yet supported. The `tree-sitter-kotlin` crate requires tree-sitter <0.23, which is incompatible with our tree-sitter 0.25 dependency. Will be added when a compatible version is released.

## Architecture

```
Claude Code / Cursor / MCP tools
        ↓
Hooks (CLI) ←→ UDS Socket ←→ Daemon
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

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, coding standards, and the PR process. Check [CHANGELOG.md](CHANGELOG.md) for what has changed between releases.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT License ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
