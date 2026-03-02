#!/usr/bin/env python3
"""
Scavenger A/B Benchmark — Multi-turn session comparison.

Runs the same sequence of prompts as a continuous session (via --resume),
once WITHOUT Scavenger and once WITH, then compares cumulative metrics.

This measures the compounding effect: session memory, graph context reuse,
and reduced re-navigation over multiple related turns.

Also captures Scavenger-side metrics (latency, effectiveness, pipeline timing)
from the daemon via `scavenger stats --json` for comprehensive analysis.

Prerequisites:
  curl https://cursor.com/install -fsS | bash
  agent login
  agent mcp enable scavenger

Usage:
  python3 scripts/benchmark.py --session scripts/benchmark-session.txt
  python3 scripts/benchmark.py --session scripts/benchmark-session.txt --model sonnet-4
  python3 scripts/benchmark.py "single prompt"  # still works for one-shot
"""

import argparse
import json
import os
import subprocess
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path


@dataclass
class TurnMetrics:
    """Metrics for a single turn (prompt) in a session."""
    turn: int = 0
    prompt: str = ""
    session_id: str = ""
    model: str = ""
    duration_ms: int = 0
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_tokens: int = 0
    cache_write_tokens: int = 0
    tool_calls: dict = field(default_factory=dict)
    mcp_calls: dict = field(default_factory=dict)
    file_reads: int = 0
    file_edits: int = 0
    grep_calls: int = 0
    glob_calls: int = 0
    shell_calls: int = 0
    semantic_search_calls: int = 0
    total_read_bytes: int = 0
    total_output_chars: int = 0
    tool_count: int = 0
    errors: list = field(default_factory=list)

    @property
    def navigation_calls(self):
        return self.file_reads + self.grep_calls + self.glob_calls + self.semantic_search_calls

    @property
    def capsule_calls(self):
        return self.mcp_calls.get("get_capsule", 0)

    @property
    def total_tokens(self):
        return self.input_tokens + self.output_tokens

    @property
    def net_input_tokens(self):
        return self.input_tokens - self.cache_read_tokens


@dataclass
class ScavengerMetrics:
    """Daemon-side metrics captured via `scavenger stats --json`."""
    capsule_latency_p50_us: int = 0
    capsule_latency_p95_us: int = 0
    capsule_latency_p99_us: int = 0
    capsule_total: int = 0
    capsule_empty: int = 0
    empty_rate: float = 0.0
    budget_utilization_avg: int = 0
    tokens_saved: int = 0
    savings_pct: float = 0.0
    reindex_count: int = 0
    reindex_p50_us: int = 0
    pipeline_gather_avg_us: int = 0
    pipeline_score_avg_us: int = 0
    pipeline_render_avg_us: int = 0
    effectiveness_score: float = 0.0
    errors: int = 0
    raw: dict = field(default_factory=dict)


@dataclass
class SessionMetrics:
    """Aggregated metrics across all turns in a session."""
    session_id: str = ""
    model: str = ""
    turns: list = field(default_factory=list)
    scavenger: ScavengerMetrics = field(default_factory=ScavengerMetrics)

    def _sum(self, attr):
        return sum(getattr(t, attr) for t in self.turns)

    def _merge_dicts(self, attr):
        merged = {}
        for t in self.turns:
            for k, v in getattr(t, attr).items():
                merged[k] = merged.get(k, 0) + v
        return merged

    @property
    def total_duration_ms(self): return self._sum("duration_ms")
    @property
    def total_input_tokens(self): return self._sum("input_tokens")
    @property
    def total_output_tokens(self): return self._sum("output_tokens")
    @property
    def total_cache_read(self): return self._sum("cache_read_tokens")
    @property
    def total_cache_write(self): return self._sum("cache_write_tokens")
    @property
    def total_net_input(self): return self._sum("net_input_tokens")
    @property
    def total_tokens(self): return self._sum("total_tokens")
    @property
    def total_tool_count(self): return self._sum("tool_count")
    @property
    def total_file_reads(self): return self._sum("file_reads")
    @property
    def total_file_edits(self): return self._sum("file_edits")
    @property
    def total_grep_calls(self): return self._sum("grep_calls")
    @property
    def total_glob_calls(self): return self._sum("glob_calls")
    @property
    def total_navigation(self): return self._sum("navigation_calls")
    @property
    def total_capsule_calls(self):
        return sum(t.capsule_calls for t in self.turns)
    @property
    def all_tool_calls(self): return self._merge_dicts("tool_calls")
    @property
    def all_mcp_calls(self): return self._merge_dicts("mcp_calls")
    @property
    def num_turns(self): return len(self.turns)
    @property
    def all_errors(self):
        return [e for t in self.turns for e in t.errors]


