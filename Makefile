.PHONY: build install test check clean daemon init doctor

# Build release binary
build:
	cargo build --release

# Install to ~/.cargo/bin/
install: build
	cargo install --path .

# Run full test suite
test:
	cargo test

# Clippy + format check (no writes)
check:
	cargo fmt -- --check
	cargo clippy -- -D warnings

# Format code in place
fmt:
	cargo fmt

# Start daemon in foreground (run from your project dir, not scavenger repo)
daemon:
	scavenger daemon

# Initialize scavenger in the current project
init:
	scavenger init

# Run health diagnostics
doctor:
	scavenger doctor

# Kill running daemon, wait for exit, clean up
stop:
	@if [ -f .scavenger/daemon.pid ]; then \
		PID=$$(cat .scavenger/daemon.pid); \
		kill $$PID 2>/dev/null || true; \
		for i in $$(seq 1 50); do \
			kill -0 $$PID 2>/dev/null || break; \
			sleep 0.1; \
		done; \
		if kill -0 $$PID 2>/dev/null; then \
			echo "SIGTERM timed out, sending SIGKILL..."; \
			kill -9 $$PID 2>/dev/null || true; \
			sleep 1; \
		fi; \
		rm -f .scavenger/daemon.pid .scavenger/daemon.sock .scavenger/daemon.lock; \
		echo "Daemon stopped."; \
	else \
		echo "No daemon PID file found."; \
	fi

# Remove build artifacts
clean:
	cargo clean

# Full rebuild: clean + build release
rebuild: clean build