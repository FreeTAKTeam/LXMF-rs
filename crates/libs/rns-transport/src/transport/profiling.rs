//! Lightweight runtime profiling compatible with the RNS 1.5 status surface.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use serde_json::{Map, Value};

type Samples = BTreeMap<String, Vec<f64>>;

fn samples() -> &'static Mutex<Samples> {
    static SAMPLES: OnceLock<Mutex<Samples>> = OnceLock::new();
    SAMPLES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn paused_tags() -> &'static Mutex<HashSet<String>> {
    static PAUSED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    PAUSED.get_or_init(|| Mutex::new(HashSet::new()))
}

fn profiler_metadata() -> &'static Mutex<BTreeMap<String, Option<String>>> {
    static METADATA: OnceLock<Mutex<BTreeMap<String, Option<String>>>> = OnceLock::new();
    METADATA.get_or_init(|| Mutex::new(BTreeMap::new()))
}

static RAN: AtomicBool = AtomicBool::new(false);

/// A named profiler. Captures are process-wide, matching RNS's global
/// profiler registry, while the value itself is cheap to clone and pass to
/// instrumentation sites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profiler {
    tag: String,
    super_tag: Option<String>,
    max_captures: usize,
}

impl Profiler {
    pub const MAX_CAPTURES: usize = 10_000;

    #[must_use]
    pub fn get_profiler(
        tag: Option<&str>,
        super_tag: Option<&str>,
        max_captures: Option<usize>,
    ) -> Self {
        let tag = tag.unwrap_or("default").to_string();
        let mut metadata = profiler_metadata().lock().expect("profiler metadata mutex poisoned");
        if let Some(existing_super) = metadata.get(&tag) {
            return Self {
                tag,
                super_tag: existing_super.clone(),
                max_captures: max_captures.unwrap_or(Self::MAX_CAPTURES).max(1),
            };
        }
        let super_tag = super_tag.map(str::to_string);
        metadata.insert(tag.clone(), super_tag.clone());
        Self { tag, super_tag, max_captures: max_captures.unwrap_or(Self::MAX_CAPTURES).max(1) }
    }

    /// Starts a capture for an explicit tag.
    #[must_use]
    pub fn profile(tag: impl Into<String>) -> ProfilerGuard {
        let tag = tag.into();
        let profiler = Self::get_profiler(Some(&tag), None, None);
        profiler.scoped()
    }

    /// Starts a capture using this profiler's tag.
    #[must_use]
    pub fn scoped(&self) -> ProfilerGuard {
        ProfilerGuard {
            tag: self.tag.clone(),
            max_captures: self.max_captures,
            started: Instant::now(),
            paused_at: None,
            paused_for: 0.0,
        }
    }

    pub fn pause(&self) {
        paused_tags().lock().expect("profiler pause mutex poisoned").insert(self.tag.clone());
    }

    pub fn resume(&self) {
        paused_tags().lock().expect("profiler pause mutex poisoned").remove(&self.tag);
    }

    #[must_use]
    pub fn ran() -> bool {
        RAN.load(Ordering::Acquire)
    }

    /// Returns JSON-compatible summary statistics grouped by profiler tag.
    #[must_use]
    pub fn results() -> Value {
        let guard = samples().lock().expect("profiler samples mutex poisoned");
        let mut result = Map::new();
        for (tag, values) in guard.iter() {
            if values.is_empty() {
                continue;
            }
            let mut sorted = values.clone();
            sorted.sort_by(f64::total_cmp);
            let count = values.len();
            let sum: f64 = values.iter().sum();
            let mean = sum / count as f64;
            let median = if count % 2 == 0 {
                (sorted[count / 2 - 1] + sorted[count / 2]) / 2.0
            } else {
                sorted[count / 2]
            };
            let min = sorted[0];
            let max = sorted[count - 1];
            let stdev = if count > 1 {
                let variance = values.iter().map(|value| (value - mean).powi(2)).sum::<f64>()
                    / (count - 1) as f64;
                Value::from(variance.sqrt())
            } else {
                Value::Null
            };
            let mut stats = Map::new();
            stats.insert("count".to_string(), Value::from(count as u64));
            stats.insert("mean".to_string(), Value::from(mean));
            stats.insert("median".to_string(), Value::from(median));
            stats.insert("min".to_string(), Value::from(min));
            stats.insert("max".to_string(), Value::from(max));
            stats.insert("stdev".to_string(), stdev);
            stats.insert("sum".to_string(), Value::from(sum));

            let mut entry = Map::new();
            entry.insert("name".to_string(), Value::from(tag.clone()));
            let super_tag = profiler_metadata()
                .lock()
                .expect("profiler metadata mutex poisoned")
                .get(tag)
                .cloned()
                .flatten();
            entry.insert("super".to_string(), super_tag.map_or(Value::Null, Value::from));
            entry.insert("stats_all".to_string(), Value::Object(stats));
            result.insert(tag.clone(), Value::Object(entry));
        }
        Value::Object(result)
    }

