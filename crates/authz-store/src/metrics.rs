//! Fixed-name process-wide counters: never attach tenant/resource/key labels.
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
pub struct Metric {
    count: AtomicU64,
    sum: AtomicU64,
}
impl Metric {
    pub const fn new() -> Self {
        Self {
            count: AtomicU64::new(0),
            sum: AtomicU64::new(0),
        }
    }
    pub fn observe(&self, value: u64) {
        self.count.fetch_add(1, Ordering::Relaxed);
        self.sum.fetch_add(value, Ordering::Relaxed);
    }
    pub fn timer(&self) -> Timer<'_> {
        Timer {
            metric: self,
            start: Instant::now(),
        }
    }
    pub fn render(&self, name: &str, scale: f64) -> String {
        format!(
            "{name}_count {}\n{name}_sum {}\n",
            self.count.load(Ordering::Relaxed),
            self.sum.load(Ordering::Relaxed) as f64 / scale
        )
    }
}
impl Default for Metric {
    fn default() -> Self {
        Self::new()
    }
}
pub struct Timer<'a> {
    metric: &'a Metric,
    start: Instant,
}
impl Drop for Timer<'_> {
    fn drop(&mut self) {
        self.metric
            .observe(self.start.elapsed().as_nanos().min(u64::MAX as u128) as u64);
    }
}
pub static POOL_WAIT: Metric = Metric::new();
pub static DATA_SQL: Metric = Metric::new();
pub static CORE: Metric = Metric::new();
pub static LOCK_WAIT: Metric = Metric::new();
pub static FACTS: Metric = Metric::new();
pub static WORK: Metric = Metric::new();
pub fn render() -> String {
    let mut s = String::new();
    for (name, metric, scale) in [
        ("relation_guard_pool_wait_seconds", &POOL_WAIT, 1e9),
        ("relation_guard_data_sql_seconds", &DATA_SQL, 1e9),
        ("relation_guard_core_seconds", &CORE, 1e9),
        ("relation_guard_write_lock_seconds", &LOCK_WAIT, 1e9),
        ("relation_guard_facts", &FACTS, 1.0),
        ("relation_guard_work_units", &WORK, 1.0),
    ] {
        s.push_str(&metric.render(name, scale));
    }
    s
}