def parse_stream_json(lines: list[str]) -> TurnMetrics:
    """Parse NDJSON stream from a single `agent -p` invocation."""
    m = TurnMetrics()

    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue

        etype = event.get("type", "")
        subtype = event.get("subtype", "")

        if etype == "system" and subtype == "init":
            m.model = event.get("model", "")
            m.session_id = event.get("session_id", "")

        elif etype == "tool_call" and subtype == "completed":
            m.tool_count += 1
            tc = event.get("tool_call", {})

            if "mcpToolCall" in tc:
                mcp = tc["mcpToolCall"]
                args = mcp.get("args", {})
                tool_name = args.get("toolName", args.get("name", "unknown"))
                tool_name = tool_name.replace("scavenger-", "")
                m.mcp_calls[tool_name] = m.mcp_calls.get(tool_name, 0) + 1
                m.total_output_chars += len(json.dumps(mcp.get("result", {})))

            elif "readToolCall" in tc:
                m.file_reads += 1
                m.tool_calls["Read"] = m.tool_calls.get("Read", 0) + 1
                result = tc["readToolCall"].get("result", {}).get("success", {})
                content = result.get("content", "")
                m.total_read_bytes += len(content) if isinstance(content, str) else result.get("totalChars", 0)

            elif "writeToolCall" in tc:
                m.file_edits += 1
                m.tool_calls["Write"] = m.tool_calls.get("Write", 0) + 1

            elif "strReplaceToolCall" in tc:
                m.file_edits += 1
                m.tool_calls["StrReplace"] = m.tool_calls.get("StrReplace", 0) + 1

            elif "grepToolCall" in tc:
                m.grep_calls += 1
                m.tool_calls["Grep"] = m.tool_calls.get("Grep", 0) + 1

            elif "globToolCall" in tc:
                m.glob_calls += 1
                m.tool_calls["Glob"] = m.tool_calls.get("Glob", 0) + 1

            elif "shellToolCall" in tc:
                m.shell_calls += 1
                m.tool_calls["Shell"] = m.tool_calls.get("Shell", 0) + 1

            elif "semanticSearchToolCall" in tc:
                m.semantic_search_calls += 1
                m.tool_calls["SemanticSearch"] = m.tool_calls.get("SemanticSearch", 0) + 1

            else:
                for key, name in {
                    "editNotebookToolCall": "EditNotebook",
                    "deleteToolCall": "Delete",
                    "webFetchToolCall": "WebFetch",
                    "todoWriteToolCall": "TodoWrite",
                    "listMcpResourcesToolCall": "ListMcpResources",
                    "fetchMcpResourceToolCall": "FetchMcpResource",
                }.items():
                    if key in tc:
                        m.tool_calls[name] = m.tool_calls.get(name, 0) + 1
                        break

        elif etype == "assistant":
            content = event.get("message", {}).get("content", [])
            for item in content:
                if item.get("type") == "text":
                    m.total_output_chars += len(item.get("text", ""))

        elif etype == "result":
            m.duration_ms = event.get("duration_ms", 0)
            if not m.session_id:
                m.session_id = event.get("session_id", "")
            usage = event.get("usage", {})
            m.input_tokens = usage.get("inputTokens", 0)
            m.output_tokens = usage.get("outputTokens", 0)
            m.cache_read_tokens = usage.get("cacheReadTokens", 0)
            m.cache_write_tokens = usage.get("cacheWriteTokens", 0)

    return m


