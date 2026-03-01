#!/usr/bin/env bash
set -euo pipefail

BENCH_DIR="${BENCH_DIR:-/tmp/scavenger-bench-target}"
MINI_REDIS_COMMIT="e186482ca00f8d884ddcbe20417f3654d03315a4"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "Scavenger Benchmark Setup"
echo "========================="
echo ""

# --- Prerequisites ---
for cmd in scavenger agent git cargo; do
    if ! command -v "$cmd" &>/dev/null; then
        echo "Error: '$cmd' not found in PATH."
        case "$cmd" in
            scavenger) echo "  Install: cargo install --path . (from scavenger repo root)" ;;
            agent)     echo "  Install: curl https://cursor.com/install -fsS | bash && agent login" ;;
            cargo)     echo "  Install: https://rustup.rs" ;;
        esac
        exit 1
    fi
done

# --- Check agent auth ---
if ! agent status &>/dev/null; then
    echo "Error: agent CLI not authenticated. Run: agent login"
    exit 1
fi

# --- Clone target project ---
if [ -d "$BENCH_DIR" ]; then
    echo "Removing existing $BENCH_DIR..."
    rm -rf "$BENCH_DIR"
fi

echo "Cloning tokio-rs/mini-redis..."
git clone --quiet https://github.com/tokio-rs/mini-redis.git "$BENCH_DIR"
cd "$BENCH_DIR"
git checkout --quiet "$MINI_REDIS_COMMIT"
echo "  Pinned to commit $MINI_REDIS_COMMIT"

# --- Initialize Scavenger ---
echo "Running scavenger init..."
scavenger init 2>&1 | tail -3

# --- Start daemon ---
echo "Starting scavenger daemon..."
scavenger daemon &
DAEMON_PID=$!
sleep 5

if ! kill -0 "$DAEMON_PID" 2>/dev/null; then
    echo "Error: daemon failed to start. Check $BENCH_DIR/.scavenger/daemon.log"
    exit 1
fi
echo "  Daemon running (PID $DAEMON_PID)"

# --- Enable MCP in agent ---
echo "Enabling scavenger MCP in agent CLI..."
agent mcp enable scavenger 2>/dev/null || true

echo ""
echo "Setup complete. Target project: $BENCH_DIR"
echo ""
echo "Run the benchmark:"
echo "  python3 $SCRIPT_DIR/benchmark.py --session $SCRIPT_DIR/sessions/mini-redis-explore.txt --project $BENCH_DIR"
echo ""
echo "When done, stop the daemon:"
echo "  kill $DAEMON_PID"
