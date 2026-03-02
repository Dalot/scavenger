use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use parking_lot::Mutex;
use serde_json::{Value, json};

/// Lightweight histogram using a sorted sample buffer.
/// Keeps the last N samples for percentile computation.
pub struct Histogram {
    samples: Mutex<Vec<u64>>,
    capacity: usize,
    sum: AtomicU64,
    count: AtomicU64,
}

impl Histogram {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: Mutex::new(Vec::with_capacity(capacity)),
            capacity,
            sum: AtomicU64::new(0),
            count: AtomicU64::new(0),
        }
    }

    pub fn record(&self, value: u64) {
        self.sum.fetch_add(value, Ordering::Relaxed);
        self.count.fetch_add(1, Ordering::Relaxed);
        let mut samples = self.samples.lock();
        if samples.len() >= self.capacity {
            samples.remove(0);
        }
        samples.push(value);
    }

    pub fn percentile(&self, p: f64) -> u64 {
        let mut samples = self.samples.lock();
        if samples.is_empty() {
            return 0;
        }
        samples.sort_unstable();
        let idx = ((p / 100.0) * (samples.len() - 1) as f64).round() as usize;
        samples[idx.min(samples.len() - 1)]
    }

    pub fn avg(&self) -> f64 {
        let count = self.count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        self.sum.load(Ordering::Relaxed) as f64 / count as f64
    }

    pub fn count(&self) -> u64 {
        self.count.load(Ordering::Relaxed)
    }

    fn to_json(&self) -> Value {
        json!({
            "count": self.count(),
            "avg": self.avg().round() as u64,
            "p50": self.percentile(50.0),
            "p95": self.percentile(95.0),
            "p99": self.percentile(99.0),
        })
    }
}

/// Atomic counter.
pub struct Counter(AtomicU64);

impl Counter {
    pub fn new() -> Self {
        Self(AtomicU64::new(0))
    }

    pub fn inc(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    pub fn get(&self) -> u64 {
        self.0.load(Ordering::Relaxed)
    }
}

const HISTOGRAM_CAPACITY: usize = 1000;

/// In-memory daemon metrics aggregator.
pub struct DaemonMetrics {
    pub started_at: Instant,

    // Request-level
    pub requests: Counter,
    pub request_latency_us: Histogram,

    // Capsule-level
    pub capsule_requests: Counter,
    pub capsule_empty: Counter,
    pub capsule_tokens: Histogram,
    pub capsule_items: Histogram,
    pub capsule_budget_util_pct: Histogram,
    pub capsule_latency_us: Histogram,

    // Pipeline stage timing (microseconds)
    pub stage_gather_us: Histogram,
    pub stage_score_us: Histogram,
    pub stage_pin_us: Histogram,
    pub stage_trim_us: Histogram,
    pub stage_group_us: Histogram,
    pub stage_render_us: Histogram,

    // Query timing
    pub query_latency_us: Histogram,

    // Reindex
    pub reindex_count: Counter,
    pub reindex_duration_us: Histogram,

    // FTS
    pub fts_query_count: Counter,
    pub fts_query_duration_us: Histogram,

    // Hooks
    pub hook_pre_count: Counter,
    pub hook_pre_injected: Counter,
    pub hook_post_count: Counter,

    // Annotations
    pub annotation_writes: Counter,
    pub annotation_dedup: Counter,

    // Errors
    pub errors: Counter,
}

impl DaemonMetrics {
    pub fn new() -> Self {
        Self {
            started_at: Instant::now(),
            requests: Counter::new(),
            request_latency_us: Histogram::new(HISTOGRAM_CAPACITY),
            capsule_requests: Counter::new(),
            capsule_empty: Counter::new(),
            capsule_tokens: Histogram::new(HISTOGRAM_CAPACITY),
            capsule_items: Histogram::new(HISTOGRAM_CAPACITY),
            capsule_budget_util_pct: Histogram::new(HISTOGRAM_CAPACITY),
            capsule_latency_us: Histogram::new(HISTOGRAM_CAPACITY),
            stage_gather_us: Histogram::new(HISTOGRAM_CAPACITY),
            stage_score_us: Histogram::new(HISTOGRAM_CAPACITY),
            stage_pin_us: Histogram::new(HISTOGRAM_CAPACITY),
            stage_trim_us: Histogram::new(HISTOGRAM_CAPACITY),
            stage_group_us: Histogram::new(HISTOGRAM_CAPACITY),
            stage_render_us: Histogram::new(HISTOGRAM_CAPACITY),
            query_latency_us: Histogram::new(HISTOGRAM_CAPACITY),
            reindex_count: Counter::new(),
            reindex_duration_us: Histogram::new(HISTOGRAM_CAPACITY),
            fts_query_count: Counter::new(),
            fts_query_duration_us: Histogram::new(HISTOGRAM_CAPACITY),
            hook_pre_count: Counter::new(),
            hook_pre_injected: Counter::new(),
            hook_post_count: Counter::new(),
            annotation_writes: Counter::new(),
            annotation_dedup: Counter::new(),
            errors: Counter::new(),
        }
    }

