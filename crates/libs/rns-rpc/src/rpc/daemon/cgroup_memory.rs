//! Process and cgroup v2 memory accounting for the daemon metrics snapshot.
//!
//! Operators reading issue 663 reports need anon, file cache, swap and
//! `memory.events` next to the daemon's own counters, so they can tell page
//! cache from heap without shell access. Everything is read from `/proc/self`
//! and the daemon's own cgroup; nothing here is a payload or identity. On
//! non-Linux hosts or without cgroup v2 the block says so instead of reporting zeros.

use serde_json::{json, Value as JsonValue};
use std::path::{Path, PathBuf};

const MEMORY_STAT_KEYS: [&str; 7] =
    ["anon", "file", "active_file", "inactive_file", "kernel", "file_dirty", "shmem"];
const MEMORY_EVENT_KEYS: [&str; 4] = ["high", "max", "oom", "oom_kill"];

/// Snapshot for the running process using the real `/proc` and `/sys/fs/cgroup` roots.
pub(crate) fn memory_accounting_snapshot() -> JsonValue {
    memory_accounting_snapshot_from(Path::new("/proc/self"), Path::new("/sys/fs/cgroup"))
}

/// Pure variant used by tests with a fake filesystem tree.
pub(crate) fn memory_accounting_snapshot_from(proc_self: &Path, cgroup_root: &Path) -> JsonValue {
    if !cfg!(target_os = "linux") {
        return json!({"available": false, "reason": "process and cgroup memory accounting is Linux-only"});
    }
    let process = process_memory(proc_self);
    let cgroup = match cgroup_dir(proc_self, cgroup_root) {
        Some(dir) => cgroup_memory(&dir),
        None => json!({"available": false, "reason": "cgroup v2 path unavailable"}),
    };
    json!({
        "available": true,
        "process": process,
        "cgroup": cgroup,
        "accounting": "Process values come from smaps_rollup; cgroup values include file cache and kernel memory charged to the daemon's cgroup.",
    })
}

fn process_memory(proc_self: &Path) -> JsonValue {
    match std::fs::read_to_string(proc_self.join("smaps_rollup")) {
        Ok(text) => {
            let values = parse_kb_lines(&text);
            json!({
                "available": true,
                "rss_bytes": values.get("Rss"),
                "anon_bytes": values.get("Anonymous"),
                "swap_bytes": values.get("Swap"),
                "shared_clean_bytes": values.get("Shared_Clean"),
                "private_dirty_bytes": values.get("Private_Dirty"),
            })
        }
        Err(error) => json!({"available": false, "reason": format!("smaps_rollup: {error}")}),
    }
}

fn cgroup_dir(proc_self: &Path, cgroup_root: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(proc_self.join("cgroup")).ok()?;
    let relative = parse_cgroup_v2_path(&text)?;
    let dir = cgroup_root.join(relative.trim_start_matches('/'));
    dir.is_dir().then_some(dir)
}

fn cgroup_memory(dir: &Path) -> JsonValue {
    let scalar = |name: &str| -> JsonValue {
        match std::fs::read_to_string(dir.join(name)) {
            Ok(text) => match text.trim() {
                "max" => JsonValue::Null,
                value => value.parse::<u64>().map(JsonValue::from).unwrap_or(JsonValue::Null),
            },
            Err(_) => JsonValue::Null,
        }
    };
    let stat = std::fs::read_to_string(dir.join("memory.stat")).map(|text| parse_kv_lines(&text));
    let events =
        std::fs::read_to_string(dir.join("memory.events")).map(|text| parse_kv_lines(&text));
    let mut stat_json = serde_json::Map::new();
    if let Ok(values) = &stat {
        for key in MEMORY_STAT_KEYS {
            stat_json.insert(key.to_string(), json!(values.get(key)));
        }
    }
    let mut events_json = serde_json::Map::new();
    if let Ok(values) = &events {
        for key in MEMORY_EVENT_KEYS {
            events_json.insert(key.to_string(), json!(values.get(key)));
        }
    }
    json!({
        "available": true,
        "path": dir.display().to_string(),
        "memory_current_bytes": scalar("memory.current"),
        "memory_swap_current_bytes": scalar("memory.swap.current"),
        "memory_high_bytes": scalar("memory.high"),
        "memory_max_bytes": scalar("memory.max"),
        "memory_stat": stat_json,
        "memory_events": events_json,
    })
}

/// `key value` lines (memory.stat, memory.events). Unparseable lines are skipped.
fn parse_kv_lines(text: &str) -> std::collections::HashMap<String, u64> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let key = parts.next()?;
            let value = parts.next()?.parse::<u64>().ok()?;
            Some((key.to_string(), value))
        })
        .collect()
}