def run_turn(prompt: str, cwd: str, label: str, session_id: str = None,
             approve_mcps: bool = False, model: str = None) -> TurnMetrics:
    """Run a single turn. If session_id is given, resumes that session."""
    print(f"  [{label}]...", end="", flush=True)

    cmd = ["agent", "-p", "--trust", "--yolo", "--output-format", "stream-json"]
    if approve_mcps:
        cmd.append("--approve-mcps")
    if session_id:
        cmd.extend(["--resume", session_id])
    if model:
        cmd.extend(["--model", model])
    cmd.append(prompt)

    try:
        result = subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, timeout=300)
    except FileNotFoundError:
        print(" FAILED")
        print("\nError: `agent` CLI not found. Install with: curl https://cursor.com/install -fsS | bash")
        sys.exit(1)
    except subprocess.TimeoutExpired:
        print(" TIMEOUT")
        t = TurnMetrics()
        t.errors.append("timeout")
        return t

    lines = result.stdout.strip().split("\n")
    metrics = parse_stream_json(lines)

    if result.returncode != 0 and not metrics.session_id:
        metrics.errors.append(f"exit code {result.returncode}")
        if result.stderr:
            metrics.errors.append(result.stderr[:200])

    dur = f"{metrics.duration_ms / 1000:.1f}s" if metrics.duration_ms else "?"
    print(f" {metrics.tool_count} tools, {dur}")
    return metrics