    /// Serialize all metrics into a JSON snapshot.
    pub fn snapshot(&self, node_count: usize, edge_count: usize) -> Value {
        let uptime_secs = self.started_at.elapsed().as_secs();
        let total = self.requests.get();
        let rate_per_min = if uptime_secs > 0 {
            total as f64 / (uptime_secs as f64 / 60.0)
        } else {
            0.0
        };

        let capsule_total = self.capsule_requests.get();
        let capsule_empty = self.capsule_empty.get();
        let empty_rate = if capsule_total > 0 {
            capsule_empty as f64 / capsule_total as f64
        } else {
            0.0
        };

        let ann_writes = self.annotation_writes.get();
        let ann_dedup = self.annotation_dedup.get();
        let dedup_rate = if ann_writes > 0 {
            ann_dedup as f64 / ann_writes as f64
        } else {
            0.0
        };

        json!({
            "uptime_secs": uptime_secs,
            "requests": {
                "total": total,
                "rate_per_min": (rate_per_min * 100.0).round() / 100.0,
                "latency_us": self.request_latency_us.to_json(),
            },
            "capsule": {
                "total": capsule_total,
                "empty": capsule_empty,
                "empty_rate": (empty_rate * 1000.0).round() / 1000.0,
                "latency_us": self.capsule_latency_us.to_json(),
                "tokens": self.capsule_tokens.to_json(),
                "items": self.capsule_items.to_json(),
                "budget_utilization_pct": self.capsule_budget_util_pct.to_json(),
            },
            "pipeline_us": {
                "gather": self.stage_gather_us.to_json(),
                "score": self.stage_score_us.to_json(),
                "pin": self.stage_pin_us.to_json(),
                "trim": self.stage_trim_us.to_json(),
                "group": self.stage_group_us.to_json(),
                "render": self.stage_render_us.to_json(),
            },
            "query": {
                "latency_us": self.query_latency_us.to_json(),
            },
            "reindex": {
                "count": self.reindex_count.get(),
                "latency_us": self.reindex_duration_us.to_json(),
            },
            "fts": {
                "queries": self.fts_query_count.get(),
                "latency_us": self.fts_query_duration_us.to_json(),
            },
            "hooks": {
                "pre_count": self.hook_pre_count.get(),
                "pre_injected": self.hook_pre_injected.get(),
                "post_count": self.hook_post_count.get(),
            },
            "annotations": {
                "writes": ann_writes,
                "dedup": ann_dedup,
                "dedup_rate": (dedup_rate * 1000.0).round() / 1000.0,
            },
            "graph": {
                "nodes": node_count,
                "edges": edge_count,
            },
            "errors": self.errors.get(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_histogram_percentiles() {
        let h = Histogram::new(100);
        for i in 1..=100 {
            h.record(i);
        }
        let p50 = h.percentile(50.0);
        assert!(p50 >= 49 && p50 <= 52, "p50 should be ~50, got {p50}");
        assert!(h.percentile(95.0) >= 94);
        assert!(h.percentile(99.0) >= 98);
        assert_eq!(h.count(), 100);
        assert!((h.avg() - 50.5).abs() < 0.1);
    }

    #[test]
    fn test_histogram_empty() {
        let h = Histogram::new(100);
        assert_eq!(h.percentile(50.0), 0);
        assert_eq!(h.count(), 0);
        assert_eq!(h.avg(), 0.0);
    }

    #[test]
    fn test_counter() {
        let c = Counter::new();
        assert_eq!(c.get(), 0);
        c.inc();
        c.inc();
        assert_eq!(c.get(), 2);
    }

    #[test]
    fn test_snapshot_structure() {
        let m = DaemonMetrics::new();
        m.requests.inc();
        m.capsule_requests.inc();
        m.capsule_tokens.record(3200);
        let snap = m.snapshot(100, 200);
        assert_eq!(snap["requests"]["total"], 1);
        assert_eq!(snap["capsule"]["total"], 1);
        assert_eq!(snap["graph"]["nodes"], 100);
        assert_eq!(snap["graph"]["edges"], 200);
    }
}