/// `Key:   123 kB` lines (smaps_rollup), converted to bytes.
fn parse_kb_lines(text: &str) -> std::collections::HashMap<String, u64> {
    text.lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(':')?;
            let mut parts = rest.split_whitespace();
            let number = parts.next()?.parse::<u64>().ok()?;
            let bytes = if parts.next() == Some("kB") { number * 1024 } else { number };
            Some((key.trim().to_string(), bytes))
        })
        .collect()
}

/// The unified-hierarchy entry (`0::/path`) of `/proc/self/cgroup`.
fn parse_cgroup_v2_path(text: &str) -> Option<&str> {
    text.lines().find_map(|line| {
        let mut parts = line.splitn(3, ':');
        match (parts.next(), parts.next(), parts.next()) {
            (Some("0"), Some(""), Some(path)) => Some(path),
            _ => None,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_tree() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let temp = tempfile::tempdir().expect("temp dir");
        let proc_self = temp.path().join("proc_self");
        let cgroup_root = temp.path().join("cgroup");
        let scope = cgroup_root.join("user.slice/test.scope");
        std::fs::create_dir_all(&proc_self).expect("proc dir");
        std::fs::create_dir_all(&scope).expect("cgroup dir");
        std::fs::write(proc_self.join("cgroup"), "0::/user.slice/test.scope\n").expect("cgroup");
        std::fs::write(
            proc_self.join("smaps_rollup"),
            "55 7fff ---p 00000000 00:00 0  [rollup]\nRss:  2048 kB\nAnonymous: 1024 kB\nSwap: 16 kB\n",
        )
        .expect("rollup");
        std::fs::write(scope.join("memory.current"), "3145728\n").expect("current");
        std::fs::write(scope.join("memory.swap.current"), "4096\n").expect("swap");
        std::fs::write(scope.join("memory.high"), "805306368\n").expect("high");
        std::fs::write(scope.join("memory.max"), "max\n").expect("max");
        std::fs::write(scope.join("memory.stat"), "anon 1048576\nfile 2097152\nkernel 7\n")
            .expect("stat");
        std::fs::write(scope.join("memory.events"), "low 0\nhigh 12\nmax 0\noom 0\noom_kill 0\n")
            .expect("events");
        (temp, proc_self, cgroup_root)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn snapshot_reads_process_and_cgroup_values_in_bytes() {
        let (_temp, proc_self, cgroup_root) = fake_tree();
        let snapshot = memory_accounting_snapshot_from(&proc_self, &cgroup_root);
        assert_eq!(snapshot["available"], true);
        assert_eq!(snapshot["process"]["rss_bytes"], 2048 * 1024);
        assert_eq!(snapshot["process"]["anon_bytes"], 1024 * 1024);
        assert_eq!(snapshot["process"]["swap_bytes"], 16 * 1024);
        assert_eq!(snapshot["cgroup"]["memory_current_bytes"], 3_145_728);
        assert_eq!(snapshot["cgroup"]["memory_high_bytes"], 805_306_368);
        assert!(snapshot["cgroup"]["memory_max_bytes"].is_null(), "`max` must not become zero");
        assert_eq!(snapshot["cgroup"]["memory_stat"]["anon"], 1_048_576);
        assert_eq!(snapshot["cgroup"]["memory_stat"]["file"], 2_097_152);
        assert!(snapshot["cgroup"]["memory_stat"]["active_file"].is_null());
        assert_eq!(snapshot["cgroup"]["memory_events"]["high"], 12);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn missing_cgroup_is_reported_as_unavailable_not_zero() {
        let (_temp, proc_self, cgroup_root) = fake_tree();
        std::fs::write(proc_self.join("cgroup"), "1:memory:/legacy\n").expect("legacy cgroup");
        let snapshot = memory_accounting_snapshot_from(&proc_self, &cgroup_root);
        assert_eq!(snapshot["cgroup"]["available"], false);
        assert_eq!(snapshot["process"]["available"], true);
    }

    #[test]
    fn parsers_skip_malformed_lines() {
        assert_eq!(parse_kv_lines("a 1\nb x\nc\n").get("a"), Some(&1));
        assert_eq!(parse_kv_lines("a 1\nb x\nc\n").len(), 1);
        assert_eq!(parse_kb_lines("Rss: 4 kB\nBad\n").get("Rss"), Some(&4096));
        assert_eq!(parse_cgroup_v2_path("0::/a/b\n"), Some("/a/b"));
        assert_eq!(parse_cgroup_v2_path("12:pids:/x\n"), None);
    }
}