def toggle_scavenger(project: str, enable: bool):
    action = "enable" if enable else "disable"
    try:
        subprocess.run(
            ["agent", "mcp", action, "scavenger"],
            cwd=project, capture_output=True, text=True, timeout=15,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass


def reset_project(project: str):
    """Revert any file edits the agent made during a session."""
    try:
        subprocess.run(
            ["git", "checkout", "--", "."],
            cwd=project, capture_output=True, text=True, timeout=15,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass


def capture_scavenger_metrics(project: str) -> ScavengerMetrics:
    """Query the daemon for current metrics via `scavenger stats --json`."""
    sm = ScavengerMetrics()
    try:
        result = subprocess.run(
            ["scavenger", "stats", "--json"],
            cwd=project, capture_output=True, text=True, timeout=10,
        )
        if result.returncode == 0 and result.stdout.strip():
            data = json.loads(result.stdout)
            sm.raw = data

            ts = data.get("token_savings", {})
            sm.tokens_saved = ts.get("tokens_saved", 0)
            sm.savings_pct = ts.get("savings_pct", 0.0)

            dm = data.get("daemon", {})
            if dm:
                cap = dm.get("capsule", {})
                sm.capsule_total = cap.get("total", 0)
                sm.capsule_empty = cap.get("empty", 0)
                sm.empty_rate = cap.get("empty_rate", 0.0)

                lat = cap.get("latency_us", {})
                sm.capsule_latency_p50_us = lat.get("p50", 0)
                sm.capsule_latency_p95_us = lat.get("p95", 0)
                sm.capsule_latency_p99_us = lat.get("p99", 0)

                budget = cap.get("budget_utilization_pct", {})
                sm.budget_utilization_avg = budget.get("avg", 0)

                ri = dm.get("reindex", {})
                sm.reindex_count = ri.get("count", 0)
                sm.reindex_p50_us = ri.get("latency_us", {}).get("p50", 0)

                pipe = dm.get("pipeline_us", {})
                sm.pipeline_gather_avg_us = pipe.get("gather", {}).get("avg", 0)
                sm.pipeline_score_avg_us = pipe.get("score", {}).get("avg", 0)
                sm.pipeline_render_avg_us = pipe.get("render", {}).get("avg", 0)

                sm.errors = dm.get("errors", 0)
    except (FileNotFoundError, subprocess.TimeoutExpired, json.JSONDecodeError):
        pass
    return sm


def run_health_gate(project: str):
    """Pre-flight check: abort if scavenger is unhealthy."""
    try:
        result = subprocess.run(
            ["scavenger", "doctor", "--format", "json"],
            cwd=project, capture_output=True, text=True, timeout=10,
        )
        if result.returncode != 0:
            data = json.loads(result.stdout) if result.stdout.strip() else {}
            health = data.get("health_score", 0)
            failed = [c["name"] for c in data.get("checks", []) if not c["passed"]]
            print(f"\nHealth gate FAILED (score: {health}/100)")
            for f in failed:
                print(f"  - {f}")
            print("\nFix issues before benchmarking. Run: scavenger doctor --verbose")
            sys.exit(1)
    except FileNotFoundError:
        print("Warning: `scavenger` CLI not found — skipping health gate")
    except (subprocess.TimeoutExpired, json.JSONDecodeError):
        print("Warning: health gate check timed out — continuing")


def auto_tag_session(project: str, session_id: str, label: str):
    """Tag a session for easier identification."""
    if not session_id:
        return
    try:
        subprocess.run(
            ["scavenger", "metrics", "tag", session_id, label],
            cwd=project, capture_output=True, text=True, timeout=10,
        )
    except (FileNotFoundError, subprocess.TimeoutExpired):
        pass


def run_session(prompts: list[str], cwd: str, condition: str,
                approve_mcps: bool = False, model: str = None) -> SessionMetrics:
    """Run a full multi-turn session and return aggregated metrics."""
    session = SessionMetrics()
    session_id = None

    for i, prompt in enumerate(prompts):
        turn_label = f"{condition} turn {i + 1}/{len(prompts)}"
        turn = run_turn(
            prompt, cwd, turn_label,
            session_id=session_id,
            approve_mcps=approve_mcps,
            model=model,
        )
        turn.turn = i + 1
        turn.prompt = prompt

        if not session_id and turn.session_id:
            session_id = turn.session_id
            session.session_id = session_id
        if turn.model:
            session.model = turn.model

        session.turns.append(turn)

    return session


def fmt(n: int) -> str:
    return f"{n:,}"


def fmt_tok(n: int) -> str:
    if n >= 1_000_000:
        return f"{n / 1_000_000:.1f}M"
    if n >= 1_000:
        return f"{n / 1_000:.1f}k"
    return str(n)


def fmt_us(n: int) -> str:
    if n >= 1_000_000:
        return f"{n / 1_000_000:.1f}s"
    if n >= 1_000:
        return f"{n / 1_000:.1f}ms"
    return f"{n}us"


def delta_str(a: int, b: int) -> str:
    if a > 0 and b > 0:
        pct = ((b - a) / a) * 100
        if abs(pct) >= 0.5:
            return f" ({pct:+.0f}%)"
    elif a == 0 and b > 0:
        return " (new)"
    return ""


def print_turn_timeline(without: SessionMetrics, with_s: SessionMetrics):
    """Print per-turn comparison timeline."""
    w = 14
    n = max(without.num_turns, with_s.num_turns)

    print(f"\n{'TURN-BY-TURN TIMELINE':─^78}")
    print(f"  {'Turn':<6s} {'Metric':<20s} {'WITHOUT':>{w}s} {'WITH':>{w}s} {'Delta':>{w}s}")
    print(f"  {'─' * 6} {'─' * 20} {'─' * w} {'─' * w} {'─' * w}")

    for i in range(n):
        tw = without.turns[i] if i < without.num_turns else TurnMetrics()
        ts = with_s.turns[i] if i < with_s.num_turns else TurnMetrics()

        prompt_preview = (tw.prompt or ts.prompt)[:50]
        print(f"\n  T{i + 1:<5d} \"{prompt_preview}{'...' if len(tw.prompt or ts.prompt) > 50 else ''}\"")

        for label, a, b, formatter in [
            ("Tools", tw.tool_count, ts.tool_count, fmt),
            ("Navigation", tw.navigation_calls, ts.navigation_calls, fmt),
            ("Capsules", tw.capsule_calls, ts.capsule_calls, fmt),
            ("File reads", tw.file_reads, ts.file_reads, fmt),
            ("Grep calls", tw.grep_calls, ts.grep_calls, fmt),
            ("Input tokens", tw.input_tokens, ts.input_tokens, fmt_tok),
            ("Output tokens", tw.output_tokens, ts.output_tokens, fmt_tok),
            ("Duration (s)", tw.duration_ms // 1000, ts.duration_ms // 1000, fmt),
        ]:
            d = delta_str(a, b)
            print(f"  {'':6s} {label:<20s} {formatter(a):>{w}s} {formatter(b):>{w}s} {d:>{w}s}")


def print_session_summary(without: SessionMetrics, with_s: SessionMetrics):
    """Print the session-level aggregated comparison."""
    w = 20
    lines = []

    lines.append(f"\n{'SESSION TOTALS':─^78}")
    lines.append(f"{'':36s} {'WITHOUT':>{w}s} {'WITH':>{w}s}")
    lines.append("─" * 78)

    sid_w = without.session_id[:12] if without.session_id else "n/a"
    sid_s = with_s.session_id[:12] if with_s.session_id else "n/a"
    lines.append(f"{'Model':<36s} {without.model:>{w}s} {with_s.model:>{w}s}")
    lines.append(f"{'Session':<36s} {sid_w:>{w}s} {sid_s:>{w}s}")
    lines.append(f"{'Turns':<36s} {without.num_turns:>{w}d} {with_s.num_turns:>{w}d}")
    lines.append("")

    def row(label, a, b, formatter=fmt):
        d = delta_str(a, b)
        lines.append(f"  {label:<34s} {formatter(a):>{w}s} {formatter(b):>{w}s}{d}")

    lines.append("NAVIGATION (session total)")
    row("File reads", without.total_file_reads, with_s.total_file_reads)
    row("Glob calls", without.total_glob_calls, with_s.total_glob_calls)
    row("Grep calls", without.total_grep_calls, with_s.total_grep_calls)
    row("Total navigation", without.total_navigation, with_s.total_navigation)
    row("Capsule calls", without.total_capsule_calls, with_s.total_capsule_calls)
    lines.append("")

    lines.append("WORK (session total)")
    row("Total tool calls", without.total_tool_count, with_s.total_tool_count)
    row("File edits", without.total_file_edits, with_s.total_file_edits)
    lines.append("")

    lines.append("TOKENS (session total)")
    row("Input tokens", without.total_input_tokens, with_s.total_input_tokens, fmt_tok)
    row("  Cache read", without.total_cache_read, with_s.total_cache_read, fmt_tok)
    row("  Cache write", without.total_cache_write, with_s.total_cache_write, fmt_tok)
    row("  Net new input", without.total_net_input, with_s.total_net_input, fmt_tok)
    row("Output tokens", without.total_output_tokens, with_s.total_output_tokens, fmt_tok)
    row("Total tokens", without.total_tokens, with_s.total_tokens, fmt_tok)
    lines.append("")

    lines.append("TIMING")
    row("Total duration (s)", without.total_duration_ms // 1000, with_s.total_duration_ms // 1000)
    lines.append("")

    all_tools = sorted(set(list(without.all_tool_calls.keys()) + list(with_s.all_tool_calls.keys())))
    if all_tools:
        lines.append("TOOL BREAKDOWN")
        for t in all_tools:
            row(t, without.all_tool_calls.get(t, 0), with_s.all_tool_calls.get(t, 0))
        lines.append("")

    all_mcp = sorted(set(list(without.all_mcp_calls.keys()) + list(with_s.all_mcp_calls.keys())))
    if all_mcp:
        lines.append("MCP BREAKDOWN")
        for t in all_mcp:
            row(t, without.all_mcp_calls.get(t, 0), with_s.all_mcp_calls.get(t, 0))
        lines.append("")

    # Scavenger internals (WITH session only — WITHOUT has zeroes)
    sm = with_s.scavenger
    if sm.capsule_total > 0:
        lines.append("SCAVENGER INTERNALS (daemon perspective)")
        lines.append(f"  {'Capsule latency P50':<34s} {fmt_us(sm.capsule_latency_p50_us):>{w}s}")
        lines.append(f"  {'Capsule latency P95':<34s} {fmt_us(sm.capsule_latency_p95_us):>{w}s}")
        lines.append(f"  {'Capsule latency P99':<34s} {fmt_us(sm.capsule_latency_p99_us):>{w}s}")
        lines.append(f"  {'Empty capsules':<34s} {f'{sm.capsule_empty}/{sm.capsule_total} ({sm.empty_rate*100:.0f}%)':>{w}s}")
        lines.append(f"  {'Budget utilization':<34s} {f'{sm.budget_utilization_avg}%':>{w}s}")
        lines.append(f"  {'Token savings':<34s} {f'{sm.savings_pct:.1f}%':>{w}s}")
        lines.append(f"  {'Pipeline gather avg':<34s} {fmt_us(sm.pipeline_gather_avg_us):>{w}s}")
        lines.append(f"  {'Pipeline score avg':<34s} {fmt_us(sm.pipeline_score_avg_us):>{w}s}")
        lines.append(f"  {'Pipeline render avg':<34s} {fmt_us(sm.pipeline_render_avg_us):>{w}s}")
        lines.append(f"  {'Reindex events':<34s} {f'{sm.reindex_count} (P50={fmt_us(sm.reindex_p50_us)})':>{w}s}")
        lines.append(f"  {'Daemon errors':<34s} {sm.errors:>{w}d}")
        if sm.effectiveness_score > 0:
            lines.append(f"  {'Effectiveness score':<34s} {f'{sm.effectiveness_score:.2f}':>{w}s}")
        lines.append("")

    errs = without.all_errors + with_s.all_errors
    if errs:
        lines.append("ERRORS")
        for e in without.all_errors:
            lines.append(f"  [WITHOUT] {e}")
        for e in with_s.all_errors:
            lines.append(f"  [WITH] {e}")

    print("\n".join(lines))


def export_json_results(
    project: str,
    prompts: list[str],
    without: SessionMetrics,
    with_s: SessionMetrics,
    model: str,
):
    """Save full benchmark results as structured JSON."""
    benchmarks_dir = Path(project) / ".scavenger" / "benchmarks"
    benchmarks_dir.mkdir(parents=True, exist_ok=True)

    timestamp = datetime.now().strftime("%Y%m%d-%H%M%S")
    out_path = benchmarks_dir / f"{timestamp}.json"

    def serialize_session(s: SessionMetrics) -> dict:
        return {
            "session_id": s.session_id,
            "model": s.model,
            "num_turns": s.num_turns,
            "total_duration_ms": s.total_duration_ms,
            "total_tokens": s.total_tokens,
            "total_input_tokens": s.total_input_tokens,
            "total_output_tokens": s.total_output_tokens,
            "total_tool_count": s.total_tool_count,
            "total_navigation": s.total_navigation,
            "total_file_reads": s.total_file_reads,
            "total_file_edits": s.total_file_edits,
            "total_capsule_calls": s.total_capsule_calls,
            "tool_breakdown": s.all_tool_calls,
            "mcp_breakdown": s.all_mcp_calls,
            "errors": s.all_errors,
            "turns": [
                {
                    "turn": t.turn,
                    "prompt": t.prompt,
                    "duration_ms": t.duration_ms,
                    "input_tokens": t.input_tokens,
                    "output_tokens": t.output_tokens,
                    "tool_count": t.tool_count,
                    "navigation_calls": t.navigation_calls,
                    "capsule_calls": t.capsule_calls,
                }
                for t in s.turns
            ],
        }

    def serialize_scavenger(sm: ScavengerMetrics) -> dict:
        return {
            "capsule_latency_p50_us": sm.capsule_latency_p50_us,
            "capsule_latency_p95_us": sm.capsule_latency_p95_us,
            "capsule_latency_p99_us": sm.capsule_latency_p99_us,
            "capsule_total": sm.capsule_total,
            "capsule_empty": sm.capsule_empty,
            "empty_rate": sm.empty_rate,
            "budget_utilization_avg": sm.budget_utilization_avg,
            "tokens_saved": sm.tokens_saved,
            "savings_pct": sm.savings_pct,
            "effectiveness_score": sm.effectiveness_score,
            "pipeline_gather_avg_us": sm.pipeline_gather_avg_us,
            "pipeline_score_avg_us": sm.pipeline_score_avg_us,
            "pipeline_render_avg_us": sm.pipeline_render_avg_us,
            "reindex_count": sm.reindex_count,
            "errors": sm.errors,
        }

    result = {
        "timestamp": datetime.now().isoformat(),
        "project": project,
        "model": model or "default",
        "prompts": prompts,
        "without": serialize_session(without),
        "with": serialize_session(with_s),
        "scavenger_metrics": serialize_scavenger(with_s.scavenger),
        "deltas": {
            "tool_count_pct": _pct_change(without.total_tool_count, with_s.total_tool_count),
            "navigation_pct": _pct_change(without.total_navigation, with_s.total_navigation),
            "tokens_pct": _pct_change(without.total_tokens, with_s.total_tokens),
            "duration_pct": _pct_change(without.total_duration_ms, with_s.total_duration_ms),
        },
    }

    with open(out_path, "w") as f:
        json.dump(result, f, indent=2)

    print(f"\nResults exported to: {out_path}")


def _pct_change(a: int, b: int) -> float:
    if a > 0:
        return round(((b - a) / a) * 100, 1)
    return 0.0


def main():
    parser = argparse.ArgumentParser(
        description="Scavenger A/B Benchmark — multi-turn session comparison"
    )
    parser.add_argument("prompt", nargs="?", help="Single prompt (one-shot mode)")
    parser.add_argument("--session", help="File with prompts for a multi-turn session (one per line)")
    parser.add_argument("--project", default=".", help="Project directory")
    parser.add_argument("--model", default=None, help="Model to use (e.g. sonnet-4)")
    args = parser.parse_args()

    if not args.prompt and not args.session:
        parser.error("Provide a prompt or --session file")

    prompts = []
    if args.prompt:
        prompts.append(args.prompt)
    if args.session:
        with open(args.session) as f:
            prompts.extend(line.strip() for line in f if line.strip() and not line.startswith("#"))

    project = os.path.abspath(args.project)
    mcp_path = Path(project) / ".cursor" / "mcp.json"

    if not mcp_path.exists():
        print(f"Error: {mcp_path} not found. Run `scavenger init` first.")
        sys.exit(1)

    is_session = len(prompts) > 1
    mode = "session" if is_session else "one-shot"

    print("Scavenger A/B Benchmark")
    print(f"Mode: {mode} ({len(prompts)} turn{'s' if is_session else ''})")
    print(f"Project: {project}")
    if args.model:
        print(f"Model: {args.model}")
    print()

    for i, p in enumerate(prompts):
        tag = f"  T{i + 1}: " if is_session else "  "
        print(f'{tag}"{p[:70]}{"..." if len(p) > 70 else ""}"')
    print()

    # Pre-flight health check
    run_health_gate(project)

    try:
        # --- WITHOUT Scavenger ---
        print("━━━ WITHOUT Scavenger ━━━")
        toggle_scavenger(project, enable=False)
        time.sleep(1)
        without = run_session(prompts, project, "WITHOUT", approve_mcps=False, model=args.model)
        auto_tag_session(project, without.session_id, "benchmark-without")
        reset_project(project)
        print("  (project reset)")

        print()

        # --- WITH Scavenger ---
        print("━━━ WITH Scavenger ━━━")
        toggle_scavenger(project, enable=True)
        time.sleep(2)
        with_s = run_session(prompts, project, "WITH", approve_mcps=True, model=args.model)
        auto_tag_session(project, with_s.session_id, "benchmark-with")

        # Capture Scavenger-side metrics after the WITH session
        print("  Capturing Scavenger daemon metrics...")
        with_s.scavenger = capture_scavenger_metrics(project)

        reset_project(project)
        print("  (project reset)")

        print()

        # --- Results ---
        if is_session:
            print_turn_timeline(without, with_s)
        print_session_summary(without, with_s)

        # Export structured JSON
        export_json_results(project, prompts, without, with_s, args.model)

    finally:
        reset_project(project)
        toggle_scavenger(project, enable=True)
        print("\n(project reset, scavenger MCP re-enabled)")


if __name__ == "__main__":
    main()
