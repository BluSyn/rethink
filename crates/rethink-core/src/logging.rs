//! Simple topic-filtered logging (mirrors util/logging.ts).

use parking_lot::RwLock;
use std::sync::OnceLock;

static FILTER: OnceLock<RwLock<Box<dyn Fn(&str) -> bool + Send + Sync>>> = OnceLock::new();

fn filter_slot() -> &'static RwLock<Box<dyn Fn(&str) -> bool + Send + Sync>> {
    FILTER.get_or_init(|| RwLock::new(Box::new(|_: &str| true)))
}

pub fn set_filter<F>(f: F)
where
    F: Fn(&str) -> bool + Send + Sync + 'static,
{
    *filter_slot().write() = Box::new(f);
}

pub fn log(topic: &str, args: &[&str]) {
    if filter_slot().read()(topic) {
        tracing::info!(topic, "{}", args.join(" "));
        // Also print for tests that don't set up tracing
        eprintln!("[{}] {}", topic, args.join(" "));
    }
}

/// Silence all logging (used by device unit tests).
pub fn silence() {
    set_filter(|_| false);
}