    /// Formats profiler summaries for human-readable status output.
    #[must_use]
    pub fn format_results(results: &Value) -> String {
        let Some(entries) = results.as_object() else {
            return String::new();
        };
        let mut output = String::new();
        for (tag, entry) in entries {
            let stats = entry.get("stats_all").and_then(Value::as_object);
            let count =
                stats.and_then(|value| value.get("count")).and_then(Value::as_u64).unwrap_or(0);
            let mean =
                stats.and_then(|value| value.get("mean")).and_then(Value::as_f64).unwrap_or(0.0);
            let total =
                stats.and_then(|value| value.get("sum")).and_then(Value::as_f64).unwrap_or(0.0);
            output.push_str(&format!("{tag}: samples={count} mean={mean:.6}s total={total:.6}s\n"));
        }
        output
    }

    fn record(&self, seconds: f64) {
        let _super_tag = self.super_tag.as_deref();
        let mut guard = samples().lock().expect("profiler samples mutex poisoned");
        let values = guard.entry(self.tag.clone()).or_default();
        if values.len() >= self.max_captures {
            values.remove(0);
        }
        values.push(seconds);
        RAN.store(true, Ordering::Release);
    }
}

/// RAII capture returned by [`Profiler::profile`] and [`Profiler::scoped`].
#[derive(Debug)]
pub struct ProfilerGuard {
    tag: String,
    max_captures: usize,
    started: Instant,
    paused_at: Option<Instant>,
    paused_for: f64,
}

impl ProfilerGuard {
    pub fn pause(&mut self) {
        if self.paused_at.is_none() {
            self.paused_at = Some(Instant::now());
        }
    }

    pub fn resume(&mut self) {
        if let Some(paused_at) = self.paused_at.take() {
            self.paused_for += paused_at.elapsed().as_secs_f64();
        }
    }
}

impl Drop for ProfilerGuard {
    fn drop(&mut self) {
        self.resume();
        if paused_tags().lock().expect("profiler pause mutex poisoned").contains(&self.tag) {
            return;
        }
        let elapsed = self.started.elapsed().as_secs_f64() - self.paused_for;
        let profiler = Profiler::get_profiler(Some(&self.tag), None, Some(self.max_captures));
        profiler.record(elapsed.max(0.0));
    }
}

impl super::Transport {
    /// Returns the current process-wide profile snapshot for status/RPC
    /// consumers, or `None` until at least one capture has completed.
    #[must_use]
    pub fn get_profiling_results(&self) -> Option<Value> {
        Profiler::ran().then(Profiler::results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn profiler_records_and_formats_named_capture() {
        let profiler = Profiler::get_profiler(Some("rns-1.5.2-test"), None, Some(4));
        {
            let _capture = profiler.scoped();
            thread::sleep(Duration::from_millis(1));
        }
        let results = Profiler::results();
        let entry = results
            .get("rns-1.5.2-test")
            .and_then(|value| value.get("stats_all"))
            .expect("profile entry");
        assert_eq!(entry.get("count").and_then(Value::as_u64), Some(1));
        assert!(entry.get("stdev").is_some_and(Value::is_null));
        assert!(Profiler::format_results(&results).contains("rns-1.5.2-test"));
    }

    #[test]
    fn profiler_honors_capture_limit() {
        let profiler = Profiler::get_profiler(Some("rns-1.5.2-limit"), None, Some(1));
        for _ in 0..3 {
            let _capture = profiler.scoped();
        }
        assert_eq!(
            Profiler::results()["rns-1.5.2-limit"]["stats_all"]["count"],
            Value::from(1_u64)
        );
    }
}
